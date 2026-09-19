//! PHASE A step 2 — `Action::Toggle`: a key under Space FLIPS a Windows
//! setting and the toast says the new state (owner, 2026-09-19: "i thought
//! those would toggle settings automatically" — the `ms-settings:` rows only
//! opened the page).
//!
//! Nine `what` ids (`TOGGLE_IDS`). Each one is ONE hidden PowerShell 5.1
//! (`powershell.exe`, the same spawn as `brightness.rs`) unless it is a
//! Win32 call from Rust (`screen_off`, `lock`) or a chord (`show_desktop`,
//! through `actions::chord` — PROBLEM 227's one-batch path, not
//! reimplemented). The script prints the NEW state on its last line — `on`
//! / `off` (`dark` / `light` for the theme), or `done` for a one-shot — and
//! the toast is built from that (`toast_text`). Exit 3 = "not available on
//! this machine"; exit 4 = Windows refused the radio (Settings › Privacy ›
//! Radios). The wait is capped at `TIMEOUT` (8 s — the WinRT radio calls can
//! be slow); past it the child is killed and the toast says the setting
//! "didn't answer". Everything but the chord runs on its OWN thread and
//! toasts from there; the engine actor is never blocked. Windows' own APIs
//! only, no third-party tool, no elevation.
//!
//! **Night light is undocumented.** Windows has no public switch for it;
//! the script edits the CloudStore blob that Settings itself writes
//! (`…\CloudStore\Store\DefaultAccount\Current\default$windows.data.bluelightreduction.bluelightreductionstate\…`,
//! value `Data`): an enabled blob carries `0x15 0x00` at offset 18 followed by
//! two extra bytes `0x10 0x00` at offset 23, a disabled one `0x13 0x00` at 18
//! and no extra bytes; the toggle flips that and bumps the 8-byte timestamp
//! at offset 10 (bytes 10..14, little-endian increment with carry) so
//! Windows notices the change. This is the algorithm every public "toggle
//! night light" script uses (2019–2025), and it is undocumented — it may
//! need re-checking after a Windows update. The catalogue row says so too.
//!
//! `screen_off`, `sleep` and `lock` are NEVER executed by a test, and `run`
//! is never called from one; `script` and `toast_text` are pure and are.

use std::time::Duration;

/// Every `what` this build knows, in catalogue order.
pub const TOGGLE_IDS: &[&str] = &[
    "bluetooth",
    "wifi",
    "dark_mode",
    "night_light",
    "taskbar_autohide",
    "screen_off",
    "sleep",
    "lock",
    "show_desktop",
];

/// The wait for one script before it is declared unresponsive.
pub const TIMEOUT: Duration = Duration::from_secs(8);

/// PHASE A step 3 (2026-09-19, 1.0.118) — the toast a NEUTRALISED toggle
/// shows instead of doing anything.
pub const NEUTRALISED_TOAST: &str = "⇄ Screen off / Sleep were removed in 1.0.118 — rebind this key";

/// PHASE A step 3 — is this `what` neutralised at run time? `screen_off` and
/// `sleep` (`features::NEUTRALISED_TOGGLE_IDS`) while
/// `features::HAZARDOUS_TOGGLES` is off: a binding from 1.0.117 stays in the
/// file, the key logs + toasts and does nothing. The owner restarted his
/// laptop to escape Screen off (every Space+U woke the panel and turned it
/// off again — six times in 37 s). The other hazardous toggles (radios,
/// theme, night light) still run if already bound: slow, not dangerous.
pub fn neutralised(what: &str) -> bool {
    !crate::features::HAZARDOUS_TOGGLES && crate::features::NEUTRALISED_TOGGLE_IDS.contains(&what)
}

/// What the flip came back with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The script printed `on` / `off` / `dark` / `light` / `done`.
    State(String),
    /// Exit 3 — the radio, key or panel does not exist on this machine.
    NotAvailable,
    /// Exit 4 — the radio API refused (Settings › Privacy › Radios).
    Refused,
    /// No answer inside `TIMEOUT`.
    TimedOut,
    /// Any other exit or a spawn failure.
    Failed,
}

