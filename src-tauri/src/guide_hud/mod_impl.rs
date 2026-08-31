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
///     machine — an OPAQUE dark tool window, sized to content, shown
///     NoActivate on demand and hidden on release. The web content only draws
///     the inside of the panel; the WINDOW is the panel.
///     (Placement moved from "bottom-centre of the primary monitor" to
///     "centred on the monitor under the cursor" — PROBLEM 169.)
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, Manager};

static HUD_VISIBLE: AtomicBool = AtomicBool::new(false);
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// PROBLEM 177 — which Space-hold we are currently in.
///
/// THE BUG THIS KILLS. Showing the HUD is DEFERRED by `guide_hud_delay_ms`
/// (500ms on the owner's machine), so between the decision to show and the
/// show itself there is half a second in which the hold can end. The engine
/// guards that with a `tokio::watch` cancel channel — but the guard is
///
///     if !*cancel_rx.borrow() { ...read config...; show_guide_hud(...) }
///
/// and **the check is not atomic with the show**. Everything between them —
/// locking the engine state, locking the config, cloning the profile name and
/// building two Vecs of bindings — is time during which a cancel can arrive,
/// complete, and be forgotten. The show then proceeds anyway, and because the
/// cancel has ALREADY run, nothing is left that will ever hide it.
///
/// Caught live in the owner's log on 2026-08-24, 81ms apart:
///
///     23:41:07.793 guide_hud: hide with action pending - window stays up for the handover
///     23:41:07.874 guide_hud: overlay window shown      <-- the loser, winning
///
/// After that line the HUD stayed on screen with no further fit and no hide,
/// which is exactly what he reported: *"right now it is stuck. Like I'm not
/// holding the space but the space hud is still stuck."*
///
/// THE FIX IS A GENERATION COUNTER, not a tighter check. `begin_hold()` stamps
/// the hold that a deferred show belongs to; every cancel and every new hold
/// moves the counter on. A show whose stamp is stale refuses, and it refuses
/// INSIDE `show_guide_hud`, on the same line as the state it would have
/// changed — so there is no gap left to lose a race in. A narrower check would
/// only have made the window smaller; this removes it.
static HOLD_EPOCH: AtomicU64 = AtomicU64::new(0);

/// Begin a Space-hold and return its stamp. Pass that stamp to
/// `show_guide_hud`; it will refuse if the hold has ended in the meantime.
pub fn begin_hold() -> u64 {
    HOLD_EPOCH.fetch_add(1, Ordering::SeqCst) + 1
}

/// End the current hold, invalidating any deferred show still in flight.
pub fn end_hold() {
    HOLD_EPOCH.fetch_add(1, Ordering::SeqCst);
}

/// Which hold the currently-published `HUD_VISIBLE == true` belongs to.
///
/// Without this, a show that aborts late cannot tell ITS OWN published flag
/// from one a newer show has since published — and an unconditional
/// `HUD_VISIBLE.store(false)` on abort would clobber the newer show's flag,
/// recreating the exact bug in a new disguise.
static VISIBLE_EPOCH: AtomicU64 = AtomicU64::new(0);

/// A show completed and no hide has consumed it yet.
///
/// This is the ONLY thing the reconciliation path may test before deciding
/// whether to touch Tauri, because every Tauri window getter blocks on the main
/// event loop and that path runs on the hook thread and on every typed space.
/// Set after `win.show()`; cleared by any hide, normal or reconciling.
static SHOW_OUTSTANDING: AtomicBool = AtomicBool::new(false);

/// Has this show been overtaken? If so, undo whatever it has already done.
///
/// Returns true when the caller must stop. Safe to call repeatedly.
fn abort_if_stale(epoch: u64, win: Option<&tauri::WebviewWindow>, at: &str) -> bool {
    let current = HOLD_EPOCH.load(Ordering::SeqCst);
    if current == epoch {
        return false;
    }
    log::warn!(
        "guide_hud: hold #{epoch} was overtaken by #{current} ({at}) — standing down. \
         Left unchecked this is what puts a window on screen that no later hide can \
         reach, because every hide path is gated on HUD_VISIBLE (PROBLEM 177)."
    );
    // Only clear the flag if it is still OURS. A newer show may already have
    // published its own, and stamping on that is how a fix becomes the bug.
    if VISIBLE_EPOCH.load(Ordering::SeqCst) == epoch {
        HUD_VISIBLE.store(false, Ordering::Relaxed);
        // Hide the window ONLY if WE showed it and no hide has since taken
        // responsibility — i.e. `SHOW_OUTSTANDING` is still set (it is set
        // right after our `win.show()` and consumed by every hide path).
        //
        // NOT unconditional, and the difference is a real bug caught on
        // re-review: at the "before show" checkpoint this invocation has shown
        // nothing yet, but the window may legitimately be up from a PREVIOUS
        // hold's action-pending handover with a toast in flight. Hiding it
        // there would take the window down under a toast — PROBLEM 135's exact
        // class, reintroduced by the code meant to prevent its cousin. The
        // swap also means a cancel that already handled the window (its swap
        // of HUD_VISIBLE succeeded and it chose hide-or-keep deliberately)
        // is never second-guessed from here.
        //
        // NO `is_visible()` probe — that getter blocks on the main event loop
        // and this can run on the hook thread. `hide()` on an already-hidden
        // window is harmless.
        if SHOW_OUTSTANDING.swap(false, Ordering::SeqCst) {
            if let Some(w) = win {
                let _ = w.hide();
                log::info!("guide_hud: hid the window this stale show had put up");
            }
        }
    }
    true
}

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

