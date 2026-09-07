/// commands.rs — All Tauri IPC command handlers (frontend → Rust).
///
/// Every function here is registered in lib.rs via `invoke_handler![]` and
/// callable from TypeScript via `invoke("command_name", { ...args })`.

use crate::{
    browser,
    config::{self, AppConfig, ConflictResult, HookStatus, Profile, SharedConfig},
    hook::{self, FULLSCREEN_ACTIVE},
    icon_extractor,
    startup,
};
use std::sync::{atomic::Ordering, Arc, Mutex};
use tauri::{Emitter, State};

// ---------------------------------------------------------------------------
// Shared state types exposed to commands via Tauri's managed state system
// ---------------------------------------------------------------------------

/// PROBLEM 112 — logical screen rect of the overlay window after a fit.
///
/// The warp handover animates a pill flying between its slot and the SPACE key,
/// but the WINDOW itself moves at the same time and that move is instant and
/// un-animatable on Win32. The frontend therefore has to convert a pill's
/// position from before the move into the coordinate space after it, which
/// needs both rects — so every fit hands back where it actually landed.
#[derive(serde::Serialize, Clone, Copy, Debug)]
pub struct OverlayRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub struct ConfigState(pub SharedConfig);
pub struct IconCacheState(pub Arc<Mutex<std::collections::HashMap<String, String>>>);

// ---------------------------------------------------------------------------
// Config commands
// ---------------------------------------------------------------------------

/// Return the full application configuration to the frontend.
#[tauri::command]
pub fn get_config(state: State<'_, ConfigState>) -> AppConfig {
    state.0.read().unwrap_or_else(|p| p.into_inner()).clone()
}

/// Persist a new configuration block from the frontend.
#[tauri::command]
pub fn save_config(
    app: tauri::AppHandle,
    new_config: AppConfig,
    state: State<'_, ConfigState>,
) -> Result<(), String> {
    // DEBUG: Log what the frontend is sending us
    let active = &new_config.active_profile;
    if let Some(profile) = new_config.profiles.iter().find(|p| p.name == *active) {
        for (key, binding) in &profile.bindings {
            if binding.is_mapped() {
                println!("[SAVE_CONFIG] key={} app={:?} url={:?} label={:?}",
                    key, binding.app, binding.web_url, binding.label);
            }
        }
    }
    // Update shared state
    *state.0.write().unwrap_or_else(|p| p.into_inner()) = new_config.clone();
    // Sync rollover_ms to hook atomic
    hook::ROLLOVER_MS.store(new_config.rollover_ms, Ordering::Relaxed);
    // PROBLEM 119 — the opacity floor slider wrote to config and nothing
    // read it. Pushed here so the change takes effect on the next scroll,
    // not on the next launch.
    crate::engine::actions::opacity::OPACITY_FLOOR_PCT
        .store(new_config.opacity_floor_pct, Ordering::Relaxed);

    // THEME RULE: one setting drives the dashboard AND the overlay. The
    // overlay is a separate webview, so it only learns about a theme/sound
    // change through an event. Emitted from Rust deliberately — a GLOBAL
    // `emit` with a single listener (the overlay page) is the only
    // arrangement that has ever delivered in this app; `emit_to` and
    // webview-to-webview emits are not trusted here.
    {
        use tauri::Emitter;
        let _ = app.emit("theme-changed", new_config.dark_mode);
        // PROBLEM 185 — the overlay needs the theme's NAME, not just "is it
        // dark". `dark_mode` is true for BOTH warcry and starry (they share a
        // nocturne base and each re-tints on top), so a boolean cannot tell
        // them apart and the overlay wore starry's palette in warcry — the
        // owner's report: "for the warcry theme the guide hud and toasts
        // colour was not matched, it's still using the ones from starry night".
        let _ = app.emit("theme-name-changed", new_config.theme.clone());
        let _ = app.emit("sound-changed", new_config.sound_enabled);
        // PROBLEM 174 — the guide-to-toast flight lives entirely in the OVERLAY
        // page, so the switch in the dashboard's Settings panel can only reach
        // it through here. Same global-emit rule as the two above.
        let _ = app.emit("flight-changed", new_config.hud_toast_flight);
        // The HUD's band count lives entirely in the OVERLAY page — the ring
        // arithmetic runs against MEASURED label widths, which exist nowhere
        // else — so the Settings pill can only reach it through here. Same
        // global-emit rule as the four above; `emit_to` has never delivered in
        // this app.
        //
        // THIS IS ONLY HALF THE WIRING. An event that fires on CHANGE leaves a
        // freshly-created overlay (first launch, or a display-change rebuild)
        // carrying whatever the module defaulted to, so `overlay.ts` ALSO
        // seeds `hud_band_count` from `get_config` on load. Both halves are
        // required — that is the rule CLAUDE.md records for the theme, and the
        // half that gets skipped is always this second one.
        let _ = app.emit("hud-band-count-changed", new_config.hud_band_count.clone());
        // The Magnetic Sector ring vs the classic 1.0.88 ring. Same story as
        // the band count directly above: the layout is chosen inside the
        // OVERLAY page, so the Settings toggle can only reach it from here,
        // and it is a GLOBAL `emit` because `emit_to` has never delivered in
        // this app.
        //
        // AND THE SAME SECOND HALF: `overlay.ts` also seeds
        // `hud_magnetic_layout` from `get_config` on load, because an event
        // that fires on CHANGE leaves a freshly-created overlay (first
        // launch, or a display-change rebuild) drawing whatever the module
        // defaulted to. Both halves are required; this one is never the half
        // that gets skipped.
        let _ = app.emit("hud-layout-changed", new_config.hud_magnetic_layout);
    }

    // Persist to disk
    config::save(&new_config)
}

/// Return just the list of profile names and their binding counts.
#[tauri::command]
pub fn get_profiles(state: State<'_, ConfigState>) -> Vec<serde_json::Value> {
    state
        .0
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .profiles
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": p.name,
                "binding_count": p.bindings.values().filter(|b| b.is_mapped()).count(),
            })
        })
        .collect()
}

/// Switch the active profile by name.
#[tauri::command]
pub fn set_active_profile(
    name: String,
    state: State<'_, ConfigState>,
) -> Result<(), String> {
    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    if !cfg.profiles.iter().any(|p| p.name == name) {
        return Err(format!("Profile '{name}' not found"));
    }
    cfg.active_profile = name;
    let snapshot = cfg.clone();
    drop(cfg);
    config::save(&snapshot)
}

// ---------------------------------------------------------------------------
// Icon extraction command
// ---------------------------------------------------------------------------

/// Extract and base64-encode the icon for an executable path.
/// Results are cached per-path to avoid repeated Win32 calls.
#[tauri::command]
pub fn extract_icon_cmd(
    exe_path: String,
    cache: State<'_, IconCacheState>,
) -> Option<String> {
    {
        let lock = cache.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(cached) = lock.get(&exe_path) {
            return Some(cached.clone());
        }
    }

    // Also try resolving via smart_cascade if it's just an exe name
    let resolved = if std::path::Path::new(&exe_path).is_absolute() {
        exe_path.clone()
    } else {
        crate::engine::actions::smart_cascade::resolve_path(&exe_path)
            .unwrap_or(exe_path.clone())
    };

    let result = icon_extractor::extract_icon(&resolved);
    if let Some(ref b64) = result {
        cache.0.lock().unwrap_or_else(|p| p.into_inner()).insert(exe_path, b64.clone());
    }
    result
}

// ---------------------------------------------------------------------------
// File picker command
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct AppInfo {
    pub name: String,
    pub path: String,
    pub icon_base64: Option<String>,
}

/// Scan both Start Menu trees plus `shell:AppsFolder` and return every
/// bindable application, icons included.
///
/// PROBLEM 237 — `async`, and the work is not even on the async runtime: it
/// goes to `picker_worker`'s dedicated STA thread, which answers from session
/// memory or the disk cache in milliseconds and scans (PowerShell + ~247
/// in-process `IShellItemImageFactory` extractions, 6-17 s measured) only when
/// neither exists — and then in the background wherever it can. The main
/// thread never blocks on this again.
///
/// PROBLEM 205 kept this synchronous because "COM has never once executed off
/// the main thread in this app". It has now, with proof:
/// `picker_worker::tests::icons_extract_on_a_non_main_sta_thread` extracts 50
/// real shortcuts' icons on a fresh STA thread with no message pump and
/// asserts zero HRESULT failures. The shape PROBLEM 205 measured still holds:
/// an async command that borrows `State<'_>` must return a `Result`, and the
/// frontend's `invoke<AppInfo[]>("list_start_menu_apps")` is unaffected
/// because Tauri resolves the promise with the `Ok` value.
///
/// A refresh that changes the list emits `picker-data-updated`
/// (`picker_worker::UPDATED_EVENT`); re-invoke this to get the new list.
#[tauri::command]
pub async fn list_start_menu_apps(
    app: tauri::AppHandle,
    cache: State<'_, IconCacheState>,
) -> Result<Vec<AppInfo>, String> {
    let cache = Arc::clone(&cache.0);
    crate::picker_worker::apps(app, cache).await
}

