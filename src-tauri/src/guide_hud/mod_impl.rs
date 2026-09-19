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

/// PROBLEM 267 — WHICH kind of HUD the published `HUD_VISIBLE == true` is
/// about: the Space ring (0) or the middle button's cursor-anchored icon ring
/// (1). One `AtomicU8`, written by each show beside `VISIBLE_EPOCH` and read
/// by `hide_guide_hud_pending`, whose only kind-specific act is WHICH event it
/// emits (`guide-hud-hide` vs `middle-ring-hide`) — the epoch discipline, the
/// `HUD_VISIBLE` swap, the chip teardown and the reconciliation path are all
/// shared, so PROBLEM 177's race can never be fixed for one ring and not the
/// other. The overlay page has one listener per event and each keeps its own
/// DOM, so an event for the wrong kind is a no-op there, not a mis-hide.
static HUD_KIND: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(HUD_KIND_SPACE);
const HUD_KIND_SPACE: u8 = 0;
const HUD_KIND_RING: u8 = 1;

/// PROBLEM 267 — is the icon ring (not the Space ring) the HUD on screen?
pub fn middle_ring_is_up() -> bool {
    HUD_VISIBLE.load(Ordering::Relaxed) && HUD_KIND.load(Ordering::Relaxed) == HUD_KIND_RING
}

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
    /// The active profile's emoji, rendered BESIDE the central SPACE pill.
    ///
    /// `None` — every config written before this field existed, and every
    /// profile the user has not given an emoji — must draw the pill EXACTLY as
    /// it drew before: the page adds no element at all in that case, so the
    /// wordmark's box, its centring and its `st-space-pop` are byte-identical.
    /// Built by `engine::profile_emoji_for`; see that function for the Lane C
    /// hand-over point.
    pub profile_emoji: Option<String>,
    pub apps: Vec<(String, String)>,
    /// PROBLEM 267 round 3 — one entry per `apps` row, in the same order: a
    /// complete `data:` URL for the app's icon (the SAME sources the icon
    /// ring uses — the binding's `site_icon` for a link, its `icon_override`,
    /// or the shell icon already in the picker's cache), or `None` for the
    /// letter disc. CACHE ONLY, never a fetch or a shell call at raise time:
    /// this rides the Space-hold latency path. Built by `engine::hud_icons_for`.
    pub app_icons: Vec<Option<String>>,
    pub specials: Vec<(String, String)>,
    /// 1.0.119 (brief 4 §2) — the FOCUSED APP for the centre pill: its short
    /// name and icon, resolved at hold start by `engine::focus`. `None` keeps
    /// the word SPACE (the desktop, the lock screen, our own windows, an
    /// unreadable process, a preview). The pill's box never changes.
    pub focus: Option<crate::engine::focus::HudFocus>,
    /// `Some` ONLY for a Settings preview (`commands::preview_hud_layout`).
    ///
    /// A real Space-hold sends `None` and the page then reads the user's own
    /// `hud_magnetic_layout` / `hud_band_count` exactly as it always has — so
    /// nothing about the shipped path changes shape. See `HudPreview`.
    pub preview: Option<HudPreview>,
}

/// A TRANSIENT layout override for one HUD show, and nothing else.
///
/// It is deliberately NOT a config write. The owner is choosing a layout in
/// Settings and wants to SEE it first; writing the config to show a preview
/// would mean a preview he cancels has already changed his HUD, and a crash
/// mid-preview would leave the wrong layout persisted. So the override travels
/// with the payload, lives for the ~4s the preview is on screen, and the page
/// falls straight back to the saved settings on the next real hold.
///
/// The two strings are the page's OWN vocabulary — `hud-layout.ts`'s
/// `HudLayout` and `hud-band-count.ts`'s `HudBandCount` — not the config's
/// (`hud_magnetic_layout` is a bool). Rust does the bool→name translation once,
/// here, for the same reason `hud-layout.ts` exists: nobody downstream should
/// be comparing a raw bool.
#[derive(serde::Serialize, Clone)]
pub struct HudPreview {
    /// `"magnetic"` | `"classic"`.
    pub layout: String,
    /// `"auto"` | `"one"` | `"two"` — APP bands only, as everywhere else.
    pub bands: String,
}

/// How long a preview stays on screen before it hides itself.
///
/// Long enough to read a 26-chip ring, short enough that the owner does not
/// reach for a way to dismiss it. It is a CEILING, not a guarantee: a real
/// Space-hold, a newer preview, or any ordinary hide supersedes it early
/// through the same epoch the rest of this file is built on.
const PREVIEW_MS: u64 = 4000;