/// The catalogue row's name — what the Space ring, the icon ring, the board
/// and the toast call this toggle. `None` for an id this build does not
/// know (a config from a newer build).
pub fn display_name(what: &str) -> Option<&'static str> {
    Some(match what {
        "bluetooth" => "Bluetooth on/off",
        "wifi" => "Wi‑Fi on/off",
        "dark_mode" => "Dark / light mode",
        "night_light" => "Night light on/off",
        "taskbar_autohide" => "Taskbar auto-hide",
        "screen_off" => "Screen off",
        "sleep" => "Sleep",
        "lock" => "Lock",
        "show_desktop" => "Show desktop",
        _ => return None,
    })
}

/// The subject of a state toast: "Bluetooth", "Wi‑Fi", "Night light"…
fn subject(what: &str) -> &'static str {
    match what {
        "bluetooth" => "Bluetooth",
        "wifi" => "Wi‑Fi",
        "dark_mode" => "Theme",
        "night_light" => "Night light",
        "taskbar_autohide" => "Taskbar auto-hide",
        _ => "",
    }
}

/// The toast for `what` after `outcome`. Pure.
pub fn toast_text(what: &str, outcome: &Outcome) -> String {
    let Some(name) = display_name(what) else {
        return "⇄ Unknown toggle".into();
    };
    match outcome {
        Outcome::State(s) => match (what, s.as_str()) {
            ("dark_mode", "dark") => "⇄ Dark mode".into(),
            ("dark_mode", "light") => "⇄ Light mode".into(),
            ("screen_off", _) => "⇄ Screen off".into(),
            ("sleep", _) => "⇄ Sleeping…".into(),
            ("lock", _) => "⇄ Locking…".into(),
            ("show_desktop", _) => "⇄ Show desktop".into(),
            (_, "on") | (_, "off") => format!("⇄ {} {s}", subject(what)),
            _ => format!("⇄ {name}: {s}"),
        },
        Outcome::NotAvailable => format!("⇄ {name} — not available on this machine"),
        Outcome::Refused => "⇄ Windows refused (Settings › Privacy › Radios)".into(),
        Outcome::TimedOut => format!("⇄ {name} didn't answer"),
        Outcome::Failed => format!("⇄ {name} failed"),
    }
}

/// The PowerShell for `what`, or `None` when the id is unknown or is not
/// done with PowerShell (`screen_off`, `lock` are Win32 from Rust;
/// `show_desktop` is a chord). Pure, so the tests can read every one.
pub fn script(what: &str) -> Option<String> {
    Some(match what {
        "bluetooth" => radio_script("Bluetooth", false),
        "wifi" => radio_script("WiFi", false),
        "dark_mode" => DARK_MODE.into(),
        "night_light" => NIGHT_LIGHT.into(),
        "taskbar_autohide" => TASKBAR_AUTOHIDE.into(),
        "sleep" => SLEEP.into(),
        _ => return None,
    })
}

/// WinRT `Windows.Devices.Radios.Radio` through `AsTask`: request access,
/// list the radios, pick `kind`, set the opposite of its state. `read_only`
/// is the PROOF form (it prints the current state and never calls
/// `SetStateAsync`) — the report's radio check, never the action's.
pub fn radio_script(kind: &str, read_only: bool) -> String {
    let set = if read_only {
        "Write-Output ('current ' + ([string]$r.State).ToLower()); exit 0"
    } else {
        "$res = Await ($r.SetStateAsync([Windows.Devices.Radios.RadioState]::$new)) ([Windows.Devices.Radios.RadioAccessStatus]); \
         if ($res -ne 'Allowed') { exit 4 }; \
         Write-Output $new.ToLower()"
    };
    format!(
        "$ErrorActionPreference='Stop'; \
         [Windows.Devices.Radios.Radio,Windows.System.Devices,ContentType=WindowsRuntime] | Out-Null; \
         Add-Type -AssemblyName System.Runtime.WindowsRuntime; \
         $asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {{ $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' }})[0]; \
         function Await($t, $rt) {{ $m = $asTaskGeneric.MakeGenericMethod($rt); $n = $m.Invoke($null, @($t)); $n.Wait(-1) | Out-Null; $n.Result }}; \
         $access = Await ([Windows.Devices.Radios.Radio]::RequestAccessAsync()) ([Windows.Devices.Radios.RadioAccessStatus]); \
         if ($access -ne 'Allowed') {{ exit 4 }}; \
         $radios = Await ([Windows.Devices.Radios.Radio]::GetRadiosAsync()) ([System.Collections.Generic.IReadOnlyList[Windows.Devices.Radios.Radio]]); \
         $r = $radios | Where-Object {{ $_.Kind -eq '{kind}' }} | Select-Object -First 1; \
         if (-not $r) {{ exit 3 }}; \
         $new = if ($r.State -eq 'On') {{ 'Off' }} else {{ 'On' }}; \
         {set}"
    )
}