/// The best starting point for picking an application, measured on a real
/// machine 2026-08-13:
///
/// | Location                  | What the user browses                    |
/// |---------------------------|------------------------------------------|
/// | Start Menu (all users)    | 151 shortcuts, one per app, human-named  |
/// | Start Menu (this user)    | 59 shortcuts                             |
/// | Program Files + (x86)     | 68 folders hiding 1567 .exe files        |
///
/// Program Files is the wrong answer twice over: the real executable is
/// buried (`Google\Chrome\Application\chrome.exe`) among updaters and crash
/// handlers, and it MISSES every per-user install — on this machine that is
/// VS Code, Ollama, Python and Antigravity, none of which appear under
/// Program Files at all.
///
/// The Start Menu is split across two roots and neither contains everything,
/// so this is a starting point, not a complete list. The editor's search box
/// remains the complete one: it scans BOTH roots plus Store/UWP apps.
#[cfg(windows)]
fn default_browse_dir() -> Option<std::path::PathBuf> {
    let candidates = [
        std::env::var("ProgramData")
            .ok()
            .map(|p| std::path::PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs")),
        std::env::var("APPDATA")
            .ok()
            .map(|p| std::path::PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs")),
    ];
    candidates.into_iter().flatten().find(|p| p.is_dir())
}

/// Open a native Windows file-open dialog and return the chosen path.
#[tauri::command]
pub async fn pick_file(
    app: tauri::AppHandle,
    filter_name: Option<String>,
    filter_ext: Option<Vec<String>>,
) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;

    let mut builder = app.dialog().file();

    if let (Some(name), Some(exts)) = (filter_name, filter_ext) {
        builder = builder.add_filter(name, &exts.iter().map(String::as_str).collect::<Vec<_>>());
    }

    // PROBLEM 96 — ALWAYS start at the Start Menu, every single time.
    //
    // Previously the dialog opened wherever Windows last left it, which for
    // most people is Downloads: a folder full of installers, where `setup.exe`
    // looks exactly as bindable as the real program.
    //
    // A first version of this fix remembered the last-browsed folder for the
    // session. The user rejected that, and they are right: the whole point is
    // that the button lands somewhere with APPLICATIONS in it. "Remembering"
    // means one detour into Downloads silently makes every later browse start
    // there again — the button quietly stops doing the thing it was fixed to
    // do, and nothing tells the user why.
    #[cfg(windows)]
    if let Some(dir) = default_browse_dir() {
        builder = builder.set_directory(&dir);
    }

    builder.blocking_pick_file().map(|p| p.to_string())
}

/// PROBLEM 96 — reject a file that is an INSTALLER rather than an application.
///
/// The picker filters to `.exe` and `.lnk`, which is correct — but `setup.exe`
/// IS an `.exe`, so it appears just as valid as the real program. A user who
/// binds one gets the installer re-running on every Space+key press; binding
/// `unins000.exe` is worse still.
///
/// Returns `Some(reason)` when the path should be refused, `None` when it is
/// fine. Matching is deliberately CONSERVATIVE — whole stems and known
/// prefixes only — because a false positive blocks a legitimate app and the
/// user has no override. "Photoshop.exe" must never trip the "shop" in a
/// substring search, which is why there is no bare `contains` on short words.
/// PROBLEM 96 — split a filename stem into WORDS.
///
/// Boundaries: any non-alphanumeric, a lower→upper transition (camelCase), and
/// a letter→digit transition. So "AppSetup" → ["app","setup"], "setup_x64" →
/// ["setup","x","64"], "setupapi" stays one token, and "Wizard101" →
/// ["wizard","101"] (which is why it is not mistaken for the word "wizard"…
/// and why "wizard" is matched on the whole stem instead).
fn tokenize_stem(stem: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev: Option<char> = None;
    for c in stem.chars() {
        let boundary = match prev {
            Some(p) => {
                !c.is_alphanumeric()
                    || (p.is_lowercase() && c.is_uppercase())
                    || (p.is_alphabetic() && c.is_numeric())
                    || (p.is_numeric() && c.is_alphabetic())
            }
            None => !c.is_alphanumeric(),
        };
        if boundary && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if c.is_alphanumeric() {
            cur.push(c.to_ascii_lowercase());
        }
        prev = Some(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[tauri::command]
pub fn check_app_path(path: String) -> Option<String> {
    // Keep the ORIGINAL case for tokenising and lowercase only for whole-stem
    // comparisons. Lowercasing first destroys the camelCase boundary, which
    // silently cost "AppSetup.exe" and "SetupWizard.exe" — caught by testing
    // the rules against real filenames before shipping them.
    let raw_stem = std::path::Path::new(&path)
        .file_stem()?
        .to_string_lossy()
        .to_string();
    let stem = raw_stem.to_lowercase();

    // Uninstallers: Inno Setup writes unins000.exe, unins001.exe, …
    // MESSAGES MUST FIT THE TOAST. `.st-toast` is a single-line pill —
    // `white-space: nowrap`, `overflow: hidden`, `max-width: 560px` — which at
    // 13px leaves room for roughly 65 characters. Longer text is silently
    // CLIPPED with no ellipsis, which the user reported. The pill's dot→pill
    // morph is the confirmed-correct overlay design (CLAUDE.md), so the
    // message fits the surface rather than the surface being rebuilt.
    if stem.starts_with("unins") || stem == "uninstall" || stem == "uninstaller" {
        return Some("That's an uninstaller — pick the app's own shortcut".into());
    }
    // PROBLEM 193 — `stem == "uninstall"` only ever matched a shortcut named
    // EXACTLY "Uninstall.lnk". The one every real installer actually writes is
    // "Uninstall <App Name>.lnk" — Windows' own naming convention, and it
    // slipped straight through this check. Live incident: the owner picked
    // "Uninstall PASCO Capstone" from the app grid (Start Menu \ Tools\)
    // believing it was the app itself, and Space+key would have re-run the
    // uninstaller on every press had he not caught it.
    //
    // Checking the FIRST TOKEN, not a substring: "uninstall" appearing
    // anywhere would also catch a legitimate app that happens to have the
    // word in its own name (rare, but "install"'s whole-stem-only rule above
    // exists for exactly that reason — "InstallShield Player"). Windows'
    // convention always puts "Uninstall" first, so anchoring there keeps the
    // same conservative promise this function documents: whole words, known
    // positions, never a bare substring.
    let tokens_for_uninstall = tokenize_stem(&raw_stem);
    if tokens_for_uninstall.first().is_some_and(|t| t == "uninstall") {
        return Some("That's an uninstaller — pick the app's own shortcut".into());
    }

    // Installers. The first version matched whole stems plus `-setup`/`_setup`
    // suffixes, and the user immediately found the hole: `setup_x64.exe`,
    // `AppSetup.exe` and `installer 2.exe` all sailed through while plain
    // `installer.exe` was caught.
    //
    // Tokenising is what actually works. Split the stem on non-alphanumerics,
    // on camelCase boundaries, and between letters and digits, then look for
    // whole WORDS. That catches every real-world spelling without resorting to
    // a substring search, which would wrongly reject "setupapi_viewer.exe"
    // (token "setupapi", not "setup") and "Wizard101.exe".
    let tokens = tokenize_stem(&raw_stem);
    let has = |w: &str| tokens.iter().any(|t| t == w);

    // "install" is deliberately NOT a token match — it appears inside
    // legitimate names like "InstallShield Player". Only the whole stem being
    // exactly "install" is conclusive.
    let is_installer = has("setup")
        || has("installer")
        || stem == "install"
        || stem == "msiexec"
        || stem == "wizard"
        || stem.starts_with("vcredist")
        || stem.starts_with("dotnetfx");
    if is_installer {
        return Some("That's an installer — pick the app's own shortcut".into());
    }

    // Support processes that live beside the real executable and start
    // nothing useful on their own.
    let is_helper = stem.ends_with("update")
        || stem.ends_with("updater")
        || stem.ends_with("crashhandler")
        || stem == "crashpad_handler"
        || stem == "elevate"
        || stem == "squirrel";
    if is_helper {
        return Some("That's a background helper, not the app itself".into());
    }

    None
}

// ---------------------------------------------------------------------------
// Hook status command
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_hook_status(state: State<'_, ConfigState>) -> HookStatus {
    use crate::hook::BYPASS_MODE;
    use std::sync::atomic::Ordering;
    
    HookStatus {
        // PROBLEM 66 — this was hardcoded `true`, so the dashboard claimed a
        // working hook even when SetWindowsHookExW had failed outright.
        installed: crate::hook::HOOK_INSTALLED.load(Ordering::Relaxed),
        bypass_active: BYPASS_MODE.load(Ordering::Relaxed),
        fullscreen_suppressed: FULLSCREEN_ACTIVE.load(Ordering::Relaxed),
        active_profile: state.0.read().unwrap_or_else(|p| p.into_inner()).active_profile.clone(),
    }
}

/// What Windows' hook-eviction timeout is set to, and how often we have been
/// evicted this session.
#[derive(serde::Serialize)]
pub struct HookHealth {
    /// Current `LowLevelHooksTimeout` in ms. `None` = the value is absent, which
    /// means Windows uses its 300 ms default.
    pub timeout_ms: Option<u32>,
    /// True once the value is at least `RECOMMENDED_HOOK_TIMEOUT_MS`.
    pub raised: bool,
    /// Watchdog reinstalls since the app started — each one is a stretch during
    /// which shortcuts did not work.
    pub evictions: u32,
    /// Other keyboard-hook programs detected, for naming a likely culprit.
    pub rivals: Vec<String>,
}

/// 5 seconds. Windows' default is 300 ms, and it is a HARD deadline: if a
/// low-level hook callback has not returned within it, Windows silently
/// unhooks you — no error, no event, the hook simply stops firing.
///
/// Our callback is microseconds of work. It overruns anyway when another
/// low-level hook sits ahead of us in the chain and is slow, because the
/// timeout is measured across the chain. With PowerToys and spacedesk both
/// installed on this machine (see the conflicts detector), that is routine
/// rather than exceptional.
///
/// 5000 is generous but not reckless: the value bounds how long a WEDGED hook
/// can stall input system-wide, and 5 s is the figure AutoHotkey's own
/// documentation has recommended for this exact problem for years.
// 1000, not the internet's folk-standard 5000 — the owner's call, 2026-08-25,
// and the reasoning is his: this limit is MACHINE-WIDE, so if ANY hooked app
// (Spaceadom, PowerToys, spacedesk) genuinely hangs, the keyboard stalls for
// the full limit before Windows evicts it. A possible 5-second system-wide
// freeze is too high a price for surviving stalls that 1s already covers.
// If HOOK_EVICTIONS_TOTAL shows 1s still is not enough on this machine, step
// to 2s — from the counter's data, never from folklore.
pub const RECOMMENDED_HOOK_TIMEOUT_MS: u32 = 1000;

/// Read the eviction timeout and the session's eviction count.
#[tauri::command]
pub fn get_hook_health() -> HookHealth {
    use std::sync::atomic::Ordering;
    let timeout_ms = read_hook_timeout();
    HookHealth {
        timeout_ms,
        raised: timeout_ms.is_some_and(|v| v >= RECOMMENDED_HOOK_TIMEOUT_MS),
        evictions: crate::hook::HOOK_EVICTIONS_TOTAL.load(Ordering::Relaxed),
        rivals: crate::hook::conflicts::detect()
            .into_iter()
            .map(|c| c.product)
            .collect(),
    }
}

#[cfg(windows)]
fn read_hook_timeout() -> Option<u32> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Control Panel\\Desktop")
        .ok()?;
    // The value is conventionally a DWORD but has historically also been
    // written as a string by other tools; accept either rather than reporting
    // "not set" for a value that is plainly there.
    key.get_value::<u32, _>("LowLevelHooksTimeout")
        .ok()
        .or_else(|| {
            key.get_value::<String, _>("LowLevelHooksTimeout")
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
        })
}

#[cfg(not(windows))]
fn read_hook_timeout() -> Option<u32> {
    None
}

/// Raise `LowLevelHooksTimeout` so Windows stops evicting our keyboard hook.
///
/// PROBLEM 173. This is the only change that addresses the CAUSE rather than
/// the symptom: the watchdog can only notice an eviction and re-hook, and the
/// user is deaf until it does.
///
/// FOUR PROPERTIES, ALL DELIBERATE:
///
/// 1. **HKCU only** — `HKEY_CURRENT_USER\Control Panel\Desktop`, this user's
///    own hive. No elevation, no UAC, and it cannot affect anyone else who
///    signs in to this PC. Same rule the autostart entry follows.
/// 2. **Never automatic.** It is a Windows setting, not ours. It is offered in
///    Settings, with what it does spelled out, and only ever written when the
///    user presses the button. A background app quietly editing Control Panel
///    keys is exactly the behaviour that gets an app removed from the Store.
/// 3. **Reversible.** `restore` puts back whatever was there, deleting the
///    value when there was none, so "undo" means the machine's original state
///    and not our idea of a default.
/// 4. **Honest about the sign-out.** Windows reads this at logon. Nothing
///    changes until the user signs out and back in, and saying so is the whole
///    difference between a setting that works and one they think is broken.
#[tauri::command]
pub fn set_hook_timeout(raise: bool) -> Result<String, String> {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
        use winreg::RegKey;
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags("Control Panel\\Desktop", KEY_SET_VALUE)
            .map_err(|e| format!("Could not open your Control Panel settings: {e}"))?;

        if raise {
            key.set_value("LowLevelHooksTimeout", &RECOMMENDED_HOOK_TIMEOUT_MS)
                .map_err(|e| format!("Could not save the setting: {e}"))?;
            log::info!(
                "hook timeout: raised LowLevelHooksTimeout to {RECOMMENDED_HOOK_TIMEOUT_MS}ms in \
                 HKCU at the user's request — takes effect at next sign-in"
            );
            Ok(format!(
                "Done — Windows will now give keyboard shortcuts {}s instead of 0.3s before \
                 giving up on them. Sign out and back in for it to take effect.",
                RECOMMENDED_HOOK_TIMEOUT_MS / 1000
            ))
        } else {
            match key.delete_value("LowLevelHooksTimeout") {
                Ok(()) => {
                    log::info!("hook timeout: removed LowLevelHooksTimeout — back to Windows' 300ms default");
                    Ok("Put back the way Windows had it. Sign out and back in to apply.".into())
                }
                // Already absent is the state being asked for, not a failure.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    Ok("It was already at the Windows default — nothing to undo.".into())
                }
                Err(e) => Err(format!("Could not undo the setting: {e}")),
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = raise;
        Err("Only applies on Windows".into())
    }
}

/// Toggle the global bypass mode from the frontend UI
#[tauri::command]
pub fn toggle_bypass(app: tauri::AppHandle) -> bool {
    use crate::hook::BYPASS_MODE;
    use std::sync::atomic::Ordering;
    
    let new_state = !BYPASS_MODE.load(Ordering::Relaxed);
    BYPASS_MODE.store(new_state, Ordering::Relaxed);

    // 1.0.96 — PUBLISH THE CHANGE. This one line is the whole reason the tray's
    // new Pause/Resume item can tell the truth.
    //
    // There are three ways to flip this state — the Settings row (here), the
    // Space+Backslash special key (`engine::handle_bypass_toggle`) and the tray
    // menu (`tray::toggle_engine`) — and the other two already emit
    // `bypass-toggled`, which is what `main.ts` drives the Settings switch from.
    // This one never did, because until now nothing outside the caller needed
    // to know: the panel updated itself from the returned bool. A tray item that
    // is not told cannot relabel itself, and a tray that says "Pause Spaceadom"
    // while the engine is already paused is the one place a user checks to find
    // out whether the app is on.
    //
    // KNOWN GAP, recorded rather than fixed here: unlike the engine's handler,
    // this path does NOT clear `MODIFIER_ACTIVE`, so pausing from the dashboard
    // during a held Space can leave the modifier latched. Both should call one
    // shared flip; that consolidation belongs with whoever owns this region
    // next (see V14_FIXES_AND_CODE.md, the tray Pause/Resume entry).
    let _ = app.emit("bypass-toggled", new_state);

    crate::show_toast(
        &app,
        if new_state { "⏸ Spaceadom Paused" } else { "▶ Spaceadom Active" }
    );

    new_state
}

/// PROBLEM 99 — the single-step undo buffer for destructive actions.
///
/// "Clear all" and "Reset this profile" are two-click armed buttons, and
/// deleting a profile asks for confirmation — but a confirmation is not a
/// safety net. It is asked BEFORE the user can see what they are about to
/// lose, and for a profile the user built themselves there is no factory
/// version to restore: those bindings and custom icons exist nowhere else.
///
/// Holds the WHOLE config as it was immediately before the action, so undo is
/// exact rather than a reconstruction. One deep only: an undo stack invites
/// the question "how far back am I?", and the failure this exists for is the
/// mis-click you notice within seconds.
/// PROBLEM 107 — a STACK, not a single slot.
///
/// It was one-deep, and the second destructive action silently overwrote the
/// first: delete A (undo armed), delete B a few seconds later, and the buffer
/// now held "the state before B" — which already had A missing. Undo brought
/// back B and lost A permanently, with nothing saying so. Newest last; each
/// entry carries its OWN deadline, so a 10s user-profile undo and a 30s
/// fallback undo can be pending at the same time and expire independently.
static UNDO_STACK: std::sync::Mutex<Vec<(u64, u64, String, AppConfig)>> =
    std::sync::Mutex::new(Vec::new());
/// Enough for a run of mis-clicks; the backup ring is the deeper net.
const UNDO_STACK_MAX: usize = 10;

/// How long an undo stays available. Matches the user's request.
/// PROBLEM 106 — how long an undo stays available, scaled to how much the
/// action costs to rebuild.
///
/// A profile the user just made by hand is cheap to recreate; the stock
/// profiles carry 26 curated bindings each; the fallback additionally breaks
/// every OTHER profile's unassigned keys and needs the longest explanation.
const UNDO_WINDOW_USER_MS: u64 = 10_000;
const UNDO_WINDOW_STOCK_MS: u64 = 20_000;
const UNDO_WINDOW_FALLBACK_MS: u64 = 30_000;
/// Non-delete actions (clear all, reset) sit in the middle.
const UNDO_WINDOW_MS: u64 = 20_000;

/// PROBLEM 106 — the undo window a profile deserves, by what it costs to lose.
fn undo_window_for_profile(name: &str) -> u64 {
    if name == crate::config::schema::FALLBACK_PROFILE {
        UNDO_WINDOW_FALLBACK_MS
    } else if crate::config::defaults::generate().iter().any(|p| p.name == name) {
        UNDO_WINDOW_STOCK_MS
    } else {
        UNDO_WINDOW_USER_MS
    }
}

fn stash_undo(label: &str, cfg: &AppConfig) {
    stash_undo_for(label, cfg, UNDO_WINDOW_MS);
}

/// PROBLEM 106 — same, with an explicit window for actions that need longer.
fn stash_undo_for(label: &str, cfg: &AppConfig, window_ms: u64) {
    let mut st = UNDO_STACK.lock().unwrap_or_else(|p| p.into_inner());
    let now = crate::hook::tick_count_pub();
    // Drop anything already expired so the stack cannot grow on dead entries.
    st.retain(|(ts, w, _, _)| now.saturating_sub(*ts) <= *w);
    st.push((now, window_ms, label.to_string(), cfg.clone()));
    while st.len() > UNDO_STACK_MAX {
        st.remove(0);
    }
}

/// What the dashboard shows in the undo banner, or `None` when nothing is
/// undoable. Also used to expire the offer, so the banner cannot outlive the
/// buffer and offer an undo that would silently do nothing.
#[tauri::command]
pub fn undo_available() -> Option<(String, u64)> {
    let mut st = UNDO_STACK.lock().unwrap_or_else(|p| p.into_inner());
    let now = crate::hook::tick_count_pub();
    st.retain(|(ts, w, _, _)| now.saturating_sub(*ts) <= *w);
    let (ts, window, label, _) = st.last()?;
    let elapsed = now.saturating_sub(*ts);
    // PROBLEM 106 — return the REMAINING seconds so the banner counts down
    // from the real deadline. It used to hardcode 10, which would now be a
    // lie for every action and would hide the offer while it was still valid.
    Some((label.clone(), (window - elapsed).div_ceil(1000)))
}

/// PROBLEM 99 — restore the config as it was before the last destructive
/// action. Refuses once the window has passed rather than silently restoring
/// something the user has since built on top of.
#[tauri::command]
pub fn undo_last_change(
    state: State<'_, ConfigState>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    // PROBLEM 107 — pop the MOST RECENT still-valid entry, so a run of
    // deletes undoes in reverse order instead of the older ones vanishing.
    let taken = {
        let mut st = UNDO_STACK.lock().unwrap_or_else(|p| p.into_inner());
        let now = crate::hook::tick_count_pub();
        st.retain(|(ts, w, _, _)| now.saturating_sub(*ts) <= *w);
        st.pop()
    };
    let Some((_ts, _window, label, previous)) = taken else {
        return Err("Nothing left to undo".into());
    };

    *state.0.write().unwrap_or_else(|p| p.into_inner()) = previous.clone();
    config::save(&previous)?;
    crate::hook::ROLLOVER_MS.store(previous.rollover_ms, std::sync::atomic::Ordering::Relaxed);
    // PROBLEM 119 — undo must restore this too, or undoing a settings
    // change silently leaves the old floor in force.
    crate::engine::actions::opacity::OPACITY_FLOOR_PCT
        .store(previous.opacity_floor_pct, std::sync::atomic::Ordering::Relaxed);
    let _ = app.emit("config-updated", previous);
    log::info!("undo: restored the config from before '{label}'");
    Ok(label)
}

/// PROBLEM 99 — clear every binding in the ACTIVE profile, undoably.
///
/// Was done entirely in the frontend by blanking each binding and calling
/// save_config, which left no way back: for a user-created profile those
/// bindings and their custom icons exist in no other copy.
#[tauri::command]
pub fn clear_active_profile(
    state: State<'_, ConfigState>,
    app: tauri::AppHandle,
) -> Result<u32, String> {
    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    stash_undo("Cleared all bindings", &cfg);

    let active = cfg.active_profile.clone();
    let Some(target) = cfg.profiles.iter_mut().find(|p| p.name == active) else {
        return Err(format!("Active profile '{active}' not found"));
    };
    let cleared = target.bindings.len() as u32;
    target.bindings.clear();

    let snapshot = cfg.clone();
    drop(cfg);
    config::save(&snapshot)?;
    let _ = app.emit("config-updated", snapshot);
    log::info!("clear_active_profile: cleared {cleared} binding(s) from '{active}' (undoable)");
    Ok(cleared)
}

/// PROBLEM 109 — put back any MISSING preset profile.
///
/// Deleting a preset used to be a one-way door: nothing in the app could
/// recreate Founders, Gamers or Professionals, so a user who removed one (to
/// tidy up, or just to see what happened) had to rebuild 26 bindings by hand
/// or dig through backups. Losing the FALLBACK profile is worse still, since
/// every other profile's unassigned keys quietly stop working.
///
/// Deliberately ADDITIVE. It restores only what is absent and never touches a
/// preset the user still has — someone who has spent months customising their
/// Founders must not have it silently reverted by a button labelled "restore".
/// That distinction is the whole reason this is not simply reset_config.
#[tauri::command]
pub fn restore_preset_profiles(
    state: State<'_, ConfigState>,
    app: tauri::AppHandle,
) -> Result<Vec<String>, String> {
    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    let factory = crate::config::defaults::generate();

    let missing: Vec<Profile> = factory
        .into_iter()
        .filter(|f| !cfg.profiles.iter().any(|p| p.name == f.name))
        .collect();

    if missing.is_empty() {
        return Ok(Vec::new());
    }

    stash_undo("Restored the preset profiles", &cfg);
    let names: Vec<String> = missing.iter().map(|p| p.name.clone()).collect();

    // Presets belong at the FRONT, in factory order: the fallback lookup and
    // the user's mental model both expect Founders first.
    let mut rebuilt = missing;
    rebuilt.extend(cfg.profiles.drain(..));
    cfg.profiles = rebuilt;

    let snapshot = cfg.clone();
    drop(cfg);
    config::save(&snapshot)?;
    let _ = app.emit("config-updated", snapshot);
    log::info!("restore_preset_profiles: restored {}", names.join(", "));
    Ok(names)
}

/// PROBLEM 92 — reset the ACTIVE PROFILE's bindings to their factory defaults.
///
/// This used to be a whole-config factory reset, reached from a gear-panel
/// button labelled "Reset to defaults" and a frontend function named
/// `resetActiveProfileToDefaults`. One click destroyed: every profile and
/// binding, every custom base64 icon, `special_keys` (which NOTHING in the UI
/// can restore), the fullscreen allowlist, the chosen browser, typing speed —
/// and `overlay_compositing`, the pixel self-test's MEASURED verdict about
/// this machine's GPU. Measured live 2026-08-13 10:51:34: config.json fell
/// from 38819 to 12158 bytes (~26 KB of the user's own data), and the next
/// launch came up in GPU mode with an invisible HUD for 12 minutes.
///
/// It now does what its name and its button say: the active profile only.
/// Stock profiles are restored from `defaults::generate()`; a user-created
/// profile has no factory version, so its bindings are cleared and it keeps
/// its name. Everything else in the config is untouched.
#[tauri::command]
pub fn reset_config(
    state: State<'_, ConfigState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    stash_undo("Reset the profile", &cfg); // PROBLEM 99

    let active = cfg.active_profile.clone();
    let factory = crate::config::defaults::generate();

    let Some(target) = cfg.profiles.iter_mut().find(|p| p.name == active) else {
        return Err(format!("Active profile '{active}' not found"));
    };

    match factory.iter().find(|f| f.name == active) {
        Some(f) => {
            target.bindings = f.bindings.clone();
            log::info!(
                "reset_config: restored stock profile '{active}' to its factory bindings \
                 ({} binding(s)); all other profiles and settings untouched",
                target.bindings.len()
            );
        }
        None => {
            // User-created profile — there is no factory version to restore.
            let had = target.bindings.len();
            target.bindings.clear();
            log::info!(
                "reset_config: '{active}' is a user-created profile with no factory \
                 version — cleared its {had} binding(s), kept the profile"
            );
        }
    }

    let snapshot = cfg.clone();
    drop(cfg);

    let result = config::save(&snapshot);
    let _ = app.emit("config-updated", snapshot);
    result
}

/// PROBLEM 92 — the ONLY way back from a software-rendering verdict.
///
/// `overlay_compositing` is a MEASUREMENT, not a preference: the pixel
/// self-test writes "software" and, by design, never switches back on its own.
/// Now that a factory reset no longer clears it, a single false-positive
/// verdict would strand the user in software rendering forever with no control
/// anywhere in the app. This is that control.
///
/// It also lets a user who KNOWS their machine is affected force software mode
/// immediately, instead of enduring three invisible HUDs first.
///
/// Takes effect at the next launch: WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS is
/// read once, when the WebView2 environment is created (lib.rs step 4b).
///
/// Kept deliberately as a manual escape hatch (invoke from devtools); the
/// ring-fix tool writes through write_compositing instead.
#[tauri::command]
pub fn set_overlay_compositing(
    mode: String,
    state: State<'_, ConfigState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if mode != "auto" && mode != "software" {
        return Err(format!("Unknown compositing mode '{mode}' (expected auto or software)"));
    }
    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    let previous = cfg.overlay_compositing.clone();
    cfg.overlay_compositing = mode.clone();
    let snapshot = cfg.clone();
    drop(cfg);

    config::save(&snapshot)?;
    let _ = app.emit("config-updated", snapshot);
    log::info!(
        "compositing: overlay rendering set to '{mode}' by the user (was '{previous}') — \
         applies at the next launch"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Crash reporting (PROBLEM 195)
// ---------------------------------------------------------------------------

/// The "Don't send logs" switch, from the bottom of Settings.
///
/// **`send_logs` IS THE POSITIVE STATEMENT: true means sending is happening.**
/// The switch the user sees is its negation, so the frontend passes
/// `!checked`. Written out here as well as in `schema.rs` and
/// `settings-panel.ts` because an inverted privacy toggle is the one bug in
/// this app a user could never detect for themselves — it would look correct
/// and do the opposite.
///
/// Goes through its own command rather than `persistConfig()` for the same
/// reason `set_overlay_compositing` does, plus one of its own: the runtime
/// state lives in an ATOMIC that the panic hook reads without taking a lock,
/// and this is the call that flips it. `config::save` republishes it too, so
/// the two can never drift; this command exists so the flip is immediate and
/// unconditional rather than a side effect of a save that might not happen.
///
/// No restart, and no second `sentry::init()`: the very next log record and
/// the very next panic read the new value.
#[tauri::command]
pub fn set_send_logs(
    send_logs: bool,
    state: State<'_, ConfigState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    cfg.send_logs = send_logs;
    let snapshot = cfg.clone();
    drop(cfg);

    // Flip the atomic FIRST. If the disk write fails, the user's stated wish
    // is still honoured for this session — the failure mode of the reverse
    // order is "you asked me to stop and I kept sending until you restarted",
    // which is the one outcome that is not acceptable here.
    crate::telemetry::set_sending_enabled(send_logs);

    config::save(&snapshot)?;
    let _ = app.emit("config-updated", snapshot);
    log::info!(
        "telemetry: user set send_logs={send_logs} — crash and error reports {}",
        if send_logs { "will be sent to Sentry" } else { "will NOT leave this machine" }
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Elevation command
// ---------------------------------------------------------------------------

/// Re-launch the application with UAC elevation (admin privileges).
#[tauri::command]
pub fn restart_elevated() -> bool {
    startup::maybe_relaunch_elevated()
}

/// Diagnostic beacon called by the overlay webview once its JS boots.
/// If this line never appears in the log, the overlay window is not
/// executing its script at all — see overlay.ts probe (2026-08-10).
#[tauri::command]
pub fn overlay_ready() {
    log::info!("overlay: webview JS alive — listener registered");
}

/// Forward overlay-webview JS errors into the Rust log. Webview consoles are
/// invisible in production; without this bridge a JS exception in the overlay
/// fails silently and the HUD just "doesn't appear".
#[tauri::command]
pub fn overlay_log(msg: String) {
    log::warn!("overlay-js: {msg}");
}

/// Same bridge for the DASHBOARD webview. Its console is invisible in a
/// shipped build, so anything a tester needs to report (the resolved visual-
/// effects state, a failed command) has to reach debug.log to be useful.
#[tauri::command]
pub fn frontend_log(msg: String) {
    log::info!("dashboard-js: {msg}");
}

// ---------------------------------------------------------------------------
// PROBLEM 217 — the ERROR-severity siblings of the two bridges above.
//
// WHY SIBLINGS AND NOT A LEVEL PARAMETER. `frontend_log` and `overlay_log` have
// call sites all over `src/` (key-detail-panel, toast, main, overlay), every one
// of them passing a single `msg`. Adding a level argument — even an optional one
// — means either touching all of them or relying on an `Option` that reads as
// "somebody forgot" at every call site. A second command adds nothing to the
// existing ones and changes no existing behaviour: INFO and WARN stay exactly
// where they were, and the new commands are used only by the two global error
// handlers.
//
// WHY IT MATTERS. `SENTRY_MINIMUM_LEVEL` is `Error`, so INFO and WARN never
// leave the machine. Until now that meant no JavaScript failure anywhere in the
// dashboard or the overlay — the entire UI layer — could ever be reported. A
// wedged frontend or a dead HUD is precisely what a friend reports as "it looks
// broken", and it was the one thing the crash reporter could not see.
//
// Both go through `telemetry::report_frontend_error`, which writes the local
// `debug.log` line unconditionally and rate-limits what it submits.
// ---------------------------------------------------------------------------

/// A DASHBOARD JavaScript error. Logged at ERROR, so it reaches the crash
/// reporter — subject, like everything else, to the "Don't send logs" switch.
#[tauri::command]
pub fn frontend_error(msg: String) {
    crate::telemetry::report_frontend_error("dashboard-js", &msg);
}

/// The same for the OVERLAY webview. `overlay_log` stays at WARN for the
/// routine chatter (listener registration, fit results); this is for the two
/// global handlers only.
#[tauri::command]
pub fn overlay_error(msg: String) {
    crate::telemetry::report_frontend_error("overlay-js", &msg);
}

/// PROBLEM 74 — set once the dashboard frontend has finished bootstrapping.
/// Read by the 10s show-fallback in lib.rs so it never double-shows.
pub static DASHBOARD_READY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// PROBLEM 74 — the frontend calls this as the LAST step of bootstrap(). Only
/// now is the window shown: a webview that can run this command can also paint
/// and pump messages, so the user never sees "(Not Responding)". The window is
/// deliberately NOT shown for an `--autostart` launch (PROBLEM 70) — at logon
/// the app stays in the tray.
#[tauri::command]
pub fn dashboard_ready(app: tauri::AppHandle, window: tauri::WebviewWindow) {
    use std::sync::atomic::Ordering;
    if DASHBOARD_READY.swap(true, Ordering::SeqCst) {
        return; // bootstrap re-ran (e.g. webview reload) — window state is settled
    }
    let autostart = std::env::args().any(|a| a == "--autostart");
    log::info!(
        "dashboard-js: frontend ready (window '{}') — {}",
        window.label(),
        if autostart { "autostart: staying hidden in the tray" } else { "showing the dashboard" }
    );
    if autostart {
        return;
    }
    // THE RULE, and it is the opposite of what this comment used to assert
    // (PROBLEM 205). In Tauri v2:
    //
    //   · `#[tauri::command] fn foo(..)`        → runs ON THE MAIN THREAD.
    //   · `#[tauri::command] async fn foo(..)`  → runs on the async runtime,
    //                                             i.e. OFF the main thread.
    //
    // This comment used to read "a command handler is not on it", which is
    // true only for `async` commands. Because `dashboard_ready` is NOT async,
    // this handler is ALREADY on the main thread and `run_on_main_thread`
    // below is a no-op hop — harmless, and kept because the rule it encodes
    // (window ops belong on the main thread) stays correct if this command is
    // ever made async.
    //
    // The cost of believing the old wording: every synchronous command in
    // this file was assumed to be off the main thread, so nobody suspected
    // `list_start_menu_apps` (~12s, sync until PROBLEM 237) of serialising
    // the entire IPC bus for both webviews for ~60 versions. If you add a
    // command that does more than a few milliseconds of work, it must be
    // `async` — or its cost must be kept off any path the user waits on, and
    // it must LOG its duration. In-process COM is NOT a reason to stay sync:
    // `picker_worker` proves it runs on a worker STA (PROBLEM 237).
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        use tauri::Manager;
        if let Some(w) = app2.get_webview_window("settings") {
            crate::ensure_on_screen(&w); // PROBLEM 83 — monitor may be gone
            let _ = w.show();
            let _ = w.set_focus();
        }
    });
}

/// PROBLEM 75 — true when a startup task from an older build exists that this
/// process cannot remove. The dashboard shows the one-click repair banner.
/// PROBLEM 141 - is a SECOND copy of Spaceadom installed?
///
/// Returns `(found, path, version, kind)`. The dashboard turns this into the
/// same one-click elevated repair the stale-task banner uses (PROBLEM 75) -
/// which is the mechanism the owner already had in mind: "there is an old
/// version installed, just press this and it will delete the old version".
/// `kind` is PROBLEM 238's `status_kind()`: `"second_copy"` /
/// `"orphaned_entry"` / `""` — lets the frontend pick the right banner copy
/// without guessing from `path`.
#[tauri::command]
pub fn get_rival_install() -> (bool, String, String, String) {
    let (found, path, version) = crate::rival_install::status();
    (found, path, version, crate::rival_install::status_kind().to_string())
}

/// PROBLEM 141 - remove the second copy. ONE UAC prompt; returns whether the
/// machine is actually clean afterwards, verified against the DISK rather than
/// against an exit code (PROBLEM 127's lesson).
#[tauri::command]
pub fn repair_rival_install() -> bool {
    crate::rival_install::repair()
}

/// PROBLEM 245 — `Some("1.0.X")` exactly once, on the first dashboard open
/// after the updater relaunched a newer build; `None` every other time. The
/// frontend shows it as a toast and does nothing else with it.
///
/// ONLY WHILE THE WINDOW IS VISIBLE. Measured 2026-09-04 22:08:34: the
/// updater relaunches with `--autostart`, which builds the dashboard webview
/// HIDDEN; its page still boots and asked for the notice, so the one toast
/// was consumed by a window nobody could see. A hidden window gets `None`
/// and the notice stays put; the page asks again on every visibility/focus
/// change, so the first VISIBLE dashboard is the one that shows it.
#[tauri::command]
pub fn get_update_notice(window: tauri::WebviewWindow) -> Option<String> {
    if !window.is_visible().unwrap_or(false) {
        return None;
    }
    crate::updater::take_update_notice()
}

#[tauri::command]
pub fn get_stale_task() -> bool {
    #[cfg(windows)]
    {
        crate::startup::STALE_TASK.load(std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(windows))]
    false
}

/// PROBLEM 75 — user clicked the repair banner: delete the stale task with ONE
/// elevated schtasks call, then register the clean Run-key autostart. Returns
/// true when the machine is fixed; false if the UAC prompt was declined.
#[tauri::command]
pub fn repair_stale_task(state: State<'_, ConfigState>) -> bool {
    #[cfg(windows)]
    {
        let run_at_startup = state.0.read().unwrap_or_else(|p| p.into_inner()).run_at_startup;
        crate::startup::repair_stale_task(run_at_startup)
    }
    #[cfg(not(windows))]
    {
        let _ = state;
        true
    }
}

/// Other keyboard-remapping software currently running.
///
/// OBSERVE AND REPORT ONLY — never kill or suspend another process. A tester
/// whose shortcuts did nothing suspected his old AutoHotkey scripts, and the
/// app had no way to confirm it. Now the dashboard can say so plainly and let
/// the user decide; terminating someone else's software would be malware
/// behaviour, not a fix.
#[tauri::command]
pub fn get_conflicts() -> Vec<crate::hook::conflicts::Conflict> {
    crate::hook::conflicts::detect()
}

/// PROBLEM 155 — close a conflicting keyboard program, on the user's request.
///
/// NEVER automatic: the settings panel arms, confirms, and tells the user in
/// advance when Windows will ask permission. `close_conflict` refuses any
/// process that is not on the known-conflicts list, so this cannot be used as
/// a general "kill by name" from the webview.
#[tauri::command]
pub fn close_conflict(
    process: String,
    permanent: bool,
    elevate: bool,
) -> crate::hook::conflict_close::CloseOutcome {
    crate::hook::conflict_close::close_conflict(&process, permanent, elevate)
}

/// Open Task Manager's Start-up apps tab — where the user turns a program off
/// themselves when Spaceadom cannot (PROBLEM 157).
/// PROBLEM 161 — the "Try again" button on the dead-hook banner.
///
/// Returns immediately: the rebuild happens on the hook thread's own watchdog
/// tick, because re-hooking has to be done FROM that thread (PROBLEM 132).
/// The UI re-reads `get_hook_status` after this and shows what it finds.
#[tauri::command]
pub fn reinstall_hook() {
    crate::hook::request_hook_rebuild();
}

#[tauri::command]
pub fn open_startup_manager() -> bool {
    crate::hook::conflict_close::open_startup_manager()
}

// ---------------------------------------------------------------------------
// PROBLEM 253 — safe mode, and "Report a problem".
//
// Four thin wrappers. Every decision lives in `safe_mode.rs` and
// `diagnostics.rs`, where it is unit-tested; nothing here does anything a test
// would want to assert on.
// ---------------------------------------------------------------------------

/// Is this a safe-mode launch, and how many failed startups produced it?
///
/// Asked by the dashboard at boot. Answers `active: false` on every normal
/// launch, which is what makes the banner cost nothing when nothing is wrong.
#[tauri::command]
pub fn get_safe_mode() -> crate::safe_mode::SafeModeState {
    crate::safe_mode::state()
}

/// The banner's "Turn back on": install the keyboard hook NOW and clear the
/// boot counter, without a restart.
///
/// `Err` carries a sentence for the user rather than a code — the only failure
/// is "there was nothing parked to start", which means the button was pressed
/// twice or this process was never in safe mode. Reported rather than swallowed
/// on purpose: a button that says it armed a keyboard hook it did not arm is
/// the worst outcome available here.
#[tauri::command]
pub fn safe_mode_turn_back_on() -> Result<(), String> {
    crate::safe_mode::turn_back_on()
}

/// Build the diagnostics zip and reveal it in Explorer. Returns its full path.
///
/// **`async`, and the work goes to a blocking thread.** PROBLEM 237: a
/// non-`async` Tauri command runs on the MAIN thread, and this one reads two
/// logs, a config and two directory listings and then deflates a few megabytes
/// — precisely the shape of work that froze the app for six to seventeen
/// seconds the last time it was done inline. `spawn_blocking` and not the plain
/// async runtime, because every call in `build_bundle` is blocking file I/O.
///
/// **Nothing is uploaded.** The archive is written to
/// `%APPDATA%\Spaceadom\reports\` and the user decides what happens to it.
#[tauri::command]
pub async fn build_diagnostics_bundle(description: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::diagnostics::build_bundle(&description)
            .map(|p| p.display().to_string())
    })
    .await
    .map_err(|e| format!("the report builder thread failed to run: {e}"))?
}

/// Open the GitHub issue tracker.
///
/// A Rust command rather than the `opener` plugin from the webview, matching
/// PROBLEM 164's rule for this codebase: every shell-out is a Rust command, and
/// the URL is a compile-time constant so the webview cannot choose where this
/// goes. `explorer.exe` is the launcher for the same reason `open_installed_apps`
/// uses it — `ShellExecute` on a URL is exactly what it does, and it needs no
/// elevation.
#[tauri::command]
pub fn open_issues_page() -> bool {
    #[cfg(windows)]
    {
        let ok = std::process::Command::new("explorer.exe")
            .arg(crate::diagnostics::ISSUES_URL)
            .spawn()
            .is_ok();
        log::info!("diagnostics: opened the issue tracker: {ok}");
        ok
    }
    #[cfg(not(windows))]
    false
}

/// PROBLEM 250 — open Settings ▸ Apps ▸ Installed apps.
///
/// The packaged rival banner tells the user to uninstall the OTHER copy from
/// there, and a banner that names a place is worse than one that goes to it —
/// "Settings > Apps > Installed apps" is four clicks and a search box for
/// someone who has never been there. `ms-settings:` is the documented URI for
/// the Settings app; `explorer.exe` is the launcher because `ShellExecute` on a
/// protocol URI is exactly what it does and it needs no elevation.
///
/// Deliberately NOT `ms-settings:appsfeatures-app` (the per-app deep link):
/// that one wants a package family name we do not have for an NSIS install.
#[tauri::command]
pub fn open_installed_apps() -> bool {
    #[cfg(windows)]
    {
        let ok = std::process::Command::new("explorer.exe")
            .arg("ms-settings:appsfeatures")
            .spawn()
            .is_ok();
        log::info!("opened Settings > Apps > Installed apps: {ok}");
        ok
    }
    #[cfg(not(windows))]
    false
}

/// PROBLEM 250 — what the "Run at startup" row should actually show.
///
/// `(packaged, state, may_change, note)`:
///
/// - `packaged` — false for every NSIS and MSI install, which is every install
///   that exists today. The frontend leaves the row exactly as it was.
/// - `state` — the live `StartupTaskState`, lower-cased
///   (`enabled` / `disabled` / `disabledbyuser` / `disabledbypolicy` /
///   `enabledbypolicy` / `unavailable`).
/// - `may_change` — whether the app is allowed to flip it. `false` for the
///   three "someone else decided" states.
/// - `note` — the one sentence to show under an inert row; empty when live.
///
/// The row asks Windows rather than reading `config.run_at_startup` because in
/// a package Windows is the owner: the user can change it in Task Manager at
/// any time and the app is never told. A row rendered from config would
/// confidently show the wrong thing, which is worse than showing nothing.
///
/// **PROBLEM 254 — the PORTABLE case rides on the same tuple**, and the
/// FIRST branch is the portable one. The frontend's `paintPackagedStartup`
/// treats the first field as "somebody other than this row owns the switch"
/// and goes inert on `!may_change`; it has no opinion about WHY, which is
/// exactly why a portable copy needed no frontend change at all. A portable
/// copy writes no Run value and no Scheduled Task by design
/// (`startup.rs`'s three guards), so the switch is genuinely off and a row
/// that could be flipped would be a control that does nothing — CLAUDE.md's
/// oldest rule in this panel.
///
/// Order does not actually matter (`portable::is_portable()` is false inside
/// a package, by that function's own precedence test), but it is written
/// portable-first so a reader does not have to hold that precedence in their
/// head to see that the two cases cannot both fire.
#[tauri::command]
pub fn get_packaged_startup() -> (bool, String, bool, String) {
    if crate::portable::is_portable() {
        return (
            true,
            "portable".into(),
            false,
            "This is the portable copy, so it never writes to the registry or Task \
             Scheduler. To start it with Windows, put a shortcut to spaceadom.exe in \
             your Startup folder (Win+R, shell:startup)."
                .into(),
        );
    }
    let packaged = crate::packaged::is_packaged();
    if !packaged {
        return (false, "unavailable".into(), true, String::new());
    }
    let state = crate::packaged::startup_task_state();
    (
        true,
        format!("{state:?}").to_lowercase(),
        crate::packaged::app_may_change(state),
        crate::packaged::startup_locked_note(state).to_string(),
    )
}

/// FEATURE 1 (owner) — the Settings ▸ About section's version / install-kind
/// line. `(version, install_kind, data_dir)`:
///
/// - `version` — `app.package_info().version`, the exact same source
///   `updater.rs` compares against every release manifest, so this can never
///   disagree with what the updater thinks it is running.
/// - `install_kind` — decided the SAME WAY the updater decides which manifest
///   to trust for THIS copy (`updater::detect_install_kind`, CLAUDE.md's
///   table), with one check ahead of it: a Store package is never an NSIS or
///   MSI install (`packaged::is_packaged`), and the updater's own detector has
///   no idea packages exist — it would read a Store install's folder as
///   "Unknown" (no `uninstall.exe`, no MSI registration) and this row would
///   call a Microsoft Store install "a dev build", which is wrong in the
///   single most confusing direction available.
/// - `data_dir` — `%APPDATA%\Spaceadom`, plain text, so a user describing a
///   bug can find it without knowing the app's own product name maps to that
///   folder.
///
/// A PLAIN COMMAND, not a `Result` — there is no failure mode here worth a
/// user-visible error: every source either answers or falls back to a static
/// string. The frontend still wraps the call in a `try`/`catch` (an older
/// build simply has no such command), which is a different kind of failure
/// than this function ever produces.
#[tauri::command]
pub fn get_about_info(app: tauri::AppHandle) -> (String, String, String) {
    let version = app.package_info().version.to_string();
    let install_kind = if crate::packaged::is_packaged() {
        "Microsoft Store".to_string()
    } else {
        match crate::updater::detect_install_kind() {
            crate::updater::InstallKind::Nsis => "Installer (setup.exe)".to_string(),
            crate::updater::InstallKind::Msi => "Windows Installer (.msi)".to_string(),
            crate::updater::InstallKind::Unknown => "Portable / development build".to_string(),
        }
    };
    let data_dir = crate::startup::data_dir().display().to_string();
    (version, install_kind, data_dir)
}

/// PROBLEM 254 — **is this copy the portable, unzipped one?**
///
/// One bool, for the frontend paths where the DIFFERENCE is only the wording:
/// `main.ts`'s rival-install banner has to name what the second copy actually
/// is ("the portable copy you unzipped", not "the other installer"), because a
/// user who is told to uninstall something they never installed will go
/// looking for an entry in Apps & features that does not exist.
///
/// Deliberately NOT folded into `get_about_info`'s `install_kind` string.
/// That value is a SENTENCE for a human to read and it already collapses
/// portable and dev builds into one phrase; a banner needs a decision, and a
/// decision taken by string-matching a human-facing sentence is one rename
/// away from silently reverting to the wrong wording.
///
/// A plain `bool` command with no `Result`: `portable::is_portable()` is a
/// cached `OnceLock` read that cannot fail, and the frontend still wraps the
/// call in a `try`/`catch` because an OLDER build has no such command — a
/// different failure than anything this function can produce.
#[tauri::command]
pub fn is_portable_install() -> bool {
    crate::portable::is_portable()
}


/// Size + position the overlay window to fit the toast stack, bottom-centre,
/// then show it. Called by the overlay page AFTER it has rendered and
/// measured its content (layout works in hidden webviews; painting doesn't —
/// so measure-then-show is safe). One-jump resize per the motion reference:
/// animating an OS window's bounds frame-by-frame tears on Windows.
#[tauri::command]
pub fn overlay_fit(app: tauri::AppHandle, width: f64, height: f64) -> Option<OverlayRect> {
    crate::crash_context::note_overlay_op(format!("overlay_fit {width}x{height} (toast, bottom-centre)"));
    use std::sync::atomic::Ordering;
    use tauri::Manager;
    if crate::guide_hud::OVERLAY_DISABLED.load(Ordering::Relaxed) {
        return None;
    }
    // The HUD owns the window while Space is held — never shrink it mid-hold.
    if crate::guide_hud::is_visible() {
        return None;
    }
    let win = app.get_webview_window("overlay")?;
    let Some(mon) = overlay_monitor(&win) else {
        log::warn!("overlay_fit: no monitor could be resolved — window NOT positioned");
        return None;
    };
    let sf = mon.scale_factor();
    let ms = mon.size().to_logical::<f64>(sf);
    let mp = mon.position().to_logical::<f64>(sf);
    let w = width.clamp(120.0, ms.width - 32.0);
    let h = height.clamp(44.0, ms.height - 32.0);
    let x = mp.x + (ms.width - w) / 2.0;
    let y = mp.y + ms.height - h - 64.0;
    let _ = win.set_size(tauri::LogicalSize::new(w, h));
    let _ = win.set_position(tauri::LogicalPosition::new(x, y));
    // INSTRUMENTATION — see overlay_fit_hud. Never remove.
    log::info!(
        "overlay_fit: asked {width:.0}x{height:.0} → {w:.0}x{h:.0} @ ({x:.0},{y:.0})          bottom-centre; monitor {:.0}x{:.0} at ({:.0},{:.0}) scale {sf}; GOT size {:?} pos {:?}",
        ms.width, ms.height, mp.x, mp.y,
        win.outer_size().map(|s| s.to_logical::<f64>(sf)).map(|s| (s.width.round(), s.height.round())),
        win.outer_position().map(|p| p.to_logical::<f64>(sf)).map(|p| (p.x.round(), p.y.round())),
    );
    // Toasts must sit above everything, same rule as the HUD. Direct Win32 —
    // `set_always_on_top(true)` on an already-topmost window is a tao no-op
    // (PROBLEM 168).
    raise_overlay_topmost(&win);
    let _ = win.show();
    Some(OverlayRect { x, y, w, h })
}

/// Size + position the overlay window to fit the RENDERED Guide HUD, called
/// by the overlay page after it has laid the HUD out. Only valid while the
/// HUD owns the window. This is what makes clipping structurally impossible:
/// the old fixed 680×600 cut off Y, Z and every special-function row on a
/// 26-binding profile (user report, 2026-08-10).
/// PROBLEM 137 - the HANDOVER window: the ring's box, extended DOWNWARD so the
/// flight can reach the toast's real bottom-centre slot.
///
/// The owner: "make it land to the bottom normal bottom center position, no
/// need any jump, make it fly to the final position." That is impossible from
/// the HUD window alone - measured on his 1707x1067 panel, the toast's final
/// resting place is 88px BELOW the HUD window's bottom edge, so the pill had
/// nowhere to fly to and PROBLEM 136 could only park the toast mid-screen.
///
/// Placement, and every term matters:
///   * TOP edge is unchanged from the centred HUD box, so the ring does not
///     move on screen when this runs. The frontend pins #st-hud to that
///     original height so it keeps centring in the same place.
///   * BOTTOM edge is `ms.height - 64`, byte-for-byte the same expression
///     `overlay_fit` uses. A toast anchored `bottom: 74px` in THIS window is
///     therefore at the exact pixel it would occupy in the normal toast
///     window - so the flight lands on the final position, and the later fit
///     (or none at all) moves nothing.
///
/// Deliberately NOT fullscreen. A fullscreen transparent webview composes ZERO
/// pixels on this machine (PROBLEM 37/80); this is the ring's width by roughly
/// 70% of the screen height, well inside what the overlay already does.
#[tauri::command]
pub fn overlay_fit_handover(
    app: tauri::AppHandle,
    width: f64,
    height: f64,
) -> Option<OverlayRect> {
    crate::crash_context::note_overlay_op(format!(
        "overlay_fit_handover {width}x{height} (ring box, extended to the toast slot)"
    ));
    use std::sync::atomic::Ordering;
    use tauri::Manager;
    if crate::guide_hud::OVERLAY_DISABLED.load(Ordering::Relaxed) {
        return None;
    }
    let win = app.get_webview_window("overlay")?;
    let Some(mon) = overlay_monitor(&win) else {
        log::warn!("overlay_fit_handover: no monitor could be resolved - not positioned");
        return None;
    };
    let sf = mon.scale_factor();
    let ms = mon.size().to_logical::<f64>(sf);
    let mp = mon.position().to_logical::<f64>(sf);

    let w = width.clamp(120.0, ms.width - 32.0);
    let ring_h = height.clamp(44.0, ms.height - 32.0);
    // Top edge: where the CENTRED ring box already is. Unchanged.
    let y = mp.y + (ms.height - ring_h) / 2.0;
    // Bottom edge: exactly overlay_fit's, so the toast slot lines up.
    let bottom = mp.y + ms.height - 64.0;
    let h = (bottom - y).clamp(ring_h, ms.height - 32.0);
    let x = mp.x + (ms.width - w) / 2.0;

    let _ = win.set_size(tauri::LogicalSize::new(w, h));
    let _ = win.set_position(tauri::LogicalPosition::new(x, y));
    log::info!(
        "overlay_fit_handover: ring {ring_h:.0}px -> window {w:.0}x{h:.0} @ ({x:.0},{y:.0}); \
         bottom {bottom:.0} matches overlay_fit; monitor {:.0}x{:.0} scale {sf}; GOT size {:?} pos {:?}",
        ms.width, ms.height,
        win.outer_size().map(|s| s.to_logical::<f64>(sf)).map(|s| (s.width.round(), s.height.round())),
        win.outer_position().map(|p| p.to_logical::<f64>(sf)).map(|p| (p.x.round(), p.y.round())),
    );
    raise_overlay_topmost(&win);
    let _ = win.show();
    Some(OverlayRect { x, y, w, h })
}

#[tauri::command]
pub fn overlay_fit_hud(app: tauri::AppHandle, width: f64, height: f64) -> Option<OverlayRect> {
    crate::crash_context::note_overlay_op(format!("overlay_fit_hud {width}x{height} (radial HUD, centred)"));
    use tauri::Manager;
    if !crate::guide_hud::is_visible() {
        // SAY SO. This early return sits ABOVE the instrumentation block below,
        // whose own comment reads "This function used to be completely silent…
        // Diagnosing it cost a whole round trip… Never remove this." The guard
        // quietly reintroduced that silence for the one case that matters: the
        // page is laying out a ring while Rust believes the HUD is down, so the
        // window keeps whatever size it had (680x600 first-frame) and a
        // 1130x572 ring is clipped by 225px per side. That is exactly the
        // owner's 2026-08-24 screenshot, and the log had nothing to say about
        // it.
        //
        // INFO, not WARN: releasing Space inside the ~15ms between show and fit
        // legitimately lands here (measured: show 23:40:47.978, fit
        // 23:40:47.993). It is a diagnostic, not an alarm.
        let sz = app
            .get_webview_window("overlay")
            .and_then(|w| w.outer_size().ok())
            .map(|s| format!("{}x{}", s.width, s.height))
            .unwrap_or_else(|| "unknown".into());
        log::info!(
            "overlay_fit_hud: REFUSED {width:.0}x{height:.0} — is_visible() is false while the \
             page is laying out a ring. The window keeps its current size ({sz} physical) and \
             the HUD will be CLIPPED if it is on screen."
        );
        return None;
    }
    let Some(win) = app.get_webview_window("overlay") else {
        log::warn!("overlay_fit_hud: no 'overlay' window — the HUD cannot be placed");
        return None;
    };
    if let Some(mon) = overlay_monitor(&win) {
        let sf = mon.scale_factor();
        let ms = mon.size().to_logical::<f64>(sf);
        let mp = mon.position().to_logical::<f64>(sf);
        // The radial HUD is CENTRED on both axes (V13's panel was bottom-
        // anchored). Clamped to 94% of the monitor and never to the full work
        // area: a fullscreen transparent window composes zero pixels here.
        let w = width.clamp(320.0, ms.width * 0.94);
        let h = height.clamp(120.0, ms.height * 0.94);
        // PROBLEM 112 — hoisted into locals so the rect can be returned. The
        // maths is UNCHANGED; it was previously computed inline in the call.
        let x = mp.x + (ms.width - w) / 2.0;
        let y = mp.y + (ms.height - h) / 2.0;
        let _ = win.set_size(tauri::LogicalSize::new(w, h));
        let _ = win.set_position(tauri::LogicalPosition::new(x, y));

        // INSTRUMENTATION (2026-08-11). This function used to be completely
        // silent, and when the HUD stopped appearing there was no way to tell
        // a wrong SIZE from a wrong POSITION from a window that never moved.
        // Diagnosing it cost a whole round trip. Log the request, the monitor
        // it was computed against, and — critically — what the window ACTUALLY
        // ended up as. Never remove this.
        let got_sz = win.outer_size().map(|s| s.to_logical::<f64>(sf));
        let got_ps = win.outer_position().map(|p| p.to_logical::<f64>(sf));
        log::info!(
            "overlay_fit_hud: asked {width:.0}x{height:.0} → clamped {w:.0}x{h:.0} @ \
             ({:.0},{:.0}); monitor {:.0}x{:.0} at ({:.0},{:.0}) scale {sf}; \
             GOT size {:?} pos {:?}; visible {:?}",
            mp.x + (ms.width - w) / 2.0,
            mp.y + (ms.height - h) / 2.0,
            ms.width, ms.height, mp.x, mp.y,
            got_sz.map(|s| (s.width.round(), s.height.round())),
            got_ps.map(|p| (p.x.round(), p.y.round())),
            win.is_visible(),
        );

        // Last chance to get back over anything that entered the topmost band
        // since show_guide_hud ran (PROBLEM 168). Cheap, and this is the final
        // placement before the ring is revealed.
        raise_overlay_topmost(&win);

        // PROBLEM 80 — the compositing self-test rides on every HUD show.
        compositing_selftest(app.clone());
        return Some(OverlayRect { x, y, w, h });
    }
    None
}

/// PROBLEM 80 — detect the driver pathology where the transparent overlay
/// composes ZERO pixels while every readback says healthy. Method (the same
/// measurement that diagnosed it live): sample screen pixels inside the
/// overlay rect NOW (entrance animation still running) and again 450ms later.
/// A live overlay changes at least one of them (the HUD pulses and animates);
/// dead composition changes none. Three consecutive dead verdicts flip the
/// config to software rendering and schedule a silent self-restart.
///
/// False-verdict safety: pixels that changed for ANY reason (video behind,
/// animation, cursor) reset the strike counter — the safe direction. The test
/// runs only while `overlay_compositing == "auto"`, so a healed machine never
/// samples again.
/// PROBLEM 93 — the probe points the self-test samples, and the desktop colours
/// underneath them captured just BEFORE the overlay was shown.
///
/// Sampling only after the show gives a differential test ("did these pixels
/// change in 450 ms"), which is really a test of whether ANYTHING on screen
/// moved: a window repainting behind an invisible overlay counted as proof
/// that composition was alive and reset the strike counter to zero. On a busy
/// screen the counter could sit at 0 indefinitely and the app would never
/// heal — the user's "sometimes it never comes back".
#[cfg(windows)]
static COMPOSITING_BASELINE: std::sync::Mutex<Option<(Vec<(i32, i32)>, Vec<u32>)>> =
    std::sync::Mutex::new(None);

/// The probe points: the centre of the overlay plus four neighbours, in
/// PHYSICAL pixels. The offsets are deliberately small — the HUD's SPACE pill
/// is 230x60 CSS px, i.e. 345x90 physical at this machine's 1.5 scale, so the
/// old +/-60 vertical probes landed OUTSIDE the opaque pill and sampled
/// whatever was behind the overlay.
#[cfg(windows)]
fn compositing_probes(win: &tauri::WebviewWindow) -> Option<Vec<(i32, i32)>> {
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else {
        return None;
    };
    let cx = pos.x + size.width as i32 / 2;
    let cy = pos.y + size.height as i32 / 2;
    Some(vec![(cx, cy), (cx - 120, cy), (cx + 120, cy), (cx, cy - 20), (cx, cy + 20)])
}

#[cfg(windows)]
unsafe fn sample_pixels(probes: &[(i32, i32)]) -> Vec<u32> {
    use windows::Win32::Graphics::Gdi::{GetDC, GetPixel, ReleaseDC};
    let hdc = GetDC(None);
    let v = probes.iter().map(|&(x, y)| GetPixel(hdc, x, y).0).collect();
    ReleaseDC(None, hdc);
    v
}

/// Is another window sitting ON TOP of the overlay at the probe points?
///
/// PROBLEM 171 — **the self-test cannot tell "I painted nothing" from "someone
/// is covering me", and until now it did not try.** It reads screen pixels
/// with `GetPixel` on the desktop DC, which returns whatever is visible at
/// that coordinate — our overlay if we are on top, and the window above us if
/// we are not. Both cases produce "the pixels did not change", and both scored
/// a strike. Three strikes silently flip the machine to software rendering and
/// restart the app.
///
/// The owner's log for 2026-08-24 is the proof: strikes reached 2/3 twice, and
/// one of them (22:11:32.841) lands on the exact HUD show he described as
/// *"space hud sound appeared but showed nothing, it appeared behind Claude"* —
/// with a PiP'd, permanently-topmost window over it (PROBLEM 167/168). The
/// overlay was composing perfectly; it was underneath. A remedy applied to a
/// misdiagnosis is worse than no remedy.
///
/// WHY NOT `WindowFromPoint`. It is the obvious call and it is WRONG here: the
/// overlay is click-through (`WS_EX_TRANSPARENT`, from
/// `set_ignore_cursor_events(true)`), and `WindowFromPoint` deliberately skips
/// transparent windows. It would never return our overlay, so every show would
/// look occluded and the test would abstain forever — silently disabling
/// detection, which is precisely the PROBLEM 122 failure this file already
/// records once.
///
/// So walk the z-order from the top instead. If we meet the overlay before any
/// visible window that covers a probe point, nothing is above us there.
#[cfg(windows)]
unsafe fn overlay_is_occluded(win: &tauri::WebviewWindow, probes: &[(i32, i32)]) -> bool {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetTopWindow, GetWindow, GetWindowRect, IsIconic, IsWindowVisible, GW_HWNDNEXT,
    };

    let Ok(ours) = win.hwnd() else { return false };
    let ours = HWND(ours.0 as *mut _);

    let mut h = GetTopWindow(HWND(std::ptr::null_mut())).unwrap_or_default();
    let mut guard = 0;
    while !h.0.is_null() && guard < 4000 {
        guard += 1;
        if h == ours {
            return false; // we are above everything that could cover the probes
        }
        if IsWindowVisible(h).as_bool() && !IsIconic(h).as_bool() {
            // A CLOAKED window still has a rect and still reports visible —
            // suspended UWP apps and virtual-desktop residents live here. They
            // paint nothing, so treating them as cover would abstain constantly.
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(
                h,
                DWMWA_CLOAKED,
                &mut cloaked as *mut _ as *mut _,
                std::mem::size_of::<u32>() as u32,
            );
            if cloaked == 0 {
                let mut r = RECT::default();
                if GetWindowRect(h, &mut r).is_ok()
                    && probes.iter().any(|&(x, y)| {
                        x >= r.left && x < r.right && y >= r.top && y < r.bottom
                    })
                {
                    return true;
                }
            }
        }
        h = GetWindow(h, GW_HWNDNEXT).unwrap_or_default();
    }
    // Never found ourselves in the z-order (or the walk ran away). Do NOT
    // abstain on an inconclusive answer — that would disable the test.
    false
}

/// Called from the HUD show path with the overlay positioned but NOT yet
/// visible. Cheap: 5 GetPixel calls.
#[cfg(windows)]
pub fn capture_compositing_baseline(win: &tauri::WebviewWindow) {
    let Some(probes) = compositing_probes(win) else { return };
    let colours = unsafe { sample_pixels(&probes) };
    *COMPOSITING_BASELINE
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = Some((probes, colours));
}

#[cfg(windows)]
fn compositing_selftest(app: tauri::AppHandle) {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    static STRIKES: AtomicU32 = AtomicU32::new(0);
    static RUNNING: AtomicBool = AtomicBool::new(false);
    static HEALED: AtomicBool = AtomicBool::new(false);
    /// PROBLEM 122 — overlay rebuilds attempted this session, bounded so a
    /// machine that can never composite the overlay does not rebuild a window
    /// on a loop for as long as the app is running.
    static REBUILDS: AtomicU32 = AtomicU32::new(0);

    if HEALED.load(Ordering::Relaxed) || RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }

    std::thread::Builder::new()
        .name("st-compositing-test".into())
        .spawn(move || {
            use tauri::Manager;
            let done = || RUNNING.store(false, Ordering::SeqCst);

            // PROBLEM 93 — only "software" stops the test. This used to be
            // `mode != "auto"`, so ANY other string — a typo, a hand-edit, a
            // future third value — permanently disabled detection, while
            // lib.rs only adds --disable-gpu for exactly "software". That
            // combination is an invisible overlay forever, with the one
            // mechanism that could have fixed it switched off.
            // PROBLEM 122 — this used to be:
            //     if mode == "software" { HEALED = true; return; }
            // "already healed; stop testing". It is the reason nothing noticed
            // the overlay was dead for seven hours on 2026-08-16: this machine
            // healed to software days earlier, so the ONE mechanism that can
            // detect an unpainted overlay had switched itself off permanently,
            // and stayed off for every later cause.
            //
            // Software mode is not a cure, it is one remedy. The test now runs
            // in BOTH modes; only the remedy differs. In auto we can still fall
            // back to software rendering. In software there is no further
            // rendering mode to try, so the remaining suspect is the window
            // itself — rebuild it, which is exactly what a restart was doing by
            // accident (PROBLEM 117).
            let software = {
                let state: tauri::State<ConfigState> = app.state();
                let mode = state.0.read().unwrap_or_else(|p| p.into_inner()).overlay_compositing.clone();
                if mode != "auto" && mode != "software" {
                    log::warn!(
                        "compositing: unrecognised overlay_compositing '{mode}' — treating as \
                         'auto' and continuing to self-test"
                    );
                }
                mode == "software"
            };

            let Some(win) = app.get_webview_window("overlay") else { done(); return };
            let Some(probes) = compositing_probes(&win) else { done(); return };

            let before = unsafe { sample_pixels(&probes) };
            std::thread::sleep(std::time::Duration::from_millis(450));

            // The HUD may have been dismissed mid-test (short hold) — a
            // hidden window legitimately paints nothing. Not a verdict.
            if !win.is_visible().unwrap_or(false) {
                log::info!(
                    "compositing: HUD dismissed inside 450ms — no verdict from this show"
                );
                done();
                return;
            }
            // PROBLEM 171 — abstain if something is covering us. Checked AFTER
            // the 450ms wait, not before: a window can be raised over the HUD
            // during the sample, and it is the state at the moment of judgement
            // that decides whether the pixels mean anything.
            if unsafe { overlay_is_occluded(&win, &probes) } {
                log::info!(
                    "compositing: another window is covering the overlay at the probe points — \
                     NO VERDICT from this show. Unchanged pixels here would mean 'someone is on \
                     top of us', not 'we painted nothing', and scoring it would eventually flip \
                     this machine to software rendering for a fault it does not have."
                );
                done();
                return;
            }

            let after = unsafe { sample_pixels(&probes) };

            // PROBLEM 93 — the ABSOLUTE half of the test. If the pixels where
            // the overlay sits still equal the desktop captured just before it
            // was shown, the overlay composed nothing — regardless of whether
            // something else on screen happened to move. Only trust the
            // baseline if it was taken at the SAME probe points (the window
            // moves between the HUD and toast placements).
            let unpainted = COMPOSITING_BASELINE
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .filter(|(pts, _)| *pts == probes)
                .map(|(_, base)| *base == after);

            let dead = match unpainted {
                // Baseline available: differential AND absolute must agree.
                Some(true) => before == after,
                // The overlay demonstrably painted over the desktop — alive,
                // even if it is a still image that did not change in 450 ms.
                Some(false) => false,
                // No usable baseline (toast path, or the window moved) — fall
                // back to the old differential test.
                None => before == after,
            };

            if dead {
                let strikes = STRIKES.fetch_add(1, Ordering::SeqCst) + 1;
                log::warn!(
                    "compositing: overlay pixels did not change across 450ms while visible \
                     (strike {strikes}/3) — GPU composition may be dead on this machine"
                );
                // PROBLEM 217 — promoted. The user sees "the HUD stopped
                // appearing"; nothing in the app tells them why, and at WARN
                // this never left the machine. Rate-limited per condition, so a
                // machine that strikes on every hold reports once, not once a
                // second.
                crate::telemetry::report_degraded(
                    crate::telemetry::Degraded::OverlayCompositingStrike,
                    &format!(
                        "the overlay was visible and composed nothing for 450ms \
                         (strike {strikes}/3) — the HUD and toasts may be invisible"
                    ),
                );
                if strikes >= 3 && software {
                    // PROBLEM 122 — already in software rendering, so there is
                    // no further rendering mode to fall back to. The remaining
                    // suspect is the overlay WINDOW: a transparent, layered,
                    // always-on-top window whose composition has stopped. That
                    // is PROBLEM 117's fault, and rebuilding it is PROBLEM
                    // 117's fix — reused here so the app repairs itself
                    // whatever the cause, not only when a display changes.
                    //
                    // Bounded on purpose. A machine where the overlay can never
                    // paint must not rebuild a window every few seconds
                    // forever; three attempts, then stop and say so plainly.
                    const MAX_REBUILDS: u32 = 3;
                    let n = REBUILDS.fetch_add(1, Ordering::SeqCst) + 1;
                    if n <= MAX_REBUILDS {
                        log::warn!(
                            "compositing: 3 dead verdicts while ALREADY in software mode — \
                             the overlay window itself has stopped compositing. Rebuilding it \
                             (attempt {n}/{MAX_REBUILDS}); PROBLEM 122."
                        );
                        // PROBLEM 217 — promoted: already in software mode and
                        // still composing nothing is the worst overlay state
                        // there is, and it was invisible to the reporter.
                        crate::telemetry::report_degraded(
                            crate::telemetry::Degraded::OverlayCompositingDead,
                            &format!(
                                "3 dead verdicts while ALREADY in software rendering — the \
                                 overlay window has stopped compositing; rebuilding it \
                                 (attempt {n}/{MAX_REBUILDS})"
                            ),
                        );
                        crate::display_watch::rebuild_overlay(&app);
                        STRIKES.store(0, Ordering::SeqCst); // a fresh three chances
                    } else {
                        HEALED.store(true, Ordering::Relaxed); // stop burning cycles
                        log::error!(
                            "compositing: the overlay still composes nothing after \
                             {MAX_REBUILDS} rebuilds in software mode. Giving up for this \
                             session — shortcuts and sound keep working, but the HUD and \
                             toasts will not be drawn. Restart the app, and if it persists \
                             this machine's compositor cannot show the overlay."
                        );
                    }
                    done();
                    return;
                }

                if strikes >= 3 {
                    // Flip the config; the env var applies at next process start.
                    {
                        let state: tauri::State<ConfigState> = app.state();
                        let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
                        cfg.overlay_compositing = "software".into();
                        let snapshot = cfg.clone();
                        drop(cfg);
                        if let Err(e) = crate::config::save(&snapshot) {
                            // PROBLEM 93 — do NOT set HEALED before this point.
                            // It used to be set first, so a failed save left
                            // the app in GPU mode with detection permanently
                            // switched off: an invisible overlay for the rest
                            // of the process, and no further attempts to fix
                            // it. Leaving HEALED false means the next HUD
                            // tries again.
                            log::error!(
                                "compositing: could not save software mode ({e}) — staying in \
                                 GPU mode; the self-test will retry on the next HUD"
                            );
                            done();
                            return;
                        }
                    }
                    HEALED.store(true, Ordering::Relaxed);
                    log::warn!(
                        "compositing: 3 dead verdicts — switched to SOFTWARE rendering. \
                         Restarting Spaceadom silently to apply (the overlay is invisible \
                         anyway; the dashboard, if open, will close and can be reopened \
                         from the tray)."
                    );
                    // PROBLEM 217 — promoted. The self-test declaring GPU
                    // composition dead is a verdict about the user's machine
                    // that nobody but the log ever heard.
                    crate::telemetry::report_degraded(
                        crate::telemetry::Degraded::OverlayCompositingDead,
                        "3 dead verdicts — GPU composition declared dead, switched to \
                         SOFTWARE rendering and restarting to apply",
                    );
                    // Detached relaunch with a 2s gap so the single-instance
                    // mutex of THIS process is released before the new one
                    // starts. `cmd /C ping` is the delay tool present on every
                    // Windows box.
                    if let Ok(exe) = std::env::current_exe() {
                        use std::os::windows::process::CommandExt;
                        let relaunch = format!(
                            "ping -n 3 127.0.0.1 >nul & start \"\" \"{}\"",
                            exe.to_string_lossy()
                        );
                        // CREATE_BREAKAWAY_FROM_JOB (0x01000000): if THIS
                        // process runs inside a job object that kills its
                        // children on exit (test harnesses do this), a plain
                        // child dies with us and the restart never happens —
                        // observed live 2026-08-13. Jobs that forbid breakaway
                        // make the spawn FAIL, so fall back to a plain spawn.
                        const NO_WINDOW: u32 = 0x0800_0000;
                        const BREAKAWAY: u32 = 0x0100_0000;
                        let spawn = |flags: u32| {
                            std::process::Command::new("cmd")
                                .args(["/C", &relaunch])
                                .creation_flags(flags)
                                .spawn()
                        };
                        if spawn(NO_WINDOW | BREAKAWAY).is_err() {
                            let _ = spawn(NO_WINDOW);
                        }
                    }
                    crate::hook::stop_hook();
                    app.exit(0);
                }
            } else if STRIKES.swap(0, Ordering::SeqCst) > 0 {
                log::info!("compositing: overlay pixels changed — composition is alive, strikes reset");
            }
            done();
        })
        .ok();
}

