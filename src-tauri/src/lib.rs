#![allow(dead_code, unused_must_use, unused_imports)]
#![allow(clippy::all)]
/// lib.rs — SpaceToggle OS application entry point.
///
/// Startup sequence:
///   1. Check & request UAC elevation if needed
///   2. Initialise rolling file logger
///   3. Register Windows startup (HKCU Run key)
///   4. Load or seed config.json
///   5. Spawn Win32 hook thread (isolated OS thread)
///   6. Start fullscreen watcher thread
///   7. Start async engine actor (tokio)
///   8. Build system tray
///   9. Wire close-to-tray for settings window
///  10. Show tray; settings window starts hidden (visible: false in tauri.conf.json)

/// PROBLEM 253 — "is this copy a portable, self-contained build (a
/// `portable.txt` marker beside the exe), and if so where does ALL of its
/// data live?" The one resolver every data-path site (`startup::data_dir`,
/// `config::backup_dir`, and everything built on top of them) goes through.
mod portable;
mod browser;
/// Chromium browser + profile detection, and every branch that decides whether
/// a binding launches into a specific profile or the untouched default browser.
mod browser_profiles;
mod commands;
/// PROBLEM 131 — breadcrumbs read by the panic hook.
mod crash_context;
/// PROBLEM 253 — the "Report a problem" bundle: one zip of the log tail, a
/// scrubbed config and a system summary, revealed in Explorer and NEVER
/// uploaded.
mod diagnostics;
mod config;
mod display_watch;
mod rival_install;
mod engine;
mod features;
mod guide_hud;
mod hook;
// Touchpad T1 — Precision Touchpad raw-input reader. `pub` because the
// `touchpad-probe` example is its only caller; nothing in the app runs it.
pub mod touchpad;
mod icon_extractor;
mod logger;
/// PROBLEM 267 — the middle button's cursor-anchored ICON RING: pure geometry
/// (layout, edge clamp, band/sector hit test), the favourites rule and the
/// payload builder. The impure halves live in `guide_hud::show_middle_ring`
/// and `engine::dispatch`'s `MiddleButtonDown` arm.
mod middle_ring;
/// PROBLEM 267 — a link binding's favicon, fetched ONCE at bind time by the
/// key editor and stored in the binding; never fetched at ring time.
mod site_icon;
/// PROBLEM 250 — "is this copy running from an MSIX package (the Microsoft
/// Store build)?", and the four things that are silently wrong when it is:
/// the in-app updater, autostart, the config's real location, and the
/// rival-install banner's one-click repair.
mod packaged;
/// PROBLEM 265 — WHEN the overlay window may be created, and the one pure
/// function that decides it. The dashboard keeps the full autostart settle
/// (PROBLEM 59/76/215); the Guide HUD's window no longer does, because it is
/// the only window in the app with a self-healing rebuild path behind it.
mod overlay_boot;
mod picker_worker;
/// PROBLEM 224 — takes `WM_ENDSESSION` before tao can set its runner to
/// `Destroyed`, which is the whole of the "cannot move state from Destroyed"
/// crash. Installed from `create_app_windows`, from `setup()` and from
/// `display_watch::rebuild_once`; all three are idempotent.
/// PROBLEM 253 — three failed startups in a row and the NEXT launch comes up
/// with no keyboard hook, no overlay and a banner. The counter lives in
/// `boot-attempts.json`; a clean `WM_ENDSESSION` exit is explicitly excluded
/// from it (PROBLEM 224 would otherwise make every third reboot look like a
/// crash).
mod safe_mode;
mod session_end;
mod startup;
/// PROBLEM 195 — crash/error reporting to Sentry, and its kill switch.
mod telemetry;
mod tray;
// PROBLEM 245 — the in-app updater.
mod updater;
// PROBLEM 249 — "What's new": the GitHub release notes for this version.
mod release_notes;
/// REVIEW FIXES 2026-09-05 (H4) — the ONE reading of Windows' app light/dark
/// setting, and the `os-theme-changed` event both windows resolve "auto"
/// against. `prefers-color-scheme` inside a webview reports what that webview
/// was configured to prefer, which is why the dashboard and the overlay used
/// to disagree.
mod theme_watch;

use commands::{ConfigState, IconCacheState};
use crossbeam_channel::bounded;
use engine::EngineState;
use std::sync::{Arc, Mutex};
use tauri::Emitter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Monitor WORK AREA (screen minus taskbar) in LOGICAL units, as
/// (width, height, x, y). Tauri exposes only the full monitor rect, so this
/// goes to Win32 for the work area — the difference is the taskbar, and
/// centring in the full rect is what pushed the dashboard's bottom controls
/// behind it on a 720p laptop (PROBLEM 46).
#[cfg(windows)]
fn work_area_logical(
    win: &tauri::WebviewWindow,
    scale: f64,
) -> Option<(f64, f64, f64, f64)> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    let hwnd = win.hwnd().ok()?;
    let raw = HWND(hwnd.0 as *mut _);
    unsafe {
        let mon = MonitorFromWindow(raw, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(mon, &mut mi).as_bool() {
            return None;
        }
        let r = mi.rcWork; // physical pixels
        Some((
            (r.right - r.left) as f64 / scale,
            (r.bottom - r.top) as f64 / scale,
            r.left as f64 / scale,
            r.top as f64 / scale,
        ))
    }
}

#[cfg(not(windows))]
fn work_area_logical(_w: &tauri::WebviewWindow, _s: f64) -> Option<(f64, f64, f64, f64)> { None }

/// True when this process was started by the logon autostart entry rather than
/// by a person (PROBLEM 64 writes `--autostart` into the HKCU Run value).
/// Read from argv each time — cheap, and avoids another global.
fn autostart_launch() -> bool {
    std::env::args().any(|a| a == "--autostart")
}

/// PROBLEM 91 — the dashboard's work-area fit, callable. Step 9c used to do
/// this inline, so the PROBLEM 59 cold-boot REBUILD produced a window that
/// never got it: a raw 1220x880 at a hard 720x520 floor, off-screen on a
/// small laptop. Moved here VERBATIM — including the read-back logging,
/// which CLAUDE.md's window rules require and which must never be 'tidied'.
pub fn fit_dashboard_to_work_area(win: &tauri::WebviewWindow) {
    let mon = win.current_monitor().ok().flatten()
        .or_else(|| win.primary_monitor().ok().flatten());
    if let Some(mon) = mon {
        let sf = mon.scale_factor();
        let ms = mon.size().to_logical::<f64>(sf);
        let mp = mon.position().to_logical::<f64>(sf);

        // WORK AREA, not mon.size() (PROBLEM 46). mon.size() is
        // the whole panel INCLUDING the taskbar, so centring in
        // it pushes the bottom of the window behind the taskbar
        // — on a tester's 1280x720@150% laptop that hid the
        // gear and the Special-keys pill entirely.
        let (wa_w, wa_h, wa_x, wa_y) = work_area_logical(&win, sf)
            .unwrap_or((ms.width, ms.height, mp.x, mp.y));

        // set_size takes INNER (client) size but the window also
        // costs decorations, so subtract them before clamping or
        // the OUTER window exceeds the work area. Measured on
        // Win11: ~14 logical px wide, ~38 tall (title bar +
        // resize borders), scale-independent in logical units.
        const DECOR_W: f64 = 16.0;
        const DECOR_H: f64 = 40.0;
        let max_w = (wa_w - DECOR_W).max(320.0);
        let max_h = (wa_h - DECOR_H).max(320.0);

        // PROBLEM 84 — tiny screens. tauri.conf.json declares
        // minWidth 720 / minHeight 520. On a 1024x600 netbook
        // at 125% the LOGICAL work area is ~819x448 — smaller
        // than the minimum, so the OS pins the window larger
        // than the screen and the bottom controls (gear,
        // Special keys) are unreachable. When the work area
        // cannot honour the declared minimum, relax it; the
        // frontend already scales the board to any size.
        if max_w < 720.0 || max_h < 520.0 {
            let _ = win.set_min_size(Some(tauri::LogicalSize::new(
                320.0, 240.0,
            )));
            log::warn!(
                "setup: work area {max_w:.0}x{max_h:.0} is below the declared \
                 720x520 minimum — min size relaxed so the window fits the screen"
            );
        }

        // PROBLEM 123 — PROPORTIONAL, not a fixed ceiling.
        //
        // This was `1220.0.min(max_w)`, and before that `1220.0.min(ms.width *
        // 0.92)`. Both are the same shape: 1220x880 is a CEILING the window can
        // never exceed, so on a large monitor the dashboard sat at a fixed size
        // in the middle of a mostly empty screen. Reported by the owner: the
        // keyboard looks small on his bigger display and the space is wasted.
        //
        // 92% of the WORK AREA (PROBLEM 46: work area, never the full monitor,
        // or the bottom controls hide behind the taskbar), floored at the old
        // 1220x880 so nothing shrinks on the screens that already fit, and
        // still bounded by max_w/max_h so a small screen behaves exactly as
        // before. The frontend scales the board to whatever it is given, so
        // growing the window is what makes the keyboard grow.
        //
        // DO NOT "simplify" this back to a `min` against a constant. That
        // constant is the bug.
        let w = (wa_w * 0.92).clamp(1220.0_f64.min(max_w), max_w);
        let h = (wa_h * 0.92).clamp(880.0_f64.min(max_h), max_h);
        let _ = win.set_size(tauri::LogicalSize::new(w, h));
        let _ = win.set_position(tauri::LogicalPosition::new(
            wa_x + (wa_w - (w + DECOR_W)) / 2.0,
            wa_y + (wa_h - (h + DECOR_H)) / 2.0,
        ));

        // READ BACK what actually happened. A set_size/
        // set_position call that silently does not stick looks
        // identical in the log to one that worked, and this
        // window has already shipped once at the wrong size.
        // Never trust the request; log the result.
        let got_sz = win.outer_size().map(|s| s.to_logical::<f64>(sf));
        let got_ps = win.outer_position().map(|p| p.to_logical::<f64>(sf));
        log::info!(
            "setup: dashboard asked for {w:.0}x{h:.0} @ ({:.0},{:.0}) on a \
             {:.0}x{:.0} monitor (scale {sf}); got size {:?} pos {:?}",
            mp.x + (ms.width - w) / 2.0,
            mp.y + (ms.height - h) / 2.0,
            ms.width, ms.height,
            got_sz.map(|s| (s.width.round(), s.height.round())),
            got_ps.map(|p| (p.x.round(), p.y.round())),
        );
    }
}

