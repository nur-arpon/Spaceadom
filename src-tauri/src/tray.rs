/// tray.rs - System tray lifecycle management.
/// Tray icon is built 100% programmatically via TrayIconBuilder to avoid
/// the E_FAIL (0x80004005) COM timing issue that occurs when Tauri tries to
/// load icon assets from tauri.conf.json before Shell is ready on the UI thread.

use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Listener, Manager,
};

/// The Pause/Resume item, kept so its LABEL can follow the engine state
/// (owner, 2026-09-01).
///
/// A tray item that says "Pause Spaceadom" while the engine is already paused
/// is worse than no tray item: the one place a user looks to find out whether
/// the app is on would be telling them the opposite. So the label is re-written
/// on every state change, from whichever of the THREE entry points caused it —
/// this menu, the Settings row, and the Space+Backslash special key — and they
/// all meet at the `bypass-toggled` event (see `wire_engine_state_listener`).
static ENGINE_ITEM: std::sync::OnceLock<MenuItem<tauri::Wry>> = std::sync::OnceLock::new();

/// The label for each state, in one place so the two writers cannot disagree.
/// Plain-spoken, matching the panel's "Off means Space is just a space again."
fn engine_item_label(paused: bool) -> &'static str {
    if paused { "Resume Spaceadom" } else { "Pause Spaceadom" }
}

/// PROBLEM 249 — the "Check for updates" item, kept for exactly the same reason
/// `ENGINE_ITEM` is: its LABEL is the only feedback this menu can give.
///
/// A tray menu has no progress bar, no toast and no place to put a sentence, so
/// the item reports on itself: "Checking…" while it works, then
/// "You're on the latest" for five seconds, then back to its normal label. It
/// follows `ENGINE_ITEM`'s pattern precisely — driven by an EVENT
/// (`update-status`), not by the click handler — so a check started from the
/// dashboard's own button relabels this menu too, and the two can never
/// disagree about what the app is doing.
static UPDATE_ITEM: std::sync::OnceLock<MenuItem<tauri::Wry>> = std::sync::OnceLock::new();

/// The item's resting label.
const UPDATE_ITEM_IDLE: &str = "Check for updates";
/// How long the outcome stays on the menu before it goes back to idle.
const UPDATE_LABEL_HOLD: std::time::Duration = std::time::Duration::from_secs(5);

/// Whose turn it is to reset the label. Every status bumps it; a five-second
/// timer only resets the label if the generation it captured is still current.
///
/// Without this, two checks five seconds apart race: the first one's timer
/// fires in the middle of the second one and rewrites "Checking…" back to
/// "Check for updates" while the download is still running — the menu lying
/// about the app's state, which is the exact bug the Pause/Resume label was
/// built to avoid.
static LABEL_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// **What the menu says for a given status, and for how long.** Pure, so the
/// wording and the timing can be asserted without a tray, a menu or a network.
///
/// `None` = leave the label exactly as it is. That is the answer for the
/// download and install states: they end in the process exiting, and a menu
/// item that says "Downloading 45%" would freeze at whatever number it last
/// showed and stay there until the app came back.
pub(crate) fn update_item_label(state: &str) -> Option<(&'static str, bool)> {
    match state {
        // (label, is_transient — a transient label reverts after the hold)
        "checking" => Some(("Checking…", false)),
        "up_to_date" => Some(("You're on the latest", true)),
        "busy" => Some(("Already checking…", true)),
        "error" => Some(("Couldn't check — try later", true)),
        "not_eligible" => Some(("Updates aren't available here", true)),
        _ => None,
    }
}