/// `AppsUseLightTheme` read, both theme values written flipped, then
/// `WM_SETTINGCHANGE` / `ImmersiveColorSet` broadcast with
/// `SendMessageTimeout(HWND_BROADCAST, …, SMTO_ABORTIFHUNG, 100)` so the
/// taskbar and open apps repaint.
const DARK_MODE: &str = "$ErrorActionPreference='Stop'; \
    $k = 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize'; \
    $cur = (Get-ItemProperty -Path $k -Name AppsUseLightTheme -ErrorAction SilentlyContinue).AppsUseLightTheme; \
    if ($null -eq $cur) { $cur = 1 }; \
    $new = if ([int]$cur -eq 1) { 0 } else { 1 }; \
    Set-ItemProperty -Path $k -Name AppsUseLightTheme -Value $new -Type DWord; \
    Set-ItemProperty -Path $k -Name SystemUsesLightTheme -Value $new -Type DWord; \
    Add-Type -Namespace Spaceadom -Name Theme -MemberDefinition '[DllImport(\"user32.dll\", CharSet=CharSet.Unicode, SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);'; \
    $res = [UIntPtr]::Zero; \
    [Spaceadom.Theme]::SendMessageTimeout([IntPtr]0xFFFF, 0x1A, [UIntPtr]::Zero, 'ImmersiveColorSet', 2, 100, [ref]$res) | Out-Null; \
    Write-Output $(if ($new -eq 0) { 'dark' } else { 'light' })";

/// The CloudStore blob flip described in the module doc. Key missing → 3.
const NIGHT_LIGHT: &str = "$ErrorActionPreference='Stop'; \
    $k = 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\CloudStore\\Store\\DefaultAccount\\Current\\default$windows.data.bluelightreduction.bluelightreductionstate\\windows.data.bluelightreduction.bluelightreductionstate'; \
    if (-not (Test-Path -LiteralPath $k)) { exit 3 }; \
    $d = (Get-ItemProperty -LiteralPath $k -Name Data -ErrorAction SilentlyContinue).Data; \
    if (-not $d -or $d.Length -lt 24) { exit 3 }; \
    [byte[]]$b = $d; \
    if ($b[18] -eq 0x15) { \
        $n = New-Object byte[] ($b.Length - 2); [Array]::Copy($b, 0, $n, 0, 23); [Array]::Copy($b, 25, $n, 23, $b.Length - 25); $n[18] = 0x13; $state = 'off' \
    } elseif ($b[18] -eq 0x13) { \
        $n = New-Object byte[] ($b.Length + 2); [Array]::Copy($b, 0, $n, 0, 23); $n[23] = 0x10; $n[24] = 0x00; [Array]::Copy($b, 23, $n, 25, $b.Length - 23); $n[18] = 0x15; $state = 'on' \
    } else { exit 3 }; \
    for ($i = 10; $i -le 14; $i++) { if ($n[$i] -ne 0xFF) { $n[$i] = [byte]($n[$i] + 1); break } else { $n[$i] = 0 } }; \
    Set-ItemProperty -LiteralPath $k -Name Data -Value $n -Type Binary; \
    Write-Output $state";

/// `SHAppBarMessage`: `ABM_GETSTATE` (4) → flip `ABS_AUTOHIDE` (1) →
/// `ABM_SETSTATE` (10). Instant, no Explorer restart, not the StuckRects
/// registry.
const TASKBAR_AUTOHIDE: &str = "$ErrorActionPreference='Stop'; \
    Add-Type -Namespace Spaceadom -Name AppBar -MemberDefinition '[StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; } [StructLayout(LayoutKind.Sequential)] public struct APPBARDATA { public uint cbSize; public IntPtr hWnd; public uint uCallbackMessage; public uint uEdge; public RECT rc; public IntPtr lParam; } [DllImport(\"shell32.dll\")] public static extern UIntPtr SHAppBarMessage(uint dwMessage, ref APPBARDATA pData);'; \
    $d = New-Object 'Spaceadom.AppBar+APPBARDATA'; \
    $d.cbSize = [System.Runtime.InteropServices.Marshal]::SizeOf($d); \
    $state = [uint32]([Spaceadom.AppBar]::SHAppBarMessage(4, [ref]$d)).ToUInt32(); \
    $new = $state -bxor 1; \
    $d.lParam = [IntPtr]$new; \
    [Spaceadom.AppBar]::SHAppBarMessage(10, [ref]$d) | Out-Null; \
    Write-Output $(if ($new -band 1) { 'on' } else { 'off' })";