/// PROBLEM 91 — the 10s wedged-frontend show fallback, callable. Same reason
/// as fit_dashboard_to_work_area: a rebuilt dashboard used to get none.
pub fn spawn_show_fallback(app_handle: &tauri::AppHandle) {
    let app_handle = app_handle.clone();
    if autostart_launch() {
        log::info!(
            "setup: autostart launch — staying in the tray, dashboard not shown"
        );
    } else {
        let app2 = app_handle.clone();
        std::thread::Builder::new()
            .name("st-show-fallback".into())
            .spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(10));
                if commands::DASHBOARD_READY.load(std::sync::atomic::Ordering::Relaxed) {
                    return; // the ready beacon already showed it
                }
                // PROBLEM 205 — this used to say "showing the window anyway"
                // and it said it HERE, before `run_on_main_thread`. That is a
                // statement of INTENT dressed as a statement of FACT, and the
                // one case it gets wrong is the exact case this fallback
                // exists for: when the main thread is blocked, the closure
                // below does not run, the window is never shown, and the log
                // still claims it was. Two hours of a startup investigation
                // were spent trusting that line. Record the ASK here and the
                // EVENT inside the closure — never one line for both.
                log::warn!(
                    "setup: dashboard_ready never arrived after 10s — ASKING the \
                     main thread to show the window (frontend wedged or webview \
                     dead; PROBLEM 74). If no 'show-fallback: window shown' line \
                     follows, the main thread is blocked and it never happened."
                );
                let app3 = app2.clone();
                let _ = app2.run_on_main_thread(move || {
                    use tauri::Manager;
                    if let Some(w) = app3.get_webview_window("settings") {
                        ensure_on_screen(&w); // PROBLEM 83
                        let _ = w.show();
                        let _ = w.set_focus();
                        log::warn!(
                            "show-fallback: window shown by the 10s fallback \
                             (main thread reached it)"
                        );
                    } else {
                        log::warn!(
                            "show-fallback: reached the main thread but there is \
                             no 'settings' window to show"
                        );
                    }
                });
            })
            .ok();
    }
}

/// PROBLEM 215 — false until `create_app_windows` has run. Read by the Guide
/// HUD so a Space hold during the autostart settle window logs "still starting"
/// instead of "the overlay is broken": during that window there is deliberately
/// no overlay to find, and calling that an error would train the owner to
/// ignore the line that means something.
static WINDOWS_CREATED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// PROBLEM 59/76/215 — how long an `--autostart` launch waits before creating
/// its windows. TEN seconds, unchanged from PROBLEM 76: at a real logon the Run
/// key already fires 1-2 minutes after power-on, and the old 30s stacked on top
/// of that is what made the owner conclude the app "didn't start". What
/// PROBLEM 215 changed is WHAT waits — the hook and the engine no longer do.
const AUTOSTART_SETTLE: std::time::Duration = std::time::Duration::from_secs(10);

/// PROBLEM 215 — has the UI been built yet? See `create_app_windows`.
pub fn windows_created() -> bool {
    WINDOWS_CREATED.load(std::sync::atomic::Ordering::SeqCst)
}


