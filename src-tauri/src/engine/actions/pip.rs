/// engine/actions/pip.rs — Picture-in-Picture 4-corner cycle.
/// Mirrors V11 TogglePiP() with border stripping and spring animation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct PipEntry {
    pub original_style: i32,
    pub original_ex_style: i32,
    pub original_x: i32,
    pub original_y: i32,
    pub original_w: i32,
    pub original_h: i32,
    pub was_maximized: bool,
    pub position_index: u8, // 0=TopLeft, 1=TopRight, 2=BottomRight, 3=BottomLeft
}

pub type PipCache = Arc<Mutex<HashMap<isize, PipEntry>>>;

/// Create a new empty PiP cache.
pub fn new_cache() -> PipCache {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Toggle PiP on the current foreground window.
/// Returns a notification message string.
pub fn toggle_pip(cache: &PipCache) -> String {
    #[cfg(windows)]
    unsafe {
        toggle_pip_win32(cache)
    }
    #[cfg(not(windows))]
    String::from("PiP not supported on this platform")
}

#[cfg(windows)]
unsafe fn toggle_pip_win32(cache: &PipCache) -> String {
    use windows::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST},
        UI::WindowsAndMessaging::{
            GetCursorPos, GetForegroundWindow, GetWindowLongW, GetWindowRect, IsZoomed,
            SetWindowLongW, SetWindowPos, ShowWindow, GWL_EXSTYLE, GWL_STYLE,
            HWND_TOPMOST, SWP_FRAMECHANGED, SWP_SHOWWINDOW, SWP_NOMOVE, SWP_NOSIZE, SW_RESTORE, WS_CAPTION, WS_THICKFRAME,
        },
    };
    use windows::Win32::Foundation::POINT;

    let hwnd = GetForegroundWindow();
    if hwnd.0.is_null() {
        log::warn!("pip: no foreground window — nothing to toggle");
        return String::new();
    }

    // Get the monitor where the cursor currently resides
    let mut cursor_pos = POINT::default();
    GetCursorPos(&mut cursor_pos).ok();
    let monitor = MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST);
    let mut mon_info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(monitor, &mut mon_info).as_bool() {
        log::warn!("pip: GetMonitorInfoW failed for hwnd {:?}", hwnd);
        return String::new();
    }

    let wa = mon_info.rcWork;
    let mon_w = (wa.right - wa.left) as i32;
    let mon_h = (wa.bottom - wa.top) as i32;

    // PiP is positioned against the WORK area (rcWork, excludes the taskbar),
    // not the raw monitor bounds — so rcMonitor's dimensions are unused.

    // PiP size = 50% × 50% of monitor (i.e., 25% area)
    let pip_w = mon_w / 2;
    let pip_h = mon_h / 2;

    let mut cache_lock = cache.lock().unwrap_or_else(|p| p.into_inner());
    let hwnd_key = hwnd.0 as isize;

    if let Some(entry) = cache_lock.get_mut(&hwnd_key) {
        entry.position_index += 1;
        let idx = entry.position_index;

        if idx >= 4 {
            // 5th tap: Restore Frame & Remove from PiP
            log::info!("pip: restoring hwnd {hwnd_key:#x} to original frame");
            unsafe { restore_window(hwnd, entry) };
            let msg = "↩️ Frame Restored".to_string();
            cache_lock.remove(&hwnd_key);
            return msg;
        }

        let (x, y) = corner_position(idx, wa.left, wa.top, pip_w, pip_h, mon_w, mon_h);
        log::info!("pip: hwnd {hwnd_key:#x} → corner {idx} at ({x},{y})");
        animate_to(hwnd, x, y, pip_w, pip_h);
        corner_label(idx)
    } else {
        // Enter PiP mode for the first time
        log::info!("pip: entering PiP for hwnd {hwnd_key:#x}");
        let style = GetWindowLongW(hwnd, GWL_STYLE);
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let maximized = IsZoomed(hwnd).as_bool();

        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).ok();

        if maximized {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }

        let entry = PipEntry {
            original_style: style,
            original_ex_style: ex_style,
            original_x: rect.left,
            original_y: rect.top,
            original_w: rect.right - rect.left,
            original_h: rect.bottom - rect.top,
            was_maximized: maximized,
            position_index: 0,
        };
        cache_lock.insert(hwnd_key, entry);

        // Strip borders
        let new_style = style & !(WS_CAPTION.0 as i32 | WS_THICKFRAME.0 as i32);
        SetWindowLongW(hwnd, GWL_STYLE, new_style);
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE,
        ).ok();

        let (x, y) = corner_position(0, wa.left, wa.top, pip_w, pip_h, mon_w, mon_h);
        animate_to(hwnd, x, y, pip_w, pip_h);

        "📺 PiP: Top-Left".into()
    }
}