/// Suspend (NOT hibernate), not forced, no wake event.
const SLEEP: &str = "Add-Type -AssemblyName System.Windows.Forms; \
    [System.Windows.Forms.Application]::SetSuspendState('Suspend', $false, $false) | Out-Null; \
    Write-Output done";

/// Do the flip and toast. NEVER called from a test — it really changes the
/// machine. `show_desktop` is sent on the caller's (engine) thread because
/// `send_keys_checked` is engine-thread only; everything else goes to a
/// worker so PowerShell start-up never blocks the actor.
pub fn run(what: &str, app_handle: tauri::AppHandle) {
    if display_name(what).is_none() {
        log::warn!("toggle: unknown `what` {what:?} — a config from a newer build? Nothing done");
        crate::show_toast(&app_handle, &toast_text(what, &Outcome::Failed));
        return;
    }
    if what == "show_desktop" {
        log::info!("toggle: show_desktop → Win+D through actions::chord (Phase A step 2)");
        let _ = super::chord::send(&[0x5B, 0x44]);
        crate::show_toast(&app_handle, &toast_text(what, &Outcome::State("done".into())));
        return;
    }
    log::info!("toggle: flipping {what} (Phase A step 2 action)");
    let what = what.to_string();
    std::thread::Builder::new()
        .name("st-toggle".into())
        .spawn(move || worker(&what, &app_handle))
        .map(|_| ())
        .unwrap_or_else(|e| log::warn!("toggle: could not start the worker thread: {e}"));
}

fn worker(what: &str, app_handle: &tauri::AppHandle) {
    match what {
        // One-shots whose toast cannot be seen afterwards: toast FIRST.
        "screen_off" => {
            crate::show_toast(app_handle, &toast_text(what, &Outcome::State("done".into())));
            std::thread::sleep(Duration::from_millis(350));
            screen_off();
        }
        "lock" => {
            crate::show_toast(app_handle, &toast_text(what, &Outcome::State("done".into())));
            std::thread::sleep(Duration::from_millis(350));
            lock_workstation();
        }
        "sleep" => {
            crate::show_toast(app_handle, &toast_text(what, &Outcome::State("done".into())));
            std::thread::sleep(Duration::from_millis(350));
            if let Some(s) = script(what) {
                let outcome = run_script(what, &s);
                if outcome != Outcome::State("done".into()) {
                    crate::show_toast(app_handle, &toast_text(what, &outcome));
                }
            }
        }
        _ => {
            let Some(s) = script(what) else {
                log::warn!("toggle: {what} has no script — nothing done");
                return;
            };
            let outcome = run_script(what, &s);
            crate::show_toast(app_handle, &toast_text(what, &outcome));
        }
    }
}

/// Runs one script with the `TIMEOUT` cap, reads the last stdout line.
fn run_script(what: &str, script: &str) -> Outcome {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let child = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                log::warn!("toggle: {what}: powershell.exe failed to start: {e}");
                return Outcome::Failed;
            }
        };
        let started = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if started.elapsed() >= TIMEOUT => {
                    log::warn!("toggle: {what} did not answer within {}s — killing it", TIMEOUT.as_secs());
                    let _ = child.kill();
                    let _ = child.wait();
                    return Outcome::TimedOut;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(40)),
                Err(e) => {
                    log::warn!("toggle: {what}: wait failed: {e}");
                    let _ = child.kill();
                    return Outcome::Failed;
                }
            }
        }
        let out = match child.wait_with_output() {
            Ok(o) => o,
            Err(e) => {
                log::warn!("toggle: {what}: could not read the script's output: {e}");
                return Outcome::Failed;
            }
        };
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let outcome = outcome_of(out.status.code(), &stdout);
        match &outcome {
            Outcome::State(s) => log::info!("toggle: {what} → {s} ({} ms)", started.elapsed().as_millis()),
            other => log::warn!(
                "toggle: {what} → {other:?} (exit {:?}, stderr: {})",
                out.status.code(),
                stderr.trim()
            ),
        }
        outcome
    }
    #[cfg(not(windows))]
    {
        let _ = (what, script);
        Outcome::Failed
    }
}

