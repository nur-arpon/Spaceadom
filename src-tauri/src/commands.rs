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
        let _ = app.emit("sound-changed", new_config.sound_enabled);
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

#[derive(serde::Serialize)]
pub struct AppInfo {
    pub name: String,
    pub path: String,
    pub icon_base64: Option<String>,
}

#[tauri::command]
pub fn list_start_menu_apps(cache: State<'_, IconCacheState>) -> Vec<AppInfo> {
    let script = r#"
        $ErrorActionPreference = 'SilentlyContinue'
        $paths = @(
            "$env:ProgramData\Microsoft\Windows\Start Menu\Programs",
            "$env:AppData\Microsoft\Windows\Start Menu\Programs"
        )
        $apps = Get-ChildItem -Path $paths -Recurse -Filter *.lnk
        $wshell = New-Object -ComObject WScript.Shell
        $results = @()
        foreach ($app in $apps) {
            $shortcut = $wshell.CreateShortcut($app.FullName)
            if ($shortcut.TargetPath -match "\.exe$") {
                # Shortcuts with arguments (Discord: Update.exe --processStart
                # Discord.exe) must be bound AS the .lnk - launching the bare
                # TargetPath starts nothing. ShellExecute runs .lnk with args.
                $path = if ($shortcut.Arguments) { $app.FullName } else { $shortcut.TargetPath }
                $results += [PSCustomObject]@{ Name = $app.BaseName; Path = $path }
            }
        }
        # Microsoft Store / UWP apps have NO .lnk with an .exe target — they
        # live in shell:AppsFolder keyed by AppUserModelID, so the Start-Menu
        # scan above misses every one of them (user report 2026-08-10:
        # "many applications do not arrive in the list").
        try {
            $shellApp = New-Object -ComObject Shell.Application
            $appsFolder = $shellApp.NameSpace("shell:AppsFolder")
            if ($appsFolder) {
                foreach ($item in $appsFolder.Items()) {
                    $aumid = $item.Path
                    # A packaged app's Path is an AppUserModelID
                    # (Package_hash!AppId) — never a filesystem path.
                    if ($aumid -and $aumid.Contains("!") -and $aumid -notmatch '[\\/]|^[A-Za-z]:') {
                        $results += [PSCustomObject]@{
                            Name = $item.Name
                            Path = "shell:AppsFolder\$aumid"
                        }
                    }
                }
            }
        } catch { }

        @($results) | Select-Object Name, Path -Unique | ConvertTo-Json -Compress
    "#;

    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut cmd = std::process::Command::new("powershell");
    cmd.args(&["-NoProfile", "-WindowStyle", "Hidden", "-Command", script]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output();

    let mut apps = Vec::new();
    if let Ok(out) = output {
        let json_str = String::from_utf8_lossy(&out.stdout);
        if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(&json_str) {
            for v in parsed {
                if let (Some(name), Some(path)) = (v["Name"].as_str(), v["Path"].as_str()) {
                    let exe_path = path.to_string();
                    // Icons come from IShellItemImageFactory, which resolves
                    // .exe, .lnk AND shell:AppsFolder\<AUMID> Store apps —
                    // so there is no longer a path type to special-case.
                    let icon_base64 = {
                        let mut lock = cache.0.lock().unwrap_or_else(|p| p.into_inner());
                        if let Some(cached) = lock.get(&exe_path) {
                            Some(cached.clone())
                        } else {
                            if let Some(b64) = crate::icon_extractor::extract_icon(&exe_path) {
                                lock.insert(exe_path.clone(), b64.clone());
                                Some(b64)
                            } else {
                                None
                            }
                        }
                    };
                    apps.push(AppInfo {
                        name: name.to_string(),
                        path: exe_path,
                        icon_base64,
                    });
                }
            }
        }
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps
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

/// Toggle the global bypass mode from the frontend UI
#[tauri::command]
pub fn toggle_bypass(app: tauri::AppHandle) -> bool {
    use crate::hook::BYPASS_MODE;
    use std::sync::atomic::Ordering;
    
    let new_state = !BYPASS_MODE.load(Ordering::Relaxed);
    BYPASS_MODE.store(new_state, Ordering::Relaxed);
    
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
    // Window ops belong on the main thread; a command handler is not on it.
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

/// Size + position the overlay window to fit the toast stack, bottom-centre,
/// then show it. Called by the overlay page AFTER it has rendered and
/// measured its content (layout works in hidden webviews; painting doesn't —
/// so measure-then-show is safe). One-jump resize per the motion reference:
/// animating an OS window's bounds frame-by-frame tears on Windows.
#[tauri::command]
pub fn overlay_fit(app: tauri::AppHandle, width: f64, height: f64) -> Option<OverlayRect> {
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
    let Ok(Some(mon)) = win.primary_monitor() else {
        log::warn!("overlay_fit: primary_monitor() returned nothing — window NOT positioned");
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
    // Toasts must sit above everything, same rule as the HUD.
    let _ = win.set_always_on_top(true);
    let _ = win.show();
    Some(OverlayRect { x, y, w, h })
}

/// Size + position the overlay window to fit the RENDERED Guide HUD, called
/// by the overlay page after it has laid the HUD out. Only valid while the
/// HUD owns the window. This is what makes clipping structurally impossible:
/// the old fixed 680×600 cut off Y, Z and every special-function row on a
/// 26-binding profile (user report, 2026-08-10).
#[tauri::command]
pub fn overlay_fit_hud(app: tauri::AppHandle, width: f64, height: f64) -> Option<OverlayRect> {
    use tauri::Manager;
    if !crate::guide_hud::is_visible() {
        return None;
    }
    let win = app.get_webview_window("overlay")?;
    if let Ok(Some(mon)) = win.primary_monitor() {
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
            {
                let state: tauri::State<ConfigState> = app.state();
                let mode = state.0.read().unwrap_or_else(|p| p.into_inner()).overlay_compositing.clone();
                if mode == "software" {
                    HEALED.store(true, Ordering::Relaxed); // already healed; stop testing
                    done();
                    return;
                }
                if mode != "auto" {
                    log::warn!(
                        "compositing: unrecognised overlay_compositing '{mode}' — treating as \
                         'auto' and continuing to self-test (only 'software' disables it)"
                    );
                }
            }

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
        return;
    }
    if let Some(win) = app.get_webview_window("overlay") {
        let _ = win.hide();
        // Clear the toast-shaped region so the next show (HUD or toast)
        // starts from a full rectangular window.
        set_overlay_region(&win, &[], 1.0);
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

/// Create a new empty profile. Validates name against alphanumeric rules.
#[tauri::command]
pub fn create_profile(
    name: String,
    state: State<'_, ConfigState>,
) -> Result<(), String> {
    // Validate name: 1–24 chars, alphanumeric + underscore only
    let re = regex_lite(&name);
    if !re {
        return Err("Profile name must be 1–24 alphanumeric characters (a-z, A-Z, 0-9, _)".into());
    }

    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    if cfg.profiles.iter().any(|p| p.name == name) {
        return Err(format!("Profile '{name}' already exists"));
    }

    let mut bindings = std::collections::HashMap::new();
    for c in 'a'..='z' {
        bindings.insert(c.to_string(), crate::config::KeyBinding::default());
    }

    cfg.profiles.push(Profile { name, bindings });
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
    // PROBLEM 99 — deleting a user-created profile destroys bindings and
    // custom icons that exist in NO other copy. Stash before touching it.
    // PROBLEM 105 — deleting the FALLBACK profile is not like deleting any
    // other one: every key left unassigned in every REMAINING profile is
    // rerouted here, so they all silently stop working. The damage shows up
    // later, in a different profile, with nothing connecting it to this act.
    //
    // The warning rides on the UNDO LABEL rather than a confirm dialog. A
    // `window.confirm` was tried first and never appeared — this webview does
    // not render native script dialogs, which is why every other destructive
    // control in this app uses a two-step "Confirm" button instead. A warning
    // the user cannot see is the same as no warning, and it is worse than
    // none because it looks like the job was done.
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
    stash_undo_for(&undo_label, &cfg, undo_window_for_profile(&name));

    if is_fallback {
        log::warn!(
            "delete_profile: '{name}' is the FALLBACK profile — keys left unassigned in other \
             profiles are rerouted here and will now do nothing. Undo is available for 10 \
             seconds; recreating a profile with this exact name also restores the behaviour."
        );
    }

    // Count what would REMAIN, not the total (PROBLEM 85): rename_profile
    // historically allowed duplicate names, and `retain` removes EVERY
    // profile with the given name — with two profiles both called "X",
    // `len() <= 1` passed, retain emptied the Vec, and `profiles[0]` panicked
    // while HOLDING the config write lock.
    let remaining = cfg.profiles.iter().filter(|p| p.name != name).count();
    if remaining == 0 {
        return Err("Cannot delete the last remaining profile".into());
    }
    cfg.profiles.retain(|p| p.name != name);
    // If deleted profile was active, switch to first
    if cfg.active_profile == name {
        if let Some(first) = cfg.profiles.first() {
            cfg.active_profile = first.name.clone();
        }
    }
    let snapshot = cfg.clone();
    drop(cfg);
    config::save(&snapshot)
}

/// Rename an existing profile.
#[tauri::command]
pub fn rename_profile(
    old_name: String,
    new_name: String,
    state: State<'_, ConfigState>,
) -> Result<(), String> {
    if !regex_lite(&new_name) {
        return Err("Profile name must be 1–24 alphanumeric characters".into());
    }

    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    // PROBLEM 85 (root cause half) — renaming B to A's name used to create
    // TWO profiles called "A": every name-keyed lookup became ambiguous, and
    // delete-by-name removed both at once. create_profile always had this
    // guard; rename never did. `new_name != old_name` keeps a same-name
    // rename a no-op instead of an error.
    if new_name != old_name && cfg.profiles.iter().any(|p| p.name == new_name) {
        return Err(format!("Profile '{new_name}' already exists"));
    }
    let profile = cfg
        .profiles
        .iter_mut()
        .find(|p| p.name == old_name)
        .ok_or_else(|| format!("Profile '{old_name}' not found"))?;
    profile.name = new_name.clone();

    if cfg.active_profile == old_name {
        cfg.active_profile = new_name;
    }
    let snapshot = cfg.clone();
    drop(cfg);
    config::save(&snapshot)
}

/// Validate a profile name string without regex crate dependency.
fn regex_lite(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 24
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
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