#[cfg(not(windows))]
fn compositing_selftest(_app: tauri::AppHandle) {}

// ---------------------------------------------------------------------------
// THE RING TOOL — "The ring isn't showing?" (owner, 2026-09-01)
// ---------------------------------------------------------------------------
//
// This region replaces the "Software overlay" SWITCH that used to live in
// Settings (PROBLEM 92). The switch asked the user to hold an opinion about GPU
// compositing; `overlay_compositing` is a MEASUREMENT this app takes about the
// machine's display driver, and a switch is the wrong shape for a measurement.
// What a user actually has is a symptom — the ring does not appear — so the
// control is now a button in the Conflicts area that RE-RUNS THE MEASUREMENT
// and says what it found and what it did.
//
// Everything below reuses `compositing_selftest`'s own machinery
// (`compositing_probes`, `sample_pixels`, `overlay_is_occluded`,
// `COMPOSITING_BASELINE`) rather than re-deriving the test. A second
// implementation of "is the overlay painting?" would be a second thing to keep
// correct, and the two would eventually disagree in front of a user who had
// just been told by one of them that their machine was fine.
//
// THE ESCAPE HATCH PROBLEM 92 ADDED IS STILL HERE, and it now works in BOTH
// directions without the user knowing what they are reversing:
//   · a machine wrongly stuck in software rendering gets handed back to the GPU
//     the moment a check finds the ring drawing correctly;
//   · a machine whose ring is genuinely dead is put into software rendering.
// `set_overlay_compositing` (above) stays registered: it is the deliberate,
// no-questions-asked override, and this tool is the diagnosis.

