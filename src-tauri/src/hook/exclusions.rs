/// hook/exclusions.rs — the per-app exception list ("App exceptions").
///
/// Apps like Photoshop, Figma and Blender use HOLD-SPACE mouse gestures (hold
/// Space, drag, the canvas pans). Spaceadom suppresses every Space-down
/// system-wide, so inside those apps the gesture is simply dead — verified
/// 2026-08-25: the target app never receives a Space keydown while Spaceadom
/// runs. The fix is a per-app passthrough: while an excluded app is the
/// foreground window, the hook passes EVERYTHING through and Space behaves
/// exactly as it would with Spaceadom closed.
///
/// Structure is a deliberate line-for-line copy of `hook/fullscreen.rs`:
///   · a NAMED background thread on a 500 ms cadence (the hook CALLBACK may do
///     lock-free atomics ONLY — no locks, no logging, no win32k calls; that is
///     PROBLEM 58/134/184, and a foreground-window query is a win32k call),
///   · `catch_unwind` around the probe,
///   · a non-panicking spawn (PROBLEM 124 — a `Builder::spawn` failure is
///     plausible on someone else's machine and must not kill the app at
///     launch).
///
/// The one deliberate DIFFERENCE from fullscreen.rs is which way the probe
/// fails. Fullscreen fails toward "not fullscreen" so a broken probe cannot
/// disable the app; this fails toward "NOT excluded" for the same reason —
/// a panicking probe that latched `true` would stand Spaceadom down
/// everywhere, silently, until restart.
use std::sync::atomic::Ordering;
use std::sync::Mutex;

/// The published list of excluded apps, as lowercase exe STEMS ("photoshop").
///
/// The POLLER may lock; the HOOK CALLBACK may not — and it never touches this,
/// it reads the single `EXCLUDED_ACTIVE` atomic the poller writes. A 500 ms
/// uncontended lock on a background thread costs nothing.
static EXCLUDED_LIST: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// PROBLEM 267 — the SAME list with each entry's scope beside it. Two lists
/// rather than one so every pre-267 reader of `EXCLUDED_LIST` (a plain stem
/// list meaning "Space stands down here") keeps its exact semantics, and the
/// scope is consulted only by `resolve_scope` on the poller.
static SCOPED_LIST: Mutex<Vec<(String, crate::config::ExceptionScope)>> = Mutex::new(Vec::new());

/// PROBLEM 267 — the middle-button verdict for the foreground app, the twin of
/// `hook::EXCLUDED_ACTIVE` for the OTHER trigger. `true` = the user's own list
/// (an `OffEntirely` or `SpaceOnly` row) stands the middle button down here.
/// Written ONLY by the poller; read by `ms_hook_proc` as one relaxed load and
/// passed to `middle_button_down_accepted` as `user_excluded`.
///
/// NOT the built-in 3D/CAD verdict — that stays `orbit_apps::ORBIT_ACTIVE`,
/// a separate parameter of the same gate, so a log can still say WHICH list
/// declined a press.
pub static MIDDLE_EXCLUDED_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// PROBLEM 267 — what the two triggers do inside the foreground app, resolved
/// from the user's scoped list and the built-in orbit list in ONE place.
///
/// Precedence, and each line is a decision:
/// 1. The user's own row for this stem wins outright, whatever the built-in
///    table says — a built-in row the user moved to `MiddleOnly` is stored in
///    the user's list, so SolidWorks can have its ring back if he wants it.
/// 2. Otherwise a built-in orbit app is `SpaceOnly`: Space works, the middle
///    button orbits the model (PROBLEM 263's rule, unchanged).
/// 3. Otherwise nothing stands down.
///
/// Returns `(space_off, middle_off, from_user_row)`. `from_user_row` is what
/// `orbit_apps::publish` needs to know so `ORBIT_ACTIVE` is only ever set by
/// the BUILT-IN table — the user's row publishes through
/// `MIDDLE_EXCLUDED_ACTIVE` instead, and the two never both claim one app.
pub fn resolve_scope(
    foreground: &str,
    scoped: &[(String, crate::config::ExceptionScope)],
    is_builtin_orbit: bool,
) -> (bool, bool, bool) {
    use crate::config::ExceptionScope::*;
    let fg = normalize_stem(foreground);
    if fg.is_empty() {
        return (false, false, false);
    }
    if let Some((_, scope)) = scoped.iter().find(|(stem, _)| *stem == fg) {
        return match scope {
            OffEntirely => (true, true, true),
            SpaceOnly => (false, true, true),
            MiddleOnly => (true, false, true),
        };
    }
    if is_builtin_orbit {
        return (false, true, false);
    }
    (false, false, false)
}