/// PROBLEM 215 — WINDOW CREATION, SPLIT OFF FROM EVERYTHING ELSE.
///
/// The owner: *"When restarting my laptop this app needs a long time to show
/// up. The main features — app opening, the Space HUD and toast — should come
/// up as soon as possible."* After a reboot his shortcuts were dead for over
/// ten seconds, and the reason was one line in `run()`:
///
///     if --autostart { sleep(10s) }   // PROBLEM 59/76
///
/// That sleep is REAL and it stays. PROBLEM 59 is measured: at a cold logon the
/// WebView2 runtime is often not serviceable yet, `CreateCoreWebView2Controller`
/// fails with HRESULT(0x80070490) ERROR_NOT_FOUND, Tauri destroys the host
/// window, and the user gets an app with no dashboard and no Guide HUD while
/// the log claims success. Waiting is what stops that.
///
/// But the wait was in the WRONG PLACE. It sat before `tauri::Builder`, so it
/// delayed the keyboard hook and the engine as well — and NEITHER OF THOSE
/// NEEDS WEBVIEW2. Nothing in `WH_KEYBOARD_LL` cares whether Edge has finished
/// starting.
///
/// So the sleep did not move; the work moved out from behind it. Both windows
/// are now declared `"create": false` in tauri.conf.json, which tells Tauri not
/// to build them during its own setup, and this function builds them from that
/// same declaration (`WebviewWindowBuilder::from_config` — the documented way,
/// and it cannot drift from the config the way a hand-copied builder can:
/// PROBLEM 81). On a manual launch it is called inline from `setup()`, at the
/// same instant Tauri would have created them. On `--autostart` it is called
/// from a settle thread after the same 10s, ON THE MAIN THREAD.
///
/// What the user gets at logon: hook armed and tray icon present within the
/// first second; **the overlay at `overlay_boot::OVERLAY_SETTLE` (PROBLEM
/// 265)**; the dashboard ten seconds later.
///
/// WHAT HAPPENS IF SPACE IS HELD DURING THE SETTLE WINDOW — decided, not
/// accidental: the shortcut WORKS (launch, focus, minimise, boss key, PiP are
/// all Rust) and NOTHING is drawn. There is no half-HUD to look broken, because
/// the overlay window does not exist yet, so every show path takes its
/// `if let Some(win)` miss and returns without emitting. `guide_hud` logs one
/// calm line saying the app is still settling rather than shouting an error.
///
/// PROBLEM 265 SHRANK THAT WINDOW FROM TEN SECONDS TO ABOUT ONE, AND WITHDREW
/// "silent but functional" AS AN ACCEPTED TRADE. The owner held Space four
/// times in the first sixteen seconds of a logon, saw no ring, and concluded
/// his shortcuts were dead — a log line nobody reads is not the app being
/// honest with its user. The overlay is now built by `overlay_boot`, ahead of
/// this function, and a hold that still finds no overlay asks for one to be
/// built immediately instead of waiting out the timer.
///
/// PROBLEM 74 IS UNTOUCHED. This function never shows a window. The dashboard
/// still appears only when the frontend calls `dashboard_ready`, or from the
/// 10s `spawn_show_fallback`. "Boot, then show" still holds.
///
/// Idempotent on purpose (PROBLEM 214's lesson, applied ahead of time): the
/// settle thread, the tray's "Open Settings" and the single-instance handler
/// can all reach it, and whoever arrives first does the work.
pub fn create_app_windows(app_handle: &tauri::AppHandle) {
    use tauri::Manager;
    if WINDOWS_CREATED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app_handle = app_handle.clone();

    // Build what tauri.conf.json declares, from the declaration itself.
    for wc in app_handle.config().app.windows.clone() {
        if app_handle.get_webview_window(&wc.label).is_some() {
            continue;
        }
        // PROBLEM 253 — safe mode creates the dashboard and NOTHING else.
        //
        // The overlay is a transparent, always-on-top, click-through WebView2
        // window, and CLAUDE.md's window rules are a list of the ways it has
        // failed on this project's own machines — a fullscreen transparent
        // sheet that composed zero pixels, a blur that made the whole window
        // vanish, a driver change that killed compositing while every readback
        // said "visible". It is a plausible cause of a launch that has now died
        // three times, and it is the one window the user does not need in order
        // to reach the button that fixes things.
        if wc.label == "overlay" && safe_mode::active() {
            log::warn!(
                "setup: SAFE MODE — the 'overlay' window was deliberately NOT created. The \
                 Guide HUD and toasts are absent for this launch; they come back at the next \
                 restart after 'Turn back on'. This is not the PROBLEM 59 cold-boot failure — \
                 nothing tried and failed."
            );
            continue;
        }
        let label = wc.label.clone();
        match tauri::WebviewWindowBuilder::from_config(&app_handle, &wc).and_then(|b| b.build()) {
            Ok(_) => {
                log::info!("setup: window '{label}' created from its tauri.conf.json declaration")
            }
            Err(e) => log::error!(
                "setup: window '{label}' could not be created ({e}) — the PROBLEM 59 recovery \
                 below will try again with an explicit builder"
            ),
        }
    }

    // 9b. Configure the always-on-top HUD/toast overlay.
    //
    // HISTORY (do not repeat):
    // • overlay.html existed but NO window ever loaded it — HUD/toasts
    //   rendered into the hidden dashboard. Fixed by declaring the
    //   window (2026-08-10, part 2).
    // • First attempt used a FULLSCREEN TRANSPARENT window. WebView2
    //   accepted it, JS ran, events arrived — and nothing EVER composed
    //   to the screen on this machine. Verified with an in-page probe
    //   ("overlay: webview JS alive") plus screenshots: JS alive,
    //   pixels absent. Same minefield as the 2026-07-10 "white box"
    //   saga, failing invisible instead of white.
    // • Current design copies what install-v11 (AHK) proved on this
    //   exact machine: an OPAQUE dark window, sized to its content,
    //   shown on demand and hidden after — never a fullscreen
    //   transparent sheet. Sizing/positioning happens in guide_hud and
    //   show_toast at display time.
    // The overlay is TRANSPARENT (re-tested 2026-08-10: the old
    // "composes zero pixels" finding was specific to a FULLSCREEN
    // transparent window; this small on-demand one renders fine).
    // All of its runtime configuration lives in
    // configure_overlay_window() — shared with the PROBLEM 59
    // rebuild path, which used to produce a half-configured window
    // (PROBLEM 81).
    {
        use tauri::Manager;
        if let Some(overlay) = app_handle.get_webview_window("overlay") {
            configure_overlay_window(&overlay);
        } else if safe_mode::active() {
            // PROBLEM 253 — expected, not a fault. An ERROR here would be the
            // log telling a reader the overlay broke when it was never built.
            log::info!("setup: no overlay to configure — safe mode did not create one");
        } else {
            log::error!("setup: 'overlay' window missing — HUD and toasts will not be visible");
        }
    }


    // 9c. Fit the dashboard to the monitor's WORK AREA and centre it.
    //
    // The V14 board is fixed-geometry (1046 x 320 design px) and the
    // frontend scales it down to whatever space it is given. That only
    // works if the WINDOW itself fits the display: the previous attempt
    // opened wider than the monitor and the keyboard ran off the edge,
    // which is the failure the user actually saw. Clamp to 92% of the
    // monitor rather than maximising — maximising a 2560x1440 display
    // leaves the keyboard adrift in empty cream.
    {
        use tauri::Manager;
        if let Some(win) = app_handle.get_webview_window("settings") {
            // current_monitor, NOT primary_monitor: this machine has a
            // 1920x1080 primary and a 2560x1600 @150% secondary, and
            // Windows may open the window on either. Fitting it to the
            // monitor it is ACTUALLY on is the only version that is
            // right in both cases. (The Guide HUD stays
            // primary-monitor-only — that is a separate, explicit user
            // decision; do not "unify" the two.)
            fit_dashboard_to_work_area(&win);

            // PROBLEM 70 — show the dashboard only when a HUMAN started
            // the app. At logon (`--autostart`) Spaceadom must come up
            // silently: hook armed, tray icon present, NO window in the
            // user's face. They open the dashboard when they want it —
            // tray click, tray "Open Settings", or launching the app
            // again (single-instance fronts the existing window).
            //
            // PROBLEM 74 — and even on a manual launch, DO NOT show it
            // here. The window used to appear while WebView2 was still
            // doing its first-run initialisation, and that gap — a
            // visible window whose webview cannot pump messages yet —
            // IS the "(Not Responding)" both testers reported. The
            // frontend now calls `dashboard_ready` as the LAST step of
            // its bootstrap, and the window is shown then: the user's
            // first sight of the dashboard is one that can already
            // paint and respond. A 10s fallback below covers a wedged
            // frontend (better a sluggish window than none).
            spawn_show_fallback(&app_handle);
        }
    }

    // PROBLEM 86 — register our own windows with the opacity action,
    // so Space+scroll can never fade Spaceadom's own dashboard or
    // overlay. (The registry was a dead thread_local before; see
    // opacity.rs.)
    #[cfg(windows)]
    {
        use tauri::Manager;
        for label in ["settings", "overlay"] {
            if let Some(w) = app_handle.get_webview_window(label) {
                if let Ok(h) = w.hwnd() {
                    engine::actions::opacity::register_own_hwnd(h.0 as isize);
                }
            }
        }
    }

    // 11. Close-to-tray for settings window
    tray::setup_close_to_tray(&app_handle);

    // PROBLEM 59 — never claim success when the webviews are missing.
    //
    // The windows are declared in tauri.conf.json, so Tauri builds them
    // before setup() runs. On a COLD BOOT the WebView2 runtime is often
    // not serviceable yet and CreateCoreWebView2Controller fails with
    // HRESULT(0x80070490) ERROR_NOT_FOUND; Tauri then destroys the host
    // window. The old code logged "fully initialised" regardless, so a
    // tester saw an app with no dashboard and no Guide HUD while the log
    // looked perfectly healthy. Detect it, say so, and rebuild once.
    {
        use tauri::Manager;
        for (label, url) in [("settings", "index.html"), ("overlay", "overlay.html")] {
            if app_handle.get_webview_window(label).is_some() {
                continue;
            }
            // PROBLEM 253 — the recovery must not undo the safe-mode decision.
            //
            // This branch exists to rebuild a window WebView2 failed to attach
            // to. In safe mode the overlay is missing because we chose not to
            // build it, and a recovery that cannot tell those two apart would
            // faithfully rebuild the exact window safe mode was avoiding —
            // silently, and with an ERROR line claiming a cold-boot failure
            // that never happened.
            if label == "overlay" && safe_mode::active() {
                continue;
            }
            log::error!(
                "setup: webview '{label}' DOES NOT EXIST — WebView2 failed to attach \
                 (cold-boot race, or no WebView2 runtime installed). Rebuilding it."
            );
            // PROBLEM 81 — the rebuild must recreate the window with
            // the SAME properties tauri.conf.json declares, or the
            // replacement is an opaque, decorated, focus-stealing
            // rectangle. The builder mirrors the conf declaration
            // field-for-field; the runtime half (click-through, DWM
            // border, no-activate) is reapplied below via the same
            // function the normal setup path uses.
            let mut builder = tauri::WebviewWindowBuilder::new(
                &app_handle,
                label,
                tauri::WebviewUrl::App(url.into()),
            )
            .visible(false);
            if label == "overlay" {
                builder = builder
                    .title("Spaceadom Overlay")
                    .transparent(true)
                    .decorations(false)
                    .always_on_top(true)
                    .skip_taskbar(true)
                    .resizable(false)
                    .focused(false)
                    .shadow(false)
                    .inner_size(600.0, 460.0);
            } else {
                builder = builder
                    .title("Spaceadom")
                    .theme(Some(tauri::Theme::Light)) // mirrors conf "theme": "Light"
                    .inner_size(1220.0, 880.0)
                    .min_inner_size(720.0, 520.0)
                    .center()
                    // PROBLEM 235 — Tauri v2 enables its OWN native drag-drop
                    // handler per window by default, which swallows HTML5
                    // dragstart/dragover/drop before the page ever sees them
                    // (the profile popover's reorder handles worked in the Vite
                    // preview and did nothing in the real WebView). This
                    // fallback rebuild path must mirror `dragDropEnabled: false`
                    // on the "settings" window in tauri.conf.json, which is
                    // where the NORMAL boot path (`WebviewWindowBuilder::
                    // from_config`, above) gets it from — this hand-built
                    // branch only runs after a cold-boot WebView2 failure and
                    // would silently re-enable native drag-drop if it forgot.
                    .drag_and_drop(false);
            }
            match builder.build() {
                Ok(w) => {
                    // No direct show here (PROBLEM 74): the rebuilt
                    // webview boots index.html, whose bootstrap ends in
                    // `dashboard_ready` — the window appears then,
                    // already responsive.
                    log::info!("setup: webview '{label}' rebuilt successfully");
                    if label == "overlay" {
                        configure_overlay_window(&w);
                    } else {
                        // PROBLEM 90 — a rebuilt settings window has NO
                        // CloseRequested handler, so the X button would
                        // exit the app (killing the hook) instead of
                        // hiding to tray. Re-attach it here; step 11's
                        // one-shot call bound to the window this
                        // replaced.
                        tray::attach_close_to_tray(&w);
                        // PROBLEM 91 — step 9c's work-area fit and its
                        // 10s show-fallback both ran BEFORE this window
                        // existed, so without these the rebuilt
                        // dashboard keeps a raw 1220x880 at a hard
                        // 720x520 floor (off-screen on a small laptop)
                        // and, if its frontend also wedges, is never
                        // shown at all.
                        fit_dashboard_to_work_area(&w);
                        spawn_show_fallback(&app_handle);
                    }
                    // PROBLEM 86 — the own-window registration loop runs
                    // before this rebuild, so a rebuilt window would be
                    // fadeable by Space+scroll.
                    #[cfg(windows)]
                    if let Ok(h) = w.hwnd() {
                        engine::actions::opacity::register_own_hwnd(h.0 as isize);
                    }
                }
                Err(e) => log::error!(
                    "setup: webview '{label}' rebuild FAILED: {e}. The app is running \
                     without its UI — install the WebView2 Runtime, or restart the app."
                ),
            }
        }
    }

    // PROBLEM 117 — the overlay stops compositing when the display
    // arrangement changes underneath it, while every readback still
    // says visible=true. Watch for the change and rebuild. Started
    // AFTER set_app_handle so a rebuild can hide the HUD first.
    // PROBLEM 253 — and not in safe mode. `display_watch` polls for an overlay
    // that is missing and rebuilds it (`rebuild_once`), which is exactly right
    // when a display change killed it and exactly wrong here: it would put the
    // overlay back a few seconds after safe mode decided not to have one, and
    // then report `OverlayRebuildFailed` if it could not. The watcher is a
    // recovery for a window that is supposed to exist.
    if safe_mode::active() {
        log::warn!(
            "setup: SAFE MODE — the display watcher was NOT started, because its job is to \
             rebuild a missing overlay and this launch has no overlay by design."
        );
    } else {
        display_watch::start(app_handle.clone());
    }

    // REVIEW FIXES 2026-09-05 (H4) — started in safe mode TOO, unlike the
    // display watcher above. That one is skipped because its job is to rebuild
    // an overlay this launch deliberately does not have; this one only reads a
    // registry value and emits an event, and a safe-mode launch still shows
    // the dashboard — in the wrong palette, if nothing tells it what Windows
    // wants. See theme_watch.rs for why this is a poll and not
    // WM_SETTINGCHANGE.
    theme_watch::start(app_handle.clone());

    // PROBLEM 224 — re-arm the WM_ENDSESSION guard now that `settings` and
    // `overlay` exist. `setup()` already armed it on tao's event target (the
    // window that actually sets `Destroyed`); this pass adds the two Tauri
    // windows, and re-arming tao's costs nothing because SetWindowSubclass
    // REPLACES an entry with the same (procedure, id) pair.
    //
    // This runs on the MAIN THREAD, and it has to: `EnumThreadWindows` is
    // thread-scoped, so the same call from a worker would find nothing and
    // install nothing. Every caller of `create_app_windows` is on the main
    // thread — `setup()` inline, the settle thread via `run_on_main_thread`,
    // and the single-instance handler.
    session_end::install();

    // 1.0.96 — THE ONE-SHOT OVERLAY RE-TEST. One line on purpose; the whole
    // decision (is this machine in software rendering? has it been re-checked?
    // draw, watch, judge, write, log) lives in `commands::retest_software_
    // overlay_once`, beside the self-test machinery it reuses.
    //
    // HERE and not earlier because this is the first point at which the overlay
    // window is known to exist. It returns immediately on every machine that is
    // not in software rendering, and it defers its own work by 8s on a
    // background thread, so nothing about the startup path changes.
    #[cfg(windows)]
    commands::retest_software_overlay_once(&app_handle);

    // PROBLEM 237 — pre-warm the app picker. One channel send on this thread;
    // the scan itself runs seconds later on `st-picker-scan`, a below-normal
    // STA worker, and only if `warm_picker_at_startup` says so. HERE because
    // this is the point that follows both launch paths' settle (inline on a
    // manual launch, after AUTOSTART_SETTLE on an autostart one), so the hook,
    // the engine and the tray are all already live before it is even queued.
    picker_worker::warm_at_boot(&app_handle);

    log::info!("setup: windows created and configured");
}


