//! PHASE A — `Action::Uri`: open a target Windows already knows how to open.
//!
//! `ms-settings:display`, `shell:Downloads`, `shell:AppsFolder\…`, any
//! `scheme:` URI — all go through `smart_cascade::open_target`, the SAME
//! `ShellExecuteExW` path the launch cascade uses for `shell:` verbs and
//! protocol URIs, so there is one launcher, not two. A target that reads as
//! a COMMAND LINE (an exe with arguments — `control.exe /name
//! Microsoft.PowerOptions`) is handed to `command::run` instead: ShellExecute
//! would take the whole string as a file name and find nothing.

/// Is this target a command line rather than a URI? Pure: an exe name
/// followed by a space and arguments. `ms-settings:display` and
/// `shell:Downloads` have no space; `shell:AppsFolder\Some App` may — the
/// `shell:` prefix wins.
pub fn is_command_line(target: &str) -> bool {
    let t = target.trim();
    if t.len() > 6 && t[..6].eq_ignore_ascii_case("shell:") {
        return false;
    }
    let Some(first) = t.split_whitespace().next() else {
        return false;
    };
    let has_args = t.len() > first.len();
    let exe = first.to_ascii_lowercase();
    has_args && (exe.ends_with(".exe") || exe.ends_with(".cmd") || exe.ends_with(".bat"))
}

/// The toast line: the binding's label if it has one, else the target.
pub fn toast_text(label: Option<&str>, target: &str, ok: bool) -> String {
    let subject = label.filter(|l| !l.trim().is_empty()).unwrap_or(target);
    if ok {
        format!("⚙ {subject}")
    } else {
        format!("❌ {subject} could not be opened")
    }
}

/// Open it. Returns the toast. NEVER called from a test.
pub fn open(target: &str, label: Option<&str>, app_handle: Option<tauri::AppHandle>) -> String {
    let t = target.trim();
    if t.is_empty() {
        return "⚙ Nothing to open — this key has an empty target".into();
    }
    if is_command_line(t) {
        log::info!("uri: '{t}' reads as a command line — running it (Phase A action)");
        let _ = super::command::run(t);
        return toast_text(label, t, true);
    }
    log::info!("uri: opening '{t}' through ShellExecute (Phase A action)");
    let ok = super::smart_cascade::open_target(t, app_handle);
    toast_text(label, t, ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uri_is_a_uri_and_an_exe_with_arguments_is_a_command() {
        assert!(!is_command_line("ms-settings:display"));
        assert!(!is_command_line("shell:Downloads"));
        assert!(!is_command_line(r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"));
        assert!(!is_command_line("https://example.com/a b"));
        assert!(!is_command_line("notepad.exe"), "a bare exe is a launch, not a command");
        assert!(is_command_line("control.exe /name Microsoft.PowerOptions"));
        assert!(is_command_line(r"C:\Windows\System32\rundll32.exe user32.dll,LockWorkStation"));
        assert!(!is_command_line(""));
    }

    #[test]
    fn the_toast_uses_the_label_when_there_is_one() {
        assert_eq!(toast_text(Some("Display"), "ms-settings:display", true), "⚙ Display");
        assert_eq!(toast_text(None, "ms-settings:display", true), "⚙ ms-settings:display");
        assert_eq!(toast_text(Some(" "), "shell:Downloads", false), "❌ shell:Downloads could not be opened");
    }
}