/// How long the ring is left up before the first pixel sample, so the window
/// has actually been placed, shown and painted once. Measured in the log the
/// self-test already writes: `show_hud_payload`'s window work costs ~80ms, and
/// the page then lays the ring out and calls `overlay_fit_hud`.
#[cfg(windows)]
const OVERLAY_FIX_SETTLE_MS: u64 = 400;

/// The watch window, IDENTICAL to `compositing_selftest`'s 450ms on purpose.
/// The two must be able to reach the same verdict about the same machine — a
/// tool that disagrees with the automatic test is worse than no tool.
#[cfg(windows)]
const OVERLAY_FIX_WATCH_MS: u64 = 450;

/// The one-shot re-test's marker file, inside `%APPDATA%\Spaceadom`.
///
/// NUMBERED, NOT VERSIONED, and that is a decision rather than a shortcut. The
/// migration corrects verdicts taken by builds that predate PROBLEM 171's
/// occlusion check — a false "software" written when the overlay was merely
/// COVERED, not dead. That is a one-time correction, so it should run exactly
/// once ever; keying the marker to `CARGO_PKG_VERSION` would re-run it on every
/// future release, putting a ring on screen at startup forever for a question
/// that was already answered. A future migration bumps the number.
#[cfg(windows)]
const OVERLAY_RETEST_MARKER: &str = "overlay-recheck-1.done";

/// What one draw-and-watch pass concluded.
///
/// `NoVerdict` carries the sentence the USER sees, not an error code: every
/// abstention here has a cause the user can act on (something was covering the
/// ring, the window is not built yet, the ring came down early), and a tool
/// that says "inconclusive" without saying why is a tool nobody presses twice.
#[cfg(windows)]
enum OverlayVerdict {
    Alive,
    Dead,
    NoVerdict(String),
}

/// The user's saved ring shape, in `PreviewLayout`'s vocabulary.
///
/// MIRRORS `ringLayoutFor` in `src/components/controls.ts` exactly — including
/// the retired `hud_band_count == "one"`, which reads as Compact in both places
/// (forced-one and auto are indistinguishable whenever the labels fit). The
/// check must draw the ring the user actually has: a machine whose Double ring
/// is invisible is not tested by drawing a Compact one.
#[cfg(windows)]
fn ring_layout_name(cfg: &AppConfig) -> &'static str {
    if !cfg.hud_magnetic_layout {
        "wide"
    } else if cfg.hud_band_count == "two" {
        "double"
    } else {
        "compact"
    }
}

/// Draw the ring once and watch whether anything reaches the screen.
///
/// BLOCKING — it sleeps ~850ms. Never call it from a non-async command: those
/// run on the MAIN THREAD in Tauri v2, and every IPC call from both webviews
/// would queue behind it (the `list_start_menu_apps` trap, PROBLEM 205).
///
/// The ring is drawn through the PREVIEW path, not through a real HUD show, for
/// the three properties `preview_hud_layout` documents: it writes nothing, it
/// uses the user's real bindings, and it cannot launch anything (no chip keys
/// and no chip rects are published). A diagnostic that could fire a shortcut
/// would be a diagnostic nobody dares run.
///
/// `hide_when_judged` takes the ring down the moment the verdict is in. The
/// button leaves it up for the full preview so the user SEES the ring they came
/// here about; the startup re-test hides it, because a ring appearing
/// unannounced eight seconds after launch reads as a fault, not as a check.
#[cfg(windows)]
fn overlay_draw_and_watch(app: &tauri::AppHandle, hide_when_judged: bool) -> OverlayVerdict {
    use tauri::Manager;

    let Some(win) = app.get_webview_window("overlay") else {
        return OverlayVerdict::NoVerdict(
            "Spaceadom has not finished starting up — the window the ring is drawn in does \
             not exist yet. Give it a few seconds and check again."
                .into(),
        );
    };

    // Built while the read guard is held, then the guard is DROPPED before the
    // show: `show_preview_hud` does Win32 window work and emits an event, and
    // holding the config lock across either is how a Space-hold ends up waiting
    // on a settings click (the same rule `preview_hud_layout` states).
    let payload = {
        let state: tauri::State<ConfigState> = app.state();
        let cfg = state.0.read().unwrap_or_else(|p| p.into_inner());
        let name = ring_layout_name(&cfg);
        let Some(which) = crate::engine::PreviewLayout::parse(name) else {
            // Unreachable: `ring_layout_name` returns one of the three literals
            // `parse` accepts. Refused rather than defaulted anyway, for the
            // reason `PreviewLayout::parse` gives — a shape we cannot name is a
            // shape we must not silently substitute in a DIAGNOSTIC.
            return OverlayVerdict::NoVerdict(format!(
                "Spaceadom could not work out which ring shape to draw ({name:?}). Nothing \
                 was changed."
            ));
        };
        crate::engine::preview_payload(&cfg, which)
    };

    let epoch = crate::guide_hud::begin_hold();
    crate::guide_hud::show_preview_hud(epoch, payload);
    log::info!("overlay-fix: drew the ring (hold #{epoch}) — watching the screen");

    std::thread::sleep(std::time::Duration::from_millis(OVERLAY_FIX_SETTLE_MS));

    // The probes are read AFTER the show, because that is when the window is at
    // the HUD placement `capture_compositing_baseline` sampled the desktop for.
    let Some(probes) = compositing_probes(&win) else {
        return OverlayVerdict::NoVerdict(
            "Spaceadom could not read where the ring's window is on screen, so there was \
             nothing to look at. Nothing was changed."
                .into(),
        );
    };

    let judged = |v: OverlayVerdict| {
        if hide_when_judged {
            crate::guide_hud::hide_preview_hud(epoch);
        }
        v
    };

    // A hidden window legitimately paints nothing. Not a verdict — the same
    // abstention `compositing_selftest` makes for a short hold.
    if !win.is_visible().unwrap_or(false) {
        return judged(OverlayVerdict::NoVerdict(
            "The ring came down before the check finished, so there was nothing to measure. \
             Nothing was changed — try again."
                .into(),
        ));
    }

    let before = unsafe { sample_pixels(&probes) };
    std::thread::sleep(std::time::Duration::from_millis(OVERLAY_FIX_WATCH_MS));

    if !win.is_visible().unwrap_or(false) {
        return judged(OverlayVerdict::NoVerdict(
            "The ring came down before the check finished, so there was nothing to measure. \
             Nothing was changed — try again."
                .into(),
        ));
    }

    // PROBLEM 171 — checked AFTER the wait, not before: a window can be raised
    // over the ring during the sample, and it is the state at the moment of
    // judgement that decides whether the pixels mean anything. Unchanged pixels
    // under someone else's window mean "they are on top of us", not "we painted
    // nothing", and acting on that would put a healthy machine into software
    // rendering for a fault it does not have.
    if unsafe { overlay_is_occluded(&win, &probes) } {
        return judged(OverlayVerdict::NoVerdict(
            "Another window was sitting on top of the ring while the check ran, so what is \
             on screen there belongs to that window and not to Spaceadom. Nothing was \
             changed — move that window aside and check again."
                .into(),
        ));
    }

    let after = unsafe { sample_pixels(&probes) };

    // PROBLEM 93's absolute test, then PROBLEM 80's differential one, in exactly
    // the order and with exactly the fallbacks `compositing_selftest` uses.
    let unpainted = COMPOSITING_BASELINE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .filter(|(pts, _)| *pts == probes)
        .map(|(_, base)| *base == after);

    let dead = match unpainted {
        Some(true) => before == after,
        Some(false) => false,
        None => before == after,
    };

    judged(if dead { OverlayVerdict::Dead } else { OverlayVerdict::Alive })
}

/// Write the compositing verdict and republish it. Returns whether the write
/// reached disk — a tool must never claim to have changed something it did not.
#[cfg(windows)]
fn write_compositing(app: &tauri::AppHandle, mode: &str) -> Result<(), String> {
    use tauri::Manager;
    let state: tauri::State<ConfigState> = app.state();
    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    cfg.overlay_compositing = mode.into();
    let snapshot = cfg.clone();
    drop(cfg);
    config::save(&snapshot)?;
    let _ = app.emit("config-updated", snapshot);
    Ok(())
}