/// PROBLEM 89 — set once the app has a tray icon and windows, i.e. once there
/// is SOME way for the user to see that Spaceadom is alive. Before this point
/// a panic is an invisible death and deserves a message box; after it, a
/// message box would be a worse experience than a logged error.
static UI_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// PROBLEM 89 — the ONLY message box in the app. It exists for the failure
/// that leaves no other UI: a GUI process has no console, so a panic message
/// otherwise goes nowhere and the user experiences "I double-clicked it and
/// nothing happened" — the least reportable bug there is.
#[cfg(windows)]
fn show_fatal_box(detail: &str) {
    unsafe {
        use windows::core::{HSTRING, PCWSTR};
        use windows::Win32::UI::WindowsAndMessaging::{
            MessageBoxW, MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MB_TOPMOST,
        };
        let body = HSTRING::from(format!(
            "Spaceadom could not start.\n\n{detail}\n\nThis is usually a missing or broken \
             Microsoft Edge WebView2 Runtime. Reinstalling Spaceadom repairs it.\n\n\
             Details: %APPDATA%\\Spaceadom\\debug.log"
        ));
        let title = HSTRING::from("Spaceadom");
        MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST,
        );
    }
}

/// PROBLEM 83 — a window can be stranded on a monitor that no longer exists
/// (laptop undocked, projector unplugged, RDP reconnect with fewer screens).
/// Its saved position stays valid-looking, but the pixels are nowhere. Called
/// every time the dashboard is about to be SHOWN: if the window's centre is
/// not inside any live monitor, re-centre it on the current one.
pub fn ensure_on_screen(win: &tauri::WebviewWindow) {
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else {
        return;
    };
    let Ok(monitors) = win.available_monitors() else { return };
    if monitors.is_empty() {
        return; // headless moment (RDP transition) — nothing sane to do
    }
    let cx = pos.x + size.width as i32 / 2;
    let cy = pos.y + size.height as i32 / 2;
    let on_screen = monitors.iter().any(|m| {
        let mp = m.position();
        let ms = m.size();
        cx >= mp.x
            && cx < mp.x + ms.width as i32
            && cy >= mp.y
            && cy < mp.y + ms.height as i32
    });
    if !on_screen {
        log::warn!(
            "window: centre ({cx},{cy}) is outside every live monitor (one was removed?) — \
             re-centring"
        );
        let _ = win.center();
    }
}

/// PROBLEM 81 — the overlay's RUNTIME configuration, shared between initial
/// setup (step 9b) and the PROBLEM 59 rebuild path. The rebuild used to call
/// a bare `WebviewWindowBuilder::new(...).visible(false)` — a window missing
/// transparency, click-through, no-activate and the DWM border fixes: an
/// opaque, decorated, focus-stealing rectangle. Everything the overlay needs
/// beyond its tauri.conf.json declaration lives HERE and nowhere else.
pub fn configure_overlay_window(overlay: &tauri::WebviewWindow) {
    let _ = overlay.hide(); // stays hidden until something shows it
    let _ = overlay.set_focusable(false); // v11 "NoActivate": never steal focus

    // DONOTROUND + BORDER_COLOR=NONE: Win11 draws a 1px border and rounds the
    // corners even on an undecorated window — both read as a "box" around the
    // toasts (measured at R27 against an R32 desktop). Cosmetic; failure just
    // brings the line back.
    #[cfg(windows)]
    if let Ok(hwnd) = overlay.hwnd() {
        use windows::Win32::Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE,
            DWMWCP_DONOTROUND,
        };
        const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
        let pref = DWMWCP_DONOTROUND;
        let none = DWMWA_COLOR_NONE;
        let raw = windows::Win32::Foundation::HWND(hwnd.0 as *mut _);
        unsafe {
            let _ = DwmSetWindowAttribute(
                raw,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &pref as *const _ as *const _,
                std::mem::size_of_val(&pref) as u32,
            );
            let _ = DwmSetWindowAttribute(
                raw,
                DWMWA_BORDER_COLOR,
                &none as *const _ as *const _,
                std::mem::size_of_val(&none) as u32,
            );
        }
    }

    // Click-through, so a toast can't swallow a click aimed at whatever is
    // underneath it. Fail CLOSED: if this fails the overlay is never shown —
    // degraded but safe. Never simplify this away.
    match overlay.set_ignore_cursor_events(true) {
        Ok(()) => {
            guide_hud::OVERLAY_DISABLED.store(false, std::sync::atomic::Ordering::Relaxed);
            // PROBLEM 265 — THE ONE LINE THAT ANSWERS "how long after logon
            // could the ring have been drawn?".
            //
            // Here and nowhere else, because this is the single point every
            // creation path goes through (the early overlay phase, the full
            // `create_app_windows`, the PROBLEM 59 cold-boot rebuild and
            // `display_watch`'s rebuild) AND it is the point at which the
            // overlay is genuinely USABLE: the window exists, click-through was
            // applied, and `OVERLAY_DISABLED` was just cleared. Logging it any
            // earlier would time a window that still fails closed.
            //
            // A rebuild hours into a session prints a large number, correctly —
            // the number is "since app start", not "how long the build took".
            log::info!(
                "overlay: configured (on-demand, click-through) — overlay usable for the \
                 Guide HUD {} ms after app start (PROBLEM 265)",
                overlay_boot::since_start().as_millis()
            );
        }
        Err(e) => {
            // PROBLEM 217 — the target moves this line off the automatic log
            // bridge and onto `report_degraded`, which rate-limits it. The
            // severity in debug.log is unchanged: still ERROR, same wording.
            log::error!(
                target: telemetry::DEGRADED_TARGET,
                "overlay: click-through FAILED ({e}) — overlay disabled; \
                 HUD/toasts will not be shown"
            );
            guide_hud::OVERLAY_DISABLED.store(true, std::sync::atomic::Ordering::Relaxed);
            telemetry::report_degraded(
                telemetry::Degraded::OverlayDisabled,
                &format!(
                    "set_ignore_cursor_events failed ({e}) — OVERLAY_DISABLED set at window \
                     configuration, so the HUD and every sound are suppressed"
                ),
            );
        }
    }
}