/// Normalise anything a user or Windows can hand us into a lowercase exe stem.
///
/// Accepts a full path (`C:\Program Files\Adobe\Photoshop.exe`), a bare file
/// name (`Photoshop.exe`) or an already-normalised stem (`photoshop`), and
/// always returns the stem, lowercased. This is the SAME normalisation the
/// conflicts / fullscreen-allowlist code applies (`file_name`, lowercased) —
/// with the `.exe` suffix additionally removed so the frontend can store, show
/// and dedupe one canonical form.
pub fn normalize_stem(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches('"');
    // Split on BOTH separators by hand rather than using `Path::file_name`:
    // on a non-Windows build (tests run everywhere) `Path` does not treat `\`
    // as a separator, so a Windows path would come back whole.
    let name = trimmed
        .rsplit(|c| c == '\\' || c == '/')
        .next()
        .unwrap_or(trimmed);
    let lower = name.to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

/// True when `foreground` (any of the forms `normalize_stem` accepts) matches
/// an entry of `list`. Entries are normalised on the way in too, so a config
/// hand-edited to hold a full path still works.
pub fn is_excluded(foreground: &str, list: &[String]) -> bool {
    let fg = normalize_stem(foreground);
    if fg.is_empty() {
        return false;
    }
    list.iter().any(|e| normalize_stem(e) == fg)
}

/// Our own exe stem, as `is_excluded` would see it.
///
/// PROBLEM 218 — falls back to the literal `spaceadom` rather than to nothing.
/// `current_exe()` can fail, and failing toward "no name" would silently turn
/// the self-exclusion guard below into a no-op — a guard that cannot fire is
/// not a guard. The identity is fixed by CLAUDE.md (exe `spaceadom.exe`), so
/// the fallback is a fact, not a guess. Under `cargo test` `current_exe()` is
/// the TEST binary, which is why the filtering itself takes the stem as an
/// argument and is tested separately.
pub fn own_stem() -> String {
    std::env::current_exe()
        .ok()
        .map(|p| normalize_stem(&p.to_string_lossy()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "spaceadom".to_string())
}

/// Drop any entry that names US. Pure, so it can be tested.
///
/// PROBLEM 218 — SPACEADOM MUST NEVER BE ABLE TO EXCLUDE ITSELF.
///
/// The reasoning is already written down in the frontend, in
/// `src/components/settings-panel.ts` (the picker refuses `spaceadom` with
/// *"Excluding the app that DRAWS this panel would be a trap: Spaceadom would
/// stand down whenever its own dashboard had focus, and the setting that
/// caused it would look like it had simply done nothing"*). The author saw the
/// trap exactly and then guarded only the doorway they were standing in.
///
/// The backend accepted whatever the config held. `excluded_apps` reaches it
/// from a hand-edited `config.json`, a restored backup, a profile import and a
/// schema migration — none of which go through that picker — and the result
/// would be report (B) verbatim: no HUD, no shortcuts, nothing at all while
/// Spaceadom's own window is focused, working again the moment it is
/// minimised, and no log line anywhere naming the cause.
///
/// GENERALISE: a rule enforced only in the UI is not enforced. Put it where
/// the value is CONSUMED, not where it is entered.
pub fn without_self(list: Vec<String>, own: &str) -> (Vec<String>, Vec<String>) {
    let own = normalize_stem(own);
    let (dropped, kept): (Vec<String>, Vec<String>) =
        list.into_iter().partition(|e| !own.is_empty() && *e == own);
    (kept, dropped)
}

/// Publish the config's exception list for the poller.
///
/// PROBLEM 180 — MUST be called from BOTH the startup config load in `lib.rs`
/// AND `config::save`. An atomic (or, here, a list) that starts empty and is
/// only fed on save means the feature is dead from launch until the user
/// happens to save something.
pub fn publish_excluded_apps(cfg: &crate::config::AppConfig) {
    use crate::config::ExceptionScope;
    // PROBLEM 267 — every row, normalised, with its scope. The plain stem
    // list below (what every pre-267 consumer means by "excluded") is the
    // rows whose scope stands SPACE down: off entirely, or middle-only.
    let scoped: Vec<(String, ExceptionScope)> = cfg
        .excluded_apps
        .iter()
        .map(|e| (normalize_stem(&e.exe), e.scope))
        .filter(|(s, _)| !s.is_empty())
        .collect();
    let list: Vec<String> = scoped
        .iter()
        .filter(|(_, scope)| matches!(scope, ExceptionScope::OffEntirely | ExceptionScope::MiddleOnly))
        .map(|(s, _)| s.clone())
        .collect();
    // PROBLEM 218 — see `without_self`. Loud, because the alternative is an
    // app that does nothing in its own window for a reason nobody can find.
    let (list, dropped) = without_self(list, &own_stem());
    let own = normalize_stem(&own_stem());
    let scoped: Vec<(String, ExceptionScope)> =
        scoped.into_iter().filter(|(s, _)| own.is_empty() || *s != own).collect();
    {
        let mut guard = SCOPED_LIST.lock().unwrap_or_else(|p| p.into_inner());
        if *guard != scoped {
            log::info!(
                "exclusions: {} scoped row(s) — {:?} (PROBLEM 267)",
                scoped.len(),
                scoped.iter().map(|(s, sc)| format!("{s}:{sc:?}")).collect::<Vec<_>>()
            );
            *guard = scoped;
        }
        if guard.is_empty() {
            MIDDLE_EXCLUDED_ACTIVE.store(false, Ordering::Relaxed);
        }
    }
    if !dropped.is_empty() {
        log::error!(
            "exclusions: the app exception list named SPACEADOM ITSELF ({dropped:?}) — \
             ignoring it. Honouring it would stand every shortcut and the Guide HUD down \
             whenever Spaceadom's own window had focus, which reads to the user as \
             \"the app only works if I minimise it\" and points at nothing. The settings \
             picker already refuses this; the entry therefore came from a hand-edited \
             config.json, a restored backup or an import (PROBLEM 218)."
        );
    }
    let mut guard = EXCLUDED_LIST.lock().unwrap_or_else(|p| p.into_inner());
    if *guard != list {
        log::info!("exclusions: {} app(s) excluded — {:?}", list.len(), list);
        *guard = list;
    }
    // If the list just became empty, do not leave a stale TRUE standing until
    // the next poll: that is up to half a second of an app the user has just
    // un-excluded still doing nothing.
    if guard.is_empty() {
        crate::hook::EXCLUDED_ACTIVE.store(false, Ordering::Relaxed);
    }
}

#[cfg(windows)]
fn snapshot() -> Vec<String> {
    EXCLUDED_LIST
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

#[cfg(windows)]
fn scoped_snapshot() -> Vec<(String, crate::config::ExceptionScope)> {
    SCOPED_LIST
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

/// Spawn the background thread that polls the foreground app every 500 ms and
/// writes the verdict into `hook::EXCLUDED_ACTIVE`.
pub fn start_exclusion_watcher() {
    std::thread::Builder::new()
        .name("st-exclusion-watcher".into())
        .spawn(move || {
            log::debug!("exclusion watcher thread started");
            #[cfg(windows)]
            let mut last_verdict = false;
            #[cfg(windows)]
            let mut last_name = String::new();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));

                #[cfg(windows)]
                {
                    let list = snapshot();
                    let name = std::panic::catch_unwind(|| unsafe { foreground_stem() })
                        .unwrap_or_else(|_| {
                            // Fail toward NOT excluded. A broken probe must
                            // never be able to stand Spaceadom down everywhere.
                            log::error!(
                                "exclusions: probe panicked — assuming the foreground app is \
                                 NOT excluded so shortcuts keep working"
                            );
                            String::new()
                        });
                    // PROBLEM 267 — ONE resolver for both triggers. The
                    // legacy `is_excluded` walk is kept as the Space verdict
                    // (it is what `list` has always meant); `resolve_scope`
                    // reads the SAME rows with their scopes for the middle
                    // button, and tells `orbit_apps` whether the user has a
                    // row of his own for this app.
                    let detected = !list.is_empty() && is_excluded(&name, &list);
                    crate::hook::EXCLUDED_ACTIVE.store(detected, Ordering::Relaxed);
                    let scoped = scoped_snapshot();
                    let builtin = crate::hook::orbit_apps::is_orbit_app(&name);
                    let (_space_off, middle_off, from_user_row) =
                        resolve_scope(&name, &scoped, builtin);
                    // The USER's rows publish here; the built-in table
                    // publishes through ORBIT_ACTIVE below. A user row for a
                    // built-in app is the one case both could speak, and the
                    // user's wins: `from_user_row` mutes the built-in verdict.
                    MIDDLE_EXCLUDED_ACTIVE.store(middle_off && from_user_row, Ordering::Relaxed);

                    // PROBLEM 263 — the BUILT-IN middle-button exclusion list
                    // rides on this thread, and on this tick's `foreground_stem`
                    // probe, which is the expensive part and is already paid
                    // for. Two lists, one probe, one 500 ms cadence.
                    //
                    // DELIBERATELY NOT MERGED WITH `detected` ABOVE. That
                    // verdict stands the WHOLE app down (Space included);
                    // `orbit_apps` stands the MIDDLE BUTTON down and nothing
                    // else, so a user's Space shortcuts keep working in
                    // SolidWorks. Feeding one into the other would silently
                    // delete those shortcuts in twenty programs.
                    //
                    // It also runs when the probe PANICKED and `name` is empty:
                    // `is_orbit_app("")` is false, so the trigger stays live —
                    // the same fail-toward-not-excluded direction as the line
                    // above — and `WATCHER_ALIVE` is still armed, which is what
                    // it is for (a watcher that is running and reading nothing
                    // is not the failure that gate exists for; a watcher that
                    // never spawned is).
                    //
                    // PROBLEM 267 — `from_user_row` mutes the built-in verdict
                    // for an app the user has his own row for (he may have
                    // moved SolidWorks to "Middle only"; his row then owns it).
                    crate::hook::orbit_apps::publish(&name, from_user_row);

                    // Logging here is legal and useful: this is the POLLER
                    // thread, not the hook callback. One line each way, on
                    // CHANGE only — an alt-tab must never spam anything, and
                    // there is deliberately no toast.
                    if detected != last_verdict {
                        if detected {
                            log::info!(
                                "exclusions: {name} is foreground — Spaceadom standing down"
                            );
                            last_name = name;
                        } else {
                            log::info!("exclusions: left {last_name} — Spaceadom resumed");
                        }
                        last_verdict = detected;
                    }
                }
            }
        })
        .map(|_| ())
        // PROBLEM 124 — never `.expect()` a spawn. This runs during setup,
        // before the keyboard hook is installed, so a panic here would kill
        // the app at launch with no window and no tray icon.
        .unwrap_or_else(|e| {
            log::error!(
                "exclusions: could not spawn the watcher thread ({e}) — continuing WITHOUT \
                 the app exception list. Shortcuts still work everywhere; they will simply \
                 not stand down inside an app the user excluded."
            );
        });
}

/// The foreground window's exe stem, lowercased. Empty when it cannot be read.
///
/// Same plumbing as `fullscreen.rs::is_allowlisted` — `GetWindowThreadProcessId`
/// then `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` +
/// `QueryFullProcessImageNameW` — but it RETURNS the name instead of comparing
/// it, so the comparison can live in the pure, testable `is_excluded`.
#[cfg(windows)]
pub(crate) unsafe fn foreground_stem() -> String {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    let hwnd = GetForegroundWindow();
    if hwnd.0.is_null() {
        return String::new();
    }

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return String::new();
    }
    process_stem_for_pid(pid)
}

