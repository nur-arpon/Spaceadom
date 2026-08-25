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

/// Publish the config's exception list for the poller.
///
/// PROBLEM 180 — MUST be called from BOTH the startup config load in `lib.rs`
/// AND `config::save`. An atomic (or, here, a list) that starts empty and is
/// only fed on save means the feature is dead from launch until the user
/// happens to save something.
pub fn publish_excluded_apps(cfg: &crate::config::AppConfig) {
    let list: Vec<String> = cfg
        .excluded_apps
        .iter()
        .map(|s| normalize_stem(s))
        .filter(|s| !s.is_empty())
        .collect();
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
                    let detected = !list.is_empty() && is_excluded(&name, &list);
                    crate::hook::EXCLUDED_ACTIVE.store(detected, Ordering::Relaxed);

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
unsafe fn foreground_stem() -> String {
    use windows::core::PWSTR;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
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
    normalize_stem(&String::from_utf16_lossy(&buf[..size as usize]))
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
}