/// Size and place the overlay window as the CANVAS of the monitor under the
/// cursor — PROBLEM 267 round 3: the whole work area (`commands::hud_canvas_for`),
/// the same rectangle `overlay_fit_hud` computes milliseconds later, so the
/// page's fit finds the window already there and the ring never jumps. The
/// Space ring's fixed stage is centred on that monitor's centre by the page.
///
/// PROBLEM 169 — this was primary-monitor-only, by the owner's explicit
/// decision of 2026-08-10, and he reversed it on 2026-08-24 after reporting
/// the HUD failures were "worse with two displays". See `overlay_monitor` for
/// why, and for why the silent do-nothing below was a bug in its own right:
/// `primary_monitor()` returns None during a display change, and this function
/// used to return having positioned nothing while the caller went on to
/// `show()` anyway — so the ring painted into whatever box the last toast had
/// left behind.
fn place_overlay_canvas(win: &tauri::WebviewWindow) {
    let Some(mon) = crate::commands::overlay_monitor(win) else {
        log::warn!(
            "guide_hud: no monitor resolved — showing the HUD at its previous size and              position rather than not at all. It may look clipped until the display settles."
        );
        return;
    };
    let (canvas, sf, _) = crate::commands::hud_canvas_for(&mon);
    crate::commands::overlay_fit_canvas(win, canvas, sf, "space ring, at show");
}

/// Show the Guide HUD for a real Space-hold — content via event, visibility
/// via the window itself.
///
/// A thin wrapper since 1.0.96: everything below the payload is shared with
/// `show_preview_hud`, and it is shared as ONE function rather than copied so
/// the PROBLEM 177 epoch checks, the topmost re-assert and the compositing
/// baseline can never exist in one path and not the other.
pub fn show_guide_hud(
    epoch: u64,
    profile_name: &str,
    profile_emoji: Option<String>,
    apps: Vec<(String, String)>,
    app_icons: Vec<Option<String>>,
    specials: Vec<(String, String)>,
    focus: Option<crate::engine::focus::HudFocus>,
) {
    show_hud_payload(
        epoch,
        GuideHudPayload {
            profile: profile_name.to_string(),
            profile_emoji,
            apps,
            app_icons,
            specials,
            focus,
            preview: None,
        },
    );
}

/// Show a Settings PREVIEW of the Guide HUD, and take it down again after
/// `PREVIEW_MS`.
///
/// THE PREVIEW IS A PROJECTION, NOT A HOLD. It draws the real ring with the
/// user's real bindings so the choice in Settings is made on the truth, but
/// nothing about it is armable:
///
///   * `show_hud_payload` does NOT call `pointer::publish_keys` for a preview,
///     and it CLEARS the chip tables instead. The page's `publishHudChips` is
///     suppressed on its side for the same show, so `CHIP_GEOM_COUNT` and
///     `CHIP_KEY_COUNT` both stay at zero and `sector_pick` has nothing to
///     return. Two independent locks on the same door, and neither of them is
///     in `hook/mod.rs` — the hook's own arming gate is untouched.
///   * `HoldTracker::tick` additionally requires `modifier_active`, which is
///     only true while Space is physically down. A preview is raised from a
///     mouse click in the dashboard, so that gate is already shut. This is the
///     belt to the two braces above, not the argument on its own: the owner
///     could be holding Space when the click lands, and the epoch below is
///     what makes that case correct rather than lucky.
///
/// EPOCH DISCIPLINE, PROBLEM 177, unchanged and reused rather than reinvented.
/// The caller stamps the preview with `begin_hold()`. A real Space-down calls
/// `begin_hold()` too and a release calls `end_hold()`, so either one moves the
/// counter past us — and then this preview's auto-hide refuses, exactly as a
/// stale deferred show refuses. A newer preview supersedes an older one by the
/// same single mechanism. There is no second timer to cancel and no flag that
/// can be left set.
pub fn show_preview_hud(epoch: u64, payload: GuideHudPayload) {
    let layout = payload
        .preview
        .as_ref()
        .map(|p| format!("{} / {} bands", p.layout, p.bands))
        .unwrap_or_else(|| "NONE — this is not a preview payload".to_string());
    log::info!(
        "guide_hud: PREVIEW #{epoch} — showing the real ring as {layout} for {PREVIEW_MS}ms \
         with {} app(s) and {} special(s). Pointer activation is inert for this show: no chip \
         keys and no chip rects are published, so there is nothing to arm.",
        payload.apps.len(),
        payload.specials.len(),
    );
    show_hud_payload(epoch, payload);

    // The auto-hide. Spawned, not slept-on: this runs from a Tauri command and
    // the dashboard must get its reply immediately.
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(PREVIEW_MS)).await;
        hide_preview_hud(epoch);
    });
}

/// Take a preview down — but ONLY if it is still the one on screen.
///
/// The whole point of the epoch check: by the time this fires, four seconds
/// later, the HUD may belong to a real Space-hold or to a newer preview.
/// Hiding then would take down somebody else's window, which is PROBLEM 177's
/// failure with the arrow pointing the other way.
pub fn hide_preview_hud(epoch: u64) {
    let current = HOLD_EPOCH.load(Ordering::SeqCst);
    if current != epoch {
        log::info!(
            "guide_hud: preview #{epoch}'s timeout fired after it had been superseded by \
             #{current} (a real Space-hold, or a newer preview) — NOT hiding. The owner of \
             the HUD now is the only thing allowed to take it down."
        );
        return;
    }
    log::info!("guide_hud: preview #{epoch} timed out — hiding");
    hide_guide_hud_pending(false);
}

