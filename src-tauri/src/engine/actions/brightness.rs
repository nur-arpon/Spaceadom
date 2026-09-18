//! PHASE A — `Action::Brightness`: nudge the INTERNAL panel's brightness by
//! `delta` through WMI, the same path the keyboard's own brightness keys
//! take (owner decision 2026-09-18: Windows' own, no Twinkle Tray, no DDC/CI,
//! external monitors out of scope).
//!
//! Done with one hidden PowerShell: read `WmiMonitorBrightness`'s
//! `CurrentBrightness`, clamp `current + delta` to 0..=100, call
//! `WmiMonitorBrightnessMethods.WmiSetBrightness(0, new)`, print the new
//! value. PowerShell start-up is a few hundred milliseconds, so the work is
//! done on its OWN thread and the toast is shown from there — the engine
//! actor is never blocked. A machine with no internal panel has no instance
//! of the class; the script exits 3 and the toast says so.

/// `current + delta`, clamped to 0..=100. Pure.
pub fn clamp_brightness(current: i32, delta: i32) -> u8 {
    (current + delta).clamp(0, 100) as u8
}

/// The toast for an outcome: `Some(pct)` after a successful set, `None`
/// when there is no built-in display.
pub fn toast_text(result: Option<u8>) -> String {
    match result {
        Some(pct) => format!("☼ Brightness {pct}%"),
        None => "☼ No built-in display to adjust".into(),
    }
}

/// The PowerShell the worker runs. Pure, so the test can read it.
pub fn script(delta: i32) -> String {
    format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         $b = Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightness | Select-Object -First 1; \
         if (-not $b) {{ exit 3 }}; \
         $n = [Math]::Max(0, [Math]::Min(100, [int]$b.CurrentBrightness + ({delta}))); \
         $m = Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightnessMethods | Select-Object -First 1; \
         if (-not $m) {{ exit 3 }}; \
         Invoke-CimMethod -InputObject $m -MethodName WmiSetBrightness -Arguments @{{Timeout=0; Brightness=$n}} | Out-Null; \
         Write-Output $n"
    )
}

/// Adjust and toast, off the engine thread. NEVER called from a test — it
/// really changes the panel.
pub fn adjust(delta: i32, app_handle: tauri::AppHandle) {
    log::info!("brightness: adjusting the internal panel by {delta:+} via WMI (Phase A action)");
    std::thread::Builder::new()
        .name("st-brightness".into())
        .spawn(move || {
            let msg = toast_text(run_script(delta));
            crate::show_toast(&app_handle, &msg);
        })
        .map(|_| ())
        .unwrap_or_else(|e| log::warn!("brightness: could not start the worker thread: {e}"));
}

/// Runs the script; `Some(new percent)` or `None` for no panel / failure.
fn run_script(delta: i32) -> Option<u8> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let out = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script(delta)])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .output();
        match out {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout);
                let pct = text.trim().lines().last().and_then(|l| l.trim().parse::<u8>().ok());
                log::info!("brightness: WMI set → {pct:?}");
                pct
            }
            Ok(o) => {
                log::warn!(
                    "brightness: PowerShell exited {:?} — no internal panel, or WMI refused \
                     (stderr: {})",
                    o.status.code(),
                    String::from_utf8_lossy(&o.stderr).trim()
                );
                None
            }
            Err(e) => {
                log::warn!("brightness: powershell.exe failed to start: {e}");
                None
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = delta;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clamp_holds_both_ends() {
        assert_eq!(clamp_brightness(50, 10), 60);
        assert_eq!(clamp_brightness(95, 10), 100);
        assert_eq!(clamp_brightness(5, -10), 0);
        assert_eq!(clamp_brightness(0, 0), 0);
    }

    #[test]
    fn the_toast_reports_the_percent_or_the_missing_panel() {
        assert_eq!(toast_text(Some(60)), "☼ Brightness 60%");
        assert_eq!(toast_text(None), "☼ No built-in display to adjust");
    }

    /// The script names both WMI classes, clamps, and carries the delta with
    /// its sign — the arithmetic lives in PowerShell, so this is the only
    /// place a wrong sign could hide.
    #[test]
    fn the_script_reads_sets_and_carries_the_delta() {
        let s = script(-10);
        assert!(s.contains("WmiMonitorBrightness "));
        assert!(s.contains("WmiMonitorBrightnessMethods"));
        assert!(s.contains("WmiSetBrightness"));
        assert!(s.contains("+ (-10)"));
        assert!(s.contains("[Math]::Min(100"));
        assert!(script(10).contains("+ (10)"));
    }
}