/// The exe stem for an arbitrary process id, lowercased. Empty when it cannot
/// be read.
///
/// Split out of `foreground_stem` (2026-08-26) so PiP — the release watcher's
/// safety guard and the entry-timing log line — can resolve a name for a
/// window whose pid it already holds, without re-querying the foreground
/// window. Same OpenProcess/QueryFullProcessImageNameW plumbing, same cost:
/// one process-handle open/close plus one kernel string query, deliberately
/// only ever called from poller threads or per-tap paths, never from the
/// keyboard-hook callback (see the header rule).
#[cfg(windows)]
pub(crate) unsafe fn process_stem_for_pid(pid: u32) -> String {
    normalize_stem(&process_path_for_pid(pid))
}

/// The FULL exe path for a process id (1.0.119, brief 4 §2 — the Space
/// ring's centre pill needs the path to find the app's icon in the picker's
/// cache, which is keyed by path). Empty when it cannot be read. The same
/// OpenProcess/QueryFullProcessImageNameW plumbing `process_stem_for_pid`
/// always used; that function is now this one plus `normalize_stem`.
#[cfg(windows)]
pub(crate) unsafe fn process_path_for_pid(pid: u32) -> String {
    use windows::core::PWSTR;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid == 0 {
        return String::new();
    }

    let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
        return String::new();
    };

    let mut buf = [0u16; 260];
    let mut size = buf.len() as u32;
    let pwstr = PWSTR(buf.as_mut_ptr());
    let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_FORMAT(0), pwstr, &mut size);
    let _ = windows::Win32::Foundation::CloseHandle(handle);

    if ok.is_err() {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..size as usize])
}

