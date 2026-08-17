/// guide_hud/mod_impl.rs — Guide HUD via the dedicated "overlay" window.
///
/// ARCHITECTURE NOTE (2026-08-10) — read before "improving" this:
///
/// v1 (pre-fork): emitted events with nobody listening in a visible window —
///     the HUD rendered inside the hidden dashboard. Hollow feature.
/// v2 (2026-08-10 part 2): fullscreen TRANSPARENT overlay window. WebView2 ran
///     the JS, events arrived, and no pixel ever reached the screen on this
///     machine. Transparent Tauri windows are not trustworthy here — this is
///     the same compositor minefield as the 2026-07-10 "white box" bug.
/// v3 (this file): the v11 AutoHotkey architecture, which is proven on this
///     machine — an OPAQUE dark tool window, sized to content, positioned
///     bottom-centre of the primary monitor, shown NoActivate on demand and
///     hidden on release. The web content only draws the inside of the panel;
///     the WINDOW is the panel.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, Manager};

static HUD_VISIBLE: AtomicBool = AtomicBool::new(false);
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// Set when the overlay window cannot be made click-through at startup.
/// A HUD that eats mouse clicks is worse than no HUD, so we never show it.
pub static OVERLAY_DISABLED: AtomicBool = AtomicBool::new(false);

/// Logical size of the guide HUD panel (v11 was 340×195 for a 7-row list;
/// ours shows the live profile's key grid plus system shortcuts, so it is
/// substantially larger — 26 app keys + 7 system entries must fit unclipped).
/// First-frame size only. The overlay page measures the rendered bloom and
/// calls `overlay_fit_hud` with the real box a few ms later, so this just
/// needs to be close enough that the resize is not a visible jump.
const HUD_W: f64 = 680.0;
const HUD_H: f64 = 600.0;

/// Called once during app setup to wire the Tauri handle.
pub fn set_app_handle(handle: AppHandle) {
    let _ = APP_HANDLE.set(handle);
}

/// Payload sent to the frontend for the guide HUD display.
/// Specials and apps are SEPARATE lists: the user's explicit direction
/// (2026-08-10) is that the HUD's job is teaching the special functions —
/// "app opening is easy to remember, it's Space + the app's initial" — so the
/// page renders specials prominently first, then a compact app grid.
#[derive(serde::Serialize, Clone)]
pub struct GuideHudPayload {
    pub profile: String,
    pub apps: Vec<(String, String)>,
    pub specials: Vec<(String, String)>,
}

/// Size and place the overlay window CENTRED on the primary monitor.
/// (Primary monitor only — the user's explicit decision, 2026-08-10.)
///
/// V13 placed this bottom-centre because the HUD was a bottom-anchored
/// rectangular panel. The V14 HUD is a radial bloom centred on screen, and
/// the frontend re-sizes it via `overlay_fit_hud` milliseconds after show.
/// Without centring HERE too, the window appears bottom-anchored for one
/// frame and then visibly jumps to the middle.
fn place_overlay_centred(win: &tauri::WebviewWindow, w: f64, h: f64) {
    if let Ok(Some(mon)) = win.primary_monitor() {
        let sf = mon.scale_factor();
        let ms = mon.size().to_logical::<f64>(sf);
        let mp = mon.position().to_logical::<f64>(sf);
        let x = mp.x + (ms.width - w) / 2.0;
        let y = mp.y + (ms.height - h) / 2.0;
        let _ = win.set_size(tauri::LogicalSize::new(w, h));
        let _ = win.set_position(tauri::LogicalPosition::new(x, y));
    }
}

