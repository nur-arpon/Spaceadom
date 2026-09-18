//! PHASE A — `Action::Command`: run a command line, detached, no window.
//!
//! `cmd.exe /C <line>` with `CREATE_NO_WINDOW`, never waited on, never
//! elevated. The engine logs the line and toasts "Ran: …". The UI only offers
//! this in Advanced mode; the engine runs whatever is in the file.

/// The toast line. Pure: the command is shortened to keep the toast one
/// line, and a blank line reads as a refusal rather than "Ran: ".
pub fn toast_text(line: &str) -> String {
    let line = line.trim();
    if line.is_empty() {
        return "▶ Nothing to run — this key has an empty command".into();
    }
    let shown: String = line.chars().take(48).collect();
    if shown.len() < line.len() {
        format!("▶ Ran: {shown}…")
    } else {
        format!("▶ Ran: {shown}")
    }
}

/// Run it. Returns the toast. NEVER called from a test.
pub fn run(line: &str) -> String {
    let line = line.trim();
    if line.is_empty() {
        return toast_text(line);
    }
    log::info!("command: running via cmd.exe /C (Phase A action): {line}");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // `raw_arg` hands the line to cmd.exe verbatim — `arg` would quote
        // it as one argument and cmd would look for a program named after
        // the whole line.
        let spawned = std::process::Command::new("cmd.exe")
            .raw_arg(format!("/C {line}"))
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match spawned {
            Ok(child) => {
                // Detached: the handle is dropped, the process runs on.
                drop(child);
            }
            Err(e) => {
                log::warn!("command: cmd.exe failed to start for '{line}': {e}");
                return format!("▶ Could not run: {e}");
            }
        }
    }
    toast_text(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toast_shows_the_line_and_shortens_a_long_one() {
        assert_eq!(toast_text("control.exe /name Microsoft.PowerOptions"), "▶ Ran: control.exe /name Microsoft.PowerOptions");
        let long = "x".repeat(80);
        let t = toast_text(&long);
        assert!(t.ends_with('…'));
        assert_eq!(t.chars().count(), "▶ Ran: ".chars().count() + 48 + 1);
        assert!(toast_text("   ").contains("Nothing to run"));
    }
}