/// Build and register the system tray with its context menu fully at runtime.
/// Called from the `.setup()` closure after the Tauri runtime is fully alive.
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    // Build the context menu
    let show  = MenuItem::with_id(app, "open-settings", "Open Settings",       true, None::<&str>)?;
    // The engine switch, second because it is the one a user reaches for when
    // something else needs the spacebar for a minute — above the separator with
    // "Open Settings", well away from Exit.
    //
    // Its starting label is read from the live atomic rather than assumed to be
    // "not paused": the tray is built inside `setup()`, and the engine's own
    // state is already whatever this process has made it.
    let paused_now = crate::hook::BYPASS_MODE.load(std::sync::atomic::Ordering::Relaxed);
    let engine = MenuItem::with_id(
        app, "toggle-engine", engine_item_label(paused_now), true, None::<&str>,
    )?;
    // PROBLEM 249 — third, under the engine switch and still above the
    // separator that keeps everything well away from Exit. A user who wonders
    // "am I up to date?" has nowhere else to look: there is no About box, and
    // the version only appears inside the Settings panel.
    let update = MenuItem::with_id(
        app, "check-updates", UPDATE_ITEM_IDLE, true, None::<&str>,
    )?;
    // PROBLEM 253 — a way to report a problem that does not require finding
    // the Settings panel first. Under "Check for updates" and above the
    // separator, well away from Exit.
    let report = MenuItem::with_id(
        app, "report-problem", "Report a problem", true, None::<&str>,
    )?;
    let sep1  = PredefinedMenuItem::separator(app)?;
    let exit  = MenuItem::with_id(app, "exit",          "Exit Spaceadom",      true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show, &engine, &update, &report, &sep1, &exit])?;

    // Held for the lifetime of the app so the label can be updated. `set` fails
    // only if the tray were built twice, which `build_tray`'s single call site
    // makes impossible — and if it ever were, the FIRST item is the one in the
    // menu that is actually on screen, so keeping it is the right choice.
    let _ = ENGINE_ITEM.set(engine);
    let _ = UPDATE_ITEM.set(update);
    wire_engine_state_listener(app);
    wire_update_state_listener(app);

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

/// Keep the tray's label honest when the engine is flipped from ANYWHERE else.
///
/// `bypass-toggled` is the event this app already publishes for exactly this —
/// `main.ts` listens to it and drives the Settings row's switch from it — so the
/// tray joins the same path rather than inventing a second one. A Rust-side
/// `listen` receives events emitted with `Emitter::emit`, which is what all
/// three writers use.
///
/// The listener is what makes "keep both in sync" true in BOTH directions: this
/// menu's own click emits (so the dashboard follows it), and the dashboard's
/// `toggle_bypass` emits (so this menu follows the dashboard).
fn wire_engine_state_listener(app: &AppHandle) {
    app.listen("bypass-toggled", |event| {
        // The payload is a bare JSON boolean. Parsed, not string-matched: a
        // future payload change would break loudly here rather than silently
        // leave the label stuck on one state.
        let paused = match serde_json::from_str::<bool>(event.payload()) {
            Ok(v) => v,
            Err(e) => {
                log::warn!(
                    "tray: could not read the bypass-toggled payload {:?} ({e}) — the \
                     Pause/Resume label may now disagree with the engine",
                    event.payload()
                );
                return;
            }
        };
        set_engine_item_label(paused);
    });
}

/// Re-write the Pause/Resume label. A no-op before the tray exists.
fn set_engine_item_label(paused: bool) {
    if let Some(item) = ENGINE_ITEM.get() {
        if let Err(e) = item.set_text(engine_item_label(paused)) {
            log::warn!("tray: could not update the Pause/Resume label ({e})");
        }
    }
}

/// PROBLEM 249 — the same arrangement for "Check for updates".
///
/// The listener, not the click handler, is what writes the label. That is what
/// makes the menu right in BOTH directions: a check started from the dashboard
/// button relabels this item too, and this item's own click goes out through
/// the same `update-status` event everything else reads.
fn wire_update_state_listener(app: &AppHandle) {
    app.listen(crate::updater::EVENT_UPDATE_STATUS, |event| {
        // Parsed as a struct, not string-matched: a payload change breaks
        // loudly here rather than leaving the label stuck on one word.
        let status = match serde_json::from_str::<serde_json::Value>(event.payload()) {
            Ok(v) => v,
            Err(e) => {
                log::warn!(
                    "tray: could not read the {} payload ({e}) — the \"Check for updates\" \
                     label may now disagree with the updater",
                    crate::updater::EVENT_UPDATE_STATUS
                );
                return;
            }
        };
        let Some(state) = status.get("state").and_then(|s| s.as_str()) else {
            log::warn!("tray: an update-status payload arrived with no state field");
            return;
        };
        let Some((label, transient)) = update_item_label(state) else { return };
        let generation = LABEL_GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        set_update_item_label(label);
        if transient {
            revert_update_item_label_after_hold(generation);
        }
    });
}