pub fn run() {
    // ----------------------------------------------------------------
    // 1. Elevation check (non-blocking — relaunches if needed)
    // ----------------------------------------------------------------
    // PROBLEM 61 — DO NOT SELF-ELEVATE.
    //
    // The app used to relaunch itself via `runas` at every launch. That was
    // never necessary: WH_KEYBOARD_LL does not require elevation. What it DID
    // cause: a UAC prompt on every start, autostart that silently failed on
    // standard (non-admin) accounts, and a tester who could only get the app
    // running by right-clicking "Run as administrator". It also gives a global
    // keyboard hook that auto-elevates at logon the exact signature AV
    // heuristics flag as a keylogger.
    //
    // The manifest now pins asInvoker and the logon task registers at
    // LeastPrivilege, so this whole branch is gone. See
    // windows-app-manifest.xml for the accepted UIPI limitation.
    // (The logon task itself is registered later, in setup(), from the
    // persisted run_at_startup setting.)

    // PROBLEM 265 — start the clock the "overlay usable after N ms" line is
    // measured against. First statement with any effect in the whole process,
    // deliberately: every number it produces is only as honest as this call is
    // early. It logs nothing (the logger does not exist yet).
    overlay_boot::mark_process_start();

    // ----------------------------------------------------------------
    // 2. Logger (must come before any log:: calls)
    // ----------------------------------------------------------------
    let data_dir = startup::data_dir();
    logger::init(&data_dir);
    log::info!("Spaceadom starting — data dir: {}", data_dir.display());
    // PROBLEM 224 — WHICH BUILD IS THIS.
    //
    // Every crash investigation in this project has had to answer that
    // question from outside the log: an installer timestamp, a file size, an
    // ASCII marker hunt. debug.log never said it, so a log on its own could
    // never identify the build that crashed — and with two installers, a Store
    // build and a repo build all able to be the running exe, that is a real
    // ambiguity, not a theoretical one. One line fixes it permanently, and it
    // doubles as a long ASCII marker for the installed-exe check in CLAUDE.md.
    log::info!(
        "Spaceadom build — version {} ({} bytes at {})",
        env!("CARGO_PKG_VERSION"),
        std::env::current_exe()
            .and_then(|p| std::fs::metadata(&p).map(|m| m.len()))
            .map(|n| n.to_string())
            .unwrap_or_else(|_| "unknown".into()),
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "unknown path".into())
    );
    // PROBLEM 250 — and WHICH WORLD is this. Third line of every log, right
    // after "which version, from where", because a Store install behaves
    // differently in four places and none of the differences are visible from
    // a version number or a path. `grep package-identity-probe debug.log`.
    packaged::log_identity_once();

    // PROBLEM 253 — FOURTH line of every log: is this a safe-mode launch?
    //
    // Placed here, immediately after the logger, because the answer decides
    // what `setup()` does — no hook, no overlay — so it has to be known before
    // the Tauri builder exists.
    //
    // **REVIEW FIXES 2026-09-05 (C1): this call is now READ-ONLY.** It used to
    // write `failed_starts + 1` here too, and that made every duplicate launch
    // a recorded startup crash: `tauri-plugin-single-instance` initialises
    // AFTER this line and a second instance exits from inside it with
    // `cleanup_before_exit()` + `process::exit(0)` — no `RunEvent`, so
    // `note_clean_exit()` never runs and the `+1` stuck. Three double-clicks on
    // the icon put a healthy app in safe mode. The increment moved to
    // `safe_mode::note_surviving_instance()`, the first line of `setup()`,
    // which only the surviving instance ever reaches.
    //
    // Deliberately BEFORE `telemetry::init()`: reading a small JSON file cannot
    // fail in a way that matters (every failure reads as zero), and a crash
    // reporter that has not started yet is a smaller loss than a boot decision
    // that never got made.
    let safe_mode_launch = safe_mode::begin();
    // PROBLEM 253 — and, right after "which world", WHERE that world's data
    // lives. A portable copy resolves every data path under its own exe
    // directory instead of %APPDATA%; this is the one line that says so.
    portable::log_root_once();

    // PROBLEM 195 — start the Sentry client. Returns None (and this is a
    // no-op) while `telemetry::SENTRY_DSN` is the empty placeholder, which is
    // what ships until the owner pastes his own DSN in.
    //
    // BOUND TO A NAME ON PURPOSE. `ClientInitGuard` closes and flushes the
    // client when it drops, so `let _ = telemetry::init();` would drop it at
    // the end of this statement and switch reporting off on the line that
    // turned it on. It has to live as long as `run()` does.
    //
    // Nothing can actually be SENT yet: `SENDING_ENABLED` starts false and is
    // only seeded once the config has been read, a few steps below. That is
    // deliberate — until the config is loaded we do not know whether this user
    // has switched sending off, and the only safe answer to that is silence.
    let _sentry_guard = telemetry::init();
    log::info!(
        "telemetry: sentry client {} — see src/telemetry.rs to paste a DSN",
        if _sentry_guard.is_some() { "started" } else { "INERT (no DSN compiled in)" }
    );

    // PROBLEM 125 — a panic used to leave NOTHING behind.
    //
    // Rust prints panics to stderr, and this is a `windows_subsystem = "windows"`
    // binary: there is no console, so stderr goes nowhere. The app vanished and
    // the log's last line was whatever happened to be written before the crash.
    // On a stranger's machine, with no way to reproduce it, that is the end of
    // the investigation.
    //
    // Installed immediately AFTER the logger so the hook has somewhere to
    // write, and BEFORE anything that could plausibly panic.
    //
    // PROBLEM 131 — THERE USED TO BE TWO OF THESE. This one was installed
    // here, and a second `set_hook` further down (the old PATCH 5d block)
    // REPLACED it wholesale a few lines later, because `set_hook` replaces and
    // that one did not chain. So everything this hook added — the thread name,
    // the "this is a crash" wording — has never once appeared in a log. All 14
    // recorded crashes were reported by the other hook, in the other format,
    // which is how the duplication was noticed at all.
    //
    // The two are now ONE hook, below, at the point where `main_tid` and
    // `UI_READY` are available. Do not add a second `set_hook` anywhere: the
    // last one installed silently wins, and the loser leaves no trace of
    // having lost. (Same class as PROBLEMS 118/120/129 — a stale thing
    // outliving the thing that replaced it.)

    // 2a. PROBLEM 64 + 59 + 76 — the HKCU Run autostart path. A Run value cannot
    // express the Scheduled Task's `/DELAY 0000:30`, so when the app is started
    // BY that Run value (`--autostart`) the cold-boot wait has to happen inside
    // the app: launching at logon races Edge/WebView2's brokers, the GPU stack
    // and the disk, and losing that race is PROBLEM 59's dead-app. PROBLEM 76
    // measured the size of it at a REAL logon (boot 14:20:48 → Run key fired
    // 14:22:31 → hook live 14:23:01) and cut 30s to 10s.
    //
    // *** THE SLEEP USED TO BE RIGHT HERE, AND THAT WAS THE BUG (PROBLEM 215). ***
    //
    // It sat before `tauri::Builder`, so it delayed EVERYTHING — including the
    // keyboard hook and the engine, neither of which touches WebView2. After a
    // reboot the owner's shortcuts were dead for ten seconds for a reason that
    // has nothing to do with keyboards.
    //
    // The wait was not removed; it was moved to the only thing that needs it.
    // `AUTOSTART_SETTLE` + `create_app_windows` now delay the WINDOW/WEBVIEW
    // creation alone, from `setup()`, after the hook, the engine and the tray
    // are already live. Do not put a sleep back on this line.

    // 2b. Panic hook — a Rust panic otherwise vanishes without a trace in a
    // release build, and "the app just disappeared" is the one report a
    // beta tester cannot debug for us. This is the crash line to look for
    // in debug.log when someone says the app closed by itself.
    let main_tid = std::thread::current().id();
    std::panic::set_hook(Box::new(move |info| {
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".into());
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".into()
        };
        // The thread name comes from the hook that PROBLEM 131 found was being
        // silently replaced. It matters here: a panic on the hook or engine
        // thread is survivable and a panic on the main thread is not, and the
        // 14 recorded crashes could not be told apart without it.
        let thread = std::thread::current().name().unwrap_or("<unnamed>").to_string();

        // PROBLEM 224 — ONE PANIC MUST BE ONE SENTRY EVENT.
        //
        // All three `log::error!` lines in this hook carry
        // `telemetry::DEGRADED_TARGET`, and that is the ONLY thing the target
        // changes. In debug.log they are still ERROR, still the same wording,
        // still in the same order — the target is a routing label, not a
        // severity, and nothing is hidden from the user's own machine.
        //
        // What it stops: `logger.rs` bridges every record to Sentry through
        // `telemetry::log_filter`, which sends anything at ERROR or above, and
        // this hook then ALSO calls `capture_panic` a few lines down. One panic
        // therefore cost FOUR events — three log records plus one exception —
        // and they grouped into TWO separate Sentry issues, so a single crash
        // read as two unrelated bugs. `log_filter` drops DEGRADED_TARGET
        // records (PROBLEM 217 added that rule for the same double-reporting
        // reason), so the only thing that leaves the machine now is
        // `capture_panic`'s single Fatal exception — the one event of the four
        // that carries a real stack trace.
        //
        // The alternative was dropping `capture_panic` and keeping the log
        // route. It was rejected: that keeps three events instead of one, and
        // loses the stacktrace and the `thread` tag with them.
        log::error!(
            target: telemetry::DEGRADED_TARGET,
            "PANIC on thread '{thread}' at {loc}: {msg}. This is a crash, not a handled \
             error — please report it with the lines above from debug.log."
        );

        // PROBLEM 253 — tell the boot counter this launch faulted.
        //
        // ABOVE the backtrace and above `capture_panic` on purpose. Both of
        // those can take real time (`force_capture` symbolises every frame; the
        // Sentry flush waits up to two seconds), and if the process is killed
        // part-way through this hook, the ONE fact that decides whether the
        // next launch protects the user has to already be on disk.
        //
        // A no-op after the first 30 seconds: a panic in a long-running session
        // is not a STARTUP crash, and treating it as one would eventually
        // disarm the shortcuts of somebody whose app works perfectly well for
        // an hour at a time.
        safe_mode::note_panic();

        // PROBLEM 131 — what the app was DOING. A backtrace says which code was
        // on the stack; this says the overlay had been rebuilt twice and a
        // display changed 4 seconds ago, which is usually what identifies the
        // trigger for a crash that only happens on someone else's machine.
        // PROBLEM 224 — same target, same reason as the line above.
        log::error!(target: telemetry::DEGRADED_TARGET, "{}", crash_context::snapshot());

        // PATCH 5d — without a backtrace, a panic INSIDE a dependency (the
        // tao "cannot move state from Destroyed" report) names the crate's
        // line but not OUR call path into it, which makes the cause
        // untestable. force_capture works regardless of RUST_BACKTRACE.
        //
        // PROBLEM 131 — this printed `0: <unknown>` for every frame in all 14
        // recorded crashes, because `spaceadom.pdb` was built into
        // target/release and the installer shipped only the .exe. The pdb is
        // now installed BESIDE the exe (tauri.conf.json `resources`), which is
        // where dbghelp looks, so these frames resolve on the user's machine.
        // PROBLEM 224 — same target, same reason. This is also the single most
        // expensive line in the hook (`force_capture` symbolises every frame),
        // which is why it stays BELOW the two cheap ones: if the process is
        // killed mid-hook, the panic line and the app context are already on
        // disk.
        log::error!(
            target: telemetry::DEGRADED_TARGET,
            "backtrace:\n{}",
            std::backtrace::Backtrace::force_capture()
        );

        // PROBLEM 195 — forward the crash to Sentry, from INSIDE this hook.
        //
        // THIS IS THE WHOLE REASON `sentry`'s default `panic` feature is
        // disabled in Cargo.toml. That feature makes `sentry::init()` install
        // its own panic hook — a SECOND one, the exact thing PROBLEM 131 cost
        // months, except worse, because a hook installed from inside a
        // dependency does not appear in any grep of this repo. Doing it by
        // hand here keeps the count at one and keeps the three log::error!
        // lines above, which are what a user without a network connection
        // still gets. (The wording avoids the hook-installing function's
        // literal name on purpose: grepping this file for that name is a
        // tripwire in this project, and a comment must not move its number.)
        //
        // Placed AFTER the logging on purpose: debug.log is the record that
        // always works, and nothing that talks to a network is allowed to run
        // before it. `capture_panic` returns immediately unless the user has
        // left sending on AND a DSN was compiled in.
        telemetry::capture_panic(&msg, &thread);

        // PROBLEM 89 — a panic on the MAIN thread BEFORE the UI exists is an
        // invisible death: no window, no tray, and a GUI process has no
        // console for the panic text. Show the one message box and exit.
        //
        // BOTH gates are mandatory. `main_tid`: a panic on the hook or engine
        // thread is already survivable (PROBLEM 82 restarts them) and must not
        // kill the app. `UI_READY`: once the tray icon exists the user can
        // see Spaceadom is alive, and a modal box would then be a worse
        // experience than the logged error.
        //
        // Do NOT try to catch this by wrapping app.run() in catch_unwind: the
        // panic originates in a tao callback invoked from the Win32 message
        // pump across an `extern "system"` boundary, where unwinding aborts.
        #[cfg(windows)]
        if std::thread::current().id() == main_tid
            && !UI_READY.load(std::sync::atomic::Ordering::Relaxed)
        {
            show_fatal_box(&msg);
            std::process::exit(1);
        }
    }));

    // ----------------------------------------------------------------
    // 4. Config (before startup registration — the task's enabled state
    //    comes from config.run_at_startup)
    // ----------------------------------------------------------------
    // PROBLEM 250 — BEFORE load_or_init, and only ever on a packaged first
    // launch: take a durable snapshot of the config and its backups, so a user
    // who installs from the Store and then uninstalls the unpackaged copy
    // cannot lose their profiles to that uninstaller. A no-op for every NSIS
    // and MSI install, which is every install that exists today.
    packaged::migrate_legacy_data_once();

    let shared_config = config::load_or_init();

    // PROBLEM 180 — publish which optional special keys are bound BEFORE the
    // hook thread starts. `config::save` republishes on every change, but the
    // atomic starts at 0: without this call every bit is clear from launch
    // until the user happens to save something, and Space+Enter / Tab / arrows
    // / F1-F12 are eaten in the meantime.
    hook::publish_bound_specials(&shared_config.read().unwrap_or_else(|p| p.into_inner()));
    // PHASE A — seed the non-letter trigger bitmap BEFORE the hook thread
    // starts, same both-ends rule: the atomics start all-clear, and without
    // this line Esc / ` / Tab / ⌫ and the rest pass through to Windows until
    // the first save of the session.
    hook::publish_bound_vks(&shared_config.read().unwrap_or_else(|p| p.into_inner()));
    // PROBLEM 180 — the App-exceptions list has to be published HERE as well
    // as in config::save, or an excluded app is not excluded until the first
    // save of the session.
    hook::exclusions::publish_excluded_apps(&shared_config.read().unwrap_or_else(|p| p.into_inner()));
    // PROBLEM 206 — seed the pointer-HUD toggle, same both-ends rule. The
    // atomic starts false; skipping this line is the silent failure where the
    // feature works all session and then reads OFF for the entire next launch
    // until the user touches any setting.
    hook::publish_pointer_hud_activation(&shared_config.read().unwrap_or_else(|p| p.into_inner()));
    // PROBLEM 263 — seed the middle-button ring trigger, same both-ends rule.
    // The atomic starts false, so skipping this line is the silent failure
    // where the feature works all session and then reads OFF for the entire
    // next launch until the user touches any setting.
    hook::publish_middle_button_ring(&shared_config.read().unwrap_or_else(|p| p.into_inner()));
    // PROBLEM 195, and the same both-ends rule a third time: `SENDING_ENABLED`
    // starts FALSE, so this is the line that actually turns crash reporting on
    // for a user who has not opted out. Without it nothing would be sent until
    // the user happened to save a setting — and a crash during startup, which
    // is the crash worth having most, would never be reported at all.
    //
    // This is also the first moment in the process where consent is KNOWN,
    // which is why it is not seeded any earlier.
    telemetry::publish(&shared_config.read().unwrap_or_else(|p| p.into_inner()));

    // ----------------------------------------------------------------
    // 4b. PROBLEM 80 — overlay compositing mode. MUST run before the Tauri
    // builder: WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS is read once, when the
    // WebView2 environment is created. On machines whose display driver
    // breaks GPU composition of the transparent overlay (virtual-display
    // drivers — spacedesk / DeX / phone-mirroring — are prime suspects), the
    // HUD and toasts compose ZERO pixels while everything else looks healthy:
    // Rust readbacks say visible=true, the JS runs, the sound plays. Proven
    // live on the owner's laptop: with --disable-gpu the exact same build
    // painted the HUD (263/861 sampled pixels), without it 0/861.
    // The flag is set by the self-test in commands.rs, never by hand.
    #[cfg(windows)]
    {
        let mode = shared_config.read().unwrap_or_else(|p| p.into_inner()).overlay_compositing.clone();
        if mode == "software" {
            let mut args = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS")
                .unwrap_or_default();
            if !args.contains("--disable-gpu") {
                if !args.is_empty() {
                    args.push(' ');
                }
                args.push_str("--disable-gpu");
            }
            std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", &args);
            log::info!(
                "compositing: SOFTWARE mode (--disable-gpu) — set after the overlay \
                 self-test detected dead GPU composition on this machine"
            );
        }
    }

    // ----------------------------------------------------------------
    // 3. Startup registration: the Spaceadom logon task (elevated, silent)
    //    + removal of legacy Run entries so old builds cannot ALSO start
    //    a second keyboard hook at logon.
    // ----------------------------------------------------------------
    // PROBLEM 55 — this used to run INLINE and froze the app on launch.
    // ensure_startup_task() shells out to schtasks (/Query, /Create, /Change)
    // and each spawn costs 100-300ms; three of them on the startup path, on
    // top of WebView2's own first-run init, is the "not responding for a few
    // moments when first opening" both testers reported. None of it is needed
    // before the window exists — the logon task only matters at the NEXT
    // logon — so it moves to a background thread and startup no longer waits.
    #[cfg(windows)]
    {
        let run_at_startup = shared_config.read().unwrap_or_else(|p| p.into_inner()).run_at_startup;
        std::thread::Builder::new()
            .name("st-startup-task".into())
            .spawn(move || {
                startup::ensure_startup_task(run_at_startup);
                // PROBLEM 141 - look for a SECOND install here, off the
                // startup path: this stats Program Files and reads a PE
                // version resource, neither of which belongs before the
                // first paint (PROBLEM 55).
                rival_install::scan();
                startup::remove_legacy_run_entries();
            })
            .ok();
    }

    // Sync rollover_ms into hook atomic from config
    {
        let cfg = shared_config.read().unwrap_or_else(|p| p.into_inner());
        hook::ROLLOVER_MS.store(cfg.rollover_ms, std::sync::atomic::Ordering::Relaxed);
        // PROBLEM 119 — seed the opacity floor from the saved config.
        engine::actions::opacity::OPACITY_FLOOR_PCT
            .store(cfg.opacity_floor_pct, std::sync::atomic::Ordering::Relaxed);
    }

    // ----------------------------------------------------------------
    // 5. Hook ↔ Engine channel (bounded 256 avoids unbounded memory growth)
    // ----------------------------------------------------------------
    let (hook_tx, hook_rx) = bounded::<hook::HookEvent>(256);

    // ----------------------------------------------------------------
    // 6. Icon cache (shared between commands and icon_extractor)
    // ----------------------------------------------------------------
    let icon_cache = Arc::new(Mutex::new(
        std::collections::HashMap::<String, String>::new(),
    ));

    // ----------------------------------------------------------------
    // Build Tauri application
    // ----------------------------------------------------------------
    let result = tauri::Builder::default()
        // --- Plugins ---
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // PROBLEM 245 — the in-app updater. Registered here; DRIVEN entirely
        // from Rust (updater.rs), so no updater permission is granted to any
        // window and the frontend never touches it.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            use tauri::Manager;
            // A second AUTOSTART instance (PROBLEM 64: the Run-key launch
            // waking from its 30s delay after the user already opened the app
            // manually) must die silently — popping the dashboard up half a
            // minute after logon reads as a haunted machine. Only a real user
            // launch fronts the window.
            if args.iter().any(|a| a == "--autostart") {
                return;
            }
            // PROBLEM 215 — a real user launch during the autostart settle
            // window finds no window to front. Asking for the app IS asking
            // for its UI, so build it now rather than opening nothing.
            if app.get_webview_window("settings").is_none() {
                log::info!(
                    "single-instance: a manual launch arrived before the settle wait \r
                     finished — creating the windows now (PROBLEM 215)"
                );
                create_app_windows(app);
            }
            if let Some(win) = app.get_webview_window("settings") {
                let _ = win.unminimize(); // geometry is meaningless while minimized
                ensure_on_screen(&win); // PROBLEM 83 — that monitor may be gone
                let _ = win.show();
                let _ = win.set_focus();
            }
        }))
        // --- Managed state ---
        .manage(ConfigState(Arc::clone(&shared_config)))
        .manage(IconCacheState(Arc::clone(&icon_cache)))
        // --- Commands ---
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::get_profiles,
            commands::set_active_profile,
            commands::extract_icon_cmd,
            commands::pick_file,
            commands::check_app_path,
            commands::get_hook_status,
            commands::get_hook_health,
            commands::set_hook_timeout,
            commands::reset_config,
            commands::clear_active_profile,
            commands::restore_preset_profiles,
            commands::undo_last_change,
            commands::undo_available,
            commands::set_overlay_compositing,
            // 1.0.96 — the Conflicts-area "The ring isn't showing?" tool that
            // replaced the "Software overlay" switch. Re-runs the pixel
            // self-test on demand and writes the verdict, in both directions.
            commands::run_overlay_fix,
            // 1.0.96 — the "Restart now" the ring check offers when its verdict
            // changed. NOT `AppHandle::restart()`: that spawns before it exits
            // and single-instance then kills the newcomer (see the command).
            commands::restart_app,
            // PROBLEM 245 — "Updated to 1.0.X", asked once by the dashboard.
            commands::get_update_notice,
            // PROBLEM 195 — the "Don't send logs" switch.
            commands::set_send_logs,
            commands::restart_elevated,
            commands::overlay_ready,
            commands::overlay_log,
            commands::overlay_error,
            commands::overlay_fit,
            commands::overlay_fit_hud,
            commands::overlay_fit_handover,
            commands::overlay_shape,
            commands::overlay_toasts_done,
            // PROBLEM 206 — the Guide HUD's chip geometry, for pointer
            // activation. Same CSS-px + dpr convention as overlay_shape.
            commands::publish_hud_chips,
            // 1.0.96 — Settings previews the REAL ring in a layout the user has
            // not chosen yet. Writes nothing and cannot launch anything.
            commands::preview_hud_layout,
            // PROBLEM 267 — the key editor fetches a link's favicon ONCE, at
            // bind time, and stores it in the binding; the Settings panel
            // reads the built-in 3D/CAD rows it shows pre-seeded at "Space
            // only". Both are read-only for the config.
            site_icon::fetch_site_icon,
            commands::get_builtin_exceptions,
            commands::find_browser_cmd,
            commands::validate_browser,
            // TASK 3 — the OS default browser (path + name + icon) for the
            // key editor's paste-row disc. Same resolver the engine uses to
            // open an unpinned URL, so the two cannot disagree.
            commands::get_default_browser,
            commands::show_conflict_check,
            commands::create_profile,
            commands::delete_profile,
            commands::rename_profile,
            // 1.0.96 — the profile editor's edit mode. `reorder_profiles` is
            // the one to notice: `profiles` is the order RAlt cycles in, so a
            // drag is a behaviour change and the command validates the whole
            // set rather than trusting the list the DOM produced.
            commands::reorder_profiles,
            commands::duplicate_profile,
            commands::set_profile_emoji,
            commands::open_emoji_panel,
            commands::export_profile,
            commands::import_profile,
            commands::list_start_menu_apps,
            browser_profiles::list_browser_profiles,
            commands::toggle_bypass,
            commands::open_log_folder,
            commands::frontend_log,
            commands::frontend_error,
            commands::dashboard_ready,
            commands::get_stale_task,
            commands::repair_stale_task,
            commands::get_rival_install,
            commands::repair_rival_install,
            commands::get_conflicts,
            commands::close_conflict,
            commands::open_startup_manager,
            commands::open_touchpad_settings,
            commands::touchpad_caps,
            commands::reinstall_hook,
            commands::set_startup_enabled,
            // PROBLEM 250 — the Store build's two extra questions: who owns
            // "Run at startup" here, and where does the user uninstall the
            // other copy.
            commands::get_packaged_startup,
            // PROBLEM 254 — "is this the unzipped, portable copy?" One bool,
            // for the frontend paths where only the WORDING differs (the
            // rival-install banner). The Settings "Run at startup" row needs
            // no new command: its portable case rides on
            // `get_packaged_startup`'s existing tuple.
            commands::is_portable_install,
            commands::open_installed_apps,
            // PROBLEM 249 — the manual check, the rollback, and What's New.
            updater::check_for_updates_now,
            updater::rollback_available,
            updater::rollback_to_previous,
            release_notes::get_release_notes,
            release_notes::get_whats_new,
            // PROBLEM 253 — safe mode and "Report a problem".
            commands::get_safe_mode,
            commands::safe_mode_turn_back_on,
            commands::build_diagnostics_bundle,
            commands::open_issues_page,
            // PROBLEM 255 — Settings ▸ About's version / install-kind /
            // data-folder line. Without this line `fetchAboutInfo()`'s first
            // branch always throws and every build silently shows the
            // `getVersion()`-only fallback — a correct degrade path, but not
            // the one the row was built for.
            commands::get_about_info,
            // REVIEW FIXES 2026-09-05 (H4) — the SEED half of the OS
            // light/dark wiring. `os-theme-changed` only fires on a change, so
            // without this a freshly-created window (a first launch, or the
            // overlay display_watch.rs rebuilds when the monitors move) would
            // resolve "auto" against theme-resolve.ts's daylight default until
            // the user next touched their Windows setting. Both halves, every
            // time.
            theme_watch::get_os_prefers_dark,
            // PROBLEM 259 — the own-window fallback (PROBLEM 257's workaround).
            // The dashboard page feeds the Space it can see to the engine
            // because the keyboard hook cannot see it while our own window
            // holds the foreground. All three guards live in `hook::`.
            commands::own_window_space_down,
            commands::own_window_key,
            // PHASE A — the key editor's chord recorder and "Try it".
            commands::chord_record_start,
            commands::chord_record_poll,
            commands::chord_record_stop,
            commands::run_command_once,
            commands::import_profile_commit,
            commands::own_window_space_up,
        ])
        // --- App setup callback ---
        .setup(move |app| {
            let app_handle = app.handle().clone();

            // REVIEW FIXES 2026-09-05 (C1) — THE BOOT COUNTER'S INCREMENT, and
            // it must stay the FIRST statement in this closure.
            //
            // `setup()` is the earliest point at which this process is known to
            // have won the single-instance mutex: plugin initialisation runs
            // strictly earlier (tauri 2.11 `app.rs` — `initialize_plugins`
            // :2440, `(setup)(app)` :2531) and a duplicate launch exits from
            // inside `tauri-plugin-single-instance` with `process::exit(0)`,
            // never reaching this line. Incrementing any earlier counted the
            // user's double-clicks on the tray icon as startup crashes.
            //
            // First, not last: everything below it — the hook, the overlay, the
            // engine — is what safe mode exists to recover from, so a crash in
            // any of it has to find the `+1` already on disk.
            safe_mode::note_surviving_instance();

            // PROBLEM 253 — start the 30-second health timer, on every
            // launch, safe mode or not. It is what resets the boot counter, so
            // a launch that never starts it is a launch that counts as a
            // failure however well it goes.
            safe_mode::spawn_healthy_timer();

            // 7. Spawn hook thread
            {
                let cfg = shared_config.read().unwrap_or_else(|p| p.into_inner());
                // PROBLEM 253 — THE SAFE-MODE BRANCH. This is the one that
                // matters: `spawn_hook_thread` installs WH_KEYBOARD_LL, and a
                // hook is the single most likely thing to be killing a launch
                // that has now died three times in a row. The channel is parked
                // instead, so the dashboard's "Turn back on" can spawn exactly
                // this thread later without a restart.
                if safe_mode_launch {
                    safe_mode::arm_pending_hook(hook_tx.clone(), cfg.rollover_ms);
                    log::warn!(
                        "setup: SAFE MODE — the keyboard hook thread was NOT spawned and \
                         the fullscreen / exception / pointer watchers were not started. \
                         Space is an ordinary space until the user presses 'Turn back \
                         on'. See the {} line above.",
                        safe_mode::MARKER
                    );
                    // The report goes out from here rather than from
                    // `safe_mode::begin()` because `telemetry::publish` has now
                    // run — consent is known, and a report sent before that
                    // would be sent without it.
                    telemetry::report_degraded(
                        telemetry::Degraded::SafeModeEntered,
                        &safe_mode::describe(),
                    );
                } else {
                    hook::spawn_hook_thread(hook_tx.clone(), cfg.rollover_ms);
                    log::info!("setup: hook thread spawned");

                    // 8. Start fullscreen watcher
                    let fullscreen_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let flag_clone = Arc::clone(&fullscreen_flag);
                    hook::fullscreen::start_fullscreen_watcher(
                        flag_clone,
                        cfg.fullscreen_allowlist.clone(),
                    );

                    // 8b. Start the App-exceptions watcher. Same shape as the
                    // fullscreen watcher above: a named 500ms poller that writes
                    // one atomic the hook reads, because the hook callback may not
                    // ask Windows which window is in front.
                    hook::exclusions::start_exclusion_watcher();

                    // 8c. Start the Guide-HUD pointer watcher (PROBLEM 206): the
                    // ~60Hz poller that turns the mouse hook's cursor atomics and
                    // the published chip geometry into an armed-chip decision.
                    // Same shape as the two pollers above, for the same reason —
                    // the hook callback may do lock-free atomics only.
                    hook::pointer::start_pointer_watcher();

                    // PROBLEM 88 — the 500ms "copier" thread that used to live
                    // here is GONE. It was the only writer to
                    // hook::FULLSCREEN_ACTIVE, so if the watcher thread died while
                    // a game had the flag true, this loop re-stored `true` forever
                    // and the hook passed EVERY key through — the whole app inert,
                    // silently, until restart. start_fullscreen_watcher now writes
                    // the flag directly (and fails toward "not fullscreen").
                } // end of the PROBLEM 253 safe-mode else
            }

            // 9. Start async engine actor
            let engine_state = Arc::new(Mutex::new(EngineState::new(
                Arc::clone(&shared_config),
                app_handle.clone(),
            )));

            // Initialise profile_index to match active_profile name
            {
                let cfg = shared_config.read().unwrap_or_else(|p| p.into_inner());
                let active = &cfg.active_profile;
                if let Some(idx) = cfg.profiles.iter().position(|p| &p.name == active) {
                    engine_state.lock().unwrap_or_else(|p| p.into_inner()).profile_index = idx;
                }
            }

            engine::start_engine(hook_rx, engine_state);
            log::info!("setup: engine actor started");

            // Wire guide HUD event emitter (renders into the "overlay" window)
            guide_hud::set_app_handle(app_handle.clone());

            // 9b/9c, the PROBLEM 86 opacity registration, step 11's
            // close-to-tray and the PROBLEM 59 recovery all MOVED into
            // `create_app_windows` (PROBLEM 215). They are dispatched below,
            // after the tray, because not one of them is needed for Space+key
            // to work and every one of them needs WebView2.


            // 9d. Report other keyboard remappers into the log at startup.
            // A tester's shortcuts silently did nothing and his own first
            // guess was leftover AutoHotkey scripts — the log could neither
            // confirm nor rule that out. Now every log a tester sends back
            // answers it on line one. Observation only; nothing is killed.
            {
                let conflicts = hook::conflicts::detect();
                if !conflicts.is_empty() {
                    log::warn!(
                        "conflicts: {} keyboard-remapping program(s) detected — Space may be \
                         captured before Spaceadom sees it",
                        conflicts.len()
                    );
                }
            }

            // 9e. TOUCHPAD T2 — the edge-gesture engine. Started here, after
            // the engine actor, because it emits `touchpad-caps`/`touchpad-live`
            // to the page and drives brightness/volume/scrub. Safe on a machine
            // with no Precision Touchpad: its supervisor simply reports
            // `presence=none` and never starts a reader. Not gated on safe mode
            // — it installs no keyboard hook; the only shared-hook touch is the
            // one BAND_LIVE atomic the existing mouse callback reads.
            #[cfg(windows)]
            {
                let cfg = shared_config.read().unwrap_or_else(|p| p.into_inner());
                touchpad::init(app_handle.clone(), &cfg);
                log::info!("setup: touchpad edge-gesture engine started");
            }

            // 10. Build system tray
            tray::build_tray(&app_handle)?;
            log::info!("setup: system tray built");


            // 10b. PROBLEM 76 — one-time promotion of the tray icon out of the
            // Win11 overflow flyout, so the user can SEE the app is running.
            // Delayed a few seconds: the shell writes the NotifyIconSettings
            // entry only after it has shown the icon at least once. Gated on a
            // config flag so a user who later hides the icon stays hidden.
            #[cfg(windows)]
            {
                let cfg_arc = Arc::clone(&shared_config);
                std::thread::Builder::new()
                    .name("st-tray-promote".into())
                    .spawn(move || {
                        // PROBLEM 250 follow-up (LIVE TEST 2026-09-05,
                        // FINDING D) — a packaged copy has nothing to do here
                        // and must not spend 24 seconds finding that out.
                        //
                        // `promote_tray_icon_once` refuses under a package and
                        // says why once (its doc comment has the measurement:
                        // HKCU writes go to the package's private hive and the
                        // shell never sees them). Without this return the loop
                        // below would sleep 3 s eight times, every launch,
                        // forever — the once-gate can never close, because it
                        // only closes on a successful promotion.
                        if crate::packaged::is_packaged() {
                            startup::promote_tray_icon_once();
                            return;
                        }
                        // PROBLEM 142 — gate on the exe PATH, not a bare bool.
                        //
                        // The old `tray_promoted` flag latched true on
                        // 2026-08-12 for the Program Files install. PROBLEM 129
                        // then moved the app to %LOCALAPPDATA% in 1.0.41, which
                        // Windows treats as a DIFFERENT icon and hides afresh —
                        // and the latch said "already done", so it never ran
                        // again. The owner had to click the chevron every time
                        // and reported it as a regression from 1.0.15, which is
                        // exactly what it was.
                        //
                        // Keying on the path preserves the reason the latch
                        // exists: within one install location this still runs
                        // once, so a user who drags the icon back into the
                        // overflow is never overridden.
                        let me = std::env::current_exe()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let done_for = cfg_arc
                            .read()
                            .map(|c| c.tray_promoted_for.clone())
                            .unwrap_or_else(|_| me.clone());
                        if !me.is_empty() && done_for.eq_ignore_ascii_case(&me) {
                            return;
                        }
                        // The shell writes the NotifyIconSettings entry only
                        // after it has shown the icon once, and at a cold logon
                        // it is still settling. Poll instead of guessing one
                        // delay — a single 5s sleep is what let this silently
                        // do nothing on a slow boot.
                        for _ in 0..8 {
                            std::thread::sleep(std::time::Duration::from_secs(3));
                            if startup::promote_tray_icon_once() {
                                if let Ok(mut c) = cfg_arc.write() {
                                    c.tray_promoted = true;
                                    c.tray_promoted_for = me.clone();
                                    let snapshot = c.clone();
                                    drop(c);
                                    let _ = config::save(&snapshot);
                                }
                                return;
                            }
                        }
                        log::info!(
                            "tray: no NotifyIconSettings entry for this exe after ~24s — the icon                              stays where Windows put it; it can still be dragged out of the                              overflow by hand, and this retries on the next launch."
                        );
                    })
                    .ok();
            }

            // PROBLEM 224 — arm the WM_ENDSESSION guard as early as the app
            // has anything to guard.
            //
            // THIS CALL IS NOT REDUNDANT WITH THE ONE IN `create_app_windows`,
            // and the autostart path is why. tao's `Tao Thread Event Target`
            // window — the ONE window whose WM_ENDSESSION handler sets the
            // runner to `Destroyed` — exists from the moment the event loop is
            // built, i.e. before this `setup` closure runs. `create_app_windows`
            // is delayed ten seconds on an autostart launch (the split below),
            // so relying on it alone would leave the crash live for the first
            // ten seconds of every logon: a reboot-driven shutdown or an
            // installer arriving in that window is exactly the case that
            // produced the recorded crashes.
            //
            // Main thread, as `EnumThreadWindows` requires: the setup closure
            // runs on it.
            session_end::install();

            // PROBLEM 215 — THE SPLIT. Everything above this line is live NOW:
            // the hook, the engine, the guide-HUD wiring, the tray icon. Only
            // the windows wait, and only on an autostart launch.
            if autostart_launch() {
                log::info!(
                    "autostart launch — hook and engine are LIVE now; the OVERLAY (the Guide \
                     HUD's window) is built {} ms in and the DASHBOARD waits {}s for the shell \
                     to settle (PROBLEM 59/76/215/265). A Space hold before the overlay exists \
                     still launches, focuses and minimises; it simply draws no HUD, and it asks \
                     for the overlay to be built at once.",
                    overlay_boot::OVERLAY_SETTLE.as_millis(),
                    AUTOSTART_SETTLE.as_secs()
                );
                let settle_handle = app_handle.clone();
                if std::thread::Builder::new()
                    .name("st-window-settle".into())
                    .spawn(move || {
                        // PROBLEM 265 — PHASE 1: the OVERLAY, and only the
                        // overlay. It is the window the user can SEE the absence
                        // of, and the only one with a self-healing rebuild path
                        // (create_app_windows' PROBLEM 59 check below, then
                        // display_watch) if this early attempt loses the
                        // cold-boot WebView2 race. Failing here costs nothing
                        // that was not already being paid.
                        std::thread::sleep(overlay_boot::OVERLAY_SETTLE);
                        let ho = settle_handle.clone();
                        if let Err(e) = settle_handle
                            .run_on_main_thread(move || overlay_boot::create_overlay_now(&ho, false))
                        {
                            log::warn!(
                                "setup: could not reach the main thread to create the overlay \
                                 early ({e}) — the full window creation below still builds it"
                            );
                        }

                        // PHASE 2: everything else, at the original mark. The
                        // dashboard's wait is UNCHANGED — PROBLEM 59's hazard is
                        // untouched for the window that has no way back from it.
                        std::thread::sleep(
                            AUTOSTART_SETTLE.saturating_sub(overlay_boot::OVERLAY_SETTLE),
                        );
                        let h = settle_handle.clone();
                        // ON THE MAIN THREAD: window creation is not thread-safe
                        // anywhere in Win32, and this is the same hop the
                        // display-watch rebuild uses.
                        if let Err(e) = settle_handle.run_on_main_thread(move || {
                            create_app_windows(&h);
                        }) {
                            log::error!(
                                "setup: could not reach the main thread to create the windows \
                                 after the settle wait ({e}) — the app has a tray icon and a \
                                 working hook but no UI"
                            );
                        }
                    })
                    .is_err()
                {
                    log::error!(
                        "setup: could not spawn the settle thread — creating the windows now \
                         instead, accepting the PROBLEM 59 cold-boot risk"
                    );
                    create_app_windows(&app_handle);
                }
            } else {
                // A manual launch: the same instant Tauri itself would have
                // built them, so nothing about this path changed.
                create_app_windows(&app_handle);
            }



            // PROBLEM 245 — the in-app updater. Two calls, both cheap and both
            // off the critical path: note which version is launching (for the
            // one-time "Updated to" toast), then start the st-updater thread,
            // which sleeps past the settle before its first check.
            updater::record_launch_version(&app_handle.package_info().version.to_string());
            updater::schedule(app_handle.clone());
            // PROBLEM 249 — "What's new". Its own thread, 25 s out, behind both
            // window creation and the updater's first check. Reads the notice
            // record_launch_version just wrote WITHOUT consuming it.
            release_notes::schedule(app_handle.clone());

            log::info!("SpaceToggle OS fully initialised");
            println!("✅ Spaceadom initialised & ready.");
            // PROBLEM 89 — from here the user has a tray icon: a later panic
            // is visible-as-absence, so no modal box. Logged only.
            UI_READY.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        })
        .build(tauri::generate_context!())
        .map(|app| {
            app.run(|_, event| {
                // PROBLEM 167 — hand back every window PiP is still holding.
                //
                // PiP pins its window above everything and shrinks it to a
                // quarter screen, and the ONLY control that released it was the
                // 5th tap of the same shortcut. Quit Spaceadom before that tap
                // and the window stayed pinned and small with nothing left that
                // could undo it — the owner's "loses its title bar and won't
                // come back", and (because a stranded topmost window sits over
                // the overlay) also why the Guide HUD started appearing behind
                // ordinary apps.
                //
                // Both Exit and ExitRequested, deliberately: ExitRequested is
                // the one that fires for a tray Quit and a WM_CLOSE, Exit is
                // the last word before the process goes. `restore_all` clears
                // its own map, so running twice restores nothing twice.
                if matches!(event, tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit) {
                    engine::actions::pip::restore_all();
                    // PROBLEM 253 — an ORDERLY exit is not a startup crash.
                    //
                    // This covers the tray's "Exit Spaceadom", the updater
                    // exiting so its installer can replace the exe, and any
                    // other `app.exit()`. Quitting the app within thirty
                    // seconds of launching it is a completely ordinary thing to
                    // do, and doing it three times would otherwise disarm the
                    // user's shortcuts and tell them their app had crashed.
                    //
                    // The OTHER clean-exit path — `WM_ENDSESSION`, a sign-out
                    // or an installer's Restart Manager — never reaches here at
                    // all: `session_end.rs` calls `std::process::exit(0)` from
                    // inside its own handler, deliberately, so tao's runner is
                    // never told (PROBLEM 224). That one calls
                    // `note_clean_exit` from `session_end::teardown`. Two
                    // paths, because there genuinely are two exits.
                    //
                    // Idempotent: it writes one number, and both events firing
                    // writes it twice.
                    safe_mode::note_clean_exit();
                }
            })
        });

    // PROBLEM 89 — this used to be `.expect(...)`. A panic here is a SILENT
    // death for the user: no window, no tray icon, nothing on screen, and the
    // panic message goes to a stderr that a GUI app has no console for. The
    // friend's experience was "I double-clicked it and nothing happened" —
    // indistinguishable from the app never launching, and unreportable.
    //
    // The single most common cause is a missing/broken WebView2 runtime, so
    // the message names it and points at the log. This is the ONLY place the
    // app is allowed to show a message box: it runs when there is no UI left
    // to show anything else.
    if let Err(e) = result {
        log::error!("FATAL: Tauri failed to start: {e}");
        #[cfg(windows)]
        show_fatal_box(&e.to_string());
        std::process::exit(1);
    }
}

pub fn show_toast(app_handle: &tauri::AppHandle, msg: &str) {
    // Content only. The overlay PAGE owns toast lifecycle now: it renders the
    // stack, measures it, and calls `overlay_fit` to size/position/show the
    // window in one jump, then `overlay_toasts_done` when the stack empties.
    // The old flow sized the window to a hardcoded 440x88 BEFORE the content
    // existed — long messages clipped at the box edge and stacked toasts
    // overflowed (user report, 2026-08-10). Layout (measurement) runs fine in
    // a hidden webview; it is PAINTING that throttles, so measure-then-show
    // is safe where paint-then-show was not.
    //
    // GLOBAL broadcast — targeted emits never arrive (see
    // guide_hud/mod_impl.rs). Only the overlay page registers this listener.
    let _ = app_handle.emit("toast-notification", msg);

    // NOTE: the old code ALSO raised a Windows notification per keypress,
    // flooding the Action Center. Removed — CORE_AIM wants a clean overlay.
}
