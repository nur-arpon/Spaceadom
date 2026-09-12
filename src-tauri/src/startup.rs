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
/// A standard, non-elevated user CANNOT create a task with `schtasks.exe
/// /Create /SC ONLOGON`: it returns `ERROR: Access is denied.` — even with
/// `/RL LIMITED` and a brand-new task name. Verified directly on this
/// machine. (PROBLEM 266 corrected the WHY: it is not the root folder but
/// the "any user" logon trigger schtasks writes; a per-user trigger through
/// the COM API registers fine, see `register_task_script`. This fallback is
/// kept for the machines where even that is refused.) Since PROBLEM 61 removed self-elevation, the app is ALWAYS
/// non-elevated, so on every non-admin machine the logon task was never
/// created and "Run at startup" (ON by default) silently did nothing. The
/// only evidence was one ERROR line in debug.log that nobody reads.
///
/// `HKCU\...\Run` always works for the current user, needs no elevation, and
/// is the canonical per-user autostart. It is used ONLY when the task could
/// not be created, so the two mechanisms can never both fire.
#[cfg(windows)]
fn set_run_key(enabled: bool) {
    // PROBLEM 250 — belt to the braces in ensure_startup_task/apply_task_enabled.
    // A Run value in a packaged install is worse than useless: it would hold the
    // absolute path of an exe under
    // `…\WindowsApps\<Publisher>.Spaceadom_<VERSION>_x64__<hash>\`, and the
    // Store rewrites that directory on every update — so the value would point
    // at a folder that no longer exists, and the shell would fail silently at
    // every logon. This guard is here as well as at the two call sites because
    // set_run_key is reached from FOUR branches of ensure_startup_task, and a
    // future fifth would otherwise inherit the bug with nothing to catch it.
    if crate::packaged::is_packaged() {
        log::info!(
            "startup: PACKAGED — refusing to touch the HKCU Run value. Windows owns \
             autostart for a Store install through the '{}' startupTask; a Run entry \
             here would name a WindowsApps path that the next Store update deletes.",
            crate::packaged::STARTUP_TASK_ID
        );
        return;
    }
    // PROBLEM 254 — a portable copy registers NOTHING with Windows. There is
    // no install to point a Run value at that survives the user simply
    // moving or deleting the folder, and "portable" is understood by anyone
    // who reaches for it to mean "does not attach itself to my system" —
    // writing an autostart entry would be exactly that. The Settings row
    // goes inert with a note (see SETTINGS-PANEL LINES TO ADD in this
    // feature's report) rather than silently doing nothing.
    if crate::portable::is_portable() {
        log::info!(
            "startup: PORTABLE — refusing to touch the HKCU Run value. Portable copies don't \
             start with Windows; put a shortcut in your Startup folder if you want that."
        );
        return;
    }
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

/// PROBLEM 266 — how long after logon the task starts the app.
///
/// PROBLEM 59 chose 30 s to dodge the cold-boot WebView2 race. Since then the
/// overlay got three self-heal paths and the dashboard a retry, and the app
/// itself waits a further 10 s before building the dashboard on an autostart
/// launch. 10 s here keeps a margin for Edge/WebView2's brokers to come up
/// while making the ring usable ~12 s after logon instead of ~100 s.
#[cfg(windows)]
const TASK_DELAY: &str = "PT10S";

/// PROBLEM 266 — the PowerShell that registers the logon task for THIS user
/// through the Task Scheduler COM API (`Register-ScheduledTask`), which a
/// non-elevated user IS allowed to do — unlike `schtasks.exe /Create`, whose
/// `/SC ONLOGON` writes an "at log on of ANY user" trigger that only an
/// administrator may create. That, not the root folder, was PROBLEM 64's
/// "Access is denied": measured 2026-09-12 on the owner's machine, both
/// `schtasks /Create` forms (with and without `/RU`) denied, while this
/// script registered, and `schtasks /Change /Run /Query /Delete` all worked
/// on the task it made. So the Run-key fallback — which Windows starts a
/// minute or more after logon — is no longer the path every install takes.
///
/// The PROBLEM 59 settings (battery-safe, no 3-day time limit) are part of
/// the same registration; there is no second PowerShell round trip.
/// Pure so the tests can read it; the exe path is single-quoted for
/// PowerShell, `'` doubled.
#[cfg(windows)]
fn register_task_script(exe: &str, delay: &str) -> String {
    let exe_q = exe.replace('\'', "''");
    format!(
        "$ErrorActionPreference='Stop'; \
         $a = New-ScheduledTaskAction -Execute '{exe_q}' -Argument '--autostart'; \
         $t = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME; $t.Delay = '{delay}'; \
         $p = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Limited; \
         $s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries \
         -ExecutionTimeLimit ([TimeSpan]::Zero) -StartWhenAvailable -MultipleInstances IgnoreNew; \
         Register-ScheduledTask -TaskName '{TASK_NAME}' -Action $a -Trigger $t -Principal $p \
         -Settings $s -Force | Out-Null"
    )
}

/// Run a PowerShell script with a hidden window and return its output.
#[cfg(windows)]
fn run_powershell(script: &str) -> Option<std::process::Output> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
}

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
    // PROBLEM 250 — the Store install takes none of this path.
    //
    // Neither of the two mechanisms below is available to a packaged app in any
    // useful form: a Scheduled Task would point at a WindowsApps directory the
    // next Store update replaces, and so would a Run value. Windows registers
    // the package's `windows.startupTask` from the manifest at INSTALL time and
    // re-points it on every update, which is exactly the thing this function
    // exists to do by hand for an NSIS install.
    //
    // So this branch REGISTERS NOTHING. It reads what Windows already decided
    // and says so in the log, and it deliberately does not force Windows to
    // agree with `run_at_startup`: a user who switched Spaceadom off in Task
    // Manager ▸ Startup apps has made a decision the app is not allowed to
    // overturn (RequestEnableAsync documents that it will not), and one who
    // switched it ON there should not have it taken away at the next launch by
    // a stale config value. The Settings row reads the live state instead
    // (`get_packaged_startup`), so the UI tells the truth without either side
    // fighting the other.
    if crate::packaged::is_packaged() {
        let state = crate::packaged::startup_task_state();
        let agrees = matches!(
            (run_at_startup, state),
            (true, crate::packaged::StartupState::Enabled)
                | (true, crate::packaged::StartupState::EnabledByPolicy)
                | (false, crate::packaged::StartupState::Disabled)
                | (false, crate::packaged::StartupState::DisabledByUser)
                | (false, crate::packaged::StartupState::DisabledByPolicy)
        );
        log::info!(
            "startup: PACKAGED — no Scheduled Task and no Run key were touched. Windows \
             registered the '{}' startupTask from AppxManifest.xml at install time and \
             re-points it on every Store update. Its state is {state:?}; config says \
             run_at_startup={run_at_startup} ({}). The user's switch is in Settings ▸ \
             Apps ▸ Startup (or Task Manager ▸ Startup apps) and Spaceadom's own row \
             follows it rather than overriding it.",
            crate::packaged::STARTUP_TASK_ID,
            if agrees { "they agree" } else { "THEY DISAGREE — Windows wins, see get_packaged_startup" }
        );
        return;
    }

    // PROBLEM 254 — a portable copy, same rule as `set_run_key`: no
    // Scheduled Task, no Run key, nothing registered with Windows at all.
    // Checked here too (not only inside `set_run_key`) so a future direct
    // caller of the Scheduled-Task branch below cannot bypass it.
    if crate::portable::is_portable() {
        log::info!(
            "startup: PORTABLE — no Scheduled Task and no Run key will be registered. A \
             portable copy does not attach itself to the machine; put a shortcut in your \
             Startup folder if you want it to launch at logon."
        );
        return;
    }

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
    // PROBLEM 59 — the cold-boot WebView2 race.
    //
    // Launching at logon puts us in a fight with Edge/WebView2's own broker
    // processes, the GPU stack and the disk, all still starting. WebView2 then
    // fails with HRESULT(0x80070490) ERROR_NOT_FOUND, Tauri destroys the host
    // window, and the app runs on with NO dashboard and NO overlay — so the
    // Guide HUD never appears and nothing looks clickable. Every manual launch
    // on the tester's machine succeeded; only the cold-boot one failed.
    // The trigger delay (TASK_DELAY) plus the in-app retry cover it.
    //
    // PROBLEM 61 — RunLevel Limited, not Highest. A Highest task fails to
    // register (or registers and cannot start) on a standard non-admin
    // account, and on an admin account it makes the app run elevated at logon
    // but non-elevated from the Start Menu — two different WebView2 user-data
    // and UIPI behaviours for the same app. WH_KEYBOARD_LL needs neither.
    //
    // PROBLEM 266 — registered through the COM API (see register_task_script),
    // because schtasks.exe /Create is what a non-elevated user is denied.
    match run_powershell(&register_task_script(&exe_str, TASK_DELAY)) {
        Some(o) if o.status.success() => {
            log::info!(
                "startup: logon task registered for this user via the Task Scheduler API — \
                 '{TASK_NAME}' → {exe_str} (logon +{TASK_DELAY}, least-privilege, battery-safe)"
            );
            // The task is authoritative when it exists — drop any Run-key
            // fallback so the app cannot be started twice at logon.
            set_run_key(false);
        }
        Some(o) => {
            // PROBLEM 64 — still possible (policy, a task of the same name
            // owned by another user, a broken ScheduledTasks module). Not
            // fatal: fall back to HKCU Run.
            log::warn!(
                "startup: task create failed ({}) — using HKCU Run autostart instead",
                String::from_utf8_lossy(&o.stderr).trim()
            );
            set_run_key(run_at_startup);
            return;
        }
        None => {
            log::warn!("startup: could not run powershell — using HKCU Run autostart instead");
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
    // PROBLEM 250 — the Settings switch, packaged edition. This is the ONE
    // place the app may ask Windows to change the startup state, and Windows
    // may refuse (DisabledByUser / *ByPolicy). The resulting state is returned
    // to the frontend by `get_packaged_startup`, which is what repaints the row
    // — so a refusal is visible instead of a switch that flips back on its own.
    if crate::packaged::is_packaged() {
        let after = crate::packaged::set_startup_task(enabled);
        log::info!(
            "startup: PACKAGED — asked Windows to set the '{}' startupTask to \
             enabled={enabled}; it is now {after:?}",
            crate::packaged::STARTUP_TASK_ID
        );
        return;
    }
    // PROBLEM 254 — nothing for the Settings switch to flip in a portable
    // copy: there is no task and `set_run_key` below already refuses, so
    // this only saves a pointless registry round-trip and logs the reason
    // at the point the user actually touched the control.
    if crate::portable::is_portable() {
        log::info!(
            "startup: PORTABLE — 'Run at startup' has nothing to apply to (no Run key, no \
             Scheduled Task, ever, for a portable copy)."
        );
        return;
    }
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

/// Does one `NotifyIconSettings` entry's `ExecutablePath` describe THIS copy of
/// Spaceadom? Pure — no registry, no filesystem — so every case below is a
/// unit test rather than something reasoned about over a live hive.
///
/// Two ways to match, in order:
///
///  1. **The exact path.** Once the shell has written our own entry, this is
///     the only one that matters.
///  2. **`spaceadom.exe` inside a directory with the SAME NAME as our own
///     exe's directory.** The shell habitually spells a path with a
///     KNOWNFOLDER GUID — `{6D809377-…}\Spaceadom\spaceadom.exe` for
///     `C:\Program Files\Spaceadom\spaceadom.exe` — so the strings differ while
///     the install does not. Comparing the last component of each parent is
///     what survives that rewriting.
///
/// **PROBLEM 250 follow-up — why rule 2 is anchored on OUR directory name and
/// not on the literal `spaceadom\spaceadom.exe`.** The literal is a claim about
/// where this app installs, and it stopped being one install ago. On
/// 2026-09-05 a packaged copy running from
/// `…\WindowsApps\LOCALTEST.Spaceadom_1.0.100.0_x64__nj4cr7rfsqc4c\spaceadom.exe`
/// matched, and promoted, two entries belonging to OTHER copies — the owner's
/// per-user NSIS install and a leftover from the agent container. Anchoring on
/// our own directory name makes the rule say what it means: *this* install's
/// icon. A dev build (`…\target\release\spaceadom.exe`) is still excluded, by
/// the same comparison and for the same reason it always was — "release" is
/// not the directory the installed copy runs from.
///
/// Generalise: **a match rule written as a constant is a fact about the world
/// frozen at the moment it was written.** Derive it from the running process
/// where you can.
#[cfg(windows)]
pub(crate) fn notify_icon_entry_matches(entry_path: &str, current_exe: &str) -> bool {
    let norm = |s: &str| s.trim().to_lowercase().replace('/', "\\");
    let entry = norm(entry_path);
    let me = norm(current_exe);
    if entry.is_empty() || me.is_empty() {
        return false;
    }
    if entry == me {
        return true;
    }
    // Rule 2. Both sides must be a `spaceadom.exe`, and both parents must have
    // the same final component. `rsplit` rather than `Path`: the entry string
    // can carry a `{GUID}` first component that is not a real path element, and
    // this only ever compares the tail.
    let tail = |p: &str| -> Option<(String, String)> {
        let mut it = p.rsplit('\\');
        let file = it.next()?.to_string();
        let parent = it.next()?.to_string();
        if file.is_empty() || parent.is_empty() {
            return None;
        }
        Some((parent, file))
    };
    match (tail(&entry), tail(&me)) {
        (Some((entry_parent, entry_file)), Some((my_parent, my_file))) => {
            entry_file == "spaceadom.exe" && my_file == "spaceadom.exe" && entry_parent == my_parent
        }
        _ => false,
    }
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
///
/// ## PROBLEM 250 follow-up — LIVE TEST 2026-09-05, FINDING D
///
/// **Under an MSIX package this function does nothing at all, deliberately.**
/// The first real packaged launch on this machine produced two log lines that
/// looked like success and were not:
///
/// ```text
/// startup: tray icon promoted … (…\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Local\Spaceadom\spaceadom.exe)
/// startup: tray icon promoted … (C:\Users\beamu\AppData\Local\Spaceadom\spaceadom.exe)
/// ```
///
/// Neither is the packaged copy. Both are STALE entries the old suffix rule
/// `norm.ends_with("spaceadom\\spaceadom.exe")` happily matched, because the
/// packaged exe lives at
/// `…\WindowsApps\<PFN>_<version>_x64__<hash>\spaceadom.exe` and cannot match
/// that suffix. Returning `true` for them told the caller the job was done, so
/// it wrote `tray_promoted_for = <WindowsApps path>` and closed the once-gate:
/// **a Store install's icon would never be promoted, and the config recorded
/// that it had been.**
///
/// And it could not have worked anyway. Registry writes from inside a package
/// are copy-on-write into a private hive — measured the same day: the writes
/// landed in `…\Packages\<PFN>\SystemAppData\Helium\User.dat`, and after the
/// package was removed the real `NotifyIconSettings` entry for the WindowsApps
/// exe still had `IsPromoted` **blank**. The shell reads the real hive. Nothing
/// this function can write reaches it.
///
/// **Is there a non-virtualised route? No — researched 2026-09-05 and closed.**
/// There is no supported API for an app to promote its own notification icon.
/// `NOTIFYICONDATA`'s only visibility state is `NIS_HIDDEN`/`NIS_SHAREDICON`
/// (there is no "promoted" flag); `Shell_NotifyIconGetRect` is read-only
/// geometry; `Windows.UI.Shell` covers taskbar PINNING of app entries, not the
/// notification area. Microsoft states the model outright on the "Notifications
/// and the Notification Area" page: only the user promotes an icon, and the
/// system may do so itself only as a sub-minute preview.
/// `HKCU\Control Panel\NotifyIconSettings\<id>\IsPromoted` is real, is what the
/// shell writes when the user drags an icon, and is an undocumented internal
/// contract — which is exactly why it is unreachable from inside a package.
/// **So Store users promote the icon themselves** (drag it out of the `^`
/// overflow, or Settings ▸ Personalisation ▸ Taskbar ▸ Other system tray
/// icons), and that belongs in the Store listing text, not in a retry loop.
///
/// Generalise: **a write you cannot read back is not a write.** This function
/// returned `true` on the strength of `RegKey::set_value` returning `Ok(())` —
/// which it did, into a hive nobody reads.
#[cfg(windows)]
pub fn promote_tray_icon_once() -> bool {
    // PROBLEM 250 follow-up — the packaged early return, logged ONCE so a
    // caller that retries cannot turn it into a log flood.
    if crate::packaged::is_packaged() {
        static SAID: std::sync::Once = std::sync::Once::new();
        SAID.call_once(|| {
            log::info!(
                "startup: PACKAGED — NOT touching NotifyIconSettings. HKCU writes from inside \
                 an MSIX package are copied into the package's private hive \
                 (…\\Packages\\<PFN>\\SystemAppData\\Helium\\User.dat) and never reach the \
                 hive the shell reads, so promoting the tray icon from here is a write nobody \
                 can read back (measured 2026-09-05, LIVE TEST FINDING D). There is no \
                 supported API for an app to promote its own notification icon — Microsoft's \
                 documented model is that only the USER promotes one. A Store user drags the \
                 icon out of the '^' overflow, or uses Settings > Personalisation > Taskbar > \
                 Other system tray icons."
            );
        });
        return false;
    }

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(root) = hkcu.open_subkey(r"Control Panel\NotifyIconSettings") else {
        return false; // pre-Win11 shell — icons are visible by default there
    };
    // PROBLEM 142 — match OUR OWN exe first. The fallback below is for the
    // shell's KNOWNFOLDER-GUID spelling of the same path; see
    // `notify_icon_entry_matches` for why it is anchored on our own directory
    // NAME rather than on the literal string "spaceadom" (PROBLEM 250
    // follow-up — the literal matched two installs that were not us).
    let me = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut promoted = false;
    for name in root.enum_keys().flatten() {
        let Ok(entry) = root.open_subkey_with_flags(&name, winreg::enums::KEY_ALL_ACCESS) else {
            continue;
        };
        let Ok(path) = entry.get_value::<String, _>("ExecutablePath") else {
            continue;
        };
        if notify_icon_entry_matches(&path, &me) {
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

/// Return the Spaceadom data directory: `%APPDATA%\Spaceadom` normally, or
/// `<exe dir>\data` for a portable copy (PROBLEM 254) — see
/// `portable::data_root`, the one resolver this now wraps. Every existing
/// caller of `data_dir()` (config, the picker cache, release-notes cache,
/// the updater's last-run-version + rollback archive, the overlay re-test
/// marker) becomes portable-aware for free.
pub fn data_dir() -> PathBuf {
    crate::portable::data_root()
}

/// Data directory of the previous product identity, for one-time migration.
pub fn legacy_data_dir() -> PathBuf {
    std::env::var("APPDATA")
        .map(|p| PathBuf::from(p).join("SpaceToggleV14"))
        .unwrap_or_else(|_| PathBuf::from("SpaceToggleV14"))
}

// ─────────────────────────────── tests ───────────────────────────────────────
//
// PROBLEM 250 follow-up — LIVE TEST 2026-09-05, FINDING D. Every case here is
// a real path taken from that run's log or from this machine's registry, not
// an invented one: the whole defect was that a rule written against imagined
// paths matched two installs that were not the running one.

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    const NSIS: &str = r"C:\Users\beamu\AppData\Local\Spaceadom\spaceadom.exe";
    const PROGRAM_FILES: &str = r"C:\Program Files\Spaceadom\spaceadom.exe";
    /// How the shell actually spells the line above in NotifyIconSettings.
    const PROGRAM_FILES_KNOWNFOLDER: &str =
        r"{6D809377-6AF0-444b-8957-A3773F02200E}\Spaceadom\spaceadom.exe";
    /// The packaged copy, from the 2026-09-05 log.
    const PACKAGED: &str =
        r"C:\Program Files\WindowsApps\LOCALTEST.Spaceadom_1.0.100.0_x64__nj4cr7rfsqc4c\spaceadom.exe";
    /// The stale entry the agent container left behind, and the second thing
    /// the old rule wrongly promoted.
    const CONTAINER: &str =
        r"C:\Users\beamu\AppData\Local\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Local\Spaceadom\spaceadom.exe";
    const DEV_BUILD: &str = r"D:\Claude-Projects\SpaceToggle-V14\src-tauri\target\release\spaceadom.exe";

    #[test]
    fn our_own_exact_path_always_matches() {
        assert!(notify_icon_entry_matches(NSIS, NSIS));
        assert!(notify_icon_entry_matches(PACKAGED, PACKAGED));
    }

    /// Case and separator style are the shell's choice, not ours.
    #[test]
    fn case_and_forward_slashes_do_not_defeat_the_exact_match() {
        assert!(notify_icon_entry_matches(
            &NSIS.to_uppercase().replace('\\', "/"),
            NSIS
        ));
    }

    /// Rule 2, and the reason it exists: same install, two spellings.
    #[test]
    fn the_shells_knownfolder_spelling_matches_the_same_install() {
        assert!(notify_icon_entry_matches(PROGRAM_FILES_KNOWNFOLDER, PROGRAM_FILES));
    }

    /// **FINDING D itself.** The packaged copy must not claim either of the
    /// two entries the old `ends_with("spaceadom\\spaceadom.exe")` rule
    /// matched — and it promoted BOTH on 2026-09-05, then recorded the job as
    /// done.
    #[test]
    fn a_packaged_copy_matches_neither_stale_entry() {
        assert!(!notify_icon_entry_matches(NSIS, PACKAGED));
        assert!(!notify_icon_entry_matches(CONTAINER, PACKAGED));
        assert!(!notify_icon_entry_matches(PROGRAM_FILES_KNOWNFOLDER, PACKAGED));
    }

    /// …and the reverse: the ordinary NSIS copy must not adopt the leftover
    /// WindowsApps entry a removed package left behind.
    #[test]
    fn an_unpackaged_copy_does_not_adopt_a_leftover_windowsapps_entry() {
        assert!(!notify_icon_entry_matches(PACKAGED, NSIS));
    }

    /// The rule PROBLEM 142 already had, kept: a `target\release` build is not
    /// the installed copy and its icon is not the user's.
    #[test]
    fn a_dev_build_is_still_excluded() {
        assert!(!notify_icon_entry_matches(DEV_BUILD, NSIS));
        assert!(!notify_icon_entry_matches(NSIS, DEV_BUILD));
    }

    /// A different program that happens to end in `spaceadom.exe` is not us.
    #[test]
    fn an_unrelated_executable_never_matches() {
        assert!(!notify_icon_entry_matches(
            r"C:\Program Files\OtherApp\OtherApp.exe",
            NSIS
        ));
        assert!(!notify_icon_entry_matches(
            r"C:\Tools\NotSpaceadom\spaceadom.exe",
            NSIS
        ));
    }

    /// An unreadable `current_exe()` yields an empty string; matching
    /// EVERYTHING at that point would promote every tray icon on the machine.
    #[test]
    fn an_empty_side_matches_nothing() {
        assert!(!notify_icon_entry_matches("", NSIS));
        assert!(!notify_icon_entry_matches(NSIS, ""));
        assert!(!notify_icon_entry_matches("", ""));
        assert!(!notify_icon_entry_matches("spaceadom.exe", NSIS));
    }

    // PROBLEM 266 — the registration script is the only thing between a
    // 12-second and a 100-second wait for the ring after logon.
    #[test]
    fn register_task_script_is_per_user_least_privilege_and_autostart() {
        let s = register_task_script(NSIS, "PT10S");
        assert!(s.contains("New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME"));
        assert!(s.contains("$t.Delay = 'PT10S'"));
        assert!(s.contains("-RunLevel Limited"));
        assert!(s.contains("-LogonType Interactive"));
        assert!(s.contains("-Argument '--autostart'"));
        assert!(s.contains(&format!("-Execute '{NSIS}'")));
        assert!(s.contains("-AllowStartIfOnBatteries"));
        assert!(s.contains("-ExecutionTimeLimit ([TimeSpan]::Zero)"));
        assert!(s.contains(&format!("-TaskName '{TASK_NAME}'")));
        assert!(!s.contains("schtasks"));
    }

    #[test]
    fn register_task_script_escapes_single_quotes_in_the_exe_path() {
        let s = register_task_script(r"C:\Users\O'Brien\spaceadom.exe", TASK_DELAY);
        assert!(s.contains(r"-Execute 'C:\Users\O''Brien\spaceadom.exe'"));
    }
}
