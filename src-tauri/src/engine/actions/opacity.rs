/// engine/actions/opacity.rs — Window opacity modifier via scroll wheel.
/// Mirrors V11 WheelUp/WheelDown handlers with 25% floor and notification suppression.

const OPACITY_STEP: u8 = 15;
const OPACITY_FLOOR: u8 = 64; // 25% of 255

/// Increase window opacity by one step.
pub fn increase_opacity() {
    #[cfg(windows)]
    unsafe { adjust_opacity(OPACITY_STEP as i16) }
}

/// Decrease window opacity by one step (floored at OPACITY_FLOOR).
pub fn decrease_opacity() {
    #[cfg(windows)]
    unsafe { adjust_opacity(-(OPACITY_STEP as i16)) }
}

#[cfg(windows)]
unsafe fn adjust_opacity(delta: i16) {
    use windows::Win32::{
        Foundation::{COLORREF, HWND},
        UI::WindowsAndMessaging::{
            GetForegroundWindow, GetLayeredWindowAttributes, GetWindowLongW,
            SetLayeredWindowAttributes, SetWindowLongW, GWL_EXSTYLE,
            LAYERED_WINDOW_ATTRIBUTES_FLAGS, LWA_ALPHA, WS_EX_LAYERED,
        },
    };

    let hwnd: HWND = GetForegroundWindow();
    if hwnd.0.is_null() {
        return;
    }

    // Skip SpaceToggle's own windows (handle stored in global below)
    if is_own_window(hwnd) {
        return;
    }

    // Ensure the window has the layered style
    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
    if (ex_style & WS_EX_LAYERED.0 as i32) == 0 {
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as i32);
    }

    // Read current alpha
    let mut key = COLORREF(0);
    let mut alpha: u8 = 255;
    let mut flags = LAYERED_WINDOW_ATTRIBUTES_FLAGS(0);
    let got = GetLayeredWindowAttributes(hwnd, Some(&mut key), Some(&mut alpha), Some(&mut flags));

    if got.is_err() || (flags.0 & LWA_ALPHA.0) == 0 {
        alpha = 255; // treat as fully opaque
    }

    // Apply delta with clamping
    let new_alpha = if delta > 0 {
        alpha.saturating_add(delta as u8)
    } else {
        let drop = (-delta) as u8;
        if alpha <= OPACITY_FLOOR + drop {
            OPACITY_FLOOR
        } else {
            alpha - drop
        }
    };

    let clamped = new_alpha.clamp(OPACITY_FLOOR, 255);
    SetLayeredWindowAttributes(hwnd, COLORREF(0), clamped, LWA_ALPHA).ok();
}

#[cfg(windows)]
fn is_own_window(hwnd: windows::Win32::Foundation::HWND) -> bool {
    // We skip our own windows by checking their HWNDs stored in the global
    OWN_HWNDS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains(&(hwnd.0 as isize))
}

/// PROBLEM 86 — this was a `thread_local!`, which is a per-THREAD list: even
/// when register_own_hwnd was called (it never was — second half of the same
/// bug), the ENGINE thread doing the check would always see an empty Vec,
/// because the registration happened on the MAIN thread. Space+scroll could
/// therefore fade Spaceadom's own dashboard to the opacity floor. A process-
/// wide Mutex is the correct shape; this never runs on the hook thread.
static OWN_HWNDS: std::sync::Mutex<Vec<isize>> = std::sync::Mutex::new(Vec::new());

/// Register a window handle as "owned by Spaceadom" so opacity changes skip it.
/// Idempotent: the cold-boot rebuild path can register the same window twice,
/// and an unbounded push would grow this list on every re-registration.
pub fn register_own_hwnd(hwnd: isize) {
    let mut v = OWN_HWNDS.lock().unwrap_or_else(|p| p.into_inner());
    if !v.contains(&hwnd) {
        v.push(hwnd);
    }
}
