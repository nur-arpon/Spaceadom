/// tray.rs - System tray lifecycle management.
/// Tray icon is built 100% programmatically via TrayIconBuilder to avoid
/// the E_FAIL (0x80004005) COM timing issue that occurs when Tauri tries to
/// load icon assets from tauri.conf.json before Shell is ready on the UI thread.

use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

/// Build and register the system tray with its context menu fully at runtime.
/// Called from the `.setup()` closure after the Tauri runtime is fully alive.
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    // Build the context menu
    let show  = MenuItem::with_id(app, "open-settings", "Open Settings",       true, None::<&str>)?;
    let sep1  = PredefinedMenuItem::separator(app)?;
    let exit  = MenuItem::with_id(app, "exit",          "Exit Spaceadom",      true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show, &sep1, &exit])?;

    // Load icon with compile-time embed as guaranteed fallback
    let icon_bytes: &[u8] = include_bytes!("../icons/32x32.png");
    let icon = Image::from_bytes(icon_bytes)
        .expect("32x32.png compile-time embed failed");

    // Build tray entirely in Rust - no tauri.conf.json trayIcon block needed
    let tray = TrayIconBuilder::with_id("spacetoggle-tray")
        // PROBLEM 67 — the tray tooltip is the app's NAME to a user hunting for
        // it in the notification area. It must not say the old product name.
        .tooltip("Spaceadom — active")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            handle_menu_event(app, event.id().as_ref());
        })
        .on_tray_icon_event(|tray, event| {
            match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
                | TrayIconEvent::DoubleClick { .. } => {
                    restore_window(tray.app_handle());
                }
                _ => {}
            }
        })
        .build(app)?;

    // Leak the handle so the icon stays in the taskbar for the app lifetime
    std::mem::forget(tray);

    Ok(())
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "open-settings" => restore_window(app),
        "exit" => {
            log::info!("tray: exit requested");
            crate::hook::stop_hook();
            app.exit(0);
        }
        _ => {}
    }
}

fn restore_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("settings") {
        // PROBLEM 83 — the window may be positioned on a monitor that was
        // unplugged since it was last shown; showing it there is invisible.
        // ORDER MATTERS: GetWindowRect reports -32000,-32000 for a MINIMIZED
        // window, which is outside every monitor — checking first would
        // re-centre the window on every single restore-from-minimized.
        // Restore the geometry first, THEN judge it.
        win.unminimize().ok();
        crate::ensure_on_screen(&win);
        win.show().ok();
        win.set_focus().ok();
    }
}



/// Intercept the settings window close button to minimise to tray instead.
pub fn setup_close_to_tray(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("settings") {
        attach_close_to_tray(&win);
    }
}

/// PROBLEM 90 — attach close-to-tray to a SPECIFIC window object.
///
/// `on_window_event` binds to the window instance it is called on. The
/// PROBLEM 59 cold-boot recovery can REBUILD the settings window, producing a
/// new instance with no handler — and the default CloseRequested behaviour
/// closes the last window, which EXITS the app. So on a machine that hit the
/// cold-boot race, the user's first click on the X would kill Spaceadom
/// instead of hiding it to the tray, and the hook would die with it.
/// Must be called on every settings window that is ever created.
pub fn attach_close_to_tray(win: &tauri::WebviewWindow) {
    let win_clone = win.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            win_clone.hide().ok();
            log::debug!("tray: settings window hidden (minimised to tray)");
        }
    });
}