/// 1.0.119 (brief 4 §2) — the foreground window as the Space ring's centre
/// pill needs it: the owning exe's full path and stem, plus the window
/// CLASS (the only cheap way to tell the desktop — `Progman` / `WorkerW`,
/// owned by explorer.exe — from an Explorer file window). `None` when there
/// is no foreground window or its process cannot be read; the caller shows
/// "SPACE" for both. Never called from the hook callback (header rule) —
/// the engine's hold-start path only, which already queries the foreground.
#[cfg(windows)]
pub(crate) unsafe fn foreground_info() -> Option<crate::engine::focus::ForegroundInfo> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId,
    };

    let hwnd = GetForegroundWindow();
    if hwnd.0.is_null() {
        return None;
    }
    let mut cls = [0u16; 128];
    let n = GetClassNameW(hwnd, &mut cls);
    let class = String::from_utf16_lossy(&cls[..n.max(0) as usize]);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return None;
    }
    let path = process_path_for_pid(pid);
    if path.is_empty() {
        return None;
    }
    Some(crate::engine::focus::ForegroundInfo { stem: normalize_stem(&path), path, class })
}

#[cfg(test)]
mod tests {
    use super::{is_excluded, normalize_stem};

    /// House rule: test the pure logic a user only reaches after something
    /// else has gone wrong. Every one of these forms is something that HAS
    /// arrived here — a full path from `QueryFullProcessImageNameW`, a bare
    /// file name from the Start-Menu scanner, a stem from our own config, and
    /// a hand-edited config entry.
    #[test]
    fn stem_normalisation_accepts_every_form() {
        assert_eq!(
            normalize_stem("C:\\Program Files\\Adobe\\Photoshop.exe"),
            "photoshop"
        );
        assert_eq!(normalize_stem("C:/Program Files/Blender/blender.EXE"), "blender");
        assert_eq!(normalize_stem("Figma.exe"), "figma");
        assert_eq!(normalize_stem("photoshop"), "photoshop");
        assert_eq!(normalize_stem("  \"C:\\a\\b\\Krita.exe\"  "), "krita");
        assert_eq!(normalize_stem(""), "");
        // An exe whose NAME contains ".exe" must not lose the middle of it.
        assert_eq!(normalize_stem("my.exe.tool.exe"), "my.exe.tool");
    }

