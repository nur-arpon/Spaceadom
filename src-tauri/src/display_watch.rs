/// display_watch.rs — rebuild the overlay when the display setup changes.
///
/// PROBLEM 117, measured live 2026-08-16.
///
/// SYMPTOM: after the app had been running 7h10m, holding Space produced sound
/// and launched applications but drew nothing. The Guide HUD and the toasts had
/// both stopped appearing. "Software overlay" was already on, and was verified
/// live: `--disable-gpu` WAS present on the WebView2 process. The overlay
/// reported itself perfectly healthy the whole time:
///
///     overlay_fit_hud: asked 1144x572 -> clamped 1144x572 @ (281,247);
///     monitor 1707x1067 at (0,0) scale 1.5; GOT size Ok((1144.0, 572.0))
///     pos Ok((281.0, 247.0)); visible Ok(true)
///
/// A screen capture of that exact rectangle, taken at the moment the app logged
/// the window as shown, contained ZERO HUD-coloured pixels against 666 in the
/// baseline taken seconds earlier. The window existed, was correctly placed and
/// claimed to be visible, and composed nothing.
///
/// ROOT CAUSE: in that same run the monitor the app saw had changed underneath
/// it — 117 log entries reported `1707x1067 @1.5` and 106 reported
/// `1920x1080 @1`, interleaved across the session. A transparent, layered,
/// always-on-top window whose composition was set up against one display
/// arrangement does not necessarily survive that arrangement changing. Nothing
/// in the app noticed, because every readback Rust can perform still answers
/// "fine".
///
/// Restarting the application restored both surfaces immediately (233 overlay
/// draws in the next 40 minutes). That is also the whole of the "self-healing"
/// reported since 2026-08-13: it never healed, it got restarted.
///
/// THE FIX: watch the display topology and rebuild the overlay window when it
/// changes, which is what a restart was doing by accident.
///
/// WHY POLLING AND NOT WM_DISPLAYCHANGE: the message is delivered to top-level
/// windows, so receiving it means subclassing a window Tauri and WebView2 both
/// own. That is version-fragile, and a mistake there breaks input for the whole
/// app. A directory-free integer comparison twice a second costs nothing
/// measurable and cannot destabilise anything.
///
/// PORTABILITY: this must behave on any x64 Windows machine, not just the
/// developer's. It assumes no particular monitor count, resolution, scale
/// factor or GPU; it reads whatever Tauri reports and only reacts to CHANGE.
/// A machine whose display never changes simply never triggers it.
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::Manager;

/// Set while a rebuild is in flight, so a burst of display events (docking a
/// laptop can emit several within a second) cannot start two at once.
static REBUILDING: AtomicBool = AtomicBool::new(false);

/// How often to compare. Display changes are human-scale events; two seconds is
/// far below any speed a person can perceive, and the check is a handful of
/// integer comparisons.
const POLL: Duration = Duration::from_secs(2);

/// Windows reports intermediate, sometimes nonsensical states while a mode
/// change is in progress. Waiting before rebuilding avoids building the new
/// window against a layout that is about to change again.
const SETTLE: Duration = Duration::from_millis(1200);

/// A comparable fingerprint of every monitor: position, size and scale.
/// Scale is quantised to whole percent because f64 has no useful equality.
fn topology(app: &tauri::AppHandle) -> Vec<(i32, i32, u32, u32, i64)> {
    let Ok(monitors) = app.available_monitors() else {
        return Vec::new();
    };
    let mut v: Vec<_> = monitors
        .iter()
        .map(|m| {
            let p = m.position();
            let s = m.size();
            (p.x, p.y, s.width, s.height, (m.scale_factor() * 100.0).round() as i64)
        })
        .collect();
    // available_monitors() gives no ordering guarantee; sort so that the same
    // physical arrangement always produces the same fingerprint.
    v.sort_unstable();
    v
}

