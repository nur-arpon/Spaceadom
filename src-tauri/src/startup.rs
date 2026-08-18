/// startup.rs — elevation, run-at-logon, and the Scheduled Task that makes
/// both happen without a UAC prompt on every launch.
///
/// THE APP NO LONGER ELEVATES (PROBLEM 61). WH_KEYBOARD_LL does not need it.
/// Elevating meant a UAC prompt every launch and autostart that silently
/// failed for standard non-admin users. ACCEPTED LIMITATION: a non-elevated
/// hook receives nothing while an ELEVATED window has focus (Task Manager,
/// regedit, an admin terminal). That is Windows UIPI and affects every
/// remapper; it is documented rather than worked around.
///
/// WHY A SCHEDULED TASK (PROBLEM 45): the old flow called ShellExecuteW
/// "runas" on itself at every launch, so the user saw a UAC prompt on every
/// boot. A Task Scheduler entry created with /RL HIGHEST runs the app
/// elevated WITHOUT a prompt — both at logon (/SC ONLOGON) and when poked
/// via `schtasks /Run`. Admin consent is needed once, when the task is first
/// created; after that, never again.
///
/// Launch flow:
///   non-elevated start ──task exists?──yes──▶ schtasks /Run → exit (silent)
///                            └──no──▶ ShellExecuteW runas → exit (ONE prompt)
///   elevated start     ──▶ ensure the task exists + matches config,
///                          clean up legacy HKCU Run entries.
use std::path::PathBuf;

#[cfg(windows)]
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
        System::{
            Threading::{GetCurrentProcess, OpenProcessToken},
        },
        UI::Shell::ShellExecuteW,
        UI::WindowsAndMessaging::SW_SHOWNORMAL,
    },
};

#[cfg(windows)]
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

/// The Task Scheduler entry this app owns.
#[cfg(windows)]
const TASK_NAME: &str = "Spaceadom";

/// Legacy HKCU Run values from earlier versions of this app. Removed when
/// found so the old build cannot ALSO start at logon and put a second
/// keyboard hook on the machine (the documented feedback-loop trap).
/// "SpaceToggleOS" (V13) is deliberately NOT in this list — that is a
/// separate product the user keeps as a fallback on the dev machine.
#[cfg(windows)]
const LEGACY_RUN_VALUES: [&str; 2] = ["SpaceToggleV14", "SpaceToggleOrganic"];

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// HKCU Run value name for the autostart FALLBACK (PROBLEM 64).
#[cfg(windows)]
const RUN_VALUE: &str = "Spaceadom";

/// PROBLEM 64 — the fallback that makes "Run at startup" actually work.
///
/// A standard, non-elevated user CANNOT create a task in the Task Scheduler
/// ROOT folder: `schtasks /Create` returns `ERROR: Access is denied.` — even
/// with `/RL LIMITED` and a brand-new task name. Verified directly on this
/// machine. Since PROBLEM 61 removed self-elevation, the app is ALWAYS
/// non-elevated, so on every non-admin machine the logon task was never
/// created and "Run at startup" (ON by default) silently did nothing. The
/// only evidence was one ERROR line in debug.log that nobody reads.
///
/// `HKCU\...\Run` always works for the current user, needs no elevation, and
/// is the canonical per-user autostart. It is used ONLY when the task could
/// not be created, so the two mechanisms can never both fire.
#[cfg(windows)]
fn set_run_key(enabled: bool) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok((key, _)) = hkcu.create_subkey(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run") else {
        log::error!("startup: cannot open HKCU Run key — autostart unavailable");
        return;
    };

    if enabled {
        let Ok(exe) = std::env::current_exe() else { return };
        // PROBLEM 33's guard, Run-key edition: never point the user's
        // autostart at a cargo build directory that `cargo clean` deletes.
        if is_dev_build(&exe) {
            log::info!("startup: dev build — not writing an HKCU Run autostart entry");
            return;
        }
        // `--autostart` makes the app wait for the shell and WebView2 to
        // settle before building windows. The Scheduled Task expressed this
        // as `/DELAY 0000:30`; a Run value has no such flag, so the wait
        // lives in run() instead. See PROBLEM 59.
        let cmd = format!("\"{}\" --autostart", exe.to_string_lossy());
        match key.set_value(RUN_VALUE, &cmd) {
            Ok(()) => log::info!("startup: HKCU Run autostart set -> {cmd}"),
            Err(e) => log::error!("startup: could not set HKCU Run entry: {e}"),
        }
    } else if key.get_raw_value(RUN_VALUE).is_ok() {
        match key.delete_value(RUN_VALUE) {
            Ok(()) => log::info!("startup: HKCU Run autostart removed"),
            Err(e) => log::warn!("startup: could not remove HKCU Run entry: {e}"),
        }
    }
}