    /// PROBLEM 218 — the app must never be able to stand itself down.
    ///
    /// Every form below is one that HAS reached `excluded_apps`: the picker's
    /// normalised stem, a full path from the Start-Menu scanner, and a
    /// hand-edited entry with the extension left on. All three must be
    /// dropped, because honouring any of them produces the owner's exact
    /// report — nothing works while Spaceadom's own window has focus.
    #[test]
    fn spaceadom_can_never_exclude_itself() {
        let own = "spaceadom";
        for form in [
            "spaceadom",
            "spaceadom.exe",
            "C:\\Users\\beamu\\AppData\\Local\\Spaceadom\\spaceadom.exe",
            "SPACEADOM.EXE",
        ] {
            let list = vec![super::normalize_stem(form)];
            let (kept, dropped) = super::without_self(list, own);
            assert!(
                kept.is_empty() && dropped.len() == 1,
                "{form} must be dropped from the exception list"
            );
        }
    }

    /// The guard must be NARROW. Dropping anything else would silently delete
    /// an exception the user deliberately set — the opposite failure, and just
    /// as invisible.
    #[test]
    fn self_exclusion_guard_touches_nothing_else() {
        let list = vec![
            "photoshop".to_string(),
            "spaceadom".to_string(),
            "blender".to_string(),
            // Not us: a different app whose name merely contains ours.
            "spaceadom-helper".to_string(),
        ];
        let (kept, dropped) = super::without_self(list, "spaceadom");
        assert_eq!(dropped, vec!["spaceadom".to_string()]);
        assert_eq!(
            kept,
            vec![
                "photoshop".to_string(),
                "blender".to_string(),
                "spaceadom-helper".to_string()
            ]
        );
    }