/// One whole run: draw, watch, decide, write, and say it in a sentence.
///
/// `source` names the caller in the log ("button" or "first-launch re-test") so
/// a support log can tell a user's deliberate check from the automatic one.
#[cfg(windows)]
fn overlay_fix_blocking(app: &tauri::AppHandle, source: &str, hide_when_judged: bool) -> String {
    use tauri::Manager;

    let before = {
        let state: tauri::State<ConfigState> = app.state();
        let mode = state
            .0
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .overlay_compositing
            .clone();
        mode
    };
    // Anything that is not exactly "software" is treated as "auto", matching
    // `compositing_selftest` (PROBLEM 93: a typo or a hand-edit must not
    // disable the remedy, and lib.rs step 4b only adds --disable-gpu for the
    // exact string "software").
    let software = before == "software";
    log::info!(
        "overlay-fix ({source}): starting a ring check — overlay_compositing is currently \
         {before:?} (treated as {})",
        if software { "software" } else { "auto" }
    );

    match overlay_draw_and_watch(app, hide_when_judged) {
        OverlayVerdict::NoVerdict(why) => {
            log::warn!("overlay-fix ({source}): NO VERDICT — {why}");
            why
        }

        // Dead, and we still have a remedy left: fall back to software rendering.
        OverlayVerdict::Dead if !software => {
            match write_compositing(app, "software") {
                Ok(()) => {
                    log::warn!(
                        "overlay-fix ({source}): THE OVERLAY COMPOSED NOTHING. The ring was \
                         visible, nothing was covering it, and the pixels where it sits still \
                         equal the desktop behind them. Switched overlay_compositing \
                         'auto' -> 'software'; --disable-gpu applies at the next launch \
                         (lib.rs step 4b)."
                    );
                    "The ring drew nothing at all — your graphics driver is not showing it, \
                     which is why nothing appears when you hold Space.\n\nSpaceadom has \
                     switched to a backup way of drawing it. Restart to use it."
                        .into()
                }
                Err(e) => {
                    log::error!(
                        "overlay-fix ({source}): the overlay composed nothing, but the new \
                         setting could NOT be saved ({e}) — staying on the graphics card. \
                         Nothing changed."
                    );
                    "The ring drew nothing at all, but Spaceadom could not save the change. \
                     Nothing has been altered — try again in a moment."
                        .into()
                }
            }
        }

        // Dead while ALREADY on the backup: there is no further rendering mode
        // to try, so this tool has nothing left to change. Say so plainly rather
        // than pretending, and name what the app itself will do next — the
        // automatic self-test rebuilds the overlay window after three dead
        // verdicts in software mode (PROBLEM 122), which is the remaining
        // remedy and does not need a restart.
        OverlayVerdict::Dead => {
            log::error!(
                "overlay-fix ({source}): the overlay composed nothing while ALREADY in \
                 software rendering. No rendering mode left to fall back to; \
                 overlay_compositing stays 'software'. The automatic self-test will rebuild \
                 the overlay window after three dead verdicts (PROBLEM 122)."
            );
            "The ring still drew nothing, and Spaceadom is already using the backup way of \
             drawing.\n\nNothing was changed — there is no other drawing mode to try. \
             Spaceadom will rebuild the ring's window by itself the next few times you hold \
             Space. If it never comes back, please send the logs."
                .into()
        }

        // Alive while on the backup: hand drawing back to the graphics card.
        // This is the half PROBLEM 92 existed for, now reachable without the
        // user knowing what compositing is.
        OverlayVerdict::Alive if software => {
            match write_compositing(app, "auto") {
                Ok(()) => {
                    log::warn!(
                        "overlay-fix ({source}): the ring DREW CORRECTLY while in software \
                         rendering — the earlier 'software' verdict no longer holds on this \
                         machine. Switched overlay_compositing 'software' -> 'auto'; the \
                         automatic self-test resumes and will fall back again if it is \
                         wrong."
                    );
                    "The ring drew correctly, and Spaceadom was still using the backup way of \
                     drawing it.\n\nDrawing has been handed back to your graphics card. \
                     Restart to use it — if the ring goes missing again, Spaceadom will \
                     notice and switch back on its own."
                        .into()
                }
                Err(e) => {
                    log::error!(
                        "overlay-fix ({source}): the ring drew correctly in software mode, \
                         but the new setting could NOT be saved ({e}) — staying on the \
                         backup. Nothing changed."
                    );
                    "The ring drew correctly, but Spaceadom could not save the change. \
                     Nothing has been altered — try again in a moment."
                        .into()
                }
            }
        }

        OverlayVerdict::Alive => {
            log::info!(
                "overlay-fix ({source}): the ring drew correctly and overlay_compositing is \
                 already 'auto' — nothing to change."
            );
            "The ring drew correctly and reached the screen.\n\nNothing needed changing. If \
             you still cannot see it while holding Space, it is being drawn somewhere you \
             are not looking — check the other display, or whether another window is on top \
             of it."
                .into()
        }
    }
}

/// "The ring isn't showing?" — the Conflicts-area button.
///
/// ASYNC, and that is load-bearing: it sleeps ~850ms, and a non-async
/// `#[tauri::command]` runs on the MAIN THREAD, where every IPC call from both
/// webviews would queue behind it. The work itself goes to `spawn_blocking` so
/// the sleeps do not sit on an async worker either.
///
/// Returns the sentence to show the user. The frontend decides whether to offer
/// a restart by re-reading `overlay_compositing` from `get_config` and comparing
/// it with what it held BEFORE the call — never by parsing this string, which
/// would be a second source of truth that drifts the first time the copy is
/// edited.
#[tauri::command]
pub async fn run_overlay_fix(app: tauri::AppHandle) -> Result<String, String> {
    #[cfg(windows)]
    {
        match tauri::async_runtime::spawn_blocking(move || {
            overlay_fix_blocking(&app, "button", false)
        })
        .await
        {
            Ok(msg) => Ok(msg),
            Err(e) => {
                log::error!("run_overlay_fix: the check could not be run ({e})");
                Err(format!("the ring check could not be run: {e}"))
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Ok("The ring check only exists on Windows.".into())
    }
}

/// THE ONE-SHOT RE-TEST, run once ever on a machine that is already in software
/// rendering (owner, 2026-09-01).
///
/// WHY IT EXISTS. `overlay_compositing` is written by the self-test and, by
/// design, never switches back on its own. Three of the builds that could write
/// it were wrong in a way we later found and fixed: before PROBLEM 171 the test
/// could not tell "I painted nothing" from "another window is on top of me", so
/// a PiP'd or always-on-top window over the ring scored strikes against a
/// machine whose compositing was perfect. Every user who was flipped to software
/// rendering by that bug is still in it, permanently, with slower drawing and no
/// reason to suspect anything — and the "Software overlay" switch that was their
/// way back has just been removed.
///
/// So: on the FIRST launch after this lands, if the config says "software",
/// re-run the measurement once with the fixed test and keep or revert per the
/// verdict. Loud in the log either way — a setting that changes itself must say
/// so, or the next diagnosis starts from a lie.
///
/// THE MARKER IS WRITTEN BEFORE THE TEST RUNS, deliberately: if the app dies
/// mid-check, the re-test does not fire again at every launch forever. The
/// user's way back is the button, which is always there.
///
/// Called from `lib.rs`'s `create_app_windows`, at the end, where the overlay
/// window is known to exist. One line there; everything else is here.
#[cfg(windows)]
pub fn retest_software_overlay_once(app: &tauri::AppHandle) {
    use tauri::Manager;

    // REVIEW FIXES 2026-09-05 (MEDIUM) — NOT IN SAFE MODE, and this guard has
    // to come before the marker file is written.
    //
    // Safe mode (PROBLEM 253) creates NO overlay window at all. This re-test
    // draws a ring INTO that window and judges the machine's compositing by
    // what comes back. With no window there is nothing to draw into and
    // nothing to read back, so the verdict is not merely unreliable — it is a
    // measurement of the wrong thing, and it would write `overlay_compositing`
    // from it.
    //
    // Worse, the ONE SHOT is spent either way: the marker is written before
    // the test runs (deliberately, so a crash mid-check cannot make the ring
    // reappear at every launch), so a safe-mode launch would consume the only
    // re-test this machine ever gets and record a verdict taken with no
    // overlay. The user's way back would then be the button in Settings and
    // nothing else.
    //
    // Same class as the display watcher's safe-mode skip in `lib.rs`: a
    // recovery mechanism that assumes the overlay exists must not run on the
    // launch that deliberately has no overlay.
    if crate::safe_mode::active() {
        log::warn!(
            "overlay-fix: SAFE MODE — the one-shot overlay re-test was NOT run and its marker \
             was NOT written, so it is still available on the next normal launch. This launch \
             has no overlay window by design ({}), and a compositing verdict measured without \
             one would be written into the config as if it meant something.",
            crate::safe_mode::MARKER
        );
        return;
    }

    let mode = {
        let state: tauri::State<ConfigState> = app.state();
        let m = state
            .0
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .overlay_compositing
            .clone();
        m
    };
    if mode != "software" {
        return; // nothing to correct — the overwhelmingly common case, and free
    }

    let marker = startup::data_dir().join(OVERLAY_RETEST_MARKER);
    if marker.exists() {
        return;
    }
    if let Err(e) = std::fs::write(
        &marker,
        "Spaceadom re-checked the overlay once after the PROBLEM 171 occlusion fix.\r\n\
         Delete this file to have it check again on the next launch.\r\n",
    ) {
        // SKIP rather than run. Without a marker the check would draw a ring on
        // screen at EVERY launch, which is a worse fault than the one it fixes.
        log::warn!(
            "overlay-fix: this machine is in software rendering and has not been re-checked, \
             but the marker file could not be written ({e}) — skipping the one-shot re-test \
             so it cannot repeat at every launch. The 'The ring isn't showing?' button in \
             Settings still runs it on demand."
        );
        return;
    }

    log::warn!(
        "overlay-fix: this machine is in SOFTWARE rendering from an earlier verdict, and has \
         not been re-checked since the occlusion fix (PROBLEM 171). Re-running the ring check \
         once, in the background, after the webview has settled. Whatever it finds will be \
         logged and written here."
    );

    let handle = app.clone();
    std::thread::Builder::new()
        .name("st-overlay-recheck".into())
        .spawn(move || {
            // The overlay window exists, but its PAGE has not loaded yet — a
            // ring drawn into an empty document paints nothing and would score
            // a false 'dead'. The dashboard's own first paint lands well inside
            // this, and a Space-hold during it simply supersedes the check
            // (`begin_hold`), which is the correct outcome: the user's own ring
            // is not something a background test may take away.
            std::thread::sleep(std::time::Duration::from_secs(8));
            let msg = overlay_fix_blocking(&handle, "first-launch re-test", true);
            log::warn!("overlay-fix (first-launch re-test): {msg}");
            // Only speak up if something actually changed. A toast that says
            // "nothing changed" for a check the user never asked for is noise.
            let now = {
                let state: tauri::State<ConfigState> = handle.state();
                let m = state
                    .0
                    .read()
                    .unwrap_or_else(|p| p.into_inner())
                    .overlay_compositing
                    .clone();
                m
            };
            if now != "software" {
                crate::show_toast(
                    &handle,
                    "🔎 The ring works again — restart to use your graphics card",
                );
            }
        })
        .ok();
}

#[cfg(not(windows))]
pub fn retest_software_overlay_once(_app: &tauri::AppHandle) {}

/// "Restart now" — the button the ring check offers when its verdict CHANGED.
///
/// `overlay_compositing` is read once, when the overlay window is created, so a
/// verdict that flips it only takes effect on the next launch. Offering the
/// restart is what makes the check a fix instead of a report.
///
/// **WHY THIS IS NOT `AppHandle::restart()`, which exists and looks right.**
/// Read `tauri::process::restart` (2.11.5, `src/process.rs`): it `spawn`s the
/// new binary and only THEN calls `exit(0)`. This app runs
/// `tauri-plugin-single-instance` — so the new process starts while the old one
/// still holds the instance mutex, forwards its arguments to the dying instance
/// and exits, and a moment later the old process exits too. The user presses
/// "Restart now" and **the app never comes back**, with nothing in the log to
/// say why. That is the whole reason this command exists rather than a one-line
/// `app.restart()`.
///
/// So the order is inverted: a detached `cmd.exe` waits for THIS process to be
/// gone and starts the app afterwards, and we exit immediately.
///
/// * `timeout /t 2` — comfortably longer than the exit below takes, and the
///   mutex is released by the kernel when the process object dies, not by any
///   code of ours that could be skipped.
/// * `start ""` — detaches the app from the `cmd` that launched it, so it does
///   not inherit a console or die with its launcher. The empty `""` is the
///   window TITLE argument; without it `start` reads a quoted path as the title
///   and opens nothing, which is the classic silent failure of this idiom.
/// * `CREATE_NO_WINDOW | DETACHED_PROCESS` — no console flash, and the helper
///   survives this process (`CREATE_NO_WINDOW` alone leaves it in our console
///   group).
///
/// The new instance is started with NO arguments on purpose. Tauri's own
/// restart re-passes `args_os`, which for an autostart launch includes
/// `--autostart` — and this app's single-instance handler returns silently for
/// that flag (PROBLEM 64), so a restart from an autostarted session would come
/// back invisible. A restart is a deliberate user action, and it should land
/// the same way a manual launch does.
///
/// `stop_hook()` first, matching the tray's Exit handler: the low-level hook is
/// torn down by the app, not left to the process teardown, so the incoming
/// instance never briefly shares the keyboard with the outgoing one.
#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;

        let exe = std::env::current_exe()
            .map_err(|e| format!("could not find this app's own path: {e}"))?;
        let exe = exe.to_string_lossy().to_string();

        // Quoted for `start`, which splits on spaces — and this app installs to
        // `%LOCALAPPDATA%\Spaceadom`, a path that contains none today and could
        // contain one tomorrow.
        let line = format!("timeout /t 2 /nobreak >nul & start \"\" \"{exe}\"");
        log::info!("restart: relaunching via a detached waiter — {line}");

        std::process::Command::new("cmd")
            .args(["/C", &line])
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn()
            .map_err(|e| {
                // LOUD, because the alternative is an app that quietly stays
                // shut: nothing below this line runs if the waiter failed.
                log::error!("restart: the relaunch helper could not be started ({e}) — NOT exiting");
                format!("could not schedule the restart: {e}")
            })?;

        log::info!("restart: waiter scheduled — stopping the hook and exiting now");
        crate::hook::stop_hook();
        app.exit(0);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Err("restarting is only implemented on Windows".into())
    }
}

#[cfg(all(test, windows))]
mod ring_layout_name_tests {
    use super::*;

    /// The full truth table, because this function has to agree — value for
    /// value — with `ringLayoutFor` in `src/components/controls.ts`, and
    /// nothing in either language can check the other. The consequence of a
    /// drift is silent and specific: the ring check would draw a shape the user
    /// does not have, then pass or fail their machine on it.
    ///
    /// The `"one"` row is the one worth having. `hud_band_count: "one"` is
    /// retired — the UI never writes it again — but every config a user
    /// selected "1 row" on between 1.0.89 and 1.0.95 still holds it, and both
    /// sides must read it as Compact.
    #[test]
    fn every_config_shape_maps_to_the_pill_the_dashboard_shows() {
        let case = |magnetic: bool, band: &str| {
            let mut cfg = crate::config::AppConfig::default();
            cfg.hud_magnetic_layout = magnetic;
            cfg.hud_band_count = band.into();
            ring_layout_name(&cfg)
        };

        assert_eq!(case(true, "auto"), "compact", "the shipped default");
        assert_eq!(
            case(true, "one"),
            "compact",
            "the RETIRED forced-one value must read as Compact — it is \
             indistinguishable from auto whenever the labels fit, and the next \
             press of the pill normalises it"
        );
        assert_eq!(case(true, "two"), "double");
        assert_eq!(case(false, "auto"), "wide");
        assert_eq!(
            case(false, "two"),
            "wide",
            "the classic ring ignores the band count entirely, so a stored \
             Double must not win over it — a detour through Wide keeps the row \
             count for the way back, it does not change the shape"
        );
        assert_eq!(
            case(false, "one"),
            "wide",
            "same rule with the retired value: magnetic:false decides alone"
        );
    }

    /// Whatever it returns must be a name `PreviewLayout::parse` accepts, or
    /// the ring check abstains instead of drawing anything — a diagnostic that
    /// silently does nothing is the failure this whole region exists to end.
    #[test]
    fn every_name_it_returns_is_one_the_preview_can_parse() {
        for (magnetic, band) in [
            (true, "auto"), (true, "one"), (true, "two"),
            (false, "auto"), (false, "one"), (false, "two"),
            (true, ""), (false, "something-a-future-build-added"),
        ] {
            let mut cfg = crate::config::AppConfig::default();
            cfg.hud_magnetic_layout = magnetic;
            cfg.hud_band_count = band.into();
            let name = ring_layout_name(&cfg);
            assert!(
                crate::engine::PreviewLayout::parse(name).is_some(),
                "ring_layout_name returned {name:?} for ({magnetic}, {band:?}), which the \
                 preview cannot parse — the ring check would draw nothing"
            );
        }
    }
}

#[derive(serde::Deserialize, Clone)]
pub struct ShapeRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Corner radius for THIS pill (CSS px) — pills at different stack depths
    /// have different shapes (squircle → capsule as they age).
    pub r: f64,
}

/// Shape the overlay window to the union of the toast pills (rounded rects,
/// client coordinates in CSS px × dpr). The window is opaque by necessity on
/// this machine, so per-pixel transparency is impossible — but a WINDOW
/// REGION gives real holes: the desktop shows through the gaps between
/// stacked toasts (user request 2026-08-10: "no rectangular box boxing the
/// three toasts together"). Empty rects = clear the region (full window).
#[tauri::command]
pub fn overlay_shape(app: tauri::AppHandle, rects: Vec<ShapeRect>, dpr: f64) {
    use tauri::Manager;
    if crate::guide_hud::is_visible() {
        return; // HUD owns the full window
    }
    let Some(win) = app.get_webview_window("overlay") else { return };
    set_overlay_region(&win, &rects, dpr);
}

/// PROBLEM 206 — the Guide HUD's chip geometry, published by the overlay page
/// once per HUD show (and once per resize rebuild), for pointer activation.
///
/// Convention is deliberately IDENTICAL to `overlay_shape`: rectangles in CSS
/// px relative to the overlay window's client area, plus `dpr`, and Rust
/// multiplies the two to reach physical pixels (`apply_region`'s exact
/// floor/ceil convention). The window is undecorated and shadowless, so its
/// outer position IS its client origin.
///
/// The position is READ BACK from the window, never taken from what a fit
/// requested: the log records asked (202,247) vs GOT (203,247) — a 1px
/// logical rounding artefact that would shear every hit-test rect.
#[tauri::command]
pub fn publish_hud_chips(
    app: tauri::AppHandle,
    chips: Vec<crate::hook::pointer::ChipRectIn>,
    dpr: f64,
) {
    use tauri::Manager;
    let Some(win) = app.get_webview_window("overlay") else {
        return;
    };
    let Ok(pos) = win.outer_position() else {
        // No position, no hit-testing: leaving the previous snapshot up
        // would aim the cursor at rects belonging to an old placement.
        crate::hook::pointer::clear_chips();
        log::warn!("publish_hud_chips: overlay position unreadable — chip snapshot cleared");
        return;
    };
    // PROBLEM 209 — the directional hit-test needs the ring's CENTRE, and the
    // ring is centred on the window's client area (`#st-hud` is
    // `position: fixed; inset: 0`; every chip is placed with `calc(50% + …)`).
    // So the size is now as load-bearing as the position, and it is read back
    // for the same reason: `overlay_fit_hud` may have CLAMPED the requested
    // size to 94% of the monitor, and the page then centres itself in what it
    // actually got. Asking for the size we requested would put the centre off
    // by half the clamp on a small display — i.e. rotate every sector.
    let Ok(size) = win.inner_size() else {
        crate::hook::pointer::clear_chips();
        log::warn!("publish_hud_chips: overlay size unreadable — chip snapshot cleared");
        return;
    };
    crate::hook::pointer::publish_chips(pos.x, pos.y, size.width, size.height, &chips, dpr);
}

/// Show the REAL Guide HUD, in a layout the user has not chosen yet, for about
/// four seconds — so a choice in Settings can be made on the truth instead of
/// on a name.
///
/// `layout` is `"compact"` | `"wide"` | `"double"`; the mapping onto the two
/// settings that actually exist lives in `engine::PreviewLayout::overrides`,
/// and the payload in `engine::preview_payload`. Both are there rather than
/// here so the preview's chip list sits beside the real one and the two can be
/// read against each other.
///
/// THREE PROPERTIES, and each one is somebody's bug if it is missed:
///
/// 1. **It uses the user's real bindings.** The whole question a preview
///    answers is "do MY twenty-six labels fit in this shape". Sample data
///    cannot answer it.
/// 2. **It writes nothing.** The layout override rides in the payload and dies
///    with the show. A preview the owner dismisses must leave his HUD exactly
///    as he found it — and a crash mid-preview must not persist a layout he was
///    only looking at.
/// 3. **It cannot launch anything.** No chip keys and no chip rects are
///    published for a preview (`guide_hud::show_preview_hud` explains the two
///    locks), so there is no armed chip, no beam and nothing for a release or a
///    click to fire. Nothing in `hook/mod.rs` is involved in making that true.
///
/// Returns `Err` for an unrecognised layout name rather than defaulting: the
/// string comes from a button in our own dashboard, so a value we do not know
/// means the two halves disagree, and silently previewing the wrong shape would
/// teach the owner something false about his own app.
#[tauri::command]
pub fn preview_hud_layout(layout: String, state: State<'_, ConfigState>) -> Result<(), String> {
    let Some(which) = crate::engine::PreviewLayout::parse(&layout) else {
        log::warn!(
            "preview_hud_layout: unknown layout {layout:?} — expected \"compact\", \"wide\" or \
             \"double\". Nothing shown; the dashboard and the backend disagree about what the \
             preview buttons are."
        );
        return Err(format!(
            "unknown layout {layout:?} — expected \"compact\", \"wide\" or \"double\""
        ));
    };

    // Built while the read guard is held, then the guard is dropped BEFORE the
    // show: `show_preview_hud` does Win32 window work and emits an event, and
    // holding the config lock across either of those is how a Space-hold ends
    // up waiting on a dashboard click.
    let payload = {
        let cfg = state.0.read().unwrap_or_else(|p| p.into_inner());
        crate::engine::preview_payload(&cfg, which)
    };

    // PROBLEM 177 — stamp it like any other hold. This is what makes "a real
    // Space-hold cleanly supersedes a preview" true rather than hoped for: the
    // stamp moves on the next `begin_hold`/`end_hold` from anywhere, and the
    // preview's own auto-hide then refuses to touch a HUD that is no longer
    // its own.
    let epoch = crate::guide_hud::begin_hold();
    crate::guide_hud::show_preview_hud(epoch, payload);
    Ok(())
}

/// Which monitor the HUD and toasts should appear on.
///
/// **The one under the mouse cursor**, falling back to primary, falling back to
/// whatever monitor exists at all.
///
/// PROBLEM 169. Every overlay placement used `primary_monitor()`, and the HUD
/// was documented as primary-monitor-only "by explicit user decision". The
/// owner plugs a second display in and out through the day, and on 2026-08-24
/// reported the HUD failures were "worse with two displays". They would be:
/// with the dashboard and his work on the external screen, the ring was drawn
/// perfectly — on the laptop panel he was not looking at. Indistinguishable,
/// from where he sat, from "it did not appear".
///
/// He reversed the decision the same day, choosing the CURSOR's screen over
/// the foreground window's. That matches what PiP already does
/// (`MonitorFromPoint(GetCursorPos())`, kept deliberately in the same
/// conversation), so both features now answer "which screen?" the same way and
/// a user only has to learn the rule once.
///
/// THE FALLBACK CHAIN IS NOT DECORATION. `primary_monitor()` returning `None`
/// is exactly what happens for a moment during a display change — a hotplug,
/// a lid close, a resolution switch — and this owner's machine does that
/// several times a day. `place_overlay_centred` used to silently do nothing in
/// that case while `show()` ran anyway, so the HUD painted into whatever box
/// the last toast had left behind: clipped, or a 300px pill somewhere near the
/// bottom of the screen. A placement that cannot find a monitor must say so
/// and still land somewhere sane.
pub(crate) fn overlay_monitor(win: &tauri::WebviewWindow) -> Option<tauri::Monitor> {
    if let Ok(pos) = win.cursor_position() {
        if let Ok(Some(mon)) = win.monitor_from_point(pos.x, pos.y) {
            return Some(mon);
        }
    }
    if let Ok(Some(mon)) = win.primary_monitor() {
        log::debug!("overlay_monitor: no monitor under the cursor — using the primary display");
        return Some(mon);
    }
    let first = win.available_monitors().ok().and_then(|m| m.into_iter().next());
    if first.is_some() {
        log::warn!(
            "overlay_monitor: neither the cursor's monitor nor the primary could be resolved \
             (a display change is probably in flight) — falling back to the first available"
        );
    } else {
        log::error!(
            "overlay_monitor: NO monitor could be resolved at all — the overlay will not be \
             positioned this time. Expect the HUD/toast to be clipped or mis-placed until the \
             display settles."
        );
    }
    first
}

