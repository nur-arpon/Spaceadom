/// hook/fullscreen.rs — Periodic exclusive full-screen game window detector.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[cfg(windows)]
use windows::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST},
    UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowLongW, GetWindowRect, GWL_EXSTYLE, GWL_STYLE,
        WS_EX_TOPMOST, WS_POPUP,
    },
};

/// Spawn a background thread that polls for exclusive full-screen windows every
/// 500 ms and writes the result straight into `hook::FULLSCREEN_ACTIVE`.
///
/// PROBLEM 88 — this used to write into an intermediate `Arc<AtomicBool>` that
/// a SECOND anonymous thread in lib.rs copied into `hook::FULLSCREEN_ACTIVE`
/// every 500 ms. Two unmonitored infinite loops, and the hook's very first
/// check is `if FULLSCREEN_ACTIVE { pass everything through }`. If the watcher
/// died while a game had the flag TRUE, the copier faithfully re-stored `true`
/// forever and the ENTIRE app went inert — silently, with no log line, until
/// the user restarted it. Killing the middleman removes one of the two threads
/// that can strand the flag, and `catch_unwind` makes a panicking probe fail
/// toward "not fullscreen" (app keeps working) instead of latching pass-through.
///
/// `flag` is still updated for any other reader, but it is no longer on the
/// path to the hook.
pub fn start_fullscreen_watcher(flag: Arc<AtomicBool>, allowlist: Vec<String>) {
    std::thread::Builder::new()
        .name("st-fullscreen-watcher".into())
        .spawn(move || {
            log::debug!("fullscreen watcher thread started");
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));

                #[cfg(windows)]
                {
                    let detected = std::panic::catch_unwind(|| unsafe {
                        check_fullscreen(&allowlist)
                    })
                    .unwrap_or_else(|_| {
                        // Fail OPEN (not fullscreen): a broken probe must never
                        // be able to disable every shortcut in the app.
                        log::error!(
                            "fullscreen: probe panicked — assuming NOT fullscreen so \
                             shortcuts keep working"
                        );
                        false
                    });
                    flag.store(detected, Ordering::Relaxed);
                    crate::hook::FULLSCREEN_ACTIVE.store(detected, Ordering::Relaxed);
                }
            }
        })
        .map(|_| ())
        // PROBLEM 124 — this was `.expect("failed to spawn fullscreen watcher
        // thread")`, i.e. a PANIC on a failure that is entirely plausible on
        // someone else's machine: `Builder::spawn` returns Err under memory
        // pressure, against a thread-count limit, or inside a restrictive job
        // object. It never fails here, which is exactly why it survived.
        //
        // The consequence was not a missing feature. This runs during setup,
        // BEFORE the keyboard hook is installed, so a panic here killed the
        // app at launch with no window, no tray icon and a log nobody reads —
        // indistinguishable from "it never started". Microsoft Store policy
        // 10.4.2 requires the opposite: handle the exception and stay running.
        //
        // Note the probe INSIDE this same file already fails OPEN, with the
        // comment "a broken probe must never be able to disable every
        // shortcut". The spawn now follows its own file's rule.
        .unwrap_or_else(|e| {
            log::error!(
                "fullscreen: could not spawn the watcher thread ({e}) — continuing WITHOUT \
                 full-screen detection. Shortcuts still work everywhere; they will simply \
                 not stand down inside an exclusive full-screen game."
            );
        });
}

/// Returns true if the foreground window appears to be an exclusive full-screen 3D app.
#[cfg(windows)]
unsafe fn check_fullscreen(allowlist: &[String]) -> bool {
    let hwnd = GetForegroundWindow();
    if hwnd.0.is_null() {
        return false;
    }

    // Check if the process is in the allowlist
    if is_allowlisted(hwnd, allowlist) {
        return false;
    }

    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;

    // Must have WS_POPUP (no frame) and WS_EX_TOPMOST
    let has_popup = (style & WS_POPUP.0) != 0;
    let has_topmost = (ex_style & WS_EX_TOPMOST.0) != 0;

    if !has_popup || !has_topmost {
        return false;
    }

    // Compare window rect to monitor work area
    let mut win_rect = RECT::default();
    if GetWindowRect(hwnd, &mut win_rect).is_err() {
        return false;
    }

    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut mon_info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(monitor, &mut mon_info).as_bool() {
        let mr = mon_info.rcMonitor;
        // Window must cover the full monitor (not just the work area)
        win_rect.left <= mr.left
            && win_rect.top <= mr.top
            && win_rect.right >= mr.right
            && win_rect.bottom >= mr.bottom
    } else {
        false
    }
}

#[cfg(windows)]
unsafe fn is_allowlisted(hwnd: HWND, allowlist: &[String]) -> bool {
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    use windows::core::PWSTR;

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));

    if pid == 0 {
        return false;
    }

    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
    let Ok(handle) = handle else { return false };

    let mut buf = [0u16; 260];
    let mut size = buf.len() as u32;
    let pwstr = PWSTR(buf.as_mut_ptr());
    let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_FORMAT(0), pwstr, &mut size);
    let _ = windows::Win32::Foundation::CloseHandle(handle);

    if ok.is_err() {
        return false;
    }

    let name = String::from_utf16_lossy(&buf[..size as usize]);
    let exe = std::path::Path::new(&name)
        .file_name()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    allowlist.iter().any(|a| a.to_lowercase() == exe.as_str())
}