#[cfg(windows)]
fn corner_position(idx: u8, left: i32, top: i32, pip_w: i32, pip_h: i32, mon_w: i32, mon_h: i32)
    -> (i32, i32)
{
    match idx {
        1 => (left + mon_w - pip_w, top),              // Top-Right
        2 => (left + mon_w - pip_w, top + mon_h - pip_h), // Bottom-Right
        3 => (left, top + mon_h - pip_h),               // Bottom-Left
        _ => (left, top),                               // Top-Left (fallback)
    }
}

#[cfg(windows)]
fn corner_label(idx: u8) -> String {
    match idx {
        0 => "📐 PiP: Top-Left".into(),
        1 => "📐 PiP: Top-Right".into(),
        2 => "📐 PiP: Bottom-Right".into(),
        3 => "📐 PiP: Bottom-Left".into(),
        _ => "📐 PiP".into(),
    }
}

/// Animate window to target position using a spring physics loop.
/// Spawns a short-lived thread so it doesn't block the engine actor.
#[cfg(windows)]
fn animate_to(hwnd: windows::Win32::Foundation::HWND, tx: i32, ty: i32, tw: i32, th: i32) {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, SetWindowPos, SWP_NOZORDER, SWP_NOACTIVATE};
    use windows::Win32::Foundation::RECT;

    let hwnd_raw = hwnd.0 as isize;
    std::thread::spawn(move || {
        // Spring constants (120fps tuned)
        let k: f64 = 0.18;
        let c: f64 = 0.42;

        let mut rect = RECT::default();
        unsafe { GetWindowRect(windows::Win32::Foundation::HWND(hwnd_raw as *mut _), &mut rect).ok() };

        let mut x = rect.left as f64;
        let mut y = rect.top as f64;
        let mut vx = 0f64;
        let mut vy = 0f64;

        for _ in 0..120 {
            let fx = -k * (x - tx as f64) - c * vx;
            let fy = -k * (y - ty as f64) - c * vy;
            vx += fx;
            vy += fy;
            x += vx;
            y += vy;

            unsafe {
                SetWindowPos(
                    windows::Win32::Foundation::HWND(hwnd_raw as *mut _),
                    windows::Win32::Foundation::HWND(std::ptr::null_mut()),
                    x.round() as i32,
                    y.round() as i32,
                    tw,
                    th,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                ).ok();
            }

            if (x - tx as f64).abs() < 0.5 && vx.abs() < 0.5 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(8));
        }

        // Snap to exact target
        unsafe {
            SetWindowPos(
                windows::Win32::Foundation::HWND(hwnd_raw as *mut _),
                windows::Win32::Foundation::HWND(std::ptr::null_mut()),
                tx, ty, tw, th,
                SWP_NOZORDER | SWP_NOACTIVATE,
            ).ok();
        }
    });
}

/// Restore window to its original state (style, position, always-on-top off).
#[cfg(windows)]
unsafe fn restore_window(hwnd: windows::Win32::Foundation::HWND, entry: &PipEntry) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowLongW, SetWindowPos, ShowWindow, GWL_EXSTYLE, GWL_STYLE,
        HWND_NOTOPMOST, SWP_FRAMECHANGED, SWP_SHOWWINDOW, SW_SHOWMAXIMIZED,
    };

    SetWindowLongW(hwnd, GWL_STYLE, entry.original_style);
    SetWindowLongW(hwnd, GWL_EXSTYLE, entry.original_ex_style);
    SetWindowPos(
        hwnd,
        HWND_NOTOPMOST,
        entry.original_x,
        entry.original_y,
        entry.original_w,
        entry.original_h,
        SWP_FRAMECHANGED | SWP_SHOWWINDOW,
    ).ok();

    if entry.was_maximized {
        let _ = ShowWindow(hwnd, SW_SHOWMAXIMIZED);
    }
}
