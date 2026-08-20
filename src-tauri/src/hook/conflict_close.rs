//! conflict_close.rs — closing a conflicting keyboard program, on request.
//!
//! PROBLEM 155. Until 2026-08-20 this app's stated position was that it
//! "only reports — it never closes anything for you", and that was the right
//! default: silently killing another program is malware behaviour. The owner
//! changed the requirement, and his reason is a good one — a user who does not
//! know what PowerToys is cannot act on a banner telling them to close it:
//!
//!   *"I was wondering of giving a button asking to temporary or permanently
//!   close those… just to make the experience seamless for users who don't
//!   know how to close the conflicting thing, confirm before closing, let them
//!   know if any prompt they have to accept."*
//!
//! So: ASKED FOR, never automatic; confirmed in the UI before it reaches here;
//! and it says in advance when Windows will ask for permission.
//!
//! THREE RULES this module will not break:
//!   1. Only processes on the KNOWN conflict list (hook::conflicts) can be
//!      touched. A process name arriving from anywhere else is refused — this
//!      command is reachable from the webview, so it must not become a
//!      "terminate anything by name" primitive.
//!   2. Ask politely first. WM_CLOSE to the process's windows, and only
//!      escalate to termination if it is still alive.
//!   3. Never elevate silently. A non-elevated app cannot end an elevated one
//!      (PowerToys usually IS elevated) — that returns NeedsPermission, and the
//!      UI offers the retry that shows the Windows prompt.

use serde::Serialize;

/// What happened, in terms the settings panel can act on.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseOutcome {
    /// The program is no longer running.
    pub closed: bool,
    /// It refused without elevation — the caller may retry with `elevate`.
    pub needs_permission: bool,
    /// Its start-with-Windows entry was removed, and from where.
    pub autostart_removed: Vec<String>,
    /// One sentence for the user. Always populated.
    pub message: String,
}

/// Refuse anything that is not a process this app already recognises as a
/// keyboard conflict. Rule 1, and the only thing standing between this command
/// and a remote-kill primitive.
fn is_known_conflict(process: &str) -> bool {
    // Delegates to detect()'s OWN matcher — see is_known_process for why this
    // must not be re-implemented here (PROBLEM 157).
    crate::hook::conflicts::is_known_process(process)
}

#[cfg(windows)]
pub fn close_conflict(process: &str, permanent: bool, elevate: bool) -> CloseOutcome {
    use std::os::windows::process::CommandExt;

    if !is_known_conflict(process) {
        log::warn!("conflict_close: REFUSED '{process}' — not a known keyboard conflict");
        return CloseOutcome {
            closed: false,
            needs_permission: false,
            autostart_removed: vec![],
            message: "Spaceadom only closes programs it recognises as keyboard conflicts.".into(),
        };
    }

    log::info!("conflict_close: asked to close '{process}' (permanent={permanent}, elevate={elevate})");

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // Rule 2 — ask first. `taskkill` without /F sends WM_CLOSE, which lets the
    // program save state and shut down properly. /T includes its children,
    // which matters for PowerToys: the Keyboard Manager engine is a child, and
    // it is the part that actually holds the hook.
    let polite = std::process::Command::new("taskkill")
        .args(["/IM", process, "/T"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let mut denied = matches!(&polite, Ok(o) if String::from_utf8_lossy(&o.stderr).to_lowercase().contains("access is denied"));

    std::thread::sleep(std::time::Duration::from_millis(600));

    if still_running(process) {
        // Rule 3 — escalate to force, and only elevate if the caller has
        // already told the user a prompt is coming.
        if elevate {
            // ShellExecute "runas" is what raises the UAC dialog. If the user
            // declines it we get an error and report honestly rather than
            // pretending the program was closed.
            let ok = run_elevated("taskkill", &format!("/IM {process} /T /F"));
            std::thread::sleep(std::time::Duration::from_millis(900));
            if !ok || still_running(process) {
                return CloseOutcome {
                    closed: false,
                    needs_permission: false,
                    autostart_removed: vec![],
                    message: format!("{process} is still running — the permission prompt was declined, or Windows refused."),
                };
            }
        } else {
            let forced = std::process::Command::new("taskkill")
                .args(["/IM", process, "/T", "/F"])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
            if matches!(&forced, Ok(o) if String::from_utf8_lossy(&o.stderr).to_lowercase().contains("access is denied")) {
                denied = true;
            }
            std::thread::sleep(std::time::Duration::from_millis(600));

            // PROBLEM 157 — if Windows refused for want of rights, raise the
            // prompt NOW. The user already confirmed they want this closed;
            // making them press a third time for a prompt they consented to is
            // how the owner ended up reporting "it did nothing, and I was not
            // given any prompt to approve". The UI's confirm step already says
            // a prompt may appear.
            if still_running(process) && denied {
                log::info!("conflict_close: '{process}' needs elevation — raising the prompt");
                let ok = run_elevated("taskkill", &format!("/IM {process} /T /F"));
                std::thread::sleep(std::time::Duration::from_millis(900));
                if !ok || still_running(process) {
                    return CloseOutcome {
                        closed: false,
                        needs_permission: false,
                        autostart_removed: vec![],
                        message: format!("{process} is still running — the permission prompt was declined, or Windows refused it."),
                    };
                }
            } else if still_running(process) {
                log::info!("conflict_close: '{process}' survived and it is not a rights problem");
                return CloseOutcome {
                    closed: false,
                    needs_permission: true,   // the UI offers Task Manager
                    autostart_removed: vec![],
                    message: format!("Spaceadom could not close {process} — it restarts itself, or Windows is protecting it."),
                };
            }
        }
    }

    let autostart_removed = if permanent { remove_autostart(process) } else { vec![] };
    log::info!("conflict_close: '{process}' closed; autostart entries removed: {autostart_removed:?}");

    let message = if !permanent {
        format!("{process} is closed. It will start again the next time you restart your PC.")
    } else if autostart_removed.is_empty() {
        format!("{process} is closed. Spaceadom found no start-with-Windows entry for it, so nothing was changed on disk.")
    } else {
        format!("{process} is closed, and it will not start with Windows any more. Removed: {}.", autostart_removed.join(", "))
    };

    CloseOutcome { closed: true, needs_permission: false, autostart_removed, message }
}

#[cfg(windows)]
fn still_running(process: &str) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // tasklist prints a "no tasks" line rather than failing when nothing
    // matches, so the process NAME appearing in stdout is the real test.
    match std::process::Command::new("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {process}"), "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .to_lowercase()
            .contains(&process.to_ascii_lowercase()),
        Err(_) => false,
    }
}