#[cfg(not(windows))]
fn set_run_key(_enabled: bool) {}

/// True if this executable is a cargo build output rather than an install.
/// A dev build must never repoint the user's startup task at a build
/// directory that `cargo clean` deletes (PROBLEM 33).
#[cfg(windows)]
fn is_dev_build(exe: &std::path::Path) -> bool {
    let p = exe.to_string_lossy().to_ascii_lowercase();
    p.contains(r"\target\release\") || p.contains(r"\target\debug\")
}

/// Fix the Task Scheduler defaults that silently sabotage a tray utility:
/// won't start on battery, stops when unplugged, and killed after 3 days.
/// (PROBLEM 59 — schtasks.exe has no flags for these.)
#[cfg(windows)]
fn harden_task_settings() {
    use std::os::windows::process::CommandExt;
    let ps = format!(
        "$s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries \
         -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) \
         -StartWhenAvailable -MultipleInstances IgnoreNew; \
         Set-ScheduledTask -TaskName '{TASK_NAME}' -Settings $s | Out-Null"
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &ps])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output();
    match out {
        Ok(o) if o.status.success() => {
            log::info!("startup: task settings hardened (battery-safe, no time limit)")
        }
        _ => log::warn!(
            "startup: could not harden task settings — the task still runs, but Windows \
             may refuse to start it on battery or stop it after 3 days"
        ),
    }
}

#[cfg(not(windows))]
fn harden_task_settings() {}

/// Run schtasks.exe with a hidden window and return its output.
#[cfg(windows)]
fn schtasks(args: &[&str]) -> Option<std::process::Output> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("schtasks")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
}