/// Re-raise the overlay to the top of the always-on-top band. **Never replace
/// this with `win.set_always_on_top(true)`.**
///
/// PROBLEM 168. Three call sites used to do exactly that, each with a comment
/// stating the intent — `guide_hud`'s *"Re-assert topmost on EVERY show: other
/// always-on-top windows appearing since the last show can end up above us in
/// the topmost band, and the user requires the HUD over everything"*, and
/// `overlay_fit`/`overlay_fit_handover`'s *"Toasts must sit above everything,
/// same rule as the HUD"*. **Not one of them did anything.**
///
/// tao caches the window's flags and diffs them before touching the OS
/// (`tao-0.35.3/src/platform_impl/windows/window_state.rs`):
///
/// ```text
/// fn apply_diff(mut self, window: HWND, mut new: WindowFlags) {
///     let mut diff = self ^ new;
///     if diff == WindowFlags::empty() { return; }          // <-- line 321
///     ...
///     if diff.contains(WindowFlags::ALWAYS_ON_TOP) {       // <-- line 339
///         SetWindowPos(window, HWND_TOPMOST, ...);
/// ```
///
/// The overlay is created `always_on_top(true)` and never turned off, so the
/// flag is ALREADY set, the diff is empty, and `apply_diff` returns at line 321
/// without reaching the `SetWindowPos` at line 339. Setting a flag to the value
/// it already holds is a no-op — but "re-assert topmost" reads as covered in
/// review, which is exactly why it survived so long.
///
/// The cost was not cosmetic. `SetWindowPos(HWND_TOPMOST)` on a window that is
/// already topmost is NOT a no-op at the OS level: it moves the window to the
/// top of the topmost band. Without it, anything that entered that band after
/// us stayed above us permanently — a media player pinned on top, an installer,
/// or (much more commonly on this machine) a window PiP marked topmost and then
/// failed to release, PROBLEM 167. The owner's report is the exact signature:
/// *"space hud sound appeared but showed nothing, it appeared behind Claude"* —
/// the page ran and played its sound, and the window was simply underneath.
///
/// So this goes straight to Win32 and skips tao's cache entirely.
/// SWP_NOACTIVATE matters: the overlay is NoActivate/click-through and must
/// never take focus. Not SWP_ASYNCWINDOWPOS (which tao uses) — the caller
/// shows the window immediately afterwards and wants the z-order already
/// applied, not posted.
pub(crate) fn raise_overlay_topmost(win: &tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        };
        let Ok(hwnd) = win.hwnd() else {
            log::warn!("raise_overlay_topmost: no HWND — the overlay may sit behind other windows");
            return;
        };
        let raw = windows::Win32::Foundation::HWND(hwnd.0 as *mut _);
        unsafe {
            if let Err(e) = SetWindowPos(
                raw,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            ) {
                log::warn!("raise_overlay_topmost: SetWindowPos failed ({e}) — the HUD/toast may be covered");
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = win.set_always_on_top(true);
    }
}

/// Apply (or clear, with an empty list) a per-pill rounded-rect union region.
/// MUST run on the window's own thread: SetWindowRgn from a foreign thread
/// silently failed to apply on this machine (first attempt, 2026-08-10) —
/// hence the run_on_main_thread marshal. After SetWindowRgn succeeds the
/// SYSTEM owns the region handle — do not delete it.
pub(crate) fn set_overlay_region(win: &tauri::WebviewWindow, rects: &[ShapeRect], dpr: f64) {
    #[cfg(windows)]
    {
        let Ok(hwnd) = win.hwnd() else { return };
        let raw = hwnd.0 as isize;
        let rects: Vec<ShapeRect> = rects.to_vec();
        let _ = win.run_on_main_thread(move || unsafe { apply_region(raw, &rects, dpr) });
    }
    #[cfg(not(windows))]
    {
        let _ = (win, rects, dpr);
    }
}

#[cfg(windows)]
unsafe fn apply_region(hwnd_raw: isize, rects: &[ShapeRect], dpr: f64) {
    use windows::Win32::Graphics::Gdi::{
        CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn, HRGN, RGN_OR,
    };
    let hwnd = windows::Win32::Foundation::HWND(hwnd_raw as *mut _);
    if rects.is_empty() {
        let _ = SetWindowRgn(hwnd, HRGN::default(), true);
        return;
    }
    let region = CreateRectRgn(0, 0, 0, 0);
    for r in rects {
        // Pad 1px outward so the pill's border isn't shaved by rounding.
        let x0 = (r.x * dpr).floor() as i32 - 1;
        let y0 = (r.y * dpr).floor() as i32 - 1;
        let x1 = ((r.x + r.w) * dpr).ceil() as i32 + 1;
        let y1 = ((r.y + r.h) * dpr).ceil() as i32 + 1;
        let rr = ((r.r * 2.0 * dpr) as i32).max(2);
        let piece = CreateRoundRectRgn(x0, y0, x1, y1, rr, rr);
        CombineRgn(region, region, piece, RGN_OR);
        let _ = DeleteObject(piece);
    }
    let res = SetWindowRgn(hwnd, region, true);
    log::info!(
        "overlay_shape: {} pill(s), dpr={dpr}, SetWindowRgn={res}",
        rects.len()
    );
}

/// Hide the overlay window once the toast stack is empty (overlay page calls
/// this after the last toast's exit animation). No-op while the HUD is up.
#[tauri::command]
pub fn overlay_toasts_done(app: tauri::AppHandle) {
    use tauri::Manager;
    if crate::guide_hud::is_visible() {
        log::debug!("overlay_toasts_done: refused — the HUD owns the window right now");
        return;
    }
    if let Some(win) = app.get_webview_window("overlay") {
        // BOTH branches are logged, and the SUCCESS matters more than the
        // refusal. This was a completely silent `win.hide()` — the exact class
        // of silence PROBLEM 135 cost three diagnostic rounds to find, and the
        // project's own window rules say every call that changes what the user
        // can see must say so. This is the single terminal path that takes the
        // overlay down, so "did the stack ever empty?" is answerable only from
        // here.
        log::info!("overlay_toasts_done: the toast stack is empty — hiding the overlay window");
        let _ = win.hide();
        // Clear the toast-shaped region so the next show (HUD or toast)
        // starts from a full rectangular window.
        set_overlay_region(&win, &[], 1.0);
    } else {
        log::warn!("overlay_toasts_done: no 'overlay' window to hide");
    }
}

// ---------------------------------------------------------------------------
// Browser commands
// ---------------------------------------------------------------------------

/// Detect Brave or Chrome installation. Returns path or null.
#[tauri::command]
pub fn find_browser_cmd() -> Option<String> {
    browser::find_browser()
}

/// Validate a user-supplied browser path.
#[tauri::command]
pub fn validate_browser(path: String) -> bool {
    browser::validate_browser_path(&path)
}

/// The OS default browser, for the key editor's paste row (TASK 3, 2026-08-26).
///
/// NOT `find_browser_cmd`, which is a different question with a similar name:
/// that one walks four hardcoded Brave/Chrome install paths and answers "is
/// there a Chromium browser lying around?". This one asks Windows which
/// browser the USER chose, through the same resolver the engine uses when it
/// opens a URL with nothing pinned (`smart_cascade::default_browser_exe` →
/// HKCU UrlAssociations\https\UserChoice → ProgId → shell\open\command). One
/// resolver, so the disc in the editor cannot show a different browser than
/// the key actually opens — a second registry walk that could disagree is
/// PROBLEM 60's whole class of bug.
///
/// Cheap enough to call whenever the editor opens: two registry reads plus one
/// icon extraction, and the icon comes from the SAME `IconCacheState` as
/// `list_start_menu_apps` and `list_browser_profiles`, keyed by exe path — so
/// a browser already drawn anywhere in the app costs nothing to draw again.
/// `None` means no http/https handler is registered (or its command line will
/// not parse), which is the same condition that makes `run_browser` fail.
#[derive(serde::Serialize)]
pub struct DefaultBrowserInfo {
    /// Absolute path to the browser executable.
    pub exe: String,
    /// Human name, e.g. "Edge" — the same naming used by the fallback toasts,
    /// so the editor and the toast never call one browser two things.
    pub name: String,
    /// Base64 PNG, 48px, or `None` if the shell had no image for it.
    pub icon_base64: Option<String>,
}

/// PROBLEM 237 — `async`, on a `spawn_blocking` thread with its own balanced
/// STA (`ComSta`), because the one icon extraction inside is in-process COM
/// and this used to run on the MAIN THREAD with no timing line at all — it
/// was one of the three commands `warmPickerData()` fires together, and the
/// only one that left no trace in the 29.7 s log-silent gap of 2026-09-03
/// 21:17. It logs its duration now. Cheap on a cache hit (two registry reads),
/// one icon otherwise; not worth the picker worker's queue.
#[tauri::command]
pub async fn get_default_browser(
    cache: State<'_, IconCacheState>,
) -> Result<Option<DefaultBrowserInfo>, String> {
    let cache = Arc::clone(&cache.0);
    tauri::async_runtime::spawn_blocking(move || {
        let t0 = std::time::Instant::now();
        let _com = icon_extractor::ComSta::new();
        let info = default_browser_blocking(&cache);
        log::info!(
            "default_browser: resolved in {}ms on thread {} — {}",
            t0.elapsed().as_millis(),
            crate::picker_worker::os_thread_id(),
            match &info {
                Some(i) => format!(
                    "{} ({}), icon: {}",
                    i.name,
                    i.exe,
                    if i.icon_base64.is_some() { "yes" } else { "none" }
                ),
                None => "no http/https handler registered".to_string(),
            }
        );
        info
    })
    .await
    .map_err(|e| format!("default browser lookup could not run: {e}"))
}

/// The body of `get_default_browser`, on whatever thread the caller chose.
fn default_browser_blocking(
    cache: &Arc<Mutex<std::collections::HashMap<String, String>>>,
) -> Option<DefaultBrowserInfo> {
    let exe = crate::engine::actions::smart_cascade::default_browser_exe()?;

    let cached = {
        let lock = cache.lock().unwrap_or_else(|p| p.into_inner());
        lock.get(&exe).cloned()
    };
    let icon_base64 = match cached {
        Some(hit) => Some(hit),
        None => crate::icon_extractor::extract_icon(&exe).inspect(|b64| {
            cache
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(exe.clone(), b64.clone());
        }),
    };

    Some(DefaultBrowserInfo {
        name: crate::browser_profiles::display_name_for_exe(&exe),
        exe,
        icon_base64,
    })
}

// ---------------------------------------------------------------------------
// Conflict detection
// ---------------------------------------------------------------------------

/// Known Windows OS-reserved hotkey combinations that users should be warned about.
const OS_RESERVED: &[(&str, &str)] = &[
    ("Win+D", "Show Desktop"),
    ("Win+L", "Lock Screen"),
    ("Win+E", "File Explorer"),
    ("Win+R", "Run Dialog"),
    ("Win+Tab", "Task View"),
    ("Win+X", "Quick Link Menu"),
    ("Ctrl+Alt+Del", "Security Screen"),
    ("Alt+F4", "Close Window"),
    ("Alt+Tab", "Switch Windows"),
    ("PrintScreen", "Screenshot"),
];

/// Check if a proposed key combo conflicts with a reserved OS hotkey.
#[tauri::command]
pub fn show_conflict_check(key_combo: String) -> ConflictResult {
    let lower = key_combo.to_lowercase();
    for &(combo, desc) in OS_RESERVED {
        if lower.contains(&combo.to_lowercase()) {
            return ConflictResult {
                has_conflict: true,
                conflicting_combo: Some(combo.to_string()),
                description: Some(desc.to_string()),
            };
        }
    }
    ConflictResult {
        has_conflict: false,
        conflicting_combo: None,
        description: None,
    }
}

// ---------------------------------------------------------------------------
// Profile management commands
// ---------------------------------------------------------------------------

/// Create a new empty profile. See `regex_lite` (PROBLEM 197) for what a
/// valid name actually requires — it is far looser than "alphanumeric" now.
#[tauri::command]
pub fn create_profile(
    name: String,
    state: State<'_, ConfigState>,
) -> Result<(), String> {
    if !regex_lite(&name) {
        return Err("Profile name must be 1–24 characters, not blank".into());
    }
    // Store the TRIMMED name, not the raw one `regex_lite` validated. Storing
    // "Foo " and later comparing against a freshly-typed "Foo" would make two
    // profiles that look identical in the pill but are != to every
    // name-keyed lookup in this file (PROBLEM 85's exact failure mode, from a
    // different cause) — validate and store the SAME string.
    let name = name.trim().to_string();

    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    if cfg.profiles.iter().any(|p| p.name == name) {
        return Err(format!("Profile '{name}' already exists"));
    }

    let mut bindings = crate::config::BindingMap::new();
    for c in 'a'..='z' {
        bindings.insert(c.to_string(), crate::config::KeyBinding::default());
    }

    // `emoji: None` — a brand-new profile has none until the user picks one,
    // and `Profile::emoji`'s contract is that None means "render the existing
    // look" everywhere. (Completed here when the field landed mid-flight.)
    cfg.profiles.push(Profile { name, bindings, emoji: None });
    let snapshot = cfg.clone();
    drop(cfg);
    config::save(&snapshot)
}

/// Delete a profile by name. Cannot delete the last remaining profile.
#[tauri::command]
pub fn delete_profile(
    name: String,
    state: State<'_, ConfigState>,
) -> Result<(), String> {
    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    apply_profile_delete(&mut cfg, &name)?;
    let snapshot = cfg.clone();
    drop(cfg);
    config::save(&snapshot)
}

/// The guard-then-effects core of `delete_profile`, extracted the same way
/// `apply_profile_rename`/`apply_profile_reorder` are — so the ORDER, not
/// just the outcome, can be asserted by a test instead of only by hand (see
/// `delete_profile_tests` below).
///
/// PROBLEM 99 — deleting a user-created profile destroys bindings and
/// custom icons that exist in NO other copy. Stash before touching it.
/// PROBLEM 105 — deleting the FALLBACK profile is not like deleting any
/// other one: every key left unassigned in every REMAINING profile is
/// rerouted here, so they all silently stop working. The damage shows up
/// later, in a different profile, with nothing connecting it to this act.
///
/// The warning rides on the UNDO LABEL rather than a confirm dialog. A
/// `window.confirm` was tried first and never appeared — this webview does
/// not render native script dialogs, which is why every other destructive
/// control in this app uses a two-step "Confirm" button instead. A warning
/// the user cannot see is the same as no warning, and it is worse than
/// none because it looks like the job was done.
/// 1.0.96 — and a FILE on disk, not only the in-memory undo above.
///
/// The undo stack answers the mis-click you notice in ten seconds. It does
/// not answer the other case, which is the one that actually cost this user
/// a config once (PROBLEM 94): noticing next week. By then the stack is
/// gone and `config-*.json` has been pruned past it. One file, named after
/// the profile, in the folder that deliberately survives an uninstall.
///
/// 1.0.96 review fix — the guard below MUST run before either side effect
/// that follows it. It used to run after the backup file was written and
/// the undo stack was stashed, so a refused delete (the last-remaining-
/// profile case) still left a stray `config-*.json` backup on disk and
/// clobbered whatever undo entry was already there — a caller sees Err and
/// reasonably assumes nothing happened, but something did.
fn apply_profile_delete(cfg: &mut AppConfig, name: &str) -> Result<(), String> {
    // Count what would REMAIN, not the total (PROBLEM 85): rename_profile
    // historically allowed duplicate names, and `retain` removes EVERY
    // profile with the given name — with two profiles both called "X",
    // `len() <= 1` passed, retain emptied the Vec, and `profiles[0]` panicked
    // while HOLDING the config write lock.
    let remaining = cfg.profiles.iter().filter(|p| p.name != name).count();
    if remaining == 0 {
        return Err("Cannot delete the last remaining profile".into());
    }

    // BEFORE the delete, and best-effort: a backup failure must never block a
    // delete the user asked for.
    if let Some(target) = cfg.profiles.iter().find(|p| p.name == name) {
        let _ = config::write_profile_backup(target);
    }

    let is_fallback = name == crate::config::schema::FALLBACK_PROFILE;
    let undo_label = if is_fallback {
        format!(
            "Deleted '{name}' — the fallback profile. Keys you have not assigned in your \
             other profiles were rerouted here, so those keys will now do nothing"
        )
    } else {
        format!("Deleted the profile '{name}'")
    };
    // PROBLEM 106 — the fallback warning is the longest text the app shows in
    // an undo banner, so it gets the longest window to read it in.
    // PROBLEM 106 — 10s for a profile the user just made, 20s for a stock one,
    // 30s for the fallback (longest to read, most to break).
    stash_undo_for(&undo_label, cfg, undo_window_for_profile(name));

    if is_fallback {
        log::warn!(
            "delete_profile: '{name}' is the FALLBACK profile — keys left unassigned in other \
             profiles are rerouted here and will now do nothing. Undo is available for 10 \
             seconds; recreating a profile with this exact name also restores the behaviour."
        );
    }

    cfg.profiles.retain(|p| p.name != name);
    // If deleted profile was active, switch to first
    if cfg.active_profile == name {
        if let Some(first) = cfg.profiles.first() {
            cfg.active_profile = first.name.clone();
        }
    }
    Ok(())
}

/// PROBLEM 85 (root cause half) — renaming B to A's name used to create
/// TWO profiles called "A": every name-keyed lookup became ambiguous, and
/// delete-by-name removed both at once. create_profile always had this
/// guard; rename never did. `new_name != old_name` keeps a same-name
/// rename a no-op instead of an error.
///
/// Pure `&mut AppConfig` logic, extracted out of `rename_profile` so the two
/// guarantees that matter most — `active_profile` follows a rename of the
/// profile it points at, and bindings are untouched by a rename — can be
/// asserted by a unit test instead of only by hand on the real machine (see
/// `profile_rename_tests` below). No live Tauri `State` is needed to call it.
fn apply_profile_rename(
    cfg: &mut AppConfig,
    old_name: &str,
    new_name: &str,
) -> Result<(), String> {
    if new_name != old_name && cfg.profiles.iter().any(|p| p.name == new_name) {
        return Err(format!("Profile '{new_name}' already exists"));
    }
    let profile = cfg
        .profiles
        .iter_mut()
        .find(|p| p.name == old_name)
        .ok_or_else(|| format!("Profile '{old_name}' not found"))?;
    profile.name = new_name.to_string();

    // Same `cfg`, same pass as the rename above: no reader can ever observe
    // a profile renamed but `active_profile` still pointing at the old name
    // (or the other way around), because both mutations land before the
    // caller's single `config::save`.
    if cfg.active_profile == old_name {
        cfg.active_profile = new_name.to_string();
    }
    Ok(())
}

/// Rename an existing profile.
#[tauri::command]
pub fn rename_profile(
    old_name: String,
    new_name: String,
    state: State<'_, ConfigState>,
) -> Result<(), String> {
    if !regex_lite(&new_name) {
        return Err("Profile name must be 1–24 characters, not blank".into());
    }
    // Same reasoning as create_profile: store what was validated.
    let new_name = new_name.trim().to_string();

    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    apply_profile_rename(&mut cfg, &old_name, &new_name)?;
    let snapshot = cfg.clone();
    drop(cfg);
    config::save(&snapshot)
}

/// Validate a profile name string without regex crate dependency.
/// PROBLEM 197 — this used to accept only `[a-zA-Z0-9_]`, and there was never
/// a reason for it. The owner, 2026-08-26: *"why is new profile name
/// restricted to only letters, numbers or underscore? People might want to
/// name with space or dash or anything."*
///
/// Traced every use of a profile name in this codebase before answering: it
/// is a plain JSON string field, compared with `==`, and on the frontend
/// written into exactly one HTML `data-*` attribute
/// (`row.dataset.profileName`), which accepts any string with no escaping
/// needed. It is never a filename, a registry key, a shell argument, or a CSS
/// selector — nothing that has real character-set rules. `install-v11.ps1`,
/// the AutoHotkey original this app ports, never even had custom profile
/// names — just three hardcoded ones compared as string literals — so the
/// restriction was not inherited from a real constraint there either. It
/// looks like a generic "identifier-safe" habit applied when CUSTOM profiles
/// were added later, never revisited.
///
/// Now: any length-1-24 string, trimmed, that is not all whitespace and
/// contains no control characters (0x00-0x1F, 0x7F/DEL) — a control
/// character could still break the single-line pill it is displayed in, or
/// embed a stray tab/newline nobody meant to type. Everything else —
/// spaces, dashes, punctuation, accented letters, emoji — is fine, because
/// nothing downstream has ever needed it not to be.
fn regex_lite(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && trimmed.chars().count() <= 24
        && trimmed.chars().all(|c| !c.is_control())
}

// ---------------------------------------------------------------------------
// Profile editor — reorder, duplicate, emoji, export/import (1.0.96)
// ---------------------------------------------------------------------------

/// Put `cfg.profiles` into exactly the order `names` gives.
///
/// **THE VALIDATION IS THE POINT, AND IT IS A SET COMPARISON, NOT A LENGTH
/// CHECK.** The frontend sends a list it built by reading the DOM after a
/// drag, and the DOM can be stale in ways nothing on that side can detect: a
/// second window, a `config-updated` event that landed mid-drag, an undo that
/// restored a profile while the popover was open. A permissive reorder that
/// "did its best" with a mismatched list would silently DELETE whatever it did
/// not find and duplicate whatever appeared twice — a destructive outcome from
/// a cosmetic gesture, and the config is saved immediately after.
///
/// So: same count, same names, no duplicates, or nothing happens at all. The
/// caller re-reads the config and re-renders.
///
/// Pure `&mut AppConfig` so the rule can be tested without a live Tauri
/// `State`, the same split `apply_profile_rename` uses above.
fn apply_profile_reorder(cfg: &mut AppConfig, names: &[String]) -> Result<(), String> {
    if names.len() != cfg.profiles.len() {
        return Err(format!(
            "The profile order does not match: {} name(s) sent, {} profile(s) exist. \
             Nothing was reordered.",
            names.len(),
            cfg.profiles.len()
        ));
    }
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for n in names {
        if !seen.insert(n.as_str()) {
            return Err(format!("'{n}' appears twice in the new order. Nothing was reordered."));
        }
        if !cfg.profiles.iter().any(|p| &p.name == n) {
            return Err(format!("There is no profile called '{n}'. Nothing was reordered."));
        }
    }

    // Drain and re-place, so ownership moves once and no profile is cloned.
    // `expect` cannot fire: every name was proven present above, and the names
    // are unique, so each `remove` finds its own profile.
    let mut old: Vec<Profile> = cfg.profiles.drain(..).collect();
    let mut ordered: Vec<Profile> = Vec::with_capacity(names.len());
    for n in names {
        let i = old
            .iter()
            .position(|p| &p.name == n)
            .expect("validated above: every name exists exactly once");
        ordered.push(old.remove(i));
    }
    cfg.profiles = ordered;
    Ok(())
}

/// Reorder the profiles. **This is a BEHAVIOUR change, not a display one.**
///
/// `AppConfig::profiles` is the order RAlt cycles in, and it is also the order
/// `delete_profile` falls back through (`profiles.first()`). Dragging a row in
/// the popover therefore changes what the next RAlt press does — which is why
/// the drag handle only exists in edit mode, and why this validates so hard.
#[tauri::command]
pub fn reorder_profiles(
    names: Vec<String>,
    state: State<'_, ConfigState>,
) -> Result<(), String> {
    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    apply_profile_reorder(&mut cfg, &names)?;
    log::info!("reorder_profiles: profile order is now [{}]", names.join(", "));
    let snapshot = cfg.clone();
    drop(cfg);
    config::save(&snapshot)
}

