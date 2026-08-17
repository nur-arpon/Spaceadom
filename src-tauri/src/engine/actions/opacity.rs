/// engine/actions/opacity.rs — Window opacity modifier via scroll wheel.
/// Mirrors V11 WheelUp/WheelDown handlers with 25% floor and notification suppression.

const OPACITY_STEP: u8 = 15;

/// PROBLEM 119 — the floor is a SETTING, and it was not wired to anything.
///
/// Settings → "Opacity floor" (10–90%) wrote `opacity_floor_pct` into
/// config.json, `schema.rs` stored it, and NOTHING ever read it: this file
/// clamped to a hardcoded 64. The slider moved, the value saved, the config
/// round-tripped, and the behaviour never changed once. Reported by the owner
/// as "I tried changing it but I don't see any difference" — which was exactly
/// correct, and is the failure `CLAUDE.md` names: a control that does nothing
/// is worse than a missing control.
///
/// Follows the ROLLOVER_MS pattern rather than reaching for the config lock:
/// this runs on a scroll event, so it must not block, and an atomic is read
/// without one. Pushed from three places — startup (lib.rs), save_config and
/// undo_last_change (commands.rs) — because a value pushed from only one of
/// them goes stale the first time the user changes their mind.
pub static OPACITY_FLOOR_PCT: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(25);

/// The arithmetic, split out from the global so it can be tested. Clamped to
/// the same 10–90 range the slider offers, so a hand-edited config cannot make
/// a window invisible (and therefore unfindable) or pin it fully opaque.
///
/// This is a free function taking `pct` rather than reading the atomic itself
/// for a reason found the hard way: when the tests drove `OPACITY_FLOOR_PCT`
/// directly they passed alone and failed in the full suite, because cargo runs
/// tests in one process on parallel threads and they were all writing the same
/// static. A pure function has nothing to share and cannot go flaky.
const fn floor_alpha_for(pct: u8) -> u8 {
    let pct = if pct < 10 { 10 } else if pct > 90 { 90 } else { pct } as u16;
    ((pct * 255) / 100) as u8
}

/// The floor as a Win32 alpha byte, for the value currently configured.
fn floor_alpha() -> u8 {
    floor_alpha_for(OPACITY_FLOOR_PCT.load(std::sync::atomic::Ordering::Relaxed))
}

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

    // Apply delta with clamping. `floor` is read once so a config change
    // mid-gesture cannot make the two comparisons below disagree.
    let floor = floor_alpha();

    let new_alpha = if delta > 0 {
        alpha.saturating_add(delta as u8)
    } else {
        let drop = (-delta) as u8;
        // saturating_add is defensive, not a fix for an observed bug: with the
        // floor clamped to 90% (229) and a 15 step the sum cannot reach 255
        // today. It is here so that raising either bound later cannot silently
        // wrap and step the window DOWN through its own floor.
        if alpha <= floor.saturating_add(drop) {
            floor
        } else {
            alpha - drop
        }
    };

    let clamped = new_alpha.clamp(floor, 255);
    log::debug!("opacity: {alpha} -> {clamped} (floor {floor}, step {delta})");
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

/* ===========================================================================
   TESTS — PROBLEM 119. The percentage-to-alpha conversion and its clamp.
   ===========================================================================
   Small, but this is the arithmetic that decides how transparent a window can
   be made, and "as transparent as you like" means a window the user cannot
   find again. PROBLEM 118's lesson applies: the branch that only matters at
   the extremes is the one nobody exercises by hand.
   =========================================================================== */
#[cfg(test)]
mod floor_tests {
    use super::floor_alpha_for as with;
    use super::{floor_alpha, OPACITY_FLOOR_PCT};
    use std::sync::atomic::Ordering;

    #[test]
    fn converts_percent_to_alpha() {
        assert_eq!(with(25), 63);   // the old hardcoded 64, to within rounding
        assert_eq!(with(50), 127);
        assert_eq!(with(90), 229);
        assert_eq!(with(10), 25);
    }

    /// A hand-edited config must not be able to make a window invisible, and
    /// therefore impossible to find and restore.
    #[test]
    fn clamps_a_config_below_the_slider_minimum() {
        assert_eq!(with(0), with(10));
        assert_eq!(with(3), with(10));
    }

    /// Nor pin it fully opaque, which would make the feature do nothing at all
    /// — the exact complaint that started this.
    #[test]
    fn clamps_a_config_above_the_slider_maximum() {
        assert_eq!(with(100), with(90));
        assert_eq!(with(255), with(90));
    }

    /// The floor must always leave room for the window to be seen AND for the
    /// step to move it. A floor at or near 255 would make scrolling a no-op.
    #[test]
    fn always_leaves_headroom_for_a_step() {
        for pct in 0u8..=255 {
            let f = with(pct);
            assert!(f >= 25,  "floor {f} at {pct}% is too transparent to find");
            assert!(f <= 229, "floor {f} at {pct}% leaves no room to fade");
            assert!(255 - f >= super::OPACITY_STEP, "no room for one step at {pct}%");
        }
    }

    /// The arithmetic being right is not the thing that broke. PROBLEM 119 was
    /// a DISCONNECTION: the slider saved a number nothing read. So this asserts
    /// the wiring itself — that the value pushed into the global is the value
    /// the scroll path actually uses.
    ///
    /// This is deliberately the ONLY test that touches `OPACITY_FLOOR_PCT`.
    /// Adding a second one re-creates the parallel-thread race described above;
    /// test new arithmetic through `floor_alpha_for` instead.
    #[test]
    fn the_configured_value_is_the_one_actually_used() {
        for pct in [10u8, 25, 50, 90] {
            OPACITY_FLOOR_PCT.store(pct, Ordering::Relaxed);
            assert_eq!(
                floor_alpha(),
                with(pct),
                "floor_alpha() ignored the configured {pct}% — the slider is dead again"
            );
        }
        OPACITY_FLOOR_PCT.store(25, Ordering::Relaxed); // leave the default behind
    }
}