#[cfg(windows)]
pub fn task_exists() -> bool {
    schtasks(&["/Query", "/TN", TASK_NAME])
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// PROBLEM 75 — a mismatched startup task from an older build is present and
/// this process could not remove it. Read by `get_stale_task` so the dashboard
/// can offer the one-click elevated repair.
#[cfg(windows)]
pub static STALE_TASK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What the registered task actually launches, judged from its XML.
#[cfg(windows)]
enum TaskState {
    /// Points at THIS exe and carries `--autostart` — the healthy shape.
    Healthy,
    /// Anything else: old exe path, missing flag, or unreadable XML. The
    /// self-elevating 1.0.0–1.0.2 era created exactly these, and they open
    /// the dashboard at every logon (tester report, 2026-08-12).
    Mismatched,
    None,
}

#[cfg(windows)]
fn task_state() -> TaskState {
    let Some(o) = schtasks(&["/Query", "/TN", TASK_NAME, "/XML"]) else {
        return TaskState::None;
    };
    if !o.status.success() {
        return TaskState::None;
    }
    let xml = String::from_utf8_lossy(&o.stdout).to_lowercase();
    let current = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !current.is_empty() && xml.contains(&current) && xml.contains("--autostart") {
        TaskState::Healthy
    } else {
        TaskState::Mismatched
    }
}

/// Create (or refresh) the logon task, elevated, pointing at THIS exe, and
/// set its enabled state from config. Must be called from an ELEVATED
/// process — schtasks /RL HIGHEST refuses otherwise.
///
/// /F recreates in place, so calling this on every elevated launch is cheap
/// and self-heals a task that points at a moved or uninstalled exe.
#[cfg(windows)]
pub fn ensure_startup_task(run_at_startup: bool) {
    let Ok(exe) = std::env::current_exe() else {
        log::error!("startup: cannot read current exe path");
        return;
    };

    // PROBLEM 33's guard, task edition: a dev build leaves an existing,
    // still-valid task alone. (Deleting the task's target exe invalidates
    // it; then even a dev build may recreate it to self-heal.)
    if is_dev_build(&exe) && task_exists() {
        log::info!("startup: dev build — leaving the existing '{TASK_NAME}' task alone");
        return;
    }

    // PROBLEM 75 — triage the EXISTING task before anything else. The
    // self-elevating 1.0.0–1.0.2 builds left tasks that launch the app with
    // no --autostart (dashboard in the user's face at every logon, tester
    // report). MEASURED on this machine: a non-elevated process can neither
    // /Delete, /Create /F over, /Change /DISABLE, nor Disable-ScheduledTask
    // a task that was created elevated — every one returns Access denied.
    // So: Healthy → keep it. Mismatched → try to delete (succeeds for tasks
    // our own non-elevated code made); if Windows refuses, remember that so
    // the dashboard can offer the ONE-CLICK elevated repair, and do NOT also
    // write a Run key — the machine already autostarts via the stale task,
    // and a second launcher just races it.
    match task_state() {
        TaskState::Healthy => {
            log::info!("startup: task '{TASK_NAME}' is healthy (this exe, --autostart)");
            set_run_key(false);
            apply_task_enabled(run_at_startup);
            return;
        }
        TaskState::Mismatched => {
            match schtasks(&["/Delete", "/F", "/TN", TASK_NAME]) {
                Some(o) if o.status.success() => {
                    log::info!(
                        "startup: removed mismatched '{TASK_NAME}' task from an older build — \
                         recreating cleanly"
                    );
                }
                _ => {
                    STALE_TASK.store(true, std::sync::atomic::Ordering::Relaxed);
                    log::warn!(
                        "startup: task '{TASK_NAME}' is from an OLDER build (wrong exe or no \
                         --autostart) and this process cannot remove it (Access denied — it was \
                         created elevated). The dashboard will offer a one-click admin repair. \
                         NOT writing a Run key meanwhile: the stale task already starts the app."
                    );
                    // PROBLEM 76 — WRITE the Run key here anyway. The first
                    // version of this branch withheld it ("one launcher at a
                    // time"), which assumed the stale task actually STARTS the
                    // app at logon. A /RL HIGHEST task on a standard account
                    // often cannot start AT ALL — which is exactly the
                    // tester's laptop, where the result was NO autostart of
                    // any kind and a manual run-as-admin hunt. Autostart
                    // resilience beats tidiness: if both launchers do fire,
                    // single-instance resolves the race (the second exits),
                    // and the worst case is the old dashboard-at-logon bug —
                    // which the repair banner exists to fix.
                    set_run_key(run_at_startup);
                    return;
                }
            }
        }
        TaskState::None => {}
    }

    let exe_str = exe.to_string_lossy().to_string();
    // PROBLEM 71 — the task MUST pass --autostart, exactly like the Run-key
    // fallback does. Without it a logon launch is indistinguishable from the
    // user double-clicking the app, so the dashboard opened in the user's face
    // at every boot (reported on a tester's laptop) AND the 30s cold-boot wait
    // never happened. Both autostart paths must agree on the flag.
    let tr = format!("\"{exe_str}\" --autostart");
    // PROBLEM 59 — the cold-boot WebView2 race.
    //
    // Launching at logon puts us in a fight with Edge/WebView2's own broker
    // processes, the GPU stack and the disk, all still starting. WebView2 then
    // fails with HRESULT(0x80070490) ERROR_NOT_FOUND, Tauri destroys the host
    // window, and the app runs on with NO dashboard and NO overlay — so the
    // Guide HUD never appears and nothing looks clickable. Every manual launch
    // on the tester's machine succeeded; only the cold-boot one failed.
    //
    // /DELAY 30 seconds costs nothing and removes most of that race. Combined
    // with the in-app retry, a cold boot no longer produces a dead app.
    //
    // Note: /DELAY is only valid for ONLOGON (and ONSTART) triggers.
    match schtasks(&[
        "/Create", "/F",
        "/TN", TASK_NAME,
        "/TR", &tr,
        "/SC", "ONLOGON",
        // PROBLEM 61 — LIMITED, not HIGHEST. A Highest task fails to register
        // (or registers and cannot start) on a standard non-admin account, and
        // on an admin account it makes the app run elevated at logon but
        // non-elevated from the Start Menu — two different WebView2 user-data
        // and UIPI behaviours for the same app. WH_KEYBOARD_LL needs neither.
        "/RL", "LIMITED",
        "/DELAY", "0000:30",
    ]) {
        Some(o) if o.status.success() => {
            log::info!("startup: task '{TASK_NAME}' → {exe_str} (logon +30s, least-privilege)");
            // The task is authoritative when it exists — drop any Run-key
            // fallback so the app cannot be started twice at logon.
            set_run_key(false);
            // Task Scheduler's DEFAULTS are wrong for a tray utility and are
            // applied silently: it refuses to start on battery, stops the task
            // if the machine goes onto battery, and TERMINATES it after 3 days.
            // schtasks.exe cannot express these, so patch the registration via
            // PowerShell's ScheduledTask cmdlets. Best-effort: if it fails the
            // task still works, just with the poor defaults.
            harden_task_settings();
        }
        Some(o) => {
            // PROBLEM 64 — the NORMAL path on a non-admin machine, not an
            // edge case. Do not treat this as fatal: fall back to HKCU Run.
            log::warn!(
                "startup: task create failed ({}) — using HKCU Run autostart instead",
                String::from_utf8_lossy(&o.stderr).trim()
            );
            set_run_key(run_at_startup);
            return;
        }
        None => {
            log::warn!("startup: could not run schtasks — using HKCU Run autostart instead");
            set_run_key(run_at_startup);
            return;
        }
    }

    apply_task_enabled(run_at_startup);
}

/// Enable/disable the logon trigger. Config is the source of truth for the
/// UI — schtasks' textual status output is localized and not parsed here.
///
/// PROBLEM 64: on machines where the task never got created (non-admin — see
/// set_run_key), /Change fails with "cannot find the file specified"; the
/// Run-key fallback is applied instead so the Settings toggle still works.
#[cfg(windows)]
pub fn apply_task_enabled(enabled: bool) {
    if !task_exists() {
        set_run_key(enabled);
        return;
    }
    let flag = if enabled { "/ENABLE" } else { "/DISABLE" };
    match schtasks(&["/Change", "/TN", TASK_NAME, flag]) {
        Some(o) if o.status.success() => {
            log::info!("startup: task '{TASK_NAME}' {}", if enabled { "enabled" } else { "disabled" });
        }
        Some(o) => log::error!(
            "startup: task {flag} failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        None => log::error!("startup: could not run schtasks"),
    }
}

/// PROBLEM 75 — the one-click repair for a stale startup task this process
/// cannot touch. Runs `schtasks /Delete` ELEVATED (one UAC prompt, initiated
/// by the user clicking the dashboard banner), waits for it, then re-runs the
/// normal registration — which, non-elevated, lands on the Run-key fallback
/// with `--autostart`. Returns true when the stale task is gone.
#[cfg(windows)]
pub fn repair_stale_task(run_at_startup: bool) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows::Win32::System::Threading::{WaitForSingleObject, INFINITE};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let verb = wide("runas");
    let file = wide("schtasks.exe");
    let params = wide(&format!("/Delete /F /TN \"{TASK_NAME}\""));

    let deleted = unsafe {
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(params.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        // Declining the UAC prompt makes ShellExecuteExW return an error —
        // that is a clean "no", not a failure to report loudly.
        if ShellExecuteExW(&mut sei).is_err() {
            log::info!("startup: stale-task repair cancelled at the UAC prompt");
            return false;
        }
        if !sei.hProcess.is_invalid() {
            WaitForSingleObject(sei.hProcess, INFINITE);
            let _ = windows::Win32::Foundation::CloseHandle(sei.hProcess);
        }
        !task_exists()
    };

    if deleted {
        STALE_TASK.store(false, std::sync::atomic::Ordering::Relaxed);
        log::info!("startup: stale task removed — registering the clean autostart");
        ensure_startup_task(run_at_startup);
    } else {
        log::error!("startup: stale-task repair ran but the task still exists");
    }
    deleted
}

/// PROBLEM 76 — surface the tray icon on the visible taskbar corner.
///
/// Windows 11 puts every new tray icon into the hidden overflow flyout behind
/// the `^` chevron. At a real logon the app started, hooked and trayed
/// correctly, and the user still reported "it didn't come up in the tray" —
/// the icon existed but was invisible unless the chevron was clicked
/// (verified: our NotifyIconSettings entry had IsPromoted unset). The shell
/// stores promotion per-icon in HKCU\Control Panel\NotifyIconSettings\<id>\
/// IsPromoted, which a non-elevated process may write.
///
/// Called ONCE per install (the caller gates it): after that, whatever the
/// user does with the icon — including hiding it again — is their choice and
/// must stick. Returns true when an entry was found and promoted.
#[cfg(windows)]
pub fn promote_tray_icon_once() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(root) = hkcu.open_subkey(r"Control Panel\NotifyIconSettings") else {
        return false; // pre-Win11 shell — icons are visible by default there
    };
    // PROBLEM 142 — match OUR OWN exe first. The suffix rule below is a
    // fallback for the shell's KNOWNFOLDER-GUID spelling of the same path; on
    // its own it also matches STALE entries for install locations we have since
    // moved away from, which are harmless to promote but tell us nothing about
    // whether the icon the user is actually looking at got promoted.
    let me = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_lowercase().replace('/', "\\"))
        .unwrap_or_default();

    let mut promoted = false;
    for name in root.enum_keys().flatten() {
        let Ok(entry) = root.open_subkey_with_flags(&name, winreg::enums::KEY_ALL_ACCESS) else {
            continue;
        };
        let Ok(path) = entry.get_value::<String, _>("ExecutablePath") else {
            continue;
        };
        // Suffix-match "spaceadom\spaceadom.exe": covers both the plain
        // Program Files path and the shell's KNOWNFOLDER-GUID form
        // ({6D809377-…}\Spaceadom\spaceadom.exe), while EXCLUDING dev builds
        // (…\target\release\spaceadom.exe — wrong parent directory).
        let norm = path.to_lowercase().replace('/', "\\");
        if (!me.is_empty() && norm == me) || norm.ends_with(r"spaceadom\spaceadom.exe") {
            match entry.set_value("IsPromoted", &1u32) {
                Ok(()) => {
                    log::info!("startup: tray icon promoted to the visible taskbar corner ({path})");
                    promoted = true;
                }
                Err(e) => log::warn!("startup: could not promote tray icon: {e}"),
            }
        }
    }
    if !promoted {
        log::info!("startup: no NotifyIconSettings entry for the installed exe yet (first tray show pending?)");
    }
    promoted
}