/// One elevated command, through the shell's `runas` verb — the only way to
/// raise the UAC prompt from a non-elevated process (this app removed
/// elevation entirely in PROBLEM 61 and is not getting it back).
#[cfg(windows)]
fn run_elevated(exe: &str, args: &str) -> bool {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let verb = HSTRING::from("runas");
    let file = HSTRING::from(exe);
    let params = HSTRING::from(args);
    unsafe {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(params.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        if ShellExecuteExW(&mut info).is_err() {
            log::warn!("conflict_close: elevated '{exe} {args}' was declined or failed");
            return false;
        }
        if !info.hProcess.is_invalid() {
            use windows::Win32::System::Threading::WaitForSingleObject;
            let _ = WaitForSingleObject(info.hProcess, 15_000);
            let _ = windows::Win32::Foundation::CloseHandle(info.hProcess);
        }
        true
    }
}

/// Remove the program's start-with-Windows entries, and REPORT each one so the
/// user is told exactly what changed on their machine and can put it back.
///
/// Deliberately limited to the two places a user-level program actually
/// registers itself. Scheduled Tasks are not touched: PowerToys' task is
/// created by its installer under the machine account, deleting it needs
/// elevation, and getting it wrong breaks a program the user chose to install.
#[cfg(windows)]
fn remove_autostart(process: &str) -> Vec<String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let needle = process.to_ascii_lowercase();
    let mut removed = Vec::new();

    // 1. HKCU Run — any value whose command line names this exe.
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(run) = hkcu.open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        KEY_READ | KEY_WRITE,
    ) {
        let hits: Vec<String> = run
            .enum_values()
            .filter_map(|v| v.ok())
            .filter(|(_, val)| val.to_string().to_ascii_lowercase().contains(&needle))
            .map(|(name, _)| name)
            .collect();
        for name in hits {
            if run.delete_value(&name).is_ok() {
                removed.push(format!("the \"{name}\" startup entry"));
            }
        }
    }

    // 2. The Startup folder — a .lnk whose name or target names this exe.
    if let Ok(appdata) = std::env::var("APPDATA") {
        let dir = std::path::Path::new(&appdata)
            .join(r"Microsoft\Windows\Start Menu\Programs\Startup");
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                let stem = needle.trim_end_matches(".exe");
                if name.contains(stem) && std::fs::remove_file(e.path()).is_ok() {
                    removed.push(format!("its shortcut in the Startup folder ({})", e.file_name().to_string_lossy()));
                }
            }
        }
    }

    removed
}

#[cfg(not(windows))]
pub fn close_conflict(_process: &str, _permanent: bool, _elevate: bool) -> CloseOutcome {
    CloseOutcome {
        closed: false,
        needs_permission: false,
        autostart_removed: vec![],
        message: "Closing programs is only supported on Windows.".into(),
    }
}

/// Open Task Manager on its Start-up apps tab — the fallback when Spaceadom
/// cannot close something itself (owner, 2026-08-20: *"if you cannot close it,
/// then direct the user to startup menu of task manager so that they can just
/// turn it off"*).
///
/// `/0 /startup` is Task Manager's own documented switch for that tab, so the
/// user lands where the fix is rather than on a process list they must search.
#[cfg(windows)]
pub fn open_startup_manager() -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let ok = std::process::Command::new("taskmgr.exe")
        .args(["/0", "/startup"])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .is_ok();
    log::info!("conflict_close: opened Task Manager startup tab: {ok}");
    ok
}

#[cfg(not(windows))]
pub fn open_startup_manager() -> bool { false }