/// Destroy the overlay window and build a replacement with identical properties.
///
/// TWO BUGS LIVED HERE IN 1.0.33, both caught by the owner's own machine within
/// an hour of shipping. Written down because both are easy to write again:
///
/// 1. `close()` is a REQUEST, not an action. It returns immediately and the
///    window goes away some time later, so building the replacement in the same
///    breath failed with `a webview with label 'overlay' already exists` — and
///    the app was left with a stale overlay bound to a monitor that no longer
///    existed. `destroy()` is the immediate form, and the label is polled until
///    it is genuinely free before rebuilding.
///
/// 2. Setting `OVERLAY_DISABLED` when the rebuild failed made the app STRICTLY
///    WORSE than having no fix at all: the old window was usually still alive
///    and usable, and the flag switched the HUD and every toast off until the
///    next restart. A repair that fires on a routine event — this owner plugs a
///    second display in and out through the day — must never be able to do more
///    damage than the fault it repairs. The flag is now set ONLY when the
///    window is genuinely gone AND could not be replaced, which is a state
///    where nothing could have been shown anyway. A later successful rebuild
///    clears it, because `configure_overlay_window` sets it false.
///
/// The waiting is done OFF the main thread. Blocking the main thread would
/// freeze the dashboard and the tray for the duration.
pub fn rebuild_overlay(app: &tauri::AppHandle) {
    if REBUILDING.swap(true, Ordering::SeqCst) {
        return;
    }
    // PROBLEM 131 — the leading hypothesis for the 14 crashes is a window
    // message arriving after its host window was destroyed. This is the ONE
    // place the app deliberately destroys a live window, so recording it makes
    // the hypothesis testable from a crash log alone: if a crash report shows a
    // rebuild moments earlier, that is the answer; if the counter is 0 in every
    // report, the hypothesis is dead and should be written off in the notes.
    crate::crash_context::note_overlay_rebuild();
    crate::crash_context::note_display_event("overlay rebuild started (display topology changed)");
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("st-overlay-rebuild".into())
        .spawn(move || {
            let done = || REBUILDING.store(false, Ordering::SeqCst);

            // ---- 1. tear the old one down, on the main thread ----
            let a = app.clone();
            if let Err(e) = app.run_on_main_thread(move || {
                crate::guide_hud::hide_guide_hud();
                if let Some(old) = a.get_webview_window("overlay") {
                    let _ = old.hide();
                    if let Err(e) = old.destroy() {
                        log::error!("display: could not destroy the old overlay ({e})");
                    }
                }
            }) {
                log::error!("display: could not reach the main thread to destroy the overlay ({e})");
                done();
                return;
            }

            // ---- 2. wait for the label to actually free up ----
            let mut gone = false;
            for _ in 0..40 {
                std::thread::sleep(Duration::from_millis(100));
                if app.get_webview_window("overlay").is_none() {
                    gone = true;
                    break;
                }
            }
            if !gone {
                // The old window outlived its own destroy request. It is still
                // there, so it is still usable — leave it alone and say so.
                // Do NOT disable the overlay: that was bug 2.
                log::error!(
                    "display: the old overlay did not go away within 4s — keeping it rather \
                     than switching the HUD off. It may be bound to the previous display."
                );
                done();
                return;
            }

            // ---- 3. build the replacement, on the main thread ----
            // Mirrors tauri.conf.json field-for-field, exactly as the PROBLEM 81
            // rebuild path does. A replacement missing any of these is an
            // opaque, decorated, focus-stealing rectangle.
            let a2 = app.clone();
            if let Err(e) = app.run_on_main_thread(move || {
                let built = tauri::WebviewWindowBuilder::new(
                    &a2,
                    "overlay",
                    tauri::WebviewUrl::App("overlay.html".into()),
                )
                .visible(false)
                .title("Spaceadom Overlay")
                .transparent(true)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .focused(false)
                .shadow(false)
                .inner_size(600.0, 460.0)
                .build();

                match built {
                    Ok(w) => {
                        crate::configure_overlay_window(&w);
                        log::info!("display: overlay rebuilt for the new display configuration");
                    }
                    Err(e) => {
                        // Genuinely gone and not replaceable. The flag is honest
                        // here: nothing could be shown either way. The next
                        // display change retries, and success clears it.
                        log::error!(
                            "display: overlay REBUILD FAILED ({e}) — the HUD and toasts cannot \
                             appear until the next display change or a restart"
                        );
                        crate::guide_hud::OVERLAY_DISABLED.store(true, Ordering::Relaxed);
                    }
                }
            }) {
                log::error!("display: could not reach the main thread to build the overlay ({e})");
            }
            done();
        })
        .is_ok();

    if !spawned {
        log::error!("display: could not spawn the rebuild thread");
        REBUILDING.store(false, Ordering::SeqCst);
    }
}

/// Start the watcher. Safe to call once, from setup.
pub fn start(app: tauri::AppHandle) {
    if std::thread::Builder::new()
        .name("st-display-watch".into())
        .spawn(move || {
            let mut last = topology(&app);
            log::info!("display: watching {} monitor(s) for configuration changes", last.len());
            loop {
                std::thread::sleep(POLL);

                let now = topology(&app);
                // An empty read means Windows is mid-transition (or the call
                // failed). Never treat that as "all monitors disappeared".
                if now.is_empty() {
                    continue;
                }
                if now == last {
                    continue;
                }

                log::warn!(
                    "display: configuration CHANGED — was {last:?}, now {now:?}. \
                     Rebuilding the overlay; without this the HUD and toasts stop \
                     painting while still reporting themselves visible (PROBLEM 117)."
                );
                last = now;

                std::thread::sleep(SETTLE);
                // Re-read after settling: a dock or undock emits several
                // changes, and the last one is the one worth building against.
                let settled = topology(&app);
                if !settled.is_empty() {
                    last = settled;
                }

                // The dashboard has the same problem in a different shape: if
                // it is open on a display that has just been unplugged, it is
                // now sitting at coordinates no monitor covers, and the only
                // way back is to close and reopen it from the tray.
                // `ensure_on_screen` (PROBLEM 83) already knows how to fix
                // that; it simply was never called while a window was open,
                // because until now nothing told the app the displays moved.
                let a = app.clone();
                let _ = app.run_on_main_thread(move || {
                    if let Some(win) = a.get_webview_window("settings") {
                        if win.is_visible().unwrap_or(false) {
                            crate::ensure_on_screen(&win);
                        }
                    }
                });

                rebuild_overlay(&app);
            }
        })
        .is_err()
    {
        log::error!(
            "display: could not spawn the watcher thread — the overlay will still stop \
             painting after a display change until the app is restarted"
        );
    }
}