fn set_update_item_label(label: &str) {
    if let Some(item) = UPDATE_ITEM.get() {
        if let Err(e) = item.set_text(label) {
            log::warn!("tray: could not update the \"Check for updates\" label ({e})");
        }
    }
}

/// Put the item back to `Check for updates` after the hold — but only if
/// nothing else has happened in the meantime. See `LABEL_GENERATION`.
///
/// Its own thread, because this runs on whatever thread delivered the event and
/// a five-second sleep on the main thread would freeze both webviews. A thread
/// that cannot be spawned leaves the outcome on the menu until the next check,
/// which is a stale label and not a wrong one — so it is logged and let go.
fn revert_update_item_label_after_hold(generation: u64) {
    let spawned = std::thread::Builder::new()
        .name("st-tray-label".into())
        .spawn(move || {
            std::thread::sleep(UPDATE_LABEL_HOLD);
            if LABEL_GENERATION.load(std::sync::atomic::Ordering::SeqCst) == generation {
                set_update_item_label(UPDATE_ITEM_IDLE);
            }
        });
    if spawned.is_err() {
        log::warn!(
            "tray: could not spawn st-tray-label — the \"Check for updates\" item keeps its \
             outcome label until the next check"
        );
    }
}

/// Flip the engine from the tray.
///
/// THE THREE THINGS THAT ARE NOT OPTIONAL, and one of them is a bug this app
/// has already had:
///
/// 1. **`MODIFIER_ACTIVE` is cleared when pausing.** The hook skips the SPACE_UP
///    event while bypassed, so pausing during a held Space would leave the
///    modifier latched forever — every subsequent keystroke read as a command.
///    `engine::handle_bypass_toggle` (the Space+Backslash path) has always done
///    this; `commands::toggle_bypass` (the Settings row) still does not, which
///    is recorded as a known gap rather than fixed from here, because that
///    function belongs to another lane this session.
/// 2. **The event is emitted**, so the dashboard's switch follows.
/// 3. **The label is updated**, so the menu the user just used tells the truth
///    the next time they open it. Done directly rather than left to the
///    listener above: the listener will also fire, `set_text` is idempotent, and
///    a tray whose label depended on an event round-trip would be stale for as
///    long as that trip took.
fn toggle_engine(app: &AppHandle) {
    use std::sync::atomic::Ordering;
    let paused = !crate::hook::BYPASS_MODE.load(Ordering::Relaxed);
    crate::hook::BYPASS_MODE.store(paused, Ordering::Relaxed);
    if paused {
        crate::hook::MODIFIER_ACTIVE.store(false, Ordering::Relaxed);
    }
    log::info!("tray: engine {} from the tray menu", if paused { "PAUSED" } else { "resumed" });
    set_engine_item_label(paused);
    let _ = app.emit("bypass-toggled", paused);
    crate::show_toast(app, if paused { "⏸ Spaceadom Paused" } else { "▶ Spaceadom Active" });
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "open-settings" => restore_window(app),
        "toggle-engine" => toggle_engine(app),
        // PROBLEM 249 — the SAME path the dashboard's button takes: same
        // `plan()`, same manifest, same installer routing, same rate limit.
        // It goes to its own thread because this callback is on the MAIN
        // thread and a check is a network round trip and possibly a 6 MB
        // download. The label is not touched here — the `update-status`
        // listener above does that, so the menu says the same thing no matter
        // which of the two entry points started the check.
        "check-updates" => {
            log::info!("tray: manual update check requested from the tray menu");
            crate::updater::manual_check_on_a_thread(app.clone());
        }
        // PROBLEM 253 — front the dashboard, then ask it for the report dialog.
        //
        // An EVENT and not a direct call to `diagnostics::build_bundle`, and
        // the difference matters: a bundle built straight from here would carry
        // an empty `description.txt`, and a report that does not say what went
        // wrong is barely a report. The dialog is where the user types that.
        //
        // `restore_window` first, and it is not optional — it is also what
        // CREATES the windows during an autostart launch's ten-second settle
        // (PROBLEM 215), so emitting without it can address a dashboard that
        // does not exist yet. Global `emit`, never `emit_to`: `emit_to` has
        // never worked in this project (CLAUDE.md, window rules).
        "report-problem" => {
            log::info!("tray: 'Report a problem' chosen — showing the dashboard and \
                        asking it to open the report dialog (PROBLEM 253)");
            restore_window(app);
            if let Err(e) = app.emit("open-report-dialog", ()) {
                log::warn!(
                    "tray: could not emit open-report-dialog ({e}) — the same dialog is \
                     still reachable from Settings ▸ About"
                );
            }
        }
        "exit" => {
            log::info!("tray: exit requested");
            crate::hook::stop_hook();
            app.exit(0);
        }
        _ => {}
    }
}