/// Exit code + stdout → outcome. Pure: 0 with a last line → that state;
/// 3 → not available; 4 → refused; anything else → failed.
pub fn outcome_of(code: Option<i32>, stdout: &str) -> Outcome {
    match code {
        Some(0) => match stdout.trim().lines().last().map(|l| l.trim().to_ascii_lowercase()) {
            Some(s) if !s.is_empty() => Outcome::State(s),
            _ => Outcome::Failed,
        },
        Some(3) => Outcome::NotAvailable,
        Some(4) => Outcome::Refused,
        _ => Outcome::Failed,
    }
}

/// `SendMessageW(HWND_BROADCAST, WM_SYSCOMMAND, SC_MONITORPOWER, 2)`: every
/// monitor to power-off. NEVER called from a test.
fn screen_off() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_SYSCOMMAND};
        const HWND_BROADCAST: HWND = HWND(0xFFFF as *mut core::ffi::c_void);
        const SC_MONITORPOWER: usize = 0xF170;
        log::info!("toggle: screen_off → SC_MONITORPOWER 2 broadcast");
        SendMessageW(HWND_BROADCAST, WM_SYSCOMMAND, WPARAM(SC_MONITORPOWER), LPARAM(2));
    }
}

/// `LockWorkStation()` (user32). NEVER called from a test.
fn lock_workstation() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Shutdown::LockWorkStation;
        match LockWorkStation() {
            Ok(()) => log::info!("toggle: lock → LockWorkStation ok"),
            Err(e) => log::warn!("toggle: lock → LockWorkStation failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every id has a display name, and every one is either a script or one
    /// of the three non-PowerShell paths — nothing falls through to
    /// "unknown".
    #[test]
    fn every_id_has_a_name_and_a_way_to_run() {
        for id in TOGGLE_IDS {
            assert!(display_name(id).is_some(), "{id} has no display name");
            let via_script = script(id).is_some();
            let via_rust = matches!(*id, "screen_off" | "lock" | "show_desktop");
            assert!(via_script ^ via_rust, "{id}: script={via_script} rust={via_rust}");
            assert!(!toast_text(id, &Outcome::State("on".into())).is_empty());
        }
        assert_eq!(display_name("nope"), None);
        assert_eq!(script("nope"), None);
    }

    /// Each script is non-empty and names the API the brief chose for it —
    /// the one place a wrong mechanism (StuckRects instead of SHAppBarMessage,
    /// Hibernate instead of Suspend) could hide.
    #[test]
    fn every_script_names_its_key_api() {
        let bt = script("bluetooth").unwrap();
        assert!(bt.contains("Windows.Devices.Radios.Radio,Windows.System.Devices,ContentType=WindowsRuntime"));
        assert!(bt.contains("RequestAccessAsync") && bt.contains("GetRadiosAsync") && bt.contains("SetStateAsync"));
        assert!(bt.contains("-eq 'Bluetooth'") && bt.contains("exit 3") && bt.contains("exit 4"));
        assert!(script("wifi").unwrap().contains("-eq 'WiFi'"));

        let dm = script("dark_mode").unwrap();
        assert!(dm.contains("Themes\\Personalize") && dm.contains("AppsUseLightTheme") && dm.contains("SystemUsesLightTheme"));
        assert!(dm.contains("SendMessageTimeout") && dm.contains("'ImmersiveColorSet'") && dm.contains("0x1A"));

        let nl = script("night_light").unwrap();
        assert!(nl.contains("bluelightreduction.bluelightreductionstate"));
        assert!(nl.contains("$b[18] -eq 0x15") && nl.contains("$n[23] = 0x10") && nl.contains("$i = 10; $i -le 14"));
        assert!(nl.contains("exit 3"));

        let tb = script("taskbar_autohide").unwrap();
        assert!(tb.contains("SHAppBarMessage(4,") && tb.contains("SHAppBarMessage(10,") && tb.contains("-bxor 1"));
        assert!(!tb.contains("StuckRects"));

        let sl = script("sleep").unwrap();
        assert!(sl.contains("SetSuspendState('Suspend', $false, $false)") && !sl.contains("Hibernate"));
    }

    /// The proof form of the radio script reads and never sets.
    #[test]
    fn the_read_only_radio_script_never_sets() {
        let s = radio_script("WiFi", true);
        assert!(!s.contains("SetStateAsync"));
        assert!(s.contains("GetRadiosAsync") && s.contains("current "));
    }

    #[test]
    fn the_toast_says_the_new_state_or_why_not() {
        let on = Outcome::State("on".into());
        let off = Outcome::State("off".into());
        assert_eq!(toast_text("bluetooth", &on), "⇄ Bluetooth on");
        assert_eq!(toast_text("wifi", &off), "⇄ Wi‑Fi off");
        assert_eq!(toast_text("dark_mode", &Outcome::State("dark".into())), "⇄ Dark mode");
        assert_eq!(toast_text("dark_mode", &Outcome::State("light".into())), "⇄ Light mode");
        assert_eq!(toast_text("night_light", &on), "⇄ Night light on");
        assert_eq!(toast_text("taskbar_autohide", &off), "⇄ Taskbar auto-hide off");
        assert_eq!(toast_text("screen_off", &Outcome::State("done".into())), "⇄ Screen off");
        assert_eq!(toast_text("sleep", &Outcome::State("done".into())), "⇄ Sleeping…");
        assert_eq!(toast_text("lock", &Outcome::State("done".into())), "⇄ Locking…");
        assert_eq!(toast_text("show_desktop", &Outcome::State("done".into())), "⇄ Show desktop");
        assert_eq!(toast_text("bluetooth", &Outcome::NotAvailable), "⇄ Bluetooth on/off — not available on this machine");
        assert_eq!(toast_text("wifi", &Outcome::Refused), "⇄ Windows refused (Settings › Privacy › Radios)");
        assert_eq!(toast_text("wifi", &Outcome::TimedOut), "⇄ Wi‑Fi on/off didn't answer");
        assert_eq!(toast_text("night_light", &Outcome::Failed), "⇄ Night light on/off failed");
        assert_eq!(toast_text("hologram", &on), "⇄ Unknown toggle");
    }

    #[test]
    fn exit_codes_map_to_outcomes() {
        assert_eq!(outcome_of(Some(0), "noise\r\nON\r\n"), Outcome::State("on".into()));
        assert_eq!(outcome_of(Some(0), "dark"), Outcome::State("dark".into()));
        assert_eq!(outcome_of(Some(0), "   "), Outcome::Failed);
        assert_eq!(outcome_of(Some(3), ""), Outcome::NotAvailable);
        assert_eq!(outcome_of(Some(4), ""), Outcome::Refused);
        assert_eq!(outcome_of(Some(1), "on"), Outcome::Failed);
        assert_eq!(outcome_of(None, "on"), Outcome::Failed);
    }

    /// The catalogue and this module must agree: every `toggle` row's
    /// `target` is an id here, its `name` is `display_name`, and every id
    /// here has a row — the ring label IS the row's name.
    #[test]
    fn the_catalogue_rows_match_the_ids_and_names() {
        let json = include_str!("../../../../src/data/windows-catalogue.json");
        let v: serde_json::Value = serde_json::from_str(json).expect("catalogue parses");
        let rows: Vec<&serde_json::Value> = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|it| it["kind"] == "toggle")
            .collect();
        assert_eq!(rows.len(), TOGGLE_IDS.len(), "one catalogue row per toggle id");
        for (row, id) in rows.iter().zip(TOGGLE_IDS) {
            assert_eq!(row["target"], *id, "catalogue order is TOGGLE_IDS order");
            assert_eq!(row["name"], display_name(id).unwrap(), "{id}");
            assert_eq!(row["id"], format!("system.toggle_{id}"));
        }
        let nl = rows.iter().find(|r| r["target"] == "night_light").unwrap();
        assert!(nl["note"].as_str().unwrap_or("").contains("no public switch"), "night light carries the caveat");
    }
}