/// The name a copy of `base` should get, given every name already taken.
///
/// "Name 2", then "Name 3", … — the owner's chosen shape. Two details that a
/// naive `format!("{base} 2")` gets wrong:
///
/// * **Duplicating a copy must not stutter.** Copying "Work 2" gives "Work 3",
///   not "Work 2 2". The trailing number is treated as a copy index, so the
///   stem is recovered before counting.
/// * **The result must still be a legal name.** `regex_lite` caps names at 24
///   CHARACTERS, and "…extremely long name… 2" can exceed that — at which
///   point `create`-style validation would reject a name the user never typed.
///   The stem is truncated to make room for the suffix instead.
fn unique_copy_name(existing: &[String], base: &str) -> String {
    // Recover the stem: "Work 2" -> "Work", "Work" -> "Work". Only a trailing
    // run of ASCII digits after a single space counts, so "Half Life 2" keeps
    // its 2 the first time and becomes "Half Life 3" the second — which is the
    // same rule, applied honestly, and is what the user sees either way.
    let stem = match base.rsplit_once(' ') {
        Some((head, tail))
            if !head.is_empty() && !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) =>
        {
            head
        }
        _ => base,
    };

    for n in 2..1000 {
        let suffix = format!(" {n}");
        // By CHARACTERS, matching regex_lite's `chars().count()` — an emoji in
        // a profile name must cost 1 toward the cap on both sides, or the two
        // checks disagree on exactly the names that are hardest to type again.
        let room = 24usize.saturating_sub(suffix.chars().count());
        let trimmed_stem: String = stem.chars().take(room).collect();
        let candidate = format!("{}{suffix}", trimmed_stem.trim_end());
        if !existing.iter().any(|e| e == &candidate) {
            return candidate;
        }
    }
    // 998 copies of one profile. Unreachable in practice; still not a panic.
    format!("{} copy", stem.chars().take(19).collect::<String>().trim_end())
}

/// Deep-copy a profile, name the copy, and put it directly AFTER the original.
///
/// "Directly after" is not a nicety: the vec order is the RAlt cycle order, so
/// a copy appended to the end would sit somewhere unrelated to the thing it
/// was copied from. Returns the new name so the frontend can name it in the
/// toast without guessing at the numbering rule.
#[tauri::command]
pub fn duplicate_profile(
    name: String,
    state: State<'_, ConfigState>,
) -> Result<String, String> {
    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    let Some(index) = cfg.profiles.iter().position(|p| p.name == name) else {
        return Err(format!("Profile '{name}' not found"));
    };

    let taken: Vec<String> = cfg.profiles.iter().map(|p| p.name.clone()).collect();
    let new_name = unique_copy_name(&taken, &name);

    // A full clone — bindings, icon overrides, browser pins, emoji. This is
    // the whole reason the command exists in Rust rather than as a frontend
    // loop: `KeyBinding` has grown three optional fields since 1.0.94, and a
    // frontend copy that named its fields would drop the next one added.
    let mut copy = cfg.profiles[index].clone();
    copy.name = new_name.clone();
    cfg.profiles.insert(index + 1, copy);

    log::info!(
        "duplicate_profile: '{name}' -> '{new_name}' ({} binding(s)), placed at position {}",
        cfg.profiles[index + 1].bindings.len(),
        index + 1
    );
    let snapshot = cfg.clone();
    drop(cfg);
    config::save(&snapshot)?;
    Ok(new_name)
}

/// Set or clear a profile's emoji. `None` (or a blank string) clears it.
///
/// Validation is `schema::emoji_is_valid` — ONE grapheme cluster, which is not
/// one `char` and not a byte budget. See that function for why 👨‍👩‍👧 must
/// be accepted and "AB" must not.
#[tauri::command]
pub fn set_profile_emoji(
    name: String,
    emoji: Option<String>,
    state: State<'_, ConfigState>,
) -> Result<(), String> {
    let cleaned = match emoji.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(e) if crate::config::schema::emoji_is_valid(e) => Some(e.to_string()),
        Some(e) => {
            return Err(format!(
                "That is not a single emoji ({} character(s)). Pick one from the panel, or \
                 use Clear to remove it.",
                e.chars().count()
            ))
        }
    };

    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    let Some(p) = cfg.profiles.iter_mut().find(|p| p.name == name) else {
        return Err(format!("Profile '{name}' not found"));
    };
    p.emoji = cleaned.clone();
    log::info!(
        "set_profile_emoji: '{name}' -> {}",
        cleaned.as_deref().unwrap_or("(cleared)")
    );
    let snapshot = cfg.clone();
    drop(cfg);
    config::save(&snapshot)
}

/// Open Windows' own emoji panel by injecting Win+period.
///
/// **IMPORT-BEFORE-IMPLEMENT, and this is what the import found:** Windows
/// exposes no API for "show the emoji panel". `Win`+`.` is the documented
/// gesture and the only route, so the app performs the gesture the user would.
///
/// Three hard rules from the keyboard-hook laws, all of them met here:
///
/// * **One `SendInput` batch** for the first attempt. `SendInput` then
///   `CallNextHookEx` does not preserve order (the `hte`-for-`the` bug), and a
///   Win-down that arrives after the period is Start-menu-opens, not
///   emoji-panel.
/// * **Cookie-tagged `0x7A7A7A7A`**, via `hook::send_keys_checked`, so our own
///   hook passes it through instead of re-processing it as a user keystroke.
///   (The cookie early return in the hook callback sits ABOVE
///   `track_modifier` — read, not assumed — so injecting LWIN here cannot make
///   the hook's modifier mask believe a Windows key is physically held. That
///   ordering is PROBLEM 230's, and it is what makes this command safe to call
///   while the hook is live.)
/// * **No modifier left down.** The batch releases LWIN itself, and
///   `send_keys_checked` sends corrective KEYUPs for anything a PARTIAL insert
///   latched (PROBLEM 227) — a stuck Windows key is not a cosmetic failure.
///
/// Engine-side, never in a hook callback: this is a `#[tauri::command]`, so it
/// runs on Tauri's thread, and `send_keys_checked` logs (which the hook
/// callback may never do — PROBLEM 58).
///
/// The panel targets whatever has KEYBOARD FOCUS, which is the dashboard's own
/// emoji input — the frontend focuses it before calling this.
///
/// # 2026-09-05 — PROBLEM 248. What was actually wrong, and what is still a guess
///
/// **Symptom (owner, 1.0.100):** clicking the emoji disc opened no panel.
/// **What the log already proved, before a line of this was written:**
/// `open_emoji_panel` DID run on the click (08:30:47 on 2026-09-05), the
/// frontend path is therefore fine, `SendInput` reported every event inserted,
/// and the hook's own `primary_injected:4` counter for that same 90-second
/// window proved all four events genuinely entered the input stream and were
/// passed through by our cookie branch. So this is not a frontend bug, not a
/// blocked injection, and not our hook eating it. **The shell received the
/// chord and declined it**, and the old `inserted=true` log line could not
/// tell those cases apart — it reported `SendInput`'s return value while
/// sounding like a report about a panel.
///
/// **Honest state of the diagnosis.** A web review of the public record
/// (2026-09-05) found NO primary source confirming any of the popular
/// explanations: not "you must fill `wScan`", not "`ctfmon`/`TextInputHost`
/// filters `LLKHF_INJECTED` or `dwExtraInfo`", not "the four events must be
/// split across `SendInput` calls with a delay". Those are folk claims. What
/// the record DOES corroborate, from multiple independent sources, is that
/// Win+`.` silently does nothing — for a physical press too — when the **Touch
/// Keyboard and Handwriting Panel Service (`TabletInputService`)** is not
/// running, and that the hotkey has an HKLM kill switch
/// (`SOFTWARE\Microsoft\Input\Settings\proc_1\loc_<LCID>\im_1` →
/// `EnableExpressiveInputShellHotkey`, locale-keyed, mirrored under
/// `WOW6432Node`). So the code below does two things, in this order of
/// confidence: it makes the injection as close to a physical press as an
/// injection can be, and it MEASURES whether a panel appeared.
///
/// **The change to the chord: scan codes.** Every event now carries
/// `wScan = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC)` alongside its virtual key,
/// and LWIN carries `KEYEVENTF_EXTENDEDKEY` because LWIN is `E0 5B` on a real
/// keyboard. This is a HYPOTHESIS, labelled as one: it costs nothing, it
/// removes the one respect in which our chord did not look like a keypress,
/// and if it is what fixes it, the probe below says so. It is not being
/// claimed as the root cause.
///
/// **`hook::kbd_input` is deliberately NOT reused, and the duplication a
/// reviewer flagged is now a real behavioural difference rather than an
/// oversight.** `kbd_input` is called from INSIDE the hook callback
/// (`inject_space`, `inject_space_then_key`), where `MapVirtualKeyW` would put
/// a syscall on the 300 ms `LowLevelHooksTimeout` path that PROBLEM 58 is
/// about, and where vk-only injection has been correct for this app's entire
/// life. Widening the shared helper would change how EVERY space and rollover
/// injection is delivered in order to test one command that never runs on that
/// path. The chord builder therefore lives beside its only caller.
///
/// **Why this now proves something.** Nobody working on this can see the
/// owner's screen, so the command snapshots every visible top-level window and
/// the foreground HWND, injects, then watches for 500 ms on a background
/// thread for either a NEW visible top-level window or a foreground change —
/// logging the class, title and PID of whatever appeared. The test is
/// deliberately broad: it does not depend on guessing the panel's window class
/// correctly, only on the panel being a window that was not visible a moment
/// earlier. (For reference, on Windows 11 the panel is class
/// `Windows.UI.Core.CoreWindow`, title `Windows Input Experience`, owned by
/// `TextInputHost.exe` — evidenced by NVDA's own window dumps, nvaccess/nvda
/// #15836. "Microsoft Text Input Application" is Task Manager's friendly
/// PROCESS name, not the window title, which is why the class/title/PID are
/// all logged rather than matched.)
///
/// **The spaced retry.** If 500 ms pass with no new window AND the foreground
/// never moved, the panel demonstrably did not open, so a second attempt is
/// free and doubles what a single owner click tells us: LWIN-down, 30 ms,
/// period-down/up, 30 ms, LWIN-up as three separate `SendInput` calls. This is
/// a deliberate, narrow exception to the one-batch law, and the law is
/// narrower than it reads: one batch exists so two ORDERED CHARACTERS cannot
/// be reordered by `CallNextHookEx` re-entry (the `hte`-for-`the` bug). A
/// modifier chord has no such race — order inside a single `SendInput` array
/// is guaranteed either way, and what a shell hotkey handler reads is the
/// modifier's HELD STATE when the period arrives. Splitting the chord cannot
/// scramble anything; it can only give an asynchronous handler time to settle.
/// Two residual risks, both named rather than hidden: LWIN is held across two
/// sleeps, so a crash inside that 60 ms window would leave it latched (the
/// release is unconditional and `send_keys_checked` repairs partial inserts,
/// which is as safe as this can be made); and if attempt A had actually opened
/// the panel and the probe missed it, attempt B would TOGGLE it shut — which
/// is exactly why the retry requires BOTH negatives, a window diff AND an
/// unchanged foreground, instead of one.
#[tauri::command]
pub fn open_emoji_panel() -> Result<(), String> {
    #[cfg(windows)]
    {
        let where_from = emoji_probe::foreground_note();
        let before = emoji_probe::visible_top_level();
        let fg_before = emoji_probe::foreground_raw();

        // Win↓  .↓  .↑  Win↑ — one batch, nothing left held.
        let inputs = [
            emoji_probe::chord_key(emoji_probe::VK_LWIN, false),
            emoji_probe::chord_key(emoji_probe::VK_OEM_PERIOD, false),
            emoji_probe::chord_key(emoji_probe::VK_OEM_PERIOD, true),
            emoji_probe::chord_key(emoji_probe::VK_LWIN, true),
        ];
        let ok = unsafe { hook::send_keys_checked(&inputs, "emoji panel: Win+.") };
        log::info!(
            "emoji panel: injected Win+. ({}) with scan codes lwin=0x{:02X} period=0x{:02X} \
             while {where_from}. This line reports SendInput and nothing else — a panel that \
             fails to appear after it is NOT an injection failure, because the events were \
             inserted and our own hook's `primary_injected` counter will show them passing \
             through. The `emoji panel: OPENED` / `NO PANEL` line that follows within ~1.2 s \
             is the actual verdict.",
            if ok { "4/4" } else { "PARTIAL — fewer than 4 of 4" },
            emoji_probe::scan_of(emoji_probe::VK_LWIN),
            emoji_probe::scan_of(emoji_probe::VK_OEM_PERIOD),
        );
        if !ok {
            return Err(
                "Windows blocked the keystroke, so the emoji panel did not open. Press \
                 Windows + . yourself — the box is already focused and will take it."
                    .into(),
            );
        }

        // Off the command thread: the frontend's `invoke` must not wait a
        // second for a diagnostic, and the emoji input must keep focus while
        // the panel opens.
        let _ = std::thread::Builder::new()
            .name("st-emoji-probe".into())
            .spawn(move || emoji_probe::watch_then_maybe_retry(before, fg_before));

        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("The emoji panel is a Windows feature.".into())
    }
}

/// Everything the emoji chord needs that is NOT shared with the hook's own
/// injection path, plus the instrument that says whether a panel appeared.
/// Private and Windows-only — see `open_emoji_panel`'s doc comment for why
/// none of this is folded into `hook::kbd_input`.
#[cfg(windows)]
mod emoji_probe {
    use crate::hook;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyW, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VIRTUAL_KEY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowTextW,
        GetWindowThreadProcessId, IsWindowVisible, GUITHREADINFO,
    };

    pub(super) const VK_LWIN: u16 = 0x5B;
    pub(super) const VK_OEM_PERIOD: u16 = 0xBE;

    /// The same cookie every other synthetic key in this app carries, so our
    /// own hook's early return passes these through (hook law 1).
    const MAGIC_INJECTED: usize = 0x7A7A7A7A;

    /// How long to wait for the panel before calling it a no-show.
    /// `TextInputHost.exe` is normally already running, so this is generous
    /// rather than tight — a false "no panel" would send the next reader down
    /// the wrong road entirely.
    const PROBE_MS: u64 = 500;
    const PROBE_STEP_MS: u64 = 25;

    /// Scan code for a virtual key on the CURRENT layout. 0 when the layout
    /// has no key for it — which is worth logging rather than papering over,
    /// and is also exactly the value the pre-PROBLEM-248 code sent for everything.
    pub(super) fn scan_of(vk: u16) -> u16 {
        unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as u16 }
    }

    /// One synthetic key event for the emoji chord: virtual key AND scan code,
    /// which is the single behavioural difference from `hook::kbd_input`.
    pub(super) fn chord_key(vk: u16, up: bool) -> INPUT {
        let mut flags = 0u32;
        if up {
            flags |= KEYEVENTF_KEYUP.0;
        }
        // LWIN is an EXTENDED key (E0 5B) on every PC layout. A physical press
        // sets this bit; filling `wScan` while omitting it would describe a key
        // that does not exist, which is worse than sending no scan code at all.
        // VK_OEM_PERIOD (main keyboard, not the numpad) is not extended.
        if vk == VK_LWIN {
            flags |= KEYEVENTF_EXTENDEDKEY.0;
        }
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: scan_of(vk),
                    dwFlags: KEYBD_EVENT_FLAGS(flags),
                    time: 0,
                    dwExtraInfo: MAGIC_INJECTED,
                },
            },
        }
    }

    fn class_of(hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        let n = unsafe { GetClassNameW(hwnd, &mut buf) };
        if n <= 0 {
            return "(no class)".into();
        }
        String::from_utf16_lossy(&buf[..n as usize])
    }

    fn title_of(hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
        if n <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..n as usize])
    }

    fn pid_of(hwnd: HWND) -> u32 {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        pid
    }

    pub(super) fn foreground_raw() -> isize {
        unsafe { GetForegroundWindow().0 as isize }
    }

    /// `foreground=<class> "<title>" pid=N (OUR OWN WINDOW) focus=<class>`.
    ///
    /// `focus` comes from `GetGUIThreadInfo` on the FOREGROUND thread, not
    /// `GetFocus()` — `GetFocus` only ever answers for the calling thread's own
    /// message queue, so from a Tauri command thread it returns null and would
    /// have logged a confident "no focused control" for a correctly focused
    /// input box. A check that cannot produce a true positive is not a check.
    pub(super) fn foreground_note() -> String {
        unsafe {
            let fg = GetForegroundWindow();
            if fg.0.is_null() {
                return "foreground=<none> focus=<none>".into();
            }
            let mut pid = 0u32;
            let tid = GetWindowThreadProcessId(fg, Some(&mut pid));
            let ours = pid == windows::Win32::System::Threading::GetCurrentProcessId();
            let mut gti = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            let focus = if GetGUIThreadInfo(tid, &mut gti).is_ok() && !gti.hwndFocus.0.is_null() {
                class_of(gti.hwndFocus)
            } else {
                "(the foreground thread reports no focused control)".to_string()
            };
            format!(
                "foreground={} \"{}\" pid={pid} {} focus={focus}",
                class_of(fg),
                title_of(fg),
                if ours {
                    "(OUR OWN WINDOW — so the panel would target our emoji input)"
                } else {
                    "(NOT OURS — the click that started this did not leave us in front)"
                },
            )
        }
    }

    unsafe extern "system" fn collect_visible(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = &mut *(lparam.0 as *mut Vec<isize>);
        if IsWindowVisible(hwnd).as_bool() {
            out.push(hwnd.0 as isize);
        }
        BOOL(1)
    }

    /// Every visible top-level window, as raw handles, sorted so the "what is
    /// new" diff is a membership test rather than an ordering question.
    pub(super) fn visible_top_level() -> Vec<isize> {
        let mut v: Vec<isize> = Vec::with_capacity(160);
        unsafe {
            // EnumWindows is synchronous, so a pointer to this local cannot
            // outlive the call (the same argument smart_cascade's enumerations
            // make, and the reason none of them box the payload).
            let _ = EnumWindows(
                Some(collect_visible),
                LPARAM(&mut v as *mut Vec<isize> as isize),
            );
        }
        v.sort_unstable();
        v
    }

    /// Poll for up to `PROBE_MS` for evidence the gesture landed. Returns a
    /// description of what appeared, or `None` for a clean no-show.
    fn wait_for_evidence(before: &[isize], fg_before: isize) -> Option<String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(PROBE_MS);
        loop {
            let fresh: Vec<isize> = visible_top_level()
                .into_iter()
                .filter(|h| before.binary_search(h).is_err())
                .collect();
            if !fresh.is_empty() {
                let parts: Vec<String> = fresh
                    .iter()
                    .take(6)
                    .map(|h| {
                        let hwnd = HWND(*h as *mut std::ffi::c_void);
                        format!(
                            "class={} title=\"{}\" pid={}",
                            class_of(hwnd),
                            title_of(hwnd),
                            pid_of(hwnd)
                        )
                    })
                    .collect();
                return Some(format!(
                    "{} window(s) became visible that were not visible before the chord: {}",
                    fresh.len(),
                    parts.join(" | ")
                ));
            }
            let fg_now = foreground_raw();
            if fg_now != fg_before {
                let hwnd = HWND(fg_now as *mut std::ffi::c_void);
                return Some(format!(
                    "no newly-visible window, but the FOREGROUND moved to class={} title=\"{}\" \
                     pid={}",
                    class_of(hwnd),
                    title_of(hwnd),
                    pid_of(hwnd)
                ));
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(PROBE_STEP_MS));
        }
    }

    /// `HKLM\SYSTEM\CurrentControlSet\Services\TabletInputService\Start`:
    /// 2 = automatic, 3 = manual, 4 = DISABLED. Read only on the failure path,
    /// because it is the one cause of "Win+. does nothing" that the public
    /// record actually corroborates from several independent sources — and a
    /// 4 here means the shortcut is dead for a physical press too, which would
    /// make every line of injection work above irrelevant.
    fn tablet_input_service_start() -> Option<u32> {
        use windows::core::w;
        use windows::Win32::System::Registry::{
            RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD,
        };
        let mut val: u32 = 0;
        let mut cb: u32 = std::mem::size_of::<u32>() as u32;
        let rc = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                w!("SYSTEM\\CurrentControlSet\\Services\\TabletInputService"),
                w!("Start"),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut val as *mut u32 as *mut std::ffi::c_void),
                Some(&mut cb),
            )
        };
        if rc.is_ok() {
            Some(val)
        } else {
            None
        }
    }

    /// The spaced chord: three `SendInput` calls with 30 ms between them, for
    /// a shell handler that might sample the modifier state asynchronously.
    /// See `open_emoji_panel`'s doc comment for why this is a justified,
    /// narrow exception to the one-batch law and what the residual risks are.
    fn inject_spaced() -> bool {
        let gap = std::time::Duration::from_millis(30);
        let down = [chord_key(VK_LWIN, false)];
        let tap = [
            chord_key(VK_OEM_PERIOD, false),
            chord_key(VK_OEM_PERIOD, true),
        ];
        let up = [chord_key(VK_LWIN, true)];
        unsafe {
            let a = hook::send_keys_checked(&down, "emoji panel: LWIN down (spaced)");
            std::thread::sleep(gap);
            let b = hook::send_keys_checked(&tap, "emoji panel: period tap (spaced)");
            std::thread::sleep(gap);
            // UNCONDITIONAL, and it must stay that way: even if the tap was
            // blocked, LWIN has to come back up. A stuck Windows key is the one
            // outcome this whole path is written to avoid.
            let c = hook::send_keys_checked(&up, "emoji panel: LWIN up (spaced)");
            a && b && c
        }
    }

    /// Watch, report, and — only on a double negative — try once more.
    pub(super) fn watch_then_maybe_retry(before: Vec<isize>, fg_before: isize) {
        if let Some(what) = wait_for_evidence(&before, fg_before) {
            log::info!(
                "emoji panel: OPENED — within {PROBE_MS} ms of the one-batch Win+. chord, \
                 {what}. On Windows 11 the panel is class Windows.UI.Core.CoreWindow titled \
                 \"Windows Input Experience\", owned by TextInputHost.exe; if that is what \
                 the line above names, the gesture reached the shell and the shell acted on \
                 it, and the scan-code chord is what made the difference."
            );
            return;
        }
        log::warn!(
            "emoji panel: NO PANEL — {PROBE_MS} ms after the one-batch Win+. chord nothing \
             became visible and the foreground never moved, so the shell declined a chord \
             that SendInput definitely inserted. Retrying ONCE with a spaced chord (LWIN \
             down, 30 ms, period down/up, 30 ms, LWIN up) to separate 'this shell ignores \
             injected Win+. entirely' from 'the atomic batch was too fast for it'."
        );
        let before2 = visible_top_level();
        let fg2 = foreground_raw();
        if !inject_spaced() {
            log::warn!(
                "emoji panel: the spaced retry could not be inserted either — SendInput was \
                 blocked partway. Corrective KEYUPs were sent, so nothing is left held."
            );
            return;
        }
        match wait_for_evidence(&before2, fg2) {
            Some(what) => log::info!(
                "emoji panel: OPENED ON THE SPACED RETRY — {what}. VERDICT: this shell needs \
                 the modifier settled before the period arrives; the atomic four-event batch \
                 is too fast for it, and the spaced chord should become the only path."
            ),
            None => log::warn!(
                "emoji panel: STILL NO PANEL after the spaced retry. VERDICT: a scan-code \
                 chord AND a spaced scan-code chord were both inserted in full and Windows \
                 opened nothing, so injecting Win+. is not a route to the emoji panel on this \
                 machine. Check, in this order: (1) does a PHYSICAL Win+. open it? if not, \
                 nothing here is an app bug; (2) TabletInputService (Touch Keyboard and \
                 Handwriting Panel Service) Start value = {} where 4 means DISABLED and the \
                 shortcut is dead for everyone, 2 automatic, 3 manual, None = not readable; \
                 (3) the HKLM kill switch SOFTWARE\\Microsoft\\Input\\Settings\\proc_1\\loc_<LCID>\
                 \\im_1 → EnableExpressiveInputShellHotkey (0 disables it; absent is fine); \
                 (4) PowerToys Keyboard Manager remapping Win or period. If a physical press \
                 works and this does not, injection is genuinely refused and the in-app emoji \
                 grid is the answer — that is the owner's call, not this file's. Nothing is \
                 left held, and the emoji box is still focused, so Windows + . by hand will \
                 still work if the shortcut works at all.",
                match tablet_input_service_start() {
                    Some(v) => v.to_string(),
                    None => "None".to_string(),
                }
            ),
        }
    }
}