fn restore_window(app: &AppHandle) {
    // PROBLEM 215 — during an autostart launch the windows do not exist for
    // the first 10s. The user clicking "Open Settings" is a direct request for
    // the UI, which outranks waiting out the cold-boot settle: build it now.
    // `create_app_windows` is idempotent, so this is a no-op once they exist.
    if app.get_webview_window("settings").is_none() {
        log::info!("tray: the dashboard was asked for before the settle wait finished — creating the windows now (PROBLEM 215)");
        crate::create_app_windows(app);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engine_label_always_names_the_action_not_the_state() {
        assert_eq!(engine_item_label(true), "Resume Spaceadom");
        assert_eq!(engine_item_label(false), "Pause Spaceadom");
        assert_ne!(engine_item_label(true), engine_item_label(false));
    }

    /// The owner's requirement, verbatim: on `up_to_date` the item briefly
    /// relabels itself "You're on the latest" for about five seconds.
    #[test]
    fn an_up_to_date_check_says_so_on_the_menu_and_then_takes_it_back() {
        assert_eq!(update_item_label("up_to_date"), Some(("You're on the latest", true)));
        assert_eq!(UPDATE_LABEL_HOLD.as_secs(), 5);
        assert_eq!(UPDATE_ITEM_IDLE, "Check for updates");
    }

    /// "Checking…" is NOT transient: it must stay up for as long as the check
    /// takes. A five-second timer under it would put "Check for updates" back
    /// on the menu in the middle of a download.
    #[test]
    fn the_checking_label_stays_until_something_else_replaces_it() {
        assert_eq!(update_item_label("checking"), Some(("Checking…", false)));
    }

    /// A tray item cannot show progress and must not try. Both of these end in
    /// the process exiting, and a label frozen at "Downloading 45%" would still
    /// be there when the app came back.
    #[test]
    fn the_menu_never_tries_to_report_a_download_or_an_install() {
        assert_eq!(update_item_label("downloading"), None);
        assert_eq!(update_item_label("installing"), None);
        assert_eq!(update_item_label("something_a_later_version_invented"), None);
        assert_eq!(update_item_label(""), None);
    }

    /// Every label this menu can show is short enough for a tray menu and says
    /// something a person can act on.
    #[test]
    fn every_menu_label_is_short_and_plain() {
        for state in ["checking", "up_to_date", "busy", "error", "not_eligible"] {
            let (label, _) = update_item_label(state).expect(state);
            assert!(!label.is_empty(), "{state}");
            assert!(label.chars().count() <= 30, "too long for a tray menu: {label:?}");
        }
        // The two "nothing is wrong" outcomes must not read as failures.
        assert!(!update_item_label("busy").unwrap().0.to_lowercase().contains("error"));
        assert!(!update_item_label("up_to_date").unwrap().0.to_lowercase().contains("error"));
    }

    /// The tray reads the states the updater publishes. If either side renames
    /// one, this fails rather than the menu silently going quiet.
    #[test]
    fn the_menu_labels_are_keyed_on_the_updaters_own_state_constants() {
        use crate::updater::*;
        assert!(update_item_label(STATE_CHECKING).is_some());
        assert!(update_item_label(STATE_UP_TO_DATE).is_some());
        assert!(update_item_label(STATE_BUSY).is_some());
        assert!(update_item_label(STATE_ERROR).is_some());
        assert!(update_item_label(STATE_NOT_ELIGIBLE).is_some());
        assert!(update_item_label(STATE_DOWNLOADING).is_none());
        assert!(update_item_label(STATE_INSTALLING).is_none());
    }
}