/// Clone of the app handle for callers OUTSIDE the engine — e.g. the
/// fullscreen watcher thread raising a PiP-release toast (pip.rs §7).
/// `AppHandle` is Clone + Send + Sync, and a second `OnceLock` elsewhere
/// would just be a copy of this one that could drift — so this module, which
/// already owns the static the hook thread's watchdog relies on, hands it
/// out. `None` only in the brief setup window before `set_app_handle` has
/// run; callers skip quietly, matching the defensive `let Some(handle) =
/// APP_HANDLE.get() else { return }` style used everywhere else here.
pub fn app_handle() -> Option<AppHandle> {
    APP_HANDLE.get().cloned()
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

/// Size and place the overlay window CENTRED on the monitor under the cursor.
///
/// V13 placed this bottom-centre because the HUD was a bottom-anchored
/// rectangular panel. The V14 HUD is a radial bloom centred on screen, and
/// the frontend re-sizes it via `overlay_fit_hud` milliseconds after show.
/// Without centring HERE too, the window appears bottom-anchored for one
/// frame and then visibly jumps to the middle.
///
/// PROBLEM 169 — this was primary-monitor-only, by the owner's explicit
/// decision of 2026-08-10, and he reversed it on 2026-08-24 after reporting
/// the HUD failures were "worse with two displays". See `overlay_monitor` for
/// why, and for why the silent do-nothing below was a bug in its own right:
/// `primary_monitor()` returns None during a display change, and this function
/// used to return having positioned nothing while the caller went on to
/// `show()` anyway — so the ring painted into whatever box the last toast had
/// left behind.
fn place_overlay_centred(win: &tauri::WebviewWindow, w: f64, h: f64) {
    let Some(mon) = crate::commands::overlay_monitor(win) else {
        log::warn!(
            "guide_hud: no monitor resolved — showing the HUD at its previous size and              position rather than not at all. It may look clipped until the display settles."
        );
        return;
    };
    let sf = mon.scale_factor();
    let ms = mon.size().to_logical::<f64>(sf);
    let mp = mon.position().to_logical::<f64>(sf);
    let x = mp.x + (ms.width - w) / 2.0;
    let y = mp.y + (ms.height - h) / 2.0;
    let _ = win.set_size(tauri::LogicalSize::new(w, h));
    let _ = win.set_position(tauri::LogicalPosition::new(x, y));
}

/// Show the Guide HUD — content via event, visibility via the window itself.
pub fn show_guide_hud(
    epoch: u64,
    profile_name: &str,
    apps: Vec<(String, String)>,
    specials: Vec<(String, String)>,
) {
    // PROBLEM 177 — the hold this show was scheduled for is over. Refuse.
    //
    // Checked HERE, not at the call site: the call site cannot be atomic with
    // the state changes below, and every gap between a check and the work it
    // guards is a race waiting to be lost. See HOLD_EPOCH.
    let current = HOLD_EPOCH.load(Ordering::SeqCst);
    if current != epoch {
        log::info!(
            "guide_hud: a deferred show for hold #{epoch} arrived after that hold had ended \
             (now #{current}) — NOT showing. Unchecked, this is what strands the HUD on \
             screen with nothing left able to hide it (PROBLEM 177)."
        );
        return;
    }
    let Some(handle) = APP_HANDLE.get() else { return };

    let payload = GuideHudPayload {
        profile: profile_name.to_string(),
        apps,
        specials,
    };

    // Mark the HUD live BEFORE the window work and the emit, not after.
    // `overlay_fit_hud` refuses to place the window unless `is_visible()` is
    // already true, and that call is made by the page as soon as it has laid
    // the ring out — so publishing the flag last leaves a window in which the
    // frontend's fit is rejected and the ring paints into an unsized box.
    // The comment above ended "Nothing reads a false-positive badly:
    // hide_guide_hud_pending clears it unconditionally." **That sentence is
    // the 1.0.73 bug, in plain sight.** It reasoned about a false POSITIVE and
    // never about a LOST UPDATE. A hide landing in the ~80ms of window work
    // below consumes a flag belonging to a show that has not shown anything
    // yet; the show then puts the window up regardless, and the terminal state
    // is `window visible, HUD_VISIBLE == false` — from which nothing in Rust
    // can recover, because every hide path is gated on that flag.
    //
    // Under 1.0.72's ordering (store LAST) the same race ended with the flag
    // TRUE, which is self-correcting: the next release hid the window. So this
    // change did not create a race, it **inverted a recoverable one into an
    // unrecoverable one**. Measured on the owner's machine, 2026-08-24:
    //
    //     23:41:07.793 guide_hud: hide with action pending
    //     23:41:07.874 guide_hud: overlay window shown     <- 81ms later
    //     (then 3m36s with zero fits, zero hides, a HUD stuck on screen)
    //
    // VISIBLE_EPOCH records WHICH hold the published `true` belongs to, so a
    // late-aborting show can tell its own flag from a newer show's and clear
    // only its own. Stored first, and with SeqCst, so no reader can observe
    // `HUD_VISIBLE == true` paired with a stale epoch.
    VISIBLE_EPOCH.store(epoch, Ordering::SeqCst);
    HUD_VISIBLE.store(true, Ordering::Relaxed);

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
            //
            // PROBLEM 168 — this line USED to be `win.set_always_on_top(true)`,
            // which cannot do what the comment above says: tao diffs the flag
            // against its cache and returns without calling SetWindowPos when
            // it has not changed, and it never changes because the overlay is
            // created always-on-top. Three years of "re-assert topmost" that
            // asserted nothing. Go straight to Win32.
            crate::commands::raise_overlay_topmost(&win);
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

            // THE LOAD-BEARING CHECK. Everything above — placing, re-regioning,
            // re-raising, sampling the desktop — takes real time (81ms
            // measured), and the check at the top of this function happened
            // BEFORE all of it. Re-asking here is what actually stops a show
            // that has been overtaken from putting a window on screen that
            // nothing will ever take down.
            if !abort_if_stale(epoch, Some(&win), "before show") {
                let _ = win.show();
                SHOW_OUTSTANDING.store(true, Ordering::SeqCst);
                log::info!("guide_hud: overlay window shown (hold #{epoch})");
            } else {
                return;
            }
        } else if !crate::windows_created() {
            // PROBLEM 215 — the autostart settle window. The overlay is not
            // missing; it has not been built yet, and that is deliberate: the
            // hook comes up ten seconds ahead of WebView2 on purpose. The
            // shortcut itself still works, only the drawing is absent. Say so
            // calmly — an ERROR here would train the owner to ignore the line
            // that means something.
            log::info!(
                "guide_hud: still starting — the overlay webview is not built yet \
                 (autostart settle, PROBLEM 59/76/215). The shortcut works; no HUD is \
                 drawn for this hold."
            );
        } else {
            // PROBLEM 214 — the window is gone while the flag still says the
            // overlay is fine. Nothing on this side of the app can rebuild it,
            // so tell the display watcher to drop its backoff and heal on its
            // next poll. The user must never have to restart for this.
            log::error!(
                "guide_hud: the overlay window does not exist — the HUD cannot be shown. \
                 Asking the display watcher to rebuild it now; no restart is required \
                 (PROBLEM 214)."
            );
            crate::display_watch::heal_now();
        }
    } else {
        // PROBLEM 214 — THIS is the state the owner reported as "shortcuts work,
        // the HUD and the sound are dead". The overlay window itself was
        // healthy; a second, racing rebuild had set this flag on its way out.
        // Do not return silently: say so, and ask for a heal.
        // PROBLEM 217 — this fires on EVERY hold while the flag is set, which
        // is the worst possible shape for an unbounded reporting path: one
        // event per shortcut press. The target takes it off the automatic
        // bridge; `report_degraded` sends a rate-limited one instead. debug.log
        // still gets every line, at ERROR, unchanged.
        log::error!(
            target: crate::telemetry::DEGRADED_TARGET,
            "guide_hud: OVERLAY_DISABLED is set, so the HUD and every sound are suppressed \
             (the sound kit is WebAudio inside the overlay page, so it dies with it). \
             Asking the display watcher to rebuild and re-enable the overlay; no restart \
             is required (PROBLEM 214)."
        );
        crate::telemetry::report_degraded(
            crate::telemetry::Degraded::OverlayDisabled,
            "OVERLAY_DISABLED was set when a HUD was requested — the HUD and every sound \
             are suppressed; a rebuild has been asked for",
        );
        crate::display_watch::heal_now();
    }

    // And again before the content emit: the page treats `guide-hud-show` as
    // proof the HUD is live and sets `_hudActive`, which gates its ONLY route
    // back to `overlay_toasts_done`. Emitting for a hold that has ended is how
    // the page ends up permanently believing a HUD is up that is not.
    if abort_if_stale(epoch, handle.get_webview_window("overlay").as_ref(), "before emit") {
        return;
    }
    // PROBLEM 206 — record which key each chip will launch, in the SAME order
    // the page builds its chips from this payload's `apps`. Geometry arrives
    // separately (the page calls `publish_hud_chips` after layout); until it
    // does, the zeroed geometry count keeps pointer activation inert.
    crate::hook::pointer::publish_keys(&payload.apps);
    // GLOBAL broadcast, on purpose. Targeted emits (emit_to) silently never
    // reached this page's listeners regardless of how they were registered —
    // Tauri 2's target matching is stricter than it looks (Labeled vs
    // WebviewWindow are different kinds). Broadcast is safe because the
    // overlay page is the ONLY window that registers these listeners; the
    // dashboard deliberately does not (main.ts step 9).
    if let Err(e) = handle.emit("guide-hud-show", payload) {
        log::warn!("guide_hud: emit failed: {e}");
    }
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
    // PROBLEM 177 — invalidate any deferred show still in flight, whether or
    // not the HUD is currently up.
    //
    // This MUST sit OUTSIDE the HUD_VISIBLE guard below. The race being closed
    // is exactly the one where the show has not happened yet — so HUD_VISIBLE
    // is still false, the guard skips its whole body, and putting the
    // invalidation inside it would do nothing in the only case that matters.
    end_hold();
    // PROBLEM 206 — a hidden HUD has nothing to point at. Cleared on every
    // hide path (two relaxed stores, cheap enough for the typed-space path);
    // the `st-hud-pointer` poller also gates on `is_visible()`, so this is
    // the second lock on the same door.
    crate::hook::pointer::clear_chips();
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
        // Consumed by the normal path — nothing left to reconcile.
        SHOW_OUTSTANDING.store(false, Ordering::SeqCst);
        return;
    }

    // RECONCILIATION — the swap said the HUD is down, but is the WINDOW?
    //
    // This is the recovery path for the state section 1 of the 2026-08-24
    // post-mortem describes: `HUD_VISIBLE == false` while the overlay window
    // is up and the page still believes `_hudActive`. The epoch checks in
    // `show_guide_hud` should now prevent that state arising at all — this
    // exists because "should" is not "does", and the failure mode is a HUD
    // stuck on the owner's screen for minutes with nothing able to clear it.
    //
    // GATED ON AN ATOMIC, AND NOTHING ELSE, BEFORE TOUCHING TAURI AT ALL.
    //
    // The first version of this block opened with `win.is_visible()`. That is a
    // **blocking** call: in tauri-runtime-wry it is `window_getter!` → a
    // `rx.recv()` with NO timeout, parked until the main event loop next turns.
    // And this function is reached from `cancel_hud` on every SpaceUp, every
    // KeyCombo and both wheel directions — i.e. **on every ordinary space the
    // owner types** — while holding the `EngineState` lock, and again from the
    // watchdog ON THE HOOK THREAD. Parking the hook thread on the UI event loop
    // is the precise mechanism this whole investigation is about; shipping it
    // would have made the eviction worse in the name of fixing its symptom.
    //
    // `SHOW_OUTSTANDING` is not a mirror of window visibility — a mirror would
    // go stale the moment `display_watch` destroys and rebuilds the overlay,
    // which is routine on this machine. It records the DISAGREEMENT itself: a
    // show that completed and that no hide has since consumed. On every normal
    // keystroke it is false and this costs one relaxed load.
    if !SHOW_OUTSTANDING.swap(false, Ordering::SeqCst) {
        return;
    }
    //
    // Note this deliberately does NOT test `action_pending`: in the observed
    // failure the swap returned false AND action_pending was true, so any
    // condition mentioning it would have skipped the one case that mattered.
    let Some(handle) = APP_HANDLE.get() else { return };
    let Some(win) = handle.get_webview_window("overlay") else { return };
    log::warn!(
        "guide_hud: RECONCILING — the overlay window is visible but HUD_VISIBLE was already \
         false, so Rust and the page disagree about who owns it. Forcing the hide the normal \
         path could not reach (action_pending={action_pending})."
    );
    if !action_pending {
        let _ = win.hide();
    }
    // The page's `_hudActive` is set by `guide-hud-show` and cleared ONLY by
    // this event. Without it the page keeps refusing every window fit and
    // never calls `overlay_toasts_done`, which is what made the stall outlast
    // nine PiP actions and a launched app.
    let _ = handle.emit("guide-hud-hide", action_pending);
}

/// Returns true if the HUD is currently displayed.
pub fn is_visible() -> bool {
    HUD_VISIBLE.load(Ordering::Relaxed)
}