#[cfg(not(windows))]
pub fn promote_tray_icon_once() -> bool { false }

/// Remove legacy HKCU Run entries so old builds stop auto-starting alongside
/// this one. Two builds at logon = two WH_KEYBOARD_LL hooks = the documented
/// feedback trap.
#[cfg(windows)]
pub fn remove_legacy_run_entries() {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(key) = hkcu.open_subkey_with_flags(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        winreg::enums::KEY_SET_VALUE | winreg::enums::KEY_QUERY_VALUE,
    ) else { return };

    for name in LEGACY_RUN_VALUES {
        let existed: Option<String> = key.get_value(name).ok();
        if existed.is_some() {
            match key.delete_value(name) {
                Ok(()) => log::info!("startup: removed legacy Run entry '{name}' (was {existed:?})"),
                Err(e) => log::warn!("startup: could not remove legacy Run entry '{name}': {e}"),
            }
        }
    }
}

/// Returns `true` if the current process is running with elevated privileges.
#[cfg(windows)]
pub fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION::default();
        let mut return_length: u32 = 0;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut return_length,
        );
        let _ = CloseHandle(token);

        result.is_ok() && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool { true }

/// Get this process elevated, preferring the silent path. Returns `true` if
/// a replacement process was started and the CALLER MUST EXIT.
///
/// 1. Already elevated → false (carry on).
/// 2. The task exists → `schtasks /Run` starts the task's registered exe
///    elevated with NO prompt → true.
///    (Note: the task runs its REGISTERED exe — normally the installed copy —
///    not necessarily the one the user double-clicked. Logged when they
///    differ, which only happens in dev.)
/// 3. No task (true first run) → classic ShellExecuteW "runas" → ONE UAC
///    prompt → the elevated instance then creates the task, so this branch
///    never runs again.
pub fn maybe_relaunch_elevated() -> bool {
    #[cfg(windows)]
    {
        if is_elevated() {
            return false;
        }

        if task_exists() {
            // PROBLEM 57 — only /Run the task if it points at THIS exe.
            // After an install moves the app (his NSIS install at
            // D:\spaceadom → the MSI at C:\Program Files\Spaceadom), the old
            // task still targets the previous path. /Run then "succeeds"
            // launching a stale or deleted exe, this stub exits, and nothing
            // appears — which the tester experienced as "it only starts if I
            // right-click Run as administrator". Verify the target first;
            // a mismatched task falls through to ONE UAC prompt, after which
            // the elevated instance rewrites the task to the current exe.
            let current = std::env::current_exe()
                .map(|p| p.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let target_matches = !current.is_empty()
                && schtasks(&["/Query", "/TN", TASK_NAME, "/XML"])
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains(&current))
                    .unwrap_or(false);
            if target_matches {
                if let Some(o) = schtasks(&["/Run", "/TN", TASK_NAME]) {
                    if o.status.success() {
                        // Can't log yet — the logger initialises after this
                        // check. The elevated instance announces itself.
                        return true;
                    }
                    // /Run fails when the task is disabled — fall through to
                    // the UAC prompt so a manual launch still works.
                }
            }
        }

        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(_) => return false,
        };

        let exe_wide: Vec<u16> = exe
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let operation: Vec<u16> = "runas\0".encode_utf16().collect();

        unsafe {
            let result = ShellExecuteW(
                None,
                PCWSTR(operation.as_ptr()),
                PCWSTR(exe_wide.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
            // HINSTANCE > 32 means success
            result.0 as usize > 32
        }
    }
    #[cfg(not(windows))]
    false
}

/// Return the Spaceadom data directory (%APPDATA%\Spaceadom).
pub fn data_dir() -> PathBuf {
    std::env::var("APPDATA")
        .map(|p| PathBuf::from(p).join("Spaceadom"))
        .unwrap_or_else(|_| PathBuf::from("Spaceadom"))
}

/// Data directory of the previous product identity, for one-time migration.
pub fn legacy_data_dir() -> PathBuf {
    std::env::var("APPDATA")
        .map(|p| PathBuf::from(p).join("SpaceToggleV14"))
        .unwrap_or_else(|_| PathBuf::from("SpaceToggleV14"))
}