/// Export ONE profile to a `.json` the user chooses. Returns the path written,
/// or `None` when they cancelled the dialog.
///
/// **The dialog is opened from RUST, not from the webview**, and that was a
/// decision, not an accident. The JS dialog plugin
/// (`@tauri-apps/plugin-dialog`) is NOT installed here — `package.json` has
/// only `@tauri-apps/api` and `plugin-opener` — and `capabilities/default.json`
/// grants `dialog:allow-open`/`allow-ask` but no `allow-save`. Adding an npm
/// dependency and widening the webview's capability set, to reach the same
/// `tauri_plugin_dialog` that is ALREADY a Rust dependency and already used by
/// `pick_file` above, would be the more expensive of two identical outcomes.
///
/// `async fn` for the same reason `pick_file` is: `blocking_save_file` must
/// not run on the main thread. The `Result` return is not optional either —
/// Tauri refuses an async command that borrows an input (`State<'_, _>`)
/// unless it returns one (the E0277 documented on `list_start_menu_apps`).
#[tauri::command]
pub async fn export_profile(
    name: String,
    app: tauri::AppHandle,
    state: State<'_, ConfigState>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    // Serialise FIRST, with the lock held only for the clone, so the dialog —
    // which blocks for as long as the user takes to answer it — is never up
    // while the config write lock is held. A modal dialog holding a lock the
    // hook path also wants is a hang with no error message.
    let json = {
        let cfg = state.0.read().unwrap_or_else(|p| p.into_inner());
        let Some(p) = cfg.profiles.iter().find(|p| p.name == name) else {
            return Err(format!("Profile '{name}' not found"));
        };
        config::profile_export_json(p)?
    };

    let suggested = format!("{}.json", crate::config::suggested_export_filename(&name));
    let picked = app
        .dialog()
        .file()
        .add_filter("Spaceadom profile", &["json"])
        .set_file_name(&suggested)
        .blocking_save_file();

    let Some(path) = picked else {
        log::info!("export_profile: '{name}' — the user cancelled the save dialog");
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|e| format!("That save location cannot be written to ({e})."))?;

    std::fs::write(&path, json.as_bytes())
        .map_err(|e| format!("Could not write {} ({e}).", path.display()))?;
    log::info!(
        "export_profile: wrote '{name}' ({} bytes) to {}",
        json.len(),
        path.display()
    );
    Ok(Some(path.to_string_lossy().to_string()))
}

/// Import a profile from a `.json` the user chooses. Returns the name it was
/// added under, or `None` when they cancelled.
///
/// The imported name is DEDUPED rather than refused: a user importing a
/// profile a friend sent them, who already has one by that name, wants both —
/// the same `unique_copy_name` rule Duplicate uses, so the two features cannot
/// produce differently-shaped names for the same situation.
#[tauri::command]
pub async fn import_profile(
    app: tauri::AppHandle,
    state: State<'_, ConfigState>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let picked = app
        .dialog()
        .file()
        .add_filter("Spaceadom profile", &["json"])
        .blocking_pick_file();

    let Some(path) = picked else {
        log::info!("import_profile: the user cancelled the open dialog");
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|e| format!("That file cannot be read ({e})."))?;

    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("Could not read {} ({e}).", path.display()))?;
    let mut incoming = config::parse_profile_export(&raw)?;

    if !regex_lite(&incoming.name) {
        return Err(
            "That profile's name is not usable here (1–24 characters, no control codes).".into(),
        );
    }

    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    let taken: Vec<String> = cfg.profiles.iter().map(|p| p.name.clone()).collect();
    if taken.iter().any(|t| t == &incoming.name) {
        incoming.name = unique_copy_name(&taken, &incoming.name);
    }
    let added = incoming.name.clone();
    let count = incoming.bindings.len();
    cfg.profiles.push(incoming);

    log::info!(
        "import_profile: added '{added}' ({count} binding(s)) from {}",
        path.display()
    );
    let snapshot = cfg.clone();
    drop(cfg);
    config::save(&snapshot)?;
    Ok(Some(added))
}

// ---------------------------------------------------------------------------
// PROBLEM 259 — the own-window fallback's three commands
//
// The owner's requirement is unconditional: *with the Spaceadom dashboard
// focused, holding Space must show the ring and Space+letter must launch —
// exactly as elsewhere.* PROBLEM 257 measured that the keyboard hook cannot
// deliver that here (primary_real:0 / reference:0 / mouse:2705 in one minute
// with our window focused, and re-hooking does not recover it), and its cause
// is outside anything this process can reach.
//
// The dashboard PAGE still receives ordinary `keydown`/`keyup`, because it is
// the focused window. `src/own-window-keys.ts` runs the same tap/hold/combo
// state machine the hook runs and calls these three commands, which translate
// straight into the events the hook would have sent — `OwnWindowSpaceDown`
// (a `SpaceDown` with a different witness), `KeyCombo::Alpha(…)` and
// `SpaceUp`. From `engine::dispatch` onwards there is ONE code path; the ring,
// the cascade and the toast are not aware this happened.
//
// Every decision lives in `hook::own_window_*`, behind pure guards with tests,
// because the failure that matters is not "the fallback did nothing" — it is
// "the fallback and a recovered hook both fired". These commands are the thin
// IPC skin over that; they add no policy of their own.
// ---------------------------------------------------------------------------

/// Space went down in the dashboard page. `true` = the fallback took the hold.
///
/// A `false` here is the normal, healthy answer on a machine where the hook
/// works: it means the hook already has this press. The page uses it only to
/// decide whether to keep running its own state machine for this hold.
#[tauri::command]
pub fn own_window_space_down() -> bool {
    hook::own_window_space_down()
}

/// A key went down while the page's Space was held. `true` = it was dispatched
/// as a combo, which is also the page's confirmation that suppressing the
/// keystroke (and removing the space the browser had already inserted) was the
/// right call.
#[tauri::command]
pub fn own_window_key(vk: u16) -> bool {
    hook::own_window_key(vk)
}

/// Space came up in the dashboard page. `had_combo` is the page's own record
/// of whether this hold fired anything, and it is OR-ed with Rust's — either
/// witness saying yes is enough, because the cost of a wrong `false` (the
/// engine treats a fired hold as a bare tap) is worse than the cost of a wrong
/// `true` (nothing: the engine's `SpaceUp` arm ignores the flag entirely).
#[tauri::command]
pub fn own_window_space_up(had_combo: bool) -> bool {
    hook::own_window_space_up(had_combo)
}

/// The 1.0.96 profile-editor commands: reorder validation and copy naming.
///
/// Both are pure `AppConfig` / `&[String]` logic, split out of their commands
/// for exactly that reason. The reorder rule is the one that MUST have a test:
/// its failure mode is silent data loss (a mismatched list deleting profiles)
/// from a gesture the user thinks is cosmetic, and no amount of hand-testing a
/// drag reaches the stale-list case.
#[cfg(test)]
mod profile_editor_tests {
    use super::*;
    use crate::config::KeyBinding;

    fn cfg_with(names: &[&str]) -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.profiles = names
            .iter()
            .map(|n| {
                let mut bindings = crate::config::BindingMap::new();
                bindings.insert(
                    "a".to_string(),
                    KeyBinding { app: Some(format!("{n}.exe")), ..Default::default() },
                );
                Profile { name: (*n).to_string(), bindings, emoji: None }
            })
            .collect();
        cfg.active_profile = names.first().unwrap_or(&"Founders").to_string();
        cfg
    }

    fn order(cfg: &AppConfig) -> Vec<String> {
        cfg.profiles.iter().map(|p| p.name.clone()).collect()
    }

    // --- reorder_profiles ---------------------------------------------

    #[test]
    fn a_valid_reorder_moves_profiles_and_keeps_their_bindings() {
        let mut cfg = cfg_with(&["Founders", "Gamers", "Work"]);
        let new_order = vec!["Work".to_string(), "Founders".to_string(), "Gamers".to_string()];
        apply_profile_reorder(&mut cfg, &new_order).expect("a complete, unique list is valid");

        assert_eq!(order(&cfg), new_order, "the vec order IS the RAlt cycle order");
        assert_eq!(
            cfg.profiles[0].bindings["a"].app.as_deref(),
            Some("Work.exe"),
            "a reorder must MOVE profiles, not rename them in place"
        );
        assert_eq!(cfg.active_profile, "Founders", "reordering must not switch profile");
    }

    /// THE CASE THIS VALIDATION EXISTS FOR. The frontend builds the list from
    /// the DOM, and the DOM can be stale — a `config-updated` event, an undo,
    /// a second window. A permissive reorder would DELETE the missing profile.
    #[test]
    fn a_short_list_reorders_nothing_rather_than_deleting_the_rest() {
        let mut cfg = cfg_with(&["Founders", "Gamers", "Work"]);
        let before = order(&cfg);
        let err = apply_profile_reorder(&mut cfg, &["Work".to_string()])
            .expect_err("a partial list must be refused");
        assert!(err.contains("Nothing was reordered"), "{err}");
        assert_eq!(order(&cfg), before, "the config must be untouched after a refusal");
    }

    #[test]
    fn a_duplicated_or_unknown_name_is_refused_and_changes_nothing() {
        let mut cfg = cfg_with(&["Founders", "Gamers"]);
        let before = order(&cfg);

        // Right LENGTH, wrong CONTENT — the case a bare `len()` check misses,
        // and the one that would have duplicated Founders and dropped Gamers.
        let err = apply_profile_reorder(
            &mut cfg,
            &["Founders".to_string(), "Founders".to_string()],
        )
        .expect_err("a duplicate must be refused");
        assert!(err.contains("twice"), "{err}");
        assert_eq!(order(&cfg), before);

        let err = apply_profile_reorder(
            &mut cfg,
            &["Founders".to_string(), "Ghost".to_string()],
        )
        .expect_err("an unknown name must be refused");
        assert!(err.contains("no profile called 'Ghost'"), "{err}");
        assert_eq!(order(&cfg), before);
    }

    #[test]
    fn reordering_to_the_same_order_is_a_no_op_not_an_error() {
        let mut cfg = cfg_with(&["Founders", "Gamers"]);
        let same = order(&cfg);
        apply_profile_reorder(&mut cfg, &same).expect("identity must be legal");
        assert_eq!(order(&cfg), same);
    }

    // --- duplicate_profile naming -------------------------------------

    #[test]
    fn a_copy_is_named_name_2_then_3() {
        let mut taken = vec!["Founders".to_string()];
        assert_eq!(unique_copy_name(&taken, "Founders"), "Founders 2");
        taken.push("Founders 2".into());
        assert_eq!(unique_copy_name(&taken, "Founders"), "Founders 3");
    }

    /// Copying a COPY must not stutter into "Work 2 2".
    #[test]
    fn duplicating_a_copy_advances_the_number_instead_of_stacking() {
        let taken = vec!["Work".to_string(), "Work 2".to_string()];
        assert_eq!(unique_copy_name(&taken, "Work 2"), "Work 3");
    }

    /// `regex_lite` caps a name at 24 CHARACTERS. A copy name that exceeded it
    /// would be a name the user never typed being refused by the app's own
    /// validator — so the stem gives way to the suffix.
    #[test]
    fn a_copy_name_can_never_exceed_the_24_character_limit() {
        let long = "A".repeat(24);
        let taken = vec![long.clone()];
        let copy = unique_copy_name(&taken, &long);
        assert!(copy.ends_with(" 2"), "{copy}");
        assert!(
            regex_lite(&copy),
            "a generated name must pass the same validator a typed one does: {copy}"
        );
        assert!(copy.chars().count() <= 24, "{} chars", copy.chars().count());
    }

    /// An emoji in a profile name costs ONE character, on both sides of the
    /// check — `regex_lite` counts `chars()` and so must this.
    #[test]
    fn an_emoji_name_is_counted_the_same_way_regex_lite_counts_it() {
        let name = "🚀".repeat(24);
        assert_eq!(name.chars().count(), 24);
        let copy = unique_copy_name(&[name.clone()], &name);
        assert!(regex_lite(&copy), "{copy}");
    }
}

/// Rename-profile feature: name validation (`regex_lite`) and the
/// `apply_profile_rename` guarantees — active profile follows a rename of
/// itself, bindings are untouched, duplicates and unknown names are rejected.
/// All against a plain in-memory `AppConfig`, no Tauri `State` needed.
#[cfg(test)]
mod profile_rename_tests {
    use super::*;
    use crate::config::KeyBinding;

    fn profile_with_binding(name: &str, app: &str) -> Profile {
        let mut bindings = crate::config::BindingMap::new();
        bindings.insert(
            "a".to_string(),
            KeyBinding {
                app: Some(app.to_string()),
                ..Default::default()
            },
        );
        Profile { name: name.to_string(), bindings, emoji: None }
    }

    /// Two profiles, `active` marked as the one currently active — mirrors
    /// what `rename_profile` actually receives via `ConfigState`, minus the
    /// `Arc<RwLock<_>>` wrapper the command peels off before ever touching
    /// the data.
    fn two_profile_config(active: &str) -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.profiles = vec![
            profile_with_binding("Founders", "brave.exe"),
            profile_with_binding("Work", "chrome.exe"),
        ];
        cfg.active_profile = active.to_string();
        cfg
    }

    // --- regex_lite (name validation) ---------------------------------

    #[test]
    fn regex_lite_rejects_empty_and_whitespace_only() {
        assert!(!regex_lite(""), "empty must be rejected");
        assert!(!regex_lite("   "), "spaces-only must be rejected");
        assert!(!regex_lite("\t\t\t"), "tabs-only must be rejected");
    }

    #[test]
    fn regex_lite_accepts_24_chars_rejects_25() {
        let ok = "a".repeat(24);
        let too_long = "a".repeat(25);
        assert!(regex_lite(&ok), "exactly 24 characters must be accepted");
        assert!(!regex_lite(&too_long), "25 characters must be rejected");
    }

    #[test]
    fn regex_lite_rejects_control_characters() {
        assert!(!regex_lite("bad\u{7}name"), "BEL must be rejected");
        assert!(!regex_lite("line1\nline2"), "embedded newline must be rejected");
        assert!(!regex_lite("a\tb"), "embedded tab must be rejected");
        assert!(!regex_lite("del\u{7f}ete"), "DEL (0x7F) must be rejected");
    }

    /// PROBLEM 197 — the whole point of the relaxed charset: spaces, dashes,
    /// punctuation, accented letters and emoji must all be accepted now.
    #[test]
    fn regex_lite_accepts_the_owners_actual_profile_names_and_more() {
        assert!(regex_lite("Founders"));
        assert!(regex_lite("sexy_tumar_mexy"));
        assert!(regex_lite("Work - Home"));
        assert!(regex_lite("Étude 🎮"));
        assert!(regex_lite("  Trimmed  "), "surrounding whitespace is trimmed, not rejected");
    }

    // --- apply_profile_rename ------------------------------------------

    #[test]
    fn rename_updates_the_profile_name() {
        let mut cfg = two_profile_config("Founders");
        apply_profile_rename(&mut cfg, "Work", "Office").expect("rename should succeed");
        assert!(cfg.profiles.iter().any(|p| p.name == "Office"));
        assert!(!cfg.profiles.iter().any(|p| p.name == "Work"));
    }

    /// The feature's core promise: rename the ACTIVE profile and
    /// `active_profile` must move with it in the same call, not on some
    /// later save.
    #[test]
    fn renaming_the_active_profile_updates_active_profile_atomically() {
        let mut cfg = two_profile_config("Founders");
        apply_profile_rename(&mut cfg, "Founders", "Home Base").expect("rename should succeed");
        assert_eq!(
            cfg.active_profile, "Home Base",
            "active_profile must follow a rename of the profile it points at"
        );
        assert!(cfg.profiles.iter().any(|p| p.name == "Home Base"));
        assert!(
            !cfg.profiles.iter().any(|p| p.name == "Founders"),
            "the old name must not linger anywhere"
        );
    }

    #[test]
    fn renaming_an_inactive_profile_leaves_active_profile_untouched() {
        let mut cfg = two_profile_config("Founders");
        apply_profile_rename(&mut cfg, "Work", "Office").expect("rename should succeed");
        assert_eq!(
            cfg.active_profile, "Founders",
            "renaming a profile that is not active must not move active_profile"
        );
    }

    #[test]
    fn rename_rejects_a_duplicate_target_name() {
        let mut cfg = two_profile_config("Founders");
        let err = apply_profile_rename(&mut cfg, "Work", "Founders").unwrap_err();
        assert!(err.contains("already exists"), "unexpected message: {err}");
        // Nothing must have moved.
        assert!(cfg.profiles.iter().any(|p| p.name == "Work"));
        assert_eq!(cfg.active_profile, "Founders");
    }

    /// PROBLEM 85 — a same-name rename is a deliberate no-op, not an error
    /// (the user re-typed exactly what was already there).
    #[test]
    fn rename_to_the_same_name_is_a_no_op_not_an_error() {
        let mut cfg = two_profile_config("Founders");
        apply_profile_rename(&mut cfg, "Founders", "Founders")
            .expect("renaming a profile to its own current name must succeed");
        assert_eq!(cfg.active_profile, "Founders");
        assert_eq!(cfg.profiles.iter().filter(|p| p.name == "Founders").count(), 1);
    }

    #[test]
    fn rename_of_an_unknown_profile_errors() {
        let mut cfg = two_profile_config("Founders");
        let err = apply_profile_rename(&mut cfg, "Ghost", "New Name").unwrap_err();
        assert!(err.contains("not found"), "unexpected message: {err}");
    }

    /// The bindings live inside the `Profile` object being renamed — a
    /// rename must never touch them.
    #[test]
    fn rename_preserves_bindings() {
        let mut cfg = two_profile_config("Founders");
        apply_profile_rename(&mut cfg, "Work", "Office").expect("rename should succeed");
        let renamed = cfg.profiles.iter().find(|p| p.name == "Office").unwrap();
        assert_eq!(
            renamed.bindings.get("a").and_then(|b| b.app.as_deref()),
            Some("chrome.exe"),
            "rename must not lose or alter bindings"
        );
    }
}

/// `apply_profile_delete` — extracted from `delete_profile` the same way
/// `apply_profile_rename` was, specifically so the 1.0.96 review fix's ORDER
/// (guard before either side effect) can be asserted here instead of only by
/// hand.
#[cfg(test)]
mod delete_profile_tests {
    use super::*;
    use crate::config::KeyBinding;

    fn cfg_with(names: &[&str]) -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.profiles = names
            .iter()
            .map(|n| {
                let mut bindings = crate::config::BindingMap::new();
                bindings.insert(
                    "a".to_string(),
                    KeyBinding { app: Some(format!("{n}.exe")), ..Default::default() },
                );
                Profile { name: (*n).to_string(), bindings, emoji: None }
            })
            .collect();
        cfg.active_profile = names.first().unwrap_or(&"Founders").to_string();
        cfg
    }

    /// The case the review fix targets: a config with two profiles both
    /// named "X" (legacy duplicate-name state — PROBLEM 85 predates the
    /// creation-time uniqueness guard, so a config saved before it can still
    /// have two). Deleting "X" must still be refused (retain would remove
    /// BOTH and leave zero profiles), and — this is the actual bug —
    /// refusing it must not have already run either side effect. Before the
    /// reorder, the guard sat AFTER `stash_undo_for`, so a refused delete
    /// still clobbered whatever undo entry was already on the stack; a
    /// caller sees `Err` and reasonably assumes nothing happened, but the
    /// undo stack disagreed.
    #[test]
    fn refusing_a_duplicate_named_delete_leaves_the_undo_stack_unchanged() {
        let mut cfg = cfg_with(&["X", "X"]);
        let before = UNDO_STACK.lock().unwrap_or_else(|p| p.into_inner()).len();

        let err = apply_profile_delete(&mut cfg, "X")
            .expect_err("both profiles are named X, so deleting X must be refused");
        assert!(err.contains("last remaining profile"), "unexpected message: {err}");

        let after = UNDO_STACK.lock().unwrap_or_else(|p| p.into_inner()).len();
        assert_eq!(before, after, "a refused delete must not stash an undo entry");
        assert_eq!(
            cfg.profiles.len(),
            2,
            "a refused delete must not touch the profiles either"
        );
    }
}

/// Open the log folder in Explorer so a tester can grab debug.log (and its
/// rotated .1/.2 siblings) to send back. Chosen over an auto-zip exporter at
/// the user's request — the folder gives testers the choice of what to share.
#[tauri::command]
pub fn open_log_folder() {
    let dir = crate::logger::log_dir();
    #[cfg(windows)]
    {
        // explorer.exe returns immediately; no window-handle juggling needed.
        let _ = std::process::Command::new("explorer").arg(&dir).spawn();
    }
    log::info!("logs: opened log folder {dir:?}");
}

/// Flip run-at-startup: persists config (source of truth) AND applies it to
/// the Scheduled Task's enabled state. One command so the two can never
/// drift apart silently.
#[tauri::command]
pub fn set_startup_enabled(
    enabled: bool,
    state: State<'_, ConfigState>,
) -> Result<(), String> {
    {
        let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
        cfg.run_at_startup = enabled;
        let snapshot = cfg.clone();
        drop(cfg);
        config::save(&snapshot)?;
    }
    #[cfg(windows)]
    crate::startup::apply_task_enabled(enabled);
    Ok(())
}

// ---------------------------------------------------------------------------
// PROBLEM 217 — the frontend bridges' SEVERITY is the whole point of the change,
// so it is what gets asserted. Everything else about these commands is a one
// line call; the level is the part that decides whether a JavaScript failure on
// somebody else's machine is ever seen.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod frontend_bridge_tests {
    use std::sync::Mutex;

    /// A capturing `log::Log`. There is exactly one logger per process, and in
    /// the test binary nothing else installs one — `logger::init` is only ever
    /// called from `run()`.
    static CAPTURED: Mutex<Vec<(log::Level, String, String)>> = Mutex::new(Vec::new());

    struct Capture;
    impl log::Log for Capture {
        fn enabled(&self, _m: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, r: &log::Record<'_>) {
            CAPTURED
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push((r.level(), r.target().to_owned(), r.args().to_string()));
        }
        fn flush(&self) {}
    }

    static LOGGER: Capture = Capture;

    /// `frontend_log` must stay INFO — many existing call sites depend on it
    /// being quiet, and INFO is below `SENTRY_MINIMUM_LEVEL`, so those lines
    /// stay on the machine. `frontend_error` must be ERROR, or the whole
    /// change is decorative.
    #[test]
    fn frontend_error_is_error_while_frontend_log_stays_info() {
        assert!(
            log::set_logger(&LOGGER).is_ok(),
            "no other test may install a logger — this one has to be the global one to observe levels"
        );
        log::set_max_level(log::LevelFilter::Trace);

        // Unique markers: the suite runs in parallel and other modules log into
        // the same buffer.
        super::frontend_log("P216-plain-marker".into());
        super::frontend_error("P216-error-marker".into());
        super::overlay_error("P216-overlay-marker".into());

        let captured = CAPTURED.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let find = |needle: &str| {
            captured
                .iter()
                .find(|(_, _, m)| m.contains(needle))
                .unwrap_or_else(|| panic!("'{needle}' never reached the logger"))
                .clone()
        };

        let (level, _target, msg) = find("P216-plain-marker");
        assert_eq!(level, log::Level::Info, "frontend_log must stay INFO: {msg}");
        assert!(msg.starts_with("dashboard-js: "), "the log convention must not drift: {msg}");

        let (level, target, msg) = find("P216-error-marker");
        assert_eq!(level, log::Level::Error, "frontend_error must be ERROR: {msg}");
        assert!(msg.starts_with("dashboard-js: "), "the log convention must not drift: {msg}");
        assert_eq!(
            target,
            crate::telemetry::DEGRADED_TARGET,
            "it is reported by hand and rate-limited, so the automatic bridge must skip it"
        );

        let (level, _target, msg) = find("P216-overlay-marker");
        assert_eq!(level, log::Level::Error, "overlay_error must be ERROR: {msg}");
        assert!(msg.starts_with("overlay-js: "), "the log convention must not drift: {msg}");
    }
}