/// The shared body of every show. `payload.preview` decides the two things
/// that differ, and nothing else does.
fn show_hud_payload(epoch: u64, payload: GuideHudPayload) {
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
    HUD_KIND.store(HUD_KIND_SPACE, Ordering::SeqCst);
    HUD_VISIBLE.store(true, Ordering::Relaxed);

    // Show the WINDOW first, then send the content. A hidden WebView2 window
    // throttles rendering; content emitted before show() painted nothing and
    // the panel came up as an empty dark box (2026-08-10).
    if !OVERLAY_DISABLED.load(Ordering::Relaxed) {
        if let Some(win) = handle.get_webview_window("overlay") {
            place_overlay_canvas(&win);
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
                // PROBLEM 243 — say WHICH window the ring just landed over.
                // Deliberately AFTER `show()`, so the query cannot delay the
                // one thing the owner is waiting to see, and deliberately not
                // a gate: nothing above this line has ever consulted the
                // foreground, and nothing below it may start.
                log_shown_over();
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
            //
            // PROBLEM 265 — AND THE HOLD NOW ASKS FOR THE OVERLAY TO BE BUILT.
            // A calm log line is the right SEVERITY and was the wrong ACTION:
            // the owner held Space four times in sixteen seconds at a logon,
            // saw nothing, and concluded the app was asleep. Asking for the app
            // is asking for its UI — the tray's "Open Settings" and the
            // single-instance handler have skipped the settle for that reason
            // since PROBLEM 215, and a Space hold is that same request aimed at
            // the overlay. This does NOT rescue the hold that made it (a webview
            // takes far longer than a hold to boot), and it is not meant to: it
            // is the difference between one hold paying a one-off cost and every
            // hold until the timer draws nothing. With the overlay's own settle
            // at ~1.2s this branch should be all but unreachable.
            log::info!(
                "guide_hud: still starting — the overlay webview is not built yet \
                 (autostart settle, PROBLEM 59/76/215). The shortcut works — this hold still \
                 launches, focuses and minimises, and Space+letter is unaffected; only the \
                 ring is missing. Asking for the overlay to be built now rather than waiting \
                 out the timer (PROBLEM 265)."
            );
            crate::overlay_boot::request_now(handle);
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
    //
    // NOT FOR A PREVIEW. The preview is a projection of a layout, not an
    // offer to launch anything, and the cheapest correct way to make it inert
    // is to give the pointer nothing to work with: no keys here, no rects from
    // the page (`publishHudChips` returns early on a preview show), so both
    // counts stay zero and `sector_pick` is never even reached. Cleared rather
    // than merely skipped, because a snapshot left over from the PREVIOUS real
    // hold would otherwise still be sitting in those tables — every hide path
    // clears them, but "every hide path" is a claim about other code and this
    // is the one line that makes it not matter.
    if payload.preview.is_none() {
        crate::hook::pointer::publish_keys(&payload.apps);
    } else {
        crate::hook::pointer::clear_chips();
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
}

/// PROBLEM 243 — the sentence the log prints once per show, naming the app the
/// ring has just been drawn over.
///
/// WHY THIS EXISTS AT ALL. The owner's requirement is that holding Space
/// **inside Spaceadom's own dashboard** raises the ring exactly as it does over
/// any other app. Answering "does it?" from the log used to mean joining two
/// numbers that describe different windows: a point-in-time
/// `overlay window shown (hold #N)` against a 60-second aggregate
/// (`saw N key event(s) … M of them while the Spaceadom window itself had
/// focus`). `docs/IF-SHORTCUTS-DIE-AGAIN.md` names that exact join as the trap
/// that nearly re-opened PROBLEM 230: **two numbers may only be compared when
/// they describe the same window.** One line, stamped at the moment of the
/// show, removes the join entirely.
///
/// PURE ON PURPOSE. The Win32 half is one `GetForegroundWindow` +
/// `QueryFullProcessImageNameW`; the DECISION — which of the three things to
/// say — is this function, so it can be tested without a desktop. Every input
/// below is one that really arrives: a stem from
/// `exclusions::foreground_stem` (already lowercased and extension-stripped),
/// our own stem from `exclusions::own_stem` (which never returns empty), and
/// the empty string that `foreground_stem` returns when the foreground window
/// has no readable process — a lock screen, a UAC prompt, or a window closing
/// underneath us.
///
/// GENERALISE: an instrument that answers a question about a MOMENT must be
/// read at that moment. Aggregates cannot be cross-examined afterwards.
pub(crate) fn shown_over_phrase(fg_stem: &str, own_stem: &str) -> String {
    let fg = fg_stem.trim();
    if fg.is_empty() {
        return "over a window whose process could not be read".to_string();
    }
    if !own_stem.trim().is_empty() && fg.eq_ignore_ascii_case(own_stem.trim()) {
        return "over own window".to_string();
    }
    format!("over {fg}.exe")
}

/// Read the foreground app and print the PROBLEM 243 line.
///
/// NOT on the hook callback — this runs on the Tauri async runtime, from
/// `show_hud_payload`, after `win.show()` has already returned. The keyboard
/// laws that ban `GetForegroundWindow` (PROBLEM 134/184) ban it *in the hook
/// callback*, where it contends on win32k with the foreground app's own UI
/// thread. Here the same call sits beside `SetWindowPos`, `SetWindowRgn` and a
/// screen-pixel sample that this function already makes.
fn log_shown_over() {
    #[cfg(windows)]
    {
        let own = crate::hook::exclusions::own_stem();
        let fg = unsafe { crate::hook::exclusions::foreground_stem() };
        log::info!(
            "guide_hud: shown {} — the ring is NOT gated on which app is in front. There is \
             no own-window check anywhere on this path (PROBLEM 243): not in the hook's \
             Space-down branch, not in the engine's SpaceDown arm, not here. If this line \
             says \"over own window\" the requirement is met for that hold; if the ring was \
             nonetheless invisible, suspect z-order or compositing, never a gate.",
            shown_over_phrase(&fg, &own)
        );
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
    // PROBLEM 267 — read the kind BEFORE the swap below, beside it: a show
    // that lands between the two would rewrite the kind for ITS ring, and the
    // hide event would then go to the wrong listener.
    let ring = HUD_KIND.load(Ordering::SeqCst) == HUD_KIND_RING;
    if HUD_VISIBLE.swap(false, Ordering::Relaxed) {
        if let Some(handle) = APP_HANDLE.get() {
            if action_pending {
                log::info!("guide_hud: hide with action pending - window stays up for the handover");
            } else if ring {
                // PROBLEM 267 — the icon ring's plain release does NOT hide
                // the window here. The design's exit (scale 1→.85, fade, 117
                // ms) is played by the page, and a window hidden on this line
                // would play it into nothing — PROBLEM 135's class, chosen
                // knowingly for the SPACE ring ("a plain release hides
                // immediately, exactly as before") and declined for this one.
                // The page calls `overlay_toasts_done` when its exit timer
                // ends (a bounded setTimeout, never an animation event that
                // can fail to fire), and THAT command is the single terminal
                // hide, logged there. A new hold inside those ~120 ms simply
                // re-shows; the late `overlay_toasts_done` is then refused by
                // its own `is_visible()` gate.
                log::info!(
                    "guide_hud: icon-ring hide requested (no action pending) — the window stays \
                     up for the page's exit; overlay_toasts_done is the terminal hide (PROBLEM 267)"
                );
            } else if let Some(win) = handle.get_webview_window("overlay") {
                // Say so. This hide was silent, and a silent window hide cost
                // three diagnostic rounds (PROBLEM 135) - the same lesson the
                // window rules already record for fits.
                log::info!("guide_hud: overlay window hidden (no action pending)");
                let _ = win.hide();
            }
            let _ = handle.emit(
                if ring { "middle-ring-hide" } else { "guide-hud-hide" },
                action_pending,
            );
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
    let _ = handle.emit(
        if ring { "middle-ring-hide" } else { "guide-hud-hide" },
        action_pending,
    );
}

/// Returns true if the HUD is currently displayed.
pub fn is_visible() -> bool {
    HUD_VISIBLE.load(Ordering::Relaxed)
}

/* ===========================================================================
   PROBLEM 267 — THE MIDDLE BUTTON'S CURSOR-ANCHORED ICON RING

   A second HUD kind, shown through the same window, the same epoch, the same
   `HUD_VISIBLE` and the same hide paths as the Space ring — only the
   PLACEMENT and the PAYLOAD differ. Placement is the whole reason it cannot
   share `show_hud_payload`: that function lets the page measure the Space
   ring and fit the window afterwards; this one lays the ring out in Rust
   (`middle_ring::choose_shape` / `layout_ring_slots`), makes the window ONE
   BIG CANVAS — the work area of the monitor the CURSOR is on
   (`middle_ring::canvas_rect`, round 3) — and, for "All", clamps the centre
   on-screen and warps the OS cursor to it. None of that can happen after the
   page has drawn (the page draws ONCE, from the payload, with no IPC while
   the ring is up).
   =========================================================================== */

/// Where the ring landed, for the marker line and for the poller.
struct RingPlacement {
    /// Centre in PHYSICAL px (the poller's unit).
    cx: i32,
    cy: i32,
    /// How far the clamp moved it from the cursor, physical px.
    dx: i32,
    dy: i32,
    scale: f64,
    /// The canvas the overlay window was fitted to, physical px.
    canvas: crate::middle_ring::WorkArea,
    /// The room the LAYOUT was measured against, physical px: the canvas,
    /// less any auto-hidden appbar band (2026-09-15). It is the canvas
    /// itself whenever no such bar is docked on this monitor — printing it
    /// is how a clipped tile and a mis-measured room stay distinguishable.
    room: crate::middle_ring::WorkArea,
    /// The ring centre in the canvas page's CSS px.
    page: (f64, f64),
    /// Which shape was chosen at the press point (owner decision 2026-09-13).
    shape: crate::middle_ring::RingShape,
    /// The monitor's position, for the log.
    monitor: (i32, i32),
}

/// Show the icon ring for hold `epoch`, anchored at `cursor_phys` (the cursor
/// as it was at the press, in physical px — captured OFF the hook callback by
/// `engine::dispatch` through `GetCursorPos`).
///
/// Order, and every line is where it is for a reason already paid for
/// elsewhere in this file: stale-epoch check → mark visible (kind = ring) →
/// canvas + region + topmost → stale check → show → warp the cursor ("All"
/// only) → publish the hit table → stale check → emit. The warp comes AFTER
/// `show()` so the user never sees the cursor jump to a place where nothing
/// has appeared yet.
pub fn show_middle_ring(
    epoch: u64,
    entries: Vec<crate::middle_ring::RingEntry>,
    scope: crate::config::MiddleRingScope,
    layout: crate::config::AllRingLayout,
    fun: bool,
    reduced: bool,
    cursor_phys: (i32, i32),
) {
    use crate::middle_ring as mr;
    // "All" only: the owner's Rings / Spiral pill. Favourites is always the
    // edge-and-corner ring law — its whole point is the shape it takes at a
    // screen edge, which a spiral has no answer for.
    let spiral = scope == crate::config::MiddleRingScope::All
        && layout == crate::config::AllRingLayout::Spiral;
    let current = HOLD_EPOCH.load(Ordering::SeqCst);
    if current != epoch {
        log::info!(
            "guide_hud: a deferred icon-ring show for hold #{epoch} arrived after that hold had \
             ended (now #{current}) — NOT showing (PROBLEM 177, PROBLEM 267)."
        );
        return;
    }
    let Some(handle) = APP_HANDLE.get() else { return };
    let n_items = entries.len();
    // PHASE A step 3 (§7 of brief 3) — say how many of the items are SPECIAL
    // tiles, because "13 item(s), scope my_eight" was read on 2026-09-19 as
    // "the specials are missing" when it meant "Favourites never carries
    // them". Zero specials + scope my_eight is the design, not a bug.
    let n_specials = entries.iter().filter(|e| e.kind == crate::middle_ring::ItemKind::Special).count();
    if n_items == 0 {
        log::info!(
            "guide_hud: the icon ring has NOTHING to show — the active profile has no bound \
             letters (PROBLEM 267). Standing down; the hold ends as a plain release."
        );
        return;
    }

    VISIBLE_EPOCH.store(epoch, Ordering::SeqCst);
    HUD_KIND.store(HUD_KIND_RING, Ordering::SeqCst);
    HUD_VISIBLE.store(true, Ordering::Relaxed);

    let mut placement: Option<RingPlacement> = None;
    // The payload and its slots are built once the monitor is known, because
    // the SHAPE depends on the room around the press point (owner decision,
    // 2026-09-13): Favourites changes shape at an edge or corner and never
    // moves its centre; "All" keeps the full circles, clamped and warped.
    let mut built: Option<(mr::MiddleRingPayload, Vec<mr::RingSlot>)> = None;
    let mut entries = Some(entries);
    if !OVERLAY_DISABLED.load(Ordering::Relaxed) {
        if let Some(win) = handle.get_webview_window("overlay") {
            // THE MONITOR THE CURSOR IS ON (owner decision 2026-09-13, round
            // 3 — "primary" plays no role any more): `MonitorFromPoint` of
            // the press point, falling back to `overlay_monitor`'s chain
            // (the cursor now, then primary, then any) during a display
            // change.
            let mon = match win.monitor_from_point(cursor_phys.0 as f64, cursor_phys.1 as f64) {
                Ok(Some(m)) => Some(m),
                _ => {
                    log::warn!(
                        "guide_hud: no monitor under the press point ({},{}) — falling back to \
                         overlay_monitor's chain for this one show (PROBLEM 267)",
                        cursor_phys.0, cursor_phys.1
                    );
                    crate::commands::overlay_monitor(&win)
                }
            };
            if let Some(mon) = mon {
                let sf = mon.scale_factor();
                let wa = mon.work_area();
                let area = mr::WorkArea {
                    x: wa.position.x as f64,
                    y: wa.position.y as f64,
                    w: wa.size.width as f64,
                    h: wa.size.height as f64,
                };
                let bounds = mr::WorkArea {
                    x: mon.position().x as f64,
                    y: mon.position().y as f64,
                    w: mon.size().width as f64,
                    h: mon.size().height as f64,
                };
                let canvas = mr::canvas_rect(area, bounds);
                // THE ROOM THE LAYOUT MAY USE — the CANVAS, not the work
                // area, less any auto-hidden taskbar band (2026-09-15).
                // Two right/bottom-only clips fixed at once; the full
                // account is on `middle_ring::room_rect`. In one sentence:
                // the window is the canvas, so anything measured against
                // the work area is 2 px outside the window on the right and
                // bottom whenever the 2-px inset applies, and an auto-hidden
                // taskbar owns a band of the work area that Windows never
                // subtracts.
                let room_area = mr::room_rect(canvas, crate::commands::autohide_reserve_for(bounds));
                // THE SHAPE, decided here and never again for this raise.
                // Favourites (round 6 — the owner's round-3 law): the centre
                // is the PRESS POINT. `choose_shape` lays arcs around it at
                // full size, then SHRINKS the unit (tile, radii and spacing
                // together) down to `TILE_MIN` before it will move the
                // centre at all; only when even the floor cannot hold the
                // count does it NUDGE by the smallest grid vector that fits
                // (`Placement.offset`, logical, warped below like a clamp),
                // and only when nothing fits anywhere does it fall back to
                // the clamp + warp. "All": full circles, always, clamped and
                // warped against THIS monitor's work area.
                let cursor_f = (cursor_phys.0 as f64, cursor_phys.1 as f64);
                // 1.0.119 (brief 4 §3) — THE FOLD IS DECIDED BY TILE COUNT,
                // NOT BY SCOPE. `placement_for`: if every tile fits the arcs
                // the room allows at FULL size (`fits_as_arc` — the same
                // capacity maths Favourites has always folded by), fold
                // around the press point whatever the scope; if not,
                // relocate as "All" always has — full circles (or the
                // spiral, which never folds) clamped and warped below. The
                // owner's rule: "adapt only when it cannot hold any more in
                // a readable way". Before this, Favourites folded and All
                // relocated regardless of how many tiles each carried.
                // As the owner settled it (2026-09-19): FAVOURITES is
                // untouched — its whole ladder (anchor → shrink → nudge →
                // clamp) stays, so a set that fits only by shrinking still
                // folds-with-shrink; the count rule only ADDS folding for
                // All when its tiles fit at full size.
                let room = mr::Room::at(cursor_f, room_area, sf);
                let favourites = scope == crate::config::MiddleRingScope::MyEight;
                let folds = !spiral && mr::fits_as_arc(n_items, room);
                log::info!(
                    "guide_hud: fold-by-count — {n_items} tile(s) under scope {:?} {} at full \
                     tile size in this room, so the ring {} (brief 4 §3: the count decides, \
                     not the scope)",
                    scope,
                    if folds { "FIT the arcs" } else if spiral { "are a spiral (never folds)" } else { "do NOT fit the arcs" },
                    if favourites { "takes Favourites' own ladder, shrink included" } else if folds { "folds around the press point" } else { "relocates as All always has" },
                );
                let mr::Placement { shape, offset, slots, arcs } =
                    mr::placement_for(n_items, room, favourites, spiral);
                // Clamp on the TILES plus their hover halo, not on the scrim:
                // the scrim fades to nothing and may be cut by the screen
                // edge; a tile may not (artboard 5). `clamp_extent` is
                // (ring_extent + CLAMP_MARGIN) × scale — physical, like the
                // work area and the cursor, and it is the ACTUAL outermost
                // ring's extent whatever the ring count. An ANCHORED shape
                // skips it: its centre is the press point plus the snap
                // (physical = logical × scale); a non-zero snap warps the
                // cursor below exactly as a clamp does.
                let (cx, cy) = if shape.anchored() {
                    (cursor_f.0 + offset.0 * sf, cursor_f.1 + offset.1 * sf)
                } else {
                    mr::clamp_ring_center(cursor_f, room_area, mr::clamp_extent(&slots, sf))
                };
                let (cx, cy) = (cx.round() as i32, cy.round() as i32);
                let page = mr::page_point((cx as f64, cy as f64), canvas, sf);
                let Some(ent) = entries.take() else { return };
                // The room in the page's own px: the scrim is clipped to it
                // so the fade ends at the screen edge, never past it.
                let room_css = mr::page_rect(room_area, canvas, sf);
                let (payload, slots) = mr::build_payload_shaped(
                    ent, scope, fun, reduced, shape, slots, &arcs, page, Some(room_css), sf,
                );
                built = Some((payload, slots));
                // THE CANVAS. Physical in, physical out; the fitter compares
                // the target with the window's current rectangle and moves
                // nothing when they already agree (a raise on the same
                // monitor after a raise is free).
                crate::commands::overlay_fit_canvas(&win, canvas, sf, "icon ring");
                crate::commands::set_overlay_region(&win, &[], 1.0);
                crate::commands::raise_overlay_topmost(&win);
                placement = Some(RingPlacement {
                    cx,
                    cy,
                    dx: cx - cursor_phys.0,
                    dy: cy - cursor_phys.1,
                    scale: sf,
                    canvas,
                    room: room_area,
                    page,
                    shape,
                    monitor: (mon.position().x, mon.position().y),
                });
            } else {
                log::error!(
                    "guide_hud: NO monitor could be resolved — the icon ring cannot be placed \
                     and will not be shown this time (PROBLEM 267)"
                );
            }

            if placement.is_none() || abort_if_stale(epoch, Some(&win), "before ring show") {
                if placement.is_none() {
                    HUD_VISIBLE.store(false, Ordering::Relaxed);
                }
                return;
            }
            let _ = win.show();
            SHOW_OUTSTANDING.store(true, Ordering::SeqCst);
            log::info!("guide_hud: overlay window shown for the icon ring (hold #{epoch})");
            log_shown_over();
        } else if !crate::windows_created() {
            log::info!(
                "guide_hud: still starting — the overlay webview is not built yet; the icon \
                 ring for this hold is skipped and the overlay is asked for now (PROBLEM 215/265)"
            );
            crate::overlay_boot::request_now(handle);
            HUD_VISIBLE.store(false, Ordering::Relaxed);
            return;
        } else {
            log::error!(
                "guide_hud: the overlay window does not exist — the icon ring cannot be shown. \
                 Asking the display watcher to rebuild it (PROBLEM 214)."
            );
            crate::display_watch::heal_now();
            HUD_VISIBLE.store(false, Ordering::Relaxed);
            return;
        }
    } else {
        log::error!(
            target: crate::telemetry::DEGRADED_TARGET,
            "guide_hud: OVERLAY_DISABLED is set, so the icon ring is suppressed; asking the \
             display watcher to rebuild the overlay (PROBLEM 214/267)."
        );
        crate::display_watch::heal_now();
        HUD_VISIBLE.store(false, Ordering::Relaxed);
        return;
    }

    let Some(pl) = placement else { return };
    let Some((payload, slots)) = built else { return };

    // THE WARP ("All" only, or the clamped fallback). The OS cursor moves to
    // the clamped centre, so what is drawn and what the hand feels agree
    // (artboard 5: "cursor warped here"). A win32k call — legal here (engine
    // thread), never in the callback. `SetCursorPos` generates one
    // WM_MOUSEMOVE our LL hook sees as a plain move: it can start nothing (a
    // move is not a press) and `note_cursor` then stores exactly this
    // position, which is inside the dead zone. The explicit `note_cursor`
    // below is the belt to that brace: the poller's cursor truth is right
    // even if the injected move never reaches the hook (UIPI, or a hook
    // evicted mid-hold).
    if pl.dx != 0 || pl.dy != 0 {
        #[cfg(windows)]
        unsafe {
            if let Err(e) = windows::Win32::UI::WindowsAndMessaging::SetCursorPos(pl.cx, pl.cy) {
                log::warn!(
                    "guide_hud: SetCursorPos({},{}) failed ({e}) — the ring is drawn at the \
                     clamped centre but the cursor stayed where it was; aiming still works, \
                     from where the cursor is (PROBLEM 267)",
                    pl.cx, pl.cy
                );
            }
        }
        crate::hook::pointer::note_cursor(pl.cx, pl.cy);
    }

    // THE HIT TABLE — codes first, geometry last (`RING_ACTIVE` goes up
    // inside `publish_ring`, after the count).
    let codes: Vec<char> = payload
        .items
        .iter()
        .filter_map(|i| i.code.chars().next())
        .collect();
    crate::hook::pointer::publish_key_codes(&codes);
    let hits = mr::hits_for(&slots, pl.scale);
    let dead = mr::dead_zone_for(&slots) * pl.scale;
    crate::hook::pointer::publish_ring(pl.cx, pl.cy, dead, &hits, spiral);

    // THE MARKER LINE — one per raise, long and literal on purpose (CLAUDE.md:
    // a long `log::` FORMAT STRING is the only exe marker that survives).
    let radii: Vec<String> = {
        let mut r: Vec<u32> = slots.iter().map(|s| s.radius.round() as u32).collect();
        r.sort_unstable();
        r.dedup();
        r.iter().map(|v| v.to_string()).collect()
    };
    let tiles: Vec<String> = {
        let mut t: Vec<u32> = slots.iter().map(|s| s.tile.round() as u32).collect();
        t.sort_unstable();
        t.dedup();
        t.iter().map(|v| v.to_string()).collect()
    };
    let counts: Vec<String> = {
        let mut rings: Vec<u8> = slots.iter().map(|s| s.ring).collect();
        rings.sort_unstable();
        rings.dedup();
        rings
            .iter()
            .map(|r| slots.iter().filter(|s| s.ring == *r).count().to_string())
            .collect()
    };
    log::info!(
        "middle-button ring v2: cursor-anchored-ring-raised-at-cursor-spaceadom-267 — centre \
         ({},{}) physical, clamp delta ({},{}), {} item(s) of which {} special tile(s) — specials ride \
         on scope All only, Favourites never carries them (Phase A step 3) — scope {}, shape {} (arc radii {} \
         logical, ring counts {}, tiles {} px), canvas {:.0}x{:.0} @ ({:.0},{:.0}) physical on \
         the monitor at ({},{}) scale {}, page centre ({:.1},{:.1}) css, room {:.0}x{:.0} @ \
         ({:.0},{:.0}) physical — the canvas less any AUTO-HIDDEN appbar band (2026-09-15) \
         (hud hold #{epoch}). A \
         circle keeps the centre on the press point with delta (0,0); half-* / quarter-* are \
         SNAPPED to the edge line / corner point and circle-clamped is pushed on-screen by \
         clamp_ring_center — either way a non-zero delta means the cursor was warped to the new \
         centre; the canvas is the work area of the cursor's monitor, never its exact bounds. \
         PROBLEM 267.",
        pl.cx, pl.cy, pl.dx, pl.dy, n_items, n_specials, payload.scope, payload.shape,
        radii.join("/"), counts.join("/"), tiles.join("/"),
        pl.canvas.w, pl.canvas.h, pl.canvas.x, pl.canvas.y,
        pl.monitor.0, pl.monitor.1, pl.scale, pl.page.0, pl.page.1,
        pl.room.w, pl.room.h, pl.room.x, pl.room.y
    );

    if abort_if_stale(epoch, handle.get_webview_window("overlay").as_ref(), "before ring emit") {
        return;
    }
    if let Err(e) = handle.emit("middle-ring-show", payload) {
        log::warn!("guide_hud: middle-ring-show emit failed: {e}");
    }
}

/// PROBLEM 243 — the Guide HUD must appear while Spaceadom's OWN dashboard is
/// the foreground window, exactly as it does over any other app.
///
/// There is no gate to test, because the audit found none: the hook's
/// Space-down branch (`hook/mod.rs`, "SPACE DOWN"), the engine's `SpaceDown`
/// arm (`engine/mod.rs`) and `show_hud_payload` above all reach `win.show()`
/// without ever asking what is in the foreground. What CAN be tested is the
/// instrument that proves it in the log — and an instrument that cannot
/// produce the answer "our own window" would be no instrument at all, which is
/// the trap CLAUDE.md records twice ("a check that cannot produce a negative
/// result is not a check").
#[cfg(test)]
mod shown_over_tests {
    use super::shown_over_phrase;

    /// The owner's scenario, in every form the exe stem really arrives in.
    /// `foreground_stem` and `own_stem` both run their input through
    /// `normalize_stem`, so both sides are already lowercase stems — but
    /// `own_stem`'s documented FALLBACK is the hard-coded literal
    /// `"spaceadom"`, which is reached when `current_exe()` fails, and a
    /// case-sensitive comparison would then silently stop recognising us.
    #[test]
    fn our_own_dashboard_is_named_as_such() {
        assert_eq!(shown_over_phrase("spaceadom", "spaceadom"), "over own window");
        assert_eq!(shown_over_phrase("Spaceadom", "spaceadom"), "over own window");
        assert_eq!(shown_over_phrase("spaceadom", "SPACEADOM"), "over own window");
        assert_eq!(shown_over_phrase(" spaceadom ", "spaceadom"), "over own window");
    }

    /// The ordinary case — the ring over somebody else's window. Named, not
    /// lumped into "not us": the whole point of the line is that the next log
    /// says which app it was without anyone having to join two windows of
    /// data (see the function's header).
    #[test]
    fn another_app_is_named_by_its_exe() {
        assert_eq!(shown_over_phrase("brave", "spaceadom"), "over brave.exe");
        assert_eq!(shown_over_phrase("explorer", "spaceadom"), "over explorer.exe");
        // Not us: a different app whose name merely CONTAINS ours. Same
        // narrowness the PROBLEM 218 self-exclusion guard is held to.
        assert_eq!(
            shown_over_phrase("spaceadom-helper", "spaceadom"),
            "over spaceadom-helper.exe"
        );
    }

    /// `foreground_stem` returns "" when it cannot read the foreground
    /// process — a lock screen, a UAC prompt, a window closing underneath us.
    /// That must NOT read as "our own window": an empty-matches-empty bug here
    /// would report the requirement as met on exactly the holds where nothing
    /// is known, which is worse than reporting nothing.
    #[test]
    fn an_unreadable_foreground_is_never_mistaken_for_us() {
        assert_eq!(
            shown_over_phrase("", "spaceadom"),
            "over a window whose process could not be read"
        );
        assert_eq!(
            shown_over_phrase("   ", "spaceadom"),
            "over a window whose process could not be read"
        );
        // And the mirror: an unreadable OWN stem must not make every app on
        // the machine look like us. `own_stem()` never returns empty, but the
        // pure function is where that promise is pinned.
        assert_eq!(shown_over_phrase("brave", ""), "over brave.exe");
    }
}