/// Show the Guide HUD — content via event, visibility via the window itself.
pub fn show_guide_hud(
    profile_name: &str,
    apps: Vec<(String, String)>,
    specials: Vec<(String, String)>,
) {
    let Some(handle) = APP_HANDLE.get() else { return };

    let payload = GuideHudPayload {
        profile: profile_name.to_string(),
        apps,
        specials,
    };

    // Show the WINDOW first, then send the content. A hidden WebView2 window
    // throttles rendering; content emitted before show() painted nothing and
    // the panel came up as an empty dark box (2026-08-10).
    if !OVERLAY_DISABLED.load(Ordering::Relaxed) {
        if let Some(win) = handle.get_webview_window("overlay") {
            place_overlay_centred(&win, HUD_W, HUD_H);
            // The toasts may have left a pill-shaped window region — the HUD
            // needs the full rectangle back.
            crate::commands::set_overlay_region(&win, &[], 1.0);
            // Re-assert topmost on EVERY show: other always-on-top windows
            // appearing since the last show can end up above us in the
            // topmost band, and the user requires the HUD over everything.
            let _ = win.set_always_on_top(true);
            // PROBLEM 93 — capture what the DESKTOP looks like at the probe
            // points BEFORE the overlay covers them. The self-test used to be
            // purely differential ("did these pixels change in 450ms"), which
            // actually measures "did anything on screen move" — a window
            // repainting BEHIND the invisible overlay read as "composition is
            // alive" and reset the strike counter. With a pre-show baseline
            // the test becomes absolute: if the pixels still equal the desktop
            // behind them, the overlay painted nothing.
            #[cfg(windows)]
            crate::commands::capture_compositing_baseline(&win);
            let _ = win.show();
            log::info!("guide_hud: overlay window shown");
        }
    }
    // GLOBAL broadcast, on purpose. Targeted emits (emit_to) silently never
    // reached this page's listeners regardless of how they were registered —
    // Tauri 2's target matching is stricter than it looks (Labeled vs
    // WebviewWindow are different kinds). Broadcast is safe because the
    // overlay page is the ONLY window that registers these listeners; the
    // dashboard deliberately does not (main.ts step 9).
    if let Err(e) = handle.emit("guide-hud-show", payload) {
        log::warn!("guide_hud: emit failed: {e}");
    }
    HUD_VISIBLE.store(true, Ordering::Relaxed);
}

/// Hide the Guide HUD — hides the window, then tells the page to clean up.
pub fn hide_guide_hud() {
    hide_guide_hud_pending(false);
}

/// Hide the Guide HUD. `action_pending` = the engine cancelled the HUD because
/// a COMBO fired, so a toast is about to arrive in this same window.
///
/// PROBLEM 135 - the `win.hide()` below used to run unconditionally, and it is
/// why the slingshot arrival was invisible through three consecutive builds
/// (1.0.46-48): the engine cancels the HUD BEFORE dispatching the action, so
/// the OS WINDOW was hidden before the toast even existed, and the entire
/// flight played out inside an invisible window. Every in-page measurement
/// said everything was fine - geometry in-bounds, no JS errors - because the
/// page cannot see that its window is gone. The toast's later overlay_fit
/// re-showed the window, which is exactly what the owner reported: ring
/// vanishes instantly, pause, toast pops with no transition.
///
/// With `action_pending` the window STAYS UP and the frontend owns the
/// choreography (hold the ring, fly the pill, then collapse). The window is
/// eventually hidden by overlay_toasts_done when the stack empties, which is
/// the same terminal path every toast already uses. A plain release (no combo)
/// hides immediately, exactly as before.
pub fn hide_guide_hud_pending(action_pending: bool) {
    if HUD_VISIBLE.swap(false, Ordering::Relaxed) {
        if let Some(handle) = APP_HANDLE.get() {
            if action_pending {
                log::info!("guide_hud: hide with action pending - window stays up for the handover");
            } else if let Some(win) = handle.get_webview_window("overlay") {
                // Say so. This hide was silent, and a silent window hide cost
                // three diagnostic rounds (PROBLEM 135) - the same lesson the
                // window rules already record for fits.
                log::info!("guide_hud: overlay window hidden (no action pending)");
                let _ = win.hide();
            }
            let _ = handle.emit("guide-hud-hide", action_pending);
        }
    }
}

/// Returns true if the HUD is currently displayed.
pub fn is_visible() -> bool {
    HUD_VISIBLE.load(Ordering::Relaxed)
}
