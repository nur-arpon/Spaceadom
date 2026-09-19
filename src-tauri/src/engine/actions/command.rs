//! PHASE A — `Action::Command`: run a PowerShell line on the user's behalf.
//!
//! STEP 3 (2026-09-19, 1.0.118): PowerToys-Run parity. The line is handed
//! to `powershell.exe -NoProfile -ExecutionPolicy Bypass -Command <line>`
//! with `CREATE_NO_WINDOW`, stdin null, and a watcher thread that KILLS it
//! after `TIMEOUT` (60 s) — a one-liner that hangs must not accumulate.
//! 1.0.116/117 ran `cmd.exe /C`; a cmd line that still needs cmd is written
//! `cmd /c …` in the box.
//!
//! `elevated: true` goes through `ShellExecuteW` with the verb `runas`, on
//! `powershell.exe` with the same arguments: WINDOWS shows its own UAC
//! prompt, every time, and the app's process never elevates (PROBLEM 61 —
//! there is no elevation anywhere in this program, and this does not add
//! any: the consent dialog and the elevated child are the OS's). Declined
//! prompt → `ShellExecuteW` fails with `ERROR_CANCELLED` and the toast says
//! "cancelled".
//!
//! The engine logs the line and toasts "Ran: …". The UI only offers this in
//! Advanced mode; the engine runs whatever is in the file. NEVER run from a
//! test: `args()` and `toast_text()` are pure and are tested; `run` is not.

use std::time::Duration;

/// The wait before a still-running line is killed.
pub const TIMEOUT: Duration = Duration::from_secs(60);

/// The `powershell.exe` argument string for `line` — ONE place, shared by the
/// plain spawn and the `runas` path, so the two can never run different
/// things. `-Command` takes the rest of the command line verbatim, which is
/// why this is a raw string and not an argv vector: PowerShell re-parses it.
pub fn args(line: &str) -> String {
    format!("-NoProfile -ExecutionPolicy Bypass -Command {}", line.trim())
}

/// The toast line. Pure: the command is shortened to keep the toast one
/// line, and a blank line reads as a refusal rather than "Ran: ".
pub fn toast_text(line: &str, elevated: bool) -> String {
    let line = line.trim();
    if line.is_empty() {
        return "▶ Nothing to run — this key has an empty command".into();
    }
    let shown: String = line.chars().take(48).collect();
    let ellipsis = if shown.len() < line.len() { "…" } else { "" };
    if elevated {
        format!("▶ Asked Windows to run as administrator: {shown}{ellipsis}")
    } else {
        format!("▶ Ran: {shown}{ellipsis}")
    }
}

/// Run it. Returns the toast. NEVER called from a test.
pub fn run(line: &str, elevated: bool) -> String {
    let line = line.trim();
    if line.is_empty() {
        return toast_text(line, elevated);
    }
    log::info!(
        "command: running via powershell.exe -NoProfile -ExecutionPolicy Bypass -Command{} (Phase A action, step 3): {line}",
        if elevated { " — ELEVATED through ShellExecuteW runas, Windows' own UAC prompt" } else { "" }
    );
    #[cfg(windows)]
    {
        if elevated {
            return match run_as_admin(&args(line)) {
                Ok(()) => toast_text(line, true),
                Err(e) => {
                    log::warn!("command: ShellExecuteW runas refused '{line}': {e}");
                    format!("▶ Administrator run cancelled: {e}")
                }
            };
        }
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // `raw_arg` hands the arguments to powershell verbatim — `arg` would
        // quote the whole line as one argument and `-Command` would look for
        // a command named after the whole line.
        let spawned = std::process::Command::new("powershell.exe")
            .raw_arg(args(line))
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match spawned {
            Ok(child) => reap_after(child, line.to_string()),
            Err(e) => {
                log::warn!("command: powershell.exe failed to start for '{line}': {e}");
                return format!("▶ Could not run: {e}");
            }
        }
    }
    toast_text(line, elevated)
}

/// Watch the child from its own thread: log the exit code when it ends, kill
/// it at `TIMEOUT`. The engine actor is never blocked.
#[cfg(windows)]
fn reap_after(mut child: std::process::Child, line: String) {
    let shown = line.clone();
    let spawned = std::thread::Builder::new().name("st-command-reaper".into()).spawn(move || {
        let started = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    log::info!("command: '{line}' exited with {status} after {} ms", started.elapsed().as_millis());
                    return;
                }
                Ok(None) if started.elapsed() >= TIMEOUT => {
                    let killed = child.kill();
                    let _ = child.wait();
                    log::warn!(
                        "command: '{line}' was still running after {} s — killed ({killed:?})",
                        TIMEOUT.as_secs()
                    );
                    return;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(250)),
                Err(e) => {
                    log::warn!("command: could not watch '{line}': {e}");
                    return;
                }
            }
        }
    });
    if let Err(e) = spawned {
        log::warn!("command: no reaper thread for '{shown}' ({e}) — it runs unwatched");
    }
}

/// `ShellExecuteW(NULL, "runas", "powershell.exe", args, NULL, SW_HIDE)`.
/// Windows raises the UAC prompt; a declined prompt is `ERROR_CANCELLED`
/// (1223). `SW_HIDE` is a request the elevated console may or may not honour
/// — the Secure Desktop transition owns that window, not us.
#[cfg(windows)]
fn run_as_admin(arguments: &str) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let verb = wide("runas");
    let file = wide("powershell.exe");
    let params = wide(arguments);
    let h = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            PCWSTR(params.as_ptr()),
            PCWSTR::null(),
            SW_HIDE,
        )
    };
    // The documented contract: > 32 is success; <= 32 is an error code.
    let code = h.0 as usize;
    if code > 32 {
        Ok(())
    } else {
        let os = std::io::Error::last_os_error();
        Err(if os.raw_os_error() == Some(1223) { "the UAC prompt was declined".to_string() } else { format!("{os}") })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toast_shows_the_line_and_shortens_a_long_one() {
        assert_eq!(toast_text("control.exe /name Microsoft.PowerOptions", false), "▶ Ran: control.exe /name Microsoft.PowerOptions");
        let long = "x".repeat(80);
        let t = toast_text(&long, false);
        assert!(t.ends_with('…'));
        assert_eq!(t.chars().count(), "▶ Ran: ".chars().count() + 48 + 1);
        assert!(toast_text("   ", false).contains("Nothing to run"));
        assert!(toast_text("   ", true).contains("Nothing to run"));
        assert_eq!(toast_text("Restart-Service Spooler", true), "▶ Asked Windows to run as administrator: Restart-Service Spooler");
    }

    /// The exact PowerShell invocation, PowerToys-Run style, and it is the
    /// same string for the plain spawn and the `runas` path.
    #[test]
    fn the_powershell_arguments_are_no_profile_bypass_command_line() {
        assert_eq!(args("Get-Date"), "-NoProfile -ExecutionPolicy Bypass -Command Get-Date");
        assert_eq!(args("  Get-Date | Out-File x.txt  "), "-NoProfile -ExecutionPolicy Bypass -Command Get-Date | Out-File x.txt");
        assert!(args("x").starts_with("-NoProfile "), "no profile: the user's profile scripts never run under a key");
        assert!(args("x").contains("-ExecutionPolicy Bypass"), "a machine policy of Restricted must not silently no-op the key");
        assert!(!args("x").contains("cmd.exe"), "1.0.116/117 ran cmd.exe /C; step 3 is PowerShell");
    }

    #[test]
    fn the_timeout_is_a_minute() {
        assert_eq!(TIMEOUT, Duration::from_secs(60));
    }
}