    /// A failure to read our own exe name must not turn the guard into a
    /// no-op that quietly passes everything, NOR into a filter that eats the
    /// whole list. `own_stem()` never returns empty, but this pins the
    /// behaviour of the pure function if it ever did.
    #[test]
    fn an_empty_own_stem_drops_nothing() {
        let list = vec!["photoshop".to_string(), "".to_string()];
        let (kept, dropped) = super::without_self(list.clone(), "");
        assert_eq!(kept, list);
        assert!(dropped.is_empty());
    }

    #[test]
    fn matching_is_case_and_form_insensitive() {
        let list = vec![
            "photoshop".to_string(),
            "D:\\Games\\Blender.exe".to_string(),
        ];
        assert!(is_excluded("C:\\Program Files\\Adobe\\PHOTOSHOP.EXE", &list));
        assert!(is_excluded("blender.exe", &list));
        assert!(!is_excluded("C:\\Windows\\explorer.exe", &list));
        // An empty foreground name is "could not read it", NOT "matches the
        // empty entry" — failing the other way would stand the app down on
        // every window it cannot query.
        assert!(!is_excluded("", &list));
        assert!(!is_excluded("photoshop", &[]));
    }

    /// PROBLEM 267 — the three scopes, the built-in fallback, and who wins.
    /// Returns `(space_off, middle_off, from_user_row)`.
    #[test]
    fn scope_resolution_user_row_wins_then_builtin_then_nothing() {
        use crate::config::ExceptionScope::*;
        let rows = vec![
            ("photoshop".to_string(), OffEntirely),
            ("game".to_string(), MiddleOnly),
            ("kicad".to_string(), SpaceOnly),
            ("sldworks".to_string(), MiddleOnly), // a built-in the user changed
        ];
        // Off entirely: both triggers stand down.
        assert_eq!(super::resolve_scope("Photoshop.exe", &rows, false), (true, true, true));
        // Middle only: Space passes through, the ring stays.
        assert_eq!(super::resolve_scope("game", &rows, false), (true, false, true));
        // Space only: Space works, the middle button is the app's.
        assert_eq!(super::resolve_scope("C:\\KiCad\\kicad.exe", &rows, true), (false, true, true));
        // The user's row beats the built-in table: SolidWorks moved to
        // Middle only gets its ring back and loses Space, as he asked.
        assert_eq!(super::resolve_scope("sldworks", &rows, true), (true, false, true));
        // A built-in with no user row: Space only, and NOT from a user row —
        // that is what lets ORBIT_ACTIVE keep owning the verdict.
        assert_eq!(super::resolve_scope("blender", &rows, true), (false, true, false));
        // Nothing listed anywhere: nothing stands down.
        assert_eq!(super::resolve_scope("explorer", &rows, false), (false, false, false));
        // Unreadable foreground: never a stand-down, from either list.
        assert_eq!(super::resolve_scope("", &rows, true), (false, false, false));
    }
}
