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

mod browser;
mod commands;
/// PROBLEM 131 — breadcrumbs read by the panic hook.
mod crash_context;
mod config;
mod display_watch;
mod rival_install;
mod engine;
mod guide_hud;
mod hook;
mod icon_extractor;
mod logger;
mod startup;
mod tray;

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
                log::warn!(
                    "setup: dashboard_ready never arrived after 10s — showing the \
                     window anyway (frontend wedged or webview dead; PROBLEM 74)"
                );
                let app3 = app2.clone();
                let _ = app2.run_on_main_thread(move || {
                    use tauri::Manager;
                    if let Some(w) = app3.get_webview_window("settings") {
                        ensure_on_screen(&w); // PROBLEM 83
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                });
            })
            .ok();
    }
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
            log::info!("overlay: configured (on-demand, click-through)");
        }
        Err(e) => {
            log::error!(
                "overlay: click-through FAILED ({e}) — overlay disabled; \
                 HUD/toasts will not be shown"
            );
            guide_hud::OVERLAY_DISABLED.store(true, std::sync::atomic::Ordering::Relaxed);
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

    // ----------------------------------------------------------------
    // 2. Logger (must come before any log:: calls)
    // ----------------------------------------------------------------
    let data_dir = startup::data_dir();
    logger::init(&data_dir);
    log::info!("Spaceadom starting — data dir: {}", data_dir.display());

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

    // 2a. PROBLEM 64 + 59 — the HKCU Run autostart path. A Run value cannot
    // express the Scheduled Task's `/DELAY 0000:30`, so when the app was
    // started BY that Run value (`--autostart`) the cold-boot wait happens
    // here instead: launching at logon races Edge/WebView2's brokers, the GPU
    // stack and the disk, and losing that race is PROBLEM 59's dead-app.
    // A manual launch never carries the flag and is not delayed.
    // PROBLEM 76 — 10s, not 30s. At a REAL logon the Run key already fires
    // ~1–2 minutes after power-on (Windows startup + sign-in + shell), and the
    // old 30s stacked ON TOP of that: the user opened the laptop, saw no tray
    // icon, pressed Space+key into a dead hook, and reasonably concluded the
    // app "didn't start" (observed at a real reboot, log-verified: boot
    // 14:20:48 → Run key fired 14:22:31 → hook live 14:23:01). The blanket
    // sleep predates the two real defences we now have — the webview-existence
    // check + rebuild (PROBLEM 59) and the dashboard_ready beacon (PROBLEM 74)
    // — so it no longer needs to carry the whole cold-boot risk by itself.
    if std::env::args().any(|a| a == "--autostart") {
        log::info!("autostart launch — waiting 10s for the shell to settle (PROBLEM 59/76)");
        std::thread::sleep(std::time::Duration::from_secs(10));
    }

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
        log::error!(
            "PANIC on thread '{thread}' at {loc}: {msg}. This is a crash, not a handled \
             error — please report it with the lines above from debug.log."
        );

        // PROBLEM 131 — what the app was DOING. A backtrace says which code was
        // on the stack; this says the overlay had been rebuilt twice and a
        // display changed 4 seconds ago, which is usually what identifies the
        // trigger for a crash that only happens on someone else's machine.
        log::error!("{}", crash_context::snapshot());

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
        log::error!(
            "backtrace:\n{}",
            std::backtrace::Backtrace::force_capture()
        );

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
    let shared_config = config::load_or_init();

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
            commands::reset_config,
            commands::clear_active_profile,
            commands::restore_preset_profiles,
            commands::undo_last_change,
            commands::undo_available,
            commands::set_overlay_compositing,
            commands::restart_elevated,
            commands::overlay_ready,
            commands::overlay_log,
            commands::overlay_fit,
            commands::overlay_fit_hud,
            commands::overlay_fit_handover,
            commands::overlay_shape,
            commands::overlay_toasts_done,
            commands::find_browser_cmd,
            commands::validate_browser,
            commands::show_conflict_check,
            commands::create_profile,
            commands::delete_profile,
            commands::rename_profile,
            commands::list_start_menu_apps,
            commands::toggle_bypass,
            commands::open_log_folder,
            commands::frontend_log,
            commands::dashboard_ready,
            commands::get_stale_task,
            commands::repair_stale_task,
            commands::get_rival_install,
            commands::repair_rival_install,
            commands::get_conflicts,
            commands::close_conflict,
            commands::open_startup_manager,
            commands::reinstall_hook,
            commands::set_startup_enabled,
        ])
        // --- App setup callback ---
        .setup(move |app| {
            let app_handle = app.handle().clone();

            // 7. Spawn hook thread
            {
                let cfg = shared_config.read().unwrap_or_else(|p| p.into_inner());
                hook::spawn_hook_thread(hook_tx.clone(), cfg.rollover_ms);
                log::info!("setup: hook thread spawned");

                // 8. Start fullscreen watcher
                let fullscreen_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let flag_clone = Arc::clone(&fullscreen_flag);
                hook::fullscreen::start_fullscreen_watcher(
                    flag_clone,
                    cfg.fullscreen_allowlist.clone(),
                );

                // PROBLEM 88 — the 500ms "copier" thread that used to live
                // here is GONE. It was the only writer to
                // hook::FULLSCREEN_ACTIVE, so if the watcher thread died while
                // a game had the flag true, this loop re-stored `true` forever
                // and the hook passed EVERY key through — the whole app inert,
                // silently, until restart. start_fullscreen_watcher now writes
                // the flag directly (and fails toward "not fullscreen").
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

            // PROBLEM 117 — the overlay stops compositing when the display
            // arrangement changes underneath it, while every readback still
            // says visible=true. Watch for the change and rebuild. Started
            // AFTER set_app_handle so a rebuild can hide the HUD first.
            display_watch::start(app_handle.clone());

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

            // 10. Build system tray
            tray::build_tray(&app_handle)?;
            log::info!("setup: system tray built");

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
                            .center();
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

            log::info!("SpaceToggle OS fully initialised");
            println!("✅ Spaceadom initialised & ready.");
            // PROBLEM 89 — from here the user has a tray icon: a later panic
            // is visible-as-absence, so no modal box. Logged only.
            UI_READY.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        })
        .build(tauri::generate_context!())
        .map(|app| app.run(|_, _| {}));

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
