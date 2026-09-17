/// engine/mod.rs — Async action dispatch engine.
/// Receives HookEvents from the hook thread via crossbeam channel
/// and dispatches to specialized action handlers. Runs on a tokio task.

pub mod actions;

use crate::{
    config::SharedConfig,
    guide_hud,
    hook::{HookEvent, KeyCombo},
};
use crossbeam_channel::Receiver;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tokio::sync::watch;

/// All mutable engine runtime state (shared between the actor and commands).
pub struct EngineState {
    pub config: SharedConfig,
    pub boss_key: Arc<Mutex<actions::boss_key::BossKeyState>>,
    pub pip_cache: actions::pip::PipCache,
    /// Double-tap timestamps for Space+Up and Space+Down
    pub last_up_ts: u64,
    pub last_down_ts: u64,
    /// Profile index for cycling (mirrors V11 ProfileIndex)
    pub profile_index: usize,
    /// Tauri app handle for emitting events to frontend
    pub app_handle: tauri::AppHandle,
    /// Canceller for the guide HUD delay task
    pub hud_cancel_tx: Option<watch::Sender<bool>>,
}

impl EngineState {
    pub fn new(config: SharedConfig, app_handle: tauri::AppHandle) -> Self {
        EngineState {
            config,
            boss_key: Arc::new(Mutex::new(actions::boss_key::BossKeyState::default())),
            pip_cache: actions::pip::new_cache(),
            last_up_ts: 0,
            last_down_ts: 0,
            profile_index: 0,
            app_handle,
            hud_cancel_tx: None,
        }
    }

    fn active_profile_name(&self) -> String {
        self.config
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .active_profile
            .clone()
    }

    fn cycle_profile(&mut self) -> String {
        let mut cfg = self.config.write().unwrap_or_else(|p| p.into_inner());
        let profiles = &cfg.profiles;
        if profiles.is_empty() {
            return cfg.active_profile.clone();
        }

        // Keep index synced with config active_profile which might have changed from UI
        if let Some(idx) = profiles.iter().position(|p| p.name == cfg.active_profile) {
            self.profile_index = idx;
        }

        self.profile_index = (self.profile_index + 1) % profiles.len();
        let new_name = profiles[self.profile_index].name.clone();
        cfg.active_profile = new_name.clone();

        // Persist immediately
        drop(cfg);
        if let Err(e) = crate::config::save(&self.config.read().unwrap_or_else(|p| p.into_inner())) {
            log::error!("engine: profile save failed: {e}");
        }
        new_name
    }

    fn emit_toast(&self, msg: &str) {
        crate::show_toast(&self.app_handle, msg);
    }

    fn emit_profile_changed(&self, name: &str) {
        let _ = self.app_handle.emit("profile-changed", name);
    }

    fn emit_bypass_toggled(&self, active: bool) {
        let _ = self.app_handle.emit("bypass-toggled", active);
    }

    /// PROBLEM 242 — "a shortcut just really fired", for the DASHBOARD.
    ///
    /// Nothing reported this before. `toast-notification` fires on every
    /// combo but is a deliberate overlay-only broadcast (`lib.rs::show_toast`),
    /// and `app-launched` in `smart_cascade` only covers a genuine shell
    /// launch — not the focus and minimize legs of the cascade, which are
    /// most of what a working shortcut does. The first-run tour's last step
    /// asks the user to actually USE their new binding, and the page cannot
    /// answer "did that work?" on its own: the hook swallows Space
    /// system-wide, so no keyboard listener in the webview will ever see the
    /// combo. This is the only thing in the process that knows.
    ///
    /// Global `emit`, never `emit_to` — that has never worked here.
    /// Fire-and-forget: a dashboard that is closed has no listener, and the
    /// engine must not care.
    fn emit_launched(&self, key: char, label: &str) {
        let _ = self.app_handle.emit(
            "st-launched",
            serde_json::json!({ "key": key.to_string(), "label": label }),
        );
    }

    /// `action_pending`: true when a combo fired and its toast is about to
    /// arrive - the overlay window must stay up for the handover (PROBLEM 135).
    fn cancel_hud(&mut self, action_pending: bool) {
        if let Some(tx) = self.hud_cancel_tx.take() {
            let _ = tx.send(true);
        }
        guide_hud::hide_guide_hud_pending(action_pending);
    }
}

/// Start the async engine actor. Call once on app startup.
pub fn start_engine(
    rx: Receiver<HookEvent>,
    state_arc: Arc<Mutex<EngineState>>,
) {
    tauri::async_runtime::spawn(async move {
        log::info!("engine: actor started");

        // PROBLEM 183 — drain the hook's diagnostics on a CLOCK, not only when
        // a Space release succeeds.
        //
        // `drain_hook_diagnostics` had one caller: the SpaceUp arm below. A
        // SpaceUp only arrives if the hook received both halves of a Space
        // press, so every diagnostics line ever written was written at a moment
        // the hook was provably alive — precisely the moments that are NOT the
        // problem. During an outage there is no Space release, so there is no
        // line, and the outage leaves no trace in the one instrument built to
        // measure it.
        //
        // On the ENGINE thread, deliberately: this function logs, and logging
        // on the hook thread delays the next callback (PROBLEM 58's territory).
        // 30s so a minute-long outage cannot fall between two ticks.
        tauri::async_runtime::spawn(async move {
            let mut tick = tokio::time::interval(tokio::time::Duration::from_secs(30));
            tick.tick().await; // the first tick fires immediately; skip it
            loop {
                tick.tick().await;
                crate::hook::drain_hook_diagnostics();
            }
        });

        loop {
            // Receive next event (blocking in an async-friendly way via spawn_blocking)
            let event = match tauri::async_runtime::spawn_blocking({
                let rx = rx.clone();
                move || rx.recv()
            })
            .await
            {
                Ok(Ok(ev)) => ev,
                Ok(Err(_)) => {
                    log::info!("engine: hook channel closed, actor exiting");
                    break;
                }
                Err(e) => {
                    log::error!("engine: spawn_blocking error: {e}");
                    break;
                }
            };

            // PROBLEM 82 (engine half) — one panic inside an action used to
            // kill this actor task silently: the channel backs up, every
            // Space+key does nothing, no log line says why. Running each
            // dispatch as its own task turns a panic into a logged, isolated
            // failure of ONE keypress; the loop lives on.
            let state2 = Arc::clone(&state_arc);
            let joined = tauri::async_runtime::spawn(async move {
                dispatch(event, &state2).await;
            })
            .await;
            if let Err(e) = joined {
                log::error!(
                    "engine: an action PANICKED ({e}) — that keypress was dropped; \
                     the engine keeps running"
                );
            }
        }
    });
}

async fn dispatch(event: HookEvent, state_arc: &Arc<Mutex<EngineState>>) {
    // PROBLEM 259 — the own-window fallback's Space-down is THE SAME EVENT
    // with a different witness: the dashboard page saw it because the hook
    // could not (PROBLEM 257). Normalised here, at the door, so there is
    // exactly ONE `SpaceDown` arm and the HUD / cascade / SpaceUp paths below
    // cannot drift into two behaviours. The only thing the origin changes is
    // the sentence the arm logs — see there.
    let via_own_window_page = matches!(event, HookEvent::OwnWindowSpaceDown);
    // PROBLEM 263 — and the MIDDLE BUTTON's hold is the same event with a third
    // witness. Normalised at the same door and for the same reason: ONE
    // `SpaceDown` arm, so the ring, the cascade, the pointer and the SpaceUp
    // path cannot drift into three behaviours. The only thing the trigger
    // changes is the sentence the arm logs — see there.
    let via_middle_button = matches!(event, HookEvent::MiddleButtonDown);
    // PROBLEM 267 — WHICH ring the middle button raises is the owner's
    // `middle_ring_style`. `GuideHud` keeps phase 1 byte-for-byte: the event
    // is normalised to `SpaceDown` here exactly as 1.0.109 does it.
    // `IconRing` (the default) leaves the event as `MiddleButtonDown`, which
    // its own arm below turns into the cursor-anchored ring. One pure
    // function decides (`routed_middle_event`), so a test can pin both.
    let middle_style = if via_middle_button {
        let s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
        let cfg = s.config.read().unwrap_or_else(|p| p.into_inner());
        cfg.middle_ring_style
    } else {
        crate::config::MiddleRingStyle::default()
    };
    let event = if via_own_window_page {
        HookEvent::SpaceDown
    } else if via_middle_button {
        routed_middle_event(middle_style)
    } else {
        event
    };
    // Only the GuideHud leg is "via the middle button" from here on: the
    // SpaceDown arm's per-hold line is phase 1's, and the icon-ring arm logs
    // its own.
    let via_middle_button = via_middle_button && matches!(event, HookEvent::SpaceDown);

    match event {
        // ---------------------------------------------------------------
        // Space held down — start the Guide HUD timer
        // ---------------------------------------------------------------
        HookEvent::SpaceDown => {
            let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
            // Honour the user's configured delay. This was hardcoded to 300ms,
            // which silently made the "Guide HUD delay" slider in Settings a
            // no-op — it persisted a value nothing ever read.
            let hud_delay_ms = {
                let mut s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
                s.hud_cancel_tx = Some(cancel_tx);
                let d = s.config.read().unwrap_or_else(|p| p.into_inner()).guide_hud_delay_ms;
                if d == 0 { 300 } else { d }
            };

            // PROBLEM 177 — stamp THIS hold. The `cancel_rx` check below is
            // not atomic with the show it guards (there are two lock
            // acquisitions and a pile of Vec building in between), so a cancel
            // that lands in that gap is honoured and then forgotten, and the
            // show proceeds with nothing left to undo it. The stamp is checked
            // inside show_guide_hud itself, where it cannot be raced.
            let epoch = guide_hud::begin_hold();

            // PROBLEM 257 — ONE line per hold, at the moment the hold starts,
            // naming the window it started over. This event can only come from
            // the primary keyboard hook (nothing else sends `SpaceDown`), so
            // the line's existence IS the proof that the hook saw the Space;
            // a hold the owner made that has no such line never reached the
            // hook at all — which is the 2026-09-06 regression, and until this
            // line the only witness was a 60-second aggregate counter. Runs on
            // the async runtime, not in the callback, so the foreground query
            // is legal here (the same reasoning as PROBLEM 243's shown-over
            // line). `over own window` is the phrase the install-proof script
            // asserts (CLAUDE.md keyboard-hook law 6).
            #[cfg(windows)]
            {
                let own = crate::hook::exclusions::own_stem();
                let fg = unsafe { crate::hook::exclusions::foreground_stem() };
                let phrase = guide_hud::shown_over_phrase(&fg, &own);
                if via_own_window_page {
                    // PROBLEM 259 — the fallback's per-hold line, and it must
                    // NOT be able to satisfy CLAUDE.md law 6.
                    //
                    // `scripts/install-proof.ps1` reads `hold start (hold #N)`
                    // + `over own window` as the proof the KEYBOARD HOOK is
                    // alive over our own window. This Space never touched the
                    // hook, so this line deliberately omits the words `hold
                    // start` — a fallback hold can never be mistaken for that
                    // proof, and the two facts stay separately observable.
                    // The fallback's own proof pair is this marker followed by
                    // `guide_hud: shown over own window`.
                    log::info!(
                        "own-window fallback: Space-down came from the dashboard page \
                         (PROBLEM 257 fallback), hook silent — fallback hold #{} began \
                         (hud hold #{epoch}) {phrase}. The keyboard hook did NOT see this \
                         Space; the page did, and `own_window_space_down` passed it to the \
                         engine. Everything after this line is the ordinary path: the ring, \
                         the cascade and the toast are the same code. If you are checking \
                         CLAUDE.md law 6, THIS LINE IS NOT THAT PROOF — law 6 wants a \
                         'hold start (hold #N) … over own window' line, which only the hook \
                         can produce.",
                        crate::hook::own_window_hold_count()
                    );
                } else if via_middle_button {
                    // PROBLEM 263 — THE MIDDLE-BUTTON TRIGGER'S PER-HOLD LINE,
                    // and the reason it exists is PROBLEM 259's reason exactly:
                    // the ring now has THREE triggers and a log that cannot say
                    // which one raised a given ring cannot answer any question
                    // about it. Long, unique and full of literal words on
                    // purpose — it is also the ASCII marker that proves this
                    // feature is in a built exe (CLAUDE.md: use a long
                    // `log::` FORMAT STRING as the marker, never a short
                    // identifier).
                    //
                    // It deliberately does NOT contain the words `hold start`,
                    // so it can never satisfy CLAUDE.md keyboard-hook law 6 /
                    // `scripts/install-proof.ps1`. This Space-down never
                    // happened and the keyboard hook was never asked anything;
                    // a ring you saw after pressing the middle button is
                    // evidence about the MOUSE hook and about nothing else.
                    log::info!(
                        "middle-button ring: the-guide-hud-ring-was-raised-by-a-middle-mouse-\
                         button-hold-spaceadom — middle-button hold #{} began (hud hold \
                         #{epoch}) {phrase}. The WM_MBUTTONDOWN was SUPPRESSED, so this \
                         process owes the world a middle click if the button comes back up \
                         inside the tap threshold; everything after this line is the ordinary \
                         path — the ring, pointer activation, the cascade and the toast are \
                         the same code the spacebar runs. THIS LINE IS NOT CLAUDE.md LAW 6's \
                         PROOF: law 6 wants a 'hold start (hold #N) … over own window' line, \
                         which only the keyboard hook can produce, and this gesture never \
                         reached it. PROBLEM 263.",
                        crate::hook::middle_hold_count()
                    );
                } else {
                    log::info!(
                        "hold start (hold #{epoch}): the primary keyboard hook saw this \
                         Space-down and delivered it {phrase} (PROBLEM 257). If no \
                         'guide_hud: overlay window shown (hold #{epoch})' follows, the fault \
                         is between the engine and the HUD; if a hold you made has NO line \
                         like this one, the hook never saw the Space — grep 'KEYBOARD DEAF, \
                         PROVEN' and 'own-window fallback:'."
                    );
                }
            }

            let state_clone = Arc::clone(state_arc);
            tauri::async_runtime::spawn(async move {
                tokio::select! {
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(hud_delay_ms)) => {
                        // Show HUD if not cancelled
                        if !*cancel_rx.borrow() {
                            let (profile_name, emoji, bindings, icons, specials) = {
                                let s = state_clone.lock().unwrap_or_else(|p| p.into_inner());
                                let cfg = s.config.read().unwrap_or_else(|p| p.into_inner());
                                let name = cfg.active_profile.clone();
                                // PROBLEM 267 round 3 — the pills' icons, from
                                // the same cache the icon ring fills; CACHE
                                // ONLY, so this path gains one HashMap lookup
                                // per chip and no shell call.
                                let icon_cache = s
                                    .app_handle
                                    .try_state::<crate::commands::IconCacheState>()
                                    .map(|c| std::sync::Arc::clone(&c.0));
                                let lookup = |target: &str| -> Option<String> {
                                    icon_cache.as_ref().and_then(|c| {
                                        c.lock().unwrap_or_else(|p| p.into_inner()).get(target).cloned()
                                    })
                                };
                                let icons = hud_icons_for(&cfg, &name, &lookup);

                                // CORE_AIM: the HUD must show the CURRENT
                                // PROFILE's shortcuts — the user's actual app
                                // keys first, system shortcuts after.
                                //
                                // EXTRACTED to `hud_apps_for` in 1.0.96, and
                                // the reason is worth a line: the Settings
                                // PREVIEW (`commands::preview_hud_layout`) has
                                // to build the SAME list from the SAME config,
                                // and a preview that showed a different label
                                // set from the real ring would be a lie told by
                                // the feature whose entire job is showing the
                                // truth. One function, two callers, no second
                                // copy to drift.
                                let binds = hud_apps_for(&cfg, &name);
                                let emoji = profile_emoji_for(&cfg, &name);

                                // System-wide shortcuts — separate list; the
                                // HUD renders these FIRST (user's direction:
                                // specials are the hard-to-remember part).
                                //
                                // The empty vec is still the ONLY mechanism —
                                // the page draws what it is given, so there is
                                // no second event and no toast.ts coupling —
                                // but since 1.0.89 it is no longer the whole
                                // DECISION. Two settings decide together, and
                                // they are split by who can actually know the
                                // answer:
                                //
                                //   RUST decides, here, deterministically:
                                //     · `hud_show_specials` off  -> empty
                                //     · `hud_band_count == "two"` -> empty
                                //       (the specials occupy the INNER band, so
                                //       they cannot coexist with two app bands;
                                //       no measurement is needed to know that)
                                //
                                //   THE PAGE decides the rest:
                                //     · `hud_band_count == "auto"` with
                                //       specials on -> the vec is sent, and the
                                //       page drops it if the labels turn out to
                                //       need two bands. Band count depends on
                                //       MEASURED label widths, which exist
                                //       nowhere but in the overlay document, so
                                //       Rust cannot resolve "auto" and must not
                                //       pretend to.
                                //
                                // So: a non-empty vec here means "show these IF
                                // one band is enough", not "show these".
                                // `specials_for_hud` below is that rule alone,
                                // as a pure function, because the truth table
                                // is the part worth a test.
                                //
                                // The KEYS THEMSELVES ARE UNTOUCHED. Esc still
                                // fires the Boss Key, backtick still PiPs, and
                                // so on — every one of those lives in the hook
                                // and the KeyCombo arm below, neither of which
                                // has ever consulted this list. Do not gate
                                // anywhere else: a hidden ring must stay a
                                // hidden ring, not a disabled feature.
                                //
                                // Free to read: `cfg` is already borrowed and
                                // this is the Space-HOLD path, not the hook.
                                let specials = specials_for_hud(
                                    cfg.hud_show_specials,
                                    &cfg.hud_band_count,
                                );

                                (name, emoji, binds, icons, specials)
                            };
                            guide_hud::show_guide_hud(
                                epoch, &profile_name, emoji, bindings, icons, specials,
                            );
                        }
                    }
                    _ = cancel_rx.changed() => {
                        // Cancelled by SpaceUp or combo
                    }
                }
            });
        }

        // ---------------------------------------------------------------
        // Space released — hide HUD, optionally nothing (Space was injected in hook)
        // ---------------------------------------------------------------
        HookEvent::SpaceUp { .. } => {
            // Report the hook's suppression counters HERE, on the engine
            // thread. The hook callback itself must never log — disk I/O on
            // the hook path gets the hook evicted by Windows (PROBLEM 58).
            crate::hook::drain_hook_diagnostics();
            let mut s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
            s.cancel_hud(false);
        }

        // ---------------------------------------------------------------
        // Combo key while Space held
        // ---------------------------------------------------------------
        HookEvent::KeyCombo(combo) => {
            // Cancel guide HUD immediately on any combo. `true`: a toast is
            // coming, so the overlay window must stay up (PROBLEM 135).
            {
                let mut s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
                s.cancel_hud(true);
            }

            run_combo(combo, state_arc);
        }

        // ---------------------------------------------------------------
        // Pointer activation (PROBLEM 206) — the cursor was resting on a
        // Guide-HUD chip when Space was released or the left button went
        // down. Deliberately the SAME shape as KeyCombo above: cancel the
        // HUD with `true` (a toast is coming — the overlay window must stay
        // up for the PROBLEM 135 handover), then the ordinary alpha path:
        // handle_alpha → smart_cascade → toast.
        // ---------------------------------------------------------------
        HookEvent::PointerActivate(ch) => {
            crate::hook::drain_hook_diagnostics();
            {
                let mut s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
                s.cancel_hud(true);
            }
            // PROBLEM 267 — the icon ring's SPECIAL tiles arrive here too, as
            // a Private Use Area char, and fire the SAME `KeyCombo` the
            // keyboard would (`middle_ring::special_combo_for`) through the
            // SAME `run_combo`. The cascade is not forked: a letter is still
            // `handle_alpha`, a special is still its handler.
            if crate::middle_ring::is_special_code(ch) {
                match crate::middle_ring::special_combo_for(ch) {
                    Some(combo) => {
                        log::info!(
                            "engine: pointer activation → special {combo:?} (armed tile on the \
                             icon ring, PROBLEM 267)"
                        );
                        run_combo(combo, state_arc);
                    }
                    None => log::warn!(
                        "engine: pointer activation carried an unknown special code {:#x} — \
                         nothing fired",
                        ch as u32
                    ),
                }
            } else {
                log::info!("engine: pointer activation → Space+{ch} (armed chip on the guide HUD)");
                handle_alpha(ch, state_arc);
            }
        }

        // ---------------------------------------------------------------
        // Mouse wheel scroll with Space held
        // ---------------------------------------------------------------
        HookEvent::WheelUp => {
            {
                let mut s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
                s.cancel_hud(false);
            }
            actions::opacity::increase_opacity();
        }
        HookEvent::WheelDown => {
            {
                let mut s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
                s.cancel_hud(false);
            }
            actions::opacity::decrease_opacity();
        }

        // PROBLEM 259 — unreachable by construction: the normalisation at the
        // top of this function rewrites `OwnWindowSpaceDown` to `SpaceDown`
        // before the match. Written as an EXPLICIT arm rather than a `_` so
        // that the next variant added to `HookEvent` still fails the build
        // here instead of being silently swallowed — and as a log line rather
        // than `unreachable!()` so that a future refactor which breaks the
        // normalisation loses one keypress with an explanation, instead of
        // panicking the dispatch task (PROBLEM 82's isolation would catch it,
        // but "an action PANICKED" names nothing).
        HookEvent::OwnWindowSpaceDown => {
            log::error!(
                "engine: OwnWindowSpaceDown reached the match — the PROBLEM 259 \
                 normalisation at the top of dispatch() has been broken; that hold \
                 did nothing"
            );
        }

        // PROBLEM 267 — THE ICON RING. Reachable ONLY when `middle_ring_style`
        // is `IconRing` (the default): `routed_middle_event` at the top left
        // the event as `MiddleButtonDown`. (Until 1.0.109 this arm was the
        // "unreachable, normalisation broken" error; that leg now lives in
        // `routed_middle_event`'s GuideHud branch, byte-for-byte.)
        //
        // What happens, in order, and why each piece is where it is:
        //   1. The CURSOR is captured NOW — `GetCursorPos` on this thread,
        //      never in the callback — because the ring is anchored where the
        //      button went DOWN, and by the time it shows the hand may have
        //      started moving toward where it expects a tile to be.
        //   2. The ENTRIES (icons included) are built on a blocking thread
        //      WHILE the tap window runs, so a warm icon cache costs the show
        //      nothing and a cold one overlaps the 250 ms the hand is holding
        //      anyway. `extract_icon` joins an STA per call (picker rules).
        //   3. At `MIDDLE_TAP_MS` — the same threshold the release uses to
        //      tell a click from a hold, so the two can never disagree about
        //      whether a ring was owed — the ring shows, unless the release
        //      (`MiddleButtonTap` → `cancel_hud`) has already cancelled it.
        HookEvent::MiddleButtonDown => {
            let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
            let (cfg_snapshot, app_handle) = {
                let mut s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
                s.hud_cancel_tx = Some(cancel_tx);
                let cfg = s.config.read().unwrap_or_else(|p| p.into_inner()).clone();
                (cfg, s.app_handle.clone())
            };
            let epoch = guide_hud::begin_hold();
            let cursor = cursor_pos_phys();
            #[cfg(windows)]
            {
                let own = crate::hook::exclusions::own_stem();
                let fg = unsafe { crate::hook::exclusions::foreground_stem() };
                let phrase = guide_hud::shown_over_phrase(&fg, &own);
                // The per-hold line for THIS leg. No `hold start` in it, for
                // PROBLEM 263's reason: the keyboard hook was never asked.
                log::info!(
                    "middle-button ring v2: middle-button hold #{} began at cursor ({},{}) \
                     physical (hud hold #{epoch}) {phrase} — the cursor-anchored icon ring \
                     will be raised in {} ms unless the button comes back up first, in which \
                     case the click is replayed. THIS LINE IS NOT CLAUDE.md LAW 6's PROOF. \
                     PROBLEM 267.",
                    crate::hook::middle_hold_count(),
                    cursor.0,
                    cursor.1,
                    crate::hook::MIDDLE_TAP_MS
                );
            }
            let scope = cfg_snapshot.middle_ring_scope;
            // 2026-09-15 — Rings or Spiral, for the "All" scope only. Read
            // from the same snapshot as the scope so one hold can never mix
            // a scope from one config with a layout from another.
            let all_layout = cfg_snapshot.all_ring_layout;
            let fun = cfg_snapshot.fun_mode;
            let reduced = cfg_snapshot.motion == "reduced";
            let build = tauri::async_runtime::spawn_blocking(move || {
                let cache = app_handle
                    .try_state::<crate::commands::IconCacheState>()
                    .map(|c| std::sync::Arc::clone(&c.0));
                let extract = |target: &str| -> Option<String> {
                    ring_icon_for(target, cache.as_ref())
                };
                let profile = cfg_snapshot.active_profile.clone();
                crate::middle_ring::build_entries(&cfg_snapshot, &profile, scope, &extract)
            });
            tauri::async_runtime::spawn(async move {
                tokio::select! {
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(
                        crate::hook::MIDDLE_TAP_MS,
                    )) => {
                        if *cancel_rx.borrow() { return; }
                        let entries = match build.await {
                            Ok(e) => e,
                            Err(e) => {
                                log::error!(
                                    "engine: the icon-ring entry build PANICKED ({e}) — this \
                                     hold shows no ring (PROBLEM 267)"
                                );
                                return;
                            }
                        };
                        // Re-asked after the build: a cold icon cache can push
                        // the build past the tap window, and a release that
                        // landed meanwhile has already cancelled this hold.
                        if *cancel_rx.borrow() { return; }
                        guide_hud::show_middle_ring(epoch, entries, scope, all_layout, fun, reduced, cursor);
                    }
                    _ = cancel_rx.changed() => {
                        // Released inside the tap window (the click is being
                        // replayed) or superseded.
                    }
                }
            });
        }

        // ---------------------------------------------------------------
        // PROBLEM 263 — the middle button came up inside the tap threshold
        // with nothing else claiming the press, so it was an ORDINARY MIDDLE
        // CLICK and this process owes the world one.
        //
        // THE REPLAY RUNS FIRST, ABOVE THE HUD TEARDOWN. The click is the part
        // the user is waiting for and its latency is already the length of
        // their own press; `cancel_hud` is bookkeeping plus an event and can
        // wait the microseconds. (There is nothing to race: the overlay is
        // click-through by construction — `lib.rs::configure_overlay_window`
        // fails CLOSED on `set_ignore_cursor_events` — so a ring that is
        // briefly still on screen cannot intercept the replayed click.)
        //
        // WHY THIS IS NOT DONE IN THE CALLBACK: `SendInput` is a win32k call,
        // and `ms_hook_proc` is the one hook in this process that makes none,
        // which is exactly why it keeps firing while the keyboard hooks are
        // evicted (keyboard law 7b). Spending its budget to save a channel
        // hop would trade the feature against the app's own liveness.
        // ---------------------------------------------------------------
        HookEvent::MiddleButtonTap => {
            let ok = crate::hook::replay_middle_click();
            {
                let mut s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
                s.cancel_hud(false);
            }
            if ok {
                log::info!(
                    "middle-button ring: a-quick-middle-click-was-replayed-through-sendinput-\
                     spaceadom — the press was shorter than the hold threshold and nothing \
                     else claimed it, so the WM_MBUTTONDOWN this process swallowed has been \
                     put back as a real middle click: down and up in ONE SendInput batch \
                     (keyboard law 2 — SendInput followed by anything else does not preserve \
                     order), tagged with the 0x7A7A7A7A cookie so our own mouse hook passes \
                     it through. Middle-clicking a link still opens a tab and middle-clicking \
                     a tab still closes it. PROBLEM 263."
                );
            } else {
                log::warn!(
                    "middle-button ring: SendInput did NOT insert the replayed middle click \
                     — another thread blocked it partway (BlockInput, or UIPI while an \
                     elevated window has focus), which is the same failure PROBLEM 227 \
                     documents for the keyboard batches. THE USER'S MIDDLE CLICK DID NOT \
                     HAPPEN. If the DOWN went in and the UP did not, a corrective \
                     MOUSEEVENTF_MIDDLEUP has already been sent (NATIVE_SAFETY.md §3) — a \
                     latched middle button is autoscroll running with no way to stop it, \
                     which is the worst outcome this feature can produce. PROBLEM 263."
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Action handlers
// ---------------------------------------------------------------------------

/// ONE combo dispatcher for every witness: the keyboard's `KeyCombo` arm, and
/// — since PROBLEM 267 — a special tile released on the icon ring. Lifted out
/// of the `KeyCombo` arm verbatim so the ring cannot fork the cascade: there is
/// exactly one place that says what Space+Esc does.
fn run_combo(combo: KeyCombo, state_arc: &Arc<Mutex<EngineState>>) {
    match combo {
        KeyCombo::Alpha(ch)       => handle_alpha(ch, state_arc),
        KeyCombo::Special(name)   => handle_special(name, state_arc),
        KeyCombo::Escape          => handle_boss_key(state_arc),
        KeyCombo::Backtick        => handle_pip(state_arc),
        KeyCombo::Tab             => handle_fullscreen_pip(state_arc),
        KeyCombo::Backspace       => handle_force_close(state_arc),
        KeyCombo::Comma           => handle_focus(state_arc),
        KeyCombo::RightAlt        => handle_profile_cycle(state_arc),
        KeyCombo::UpArrow         => handle_double_tap_up(state_arc),
        KeyCombo::DownArrow       => handle_double_tap_down(state_arc),
        KeyCombo::Period          => handle_bypass_toggle(state_arc),
        KeyCombo::Semicolon       => handle_voice_typing(state_arc),
        KeyCombo::Slash           => handle_screenshot(state_arc),
    }
}

/// Space + / (and the ring's "Screenshot" tile): Windows' own region snip.
fn handle_screenshot(state_arc: &Arc<Mutex<EngineState>>) {
    let app_handle = {
        let s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
        s.app_handle.clone()
    };
    let msg = actions::screenshot::start_snip();
    crate::show_toast(&app_handle, msg);
}

/// Space + ; (and the ring's "Voice Typing" tile): Windows dictation.
fn handle_voice_typing(state_arc: &Arc<Mutex<EngineState>>) {
    let app_handle = {
        let s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
        s.app_handle.clone()
    };
    let msg = actions::voice_typing::start_voice_typing();
    crate::show_toast(&app_handle, msg);
}

/// PROBLEM 267 — what a `MiddleButtonDown` becomes for the match in
/// `dispatch`, by the owner's `middle_ring_style`:
///
/// * `GuideHud` → `SpaceDown`, the PROBLEM 263 normalisation, byte-for-byte:
///   the centred Guide HUD, the same arm, the same log line.
/// * `IconRing` → `MiddleButtonDown` stays itself and its own arm raises the
///   cursor-anchored ring.
///
/// Pure, so `middle_route_tests` pins both values without a mouse.
pub(crate) fn routed_middle_event(style: crate::config::MiddleRingStyle) -> HookEvent {
    match crate::middle_ring::middle_down_route(style) {
        crate::middle_ring::MiddleRoute::GuideHud => HookEvent::SpaceDown,
        crate::middle_ring::MiddleRoute::IconRing => HookEvent::MiddleButtonDown,
    }
}

/// PROBLEM 267 — the cursor in PHYSICAL screen px, read on the ENGINE thread
/// (a win32k call; legal here, never in a hook callback — PROBLEM 58/134).
/// `(0,0)` when it cannot be read: the clamp then lands the ring in the
/// top-left corner of the primary work area and warps the cursor to it, which
/// is visible and recoverable rather than a ring nowhere.
fn cursor_pos_phys() -> (i32, i32) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut pt = windows::Win32::Foundation::POINT::default();
        if unsafe { GetCursorPos(&mut pt) }.is_ok() {
            return (pt.x, pt.y);
        }
        log::warn!("engine: GetCursorPos failed — anchoring the icon ring at (0,0) (PROBLEM 267)");
        (0, 0)
    }
    #[cfg(not(windows))]
    {
        (0, 0)
    }
}

/// PROBLEM 267 — the icon ring's REAL-ICON resolver, cache first.
///
/// `target` is whatever the binding names: an absolute exe, a bare exe name
/// (`brave.exe`), a `.lnk`, a `shell:AppsFolder\…` AUMID or a folder. The
/// picker's `IconCache` is keyed by the target string exactly as
/// `extract_icon_cmd` keys it, so a key the user bound through the picker is a
/// warm hit; anything else is resolved the way `extract_icon_cmd` resolves it
/// and extracted through the one shell extractor (`IShellItemImageFactory`,
/// which handles folders and documents as well as exes), then cached for the
/// next hold. Returns bare base64 PNG, as the extractor does; the payload
/// builder wraps it as a data URL.
fn ring_icon_for(
    target: &str,
    cache: Option<&std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>>,
) -> Option<String> {
    if let Some(c) = cache {
        if let Some(hit) = c.lock().unwrap_or_else(|p| p.into_inner()).get(target) {
            return Some(hit.clone());
        }
    }
    let resolved = if std::path::Path::new(target).is_absolute() {
        target.to_string()
    } else {
        actions::smart_cascade::resolve_path(target).unwrap_or_else(|| target.to_string())
    };
    let png = crate::icon_extractor::extract_icon(&resolved)?;
    if let Some(c) = cache {
        c.lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(target.to_string())
            .or_insert_with(|| png.clone());
    }
    Some(png)
}

/// Handle Space + F1–F12 / Enter / Tab / Left / Right.
/// Routes through AppConfig.special_keys if user has configured a binding.
fn handle_special(key_name: String, state_arc: &Arc<Mutex<EngineState>>) {
    println!("Received modifier: Space, Trigger: Key({})", key_name);

    let (binding, fallback, claims) = {
        let s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
        let cfg = s.config.read().unwrap_or_else(|p| p.into_inner());
        let bind = cfg.special_keys.get(&key_name).cloned();
        // Which browser profiles OTHER reachable bindings have pinned — read
        // inside the read guard the binding lookup already holds open, so this
        // costs no extra lock and no I/O on the Space-hold latency path. See
        // `browser_profiles::active_profile_claims`.
        let claims = crate::browser_profiles::active_profile_claims(&cfg);
        // Founders-profile fallback not applicable for special keys, but keep API consistent
        (bind, None::<crate::config::KeyBinding>, claims)
    };

    let Some(bind) = binding else {
        log::debug!("engine: no special_key binding for Space+{key_name}");
        // PROBLEM 180 — this used to say "key passes through — not configured",
        // which was never true: the hook had already destroyed the keystroke
        // with LRESULT(1) before this line could run, so there was nothing left
        // to pass through. The hook consults BOUND_SPECIALS now and genuinely
        // does pass unbound keys through, so reaching here means the binding
        // was removed between the hook reading its mask and this dispatch — a
        // harmless race.
        return;
    };

    if !bind.is_mapped() {
        return;
    }

    let label = bind.label.clone().unwrap_or_else(|| key_name.to_uppercase());
    
    // NOTE: this used to unminimize/show/focus the "settings" dashboard on
    // EVERY Space+key press, so summoning your browser also threw the
    // SpaceToggle window in your face. It was not needed for foreground
    // rights either — smart_cascade already does the AttachThreadInput dance.
    let app_handle = {
        let s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
        s.app_handle.clone()
    };
    
    let outcome =
        actions::smart_cascade::smart_cascade(&bind, fallback.as_ref(), &claims, Some(app_handle));

    let s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
    s.emit_toast(&cascade_toast(outcome, &label, fallback.as_ref()));
}

/// Toast text that tells the truth about what the cascade did — a silent
/// Founders fallback is exactly how the user ended up asking "why did the
/// wrong app open?" (2026-08-10).
fn cascade_toast(
    outcome: actions::smart_cascade::CascadeOutcome,
    label: &str,
    fallback: Option<&crate::config::KeyBinding>,
) -> String {
    use actions::smart_cascade::CascadeOutcome;
    match outcome {
        CascadeOutcome::Primary => format!("⚡ {label}"),
        CascadeOutcome::Fallback => {
            let fb_label = fallback
                .and_then(|f| f.label.clone())
                .unwrap_or_else(|| "Founders binding".to_string());
            format!("⚠️ {label} unavailable → {fb_label} (Founders)")
        }
        CascadeOutcome::Failed => format!("❌ {label} could not be opened"),
    }
}

fn handle_alpha(ch: char, state_arc: &Arc<Mutex<EngineState>>) {
    // log::info, not println — stdout is invisible for a tray app.
    log::info!("engine: combo Space+{ch} received");
            crate::crash_context::note_action(format!("Space+{ch}"));

    let (profile_name, binding, fallback, claims) = {
        let s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
        let cfg = s.config.read().unwrap_or_else(|p| p.into_inner());
        let pname = cfg.active_profile.clone();
        let key = ch.to_string();

        // Which browser profiles OTHER reachable bindings have pinned. Read
        // here, inside the read guard the binding lookup already holds open —
        // the config is an in-memory Arc<RwLock<AppConfig>>, so this is a walk
        // over the active profile's bindings and nothing else: no second lock,
        // no file I/O, no cache to go stale. For everyone who pins nothing it
        // allocates nothing and returns empty, and an empty claim list means
        // the cascade takes the byte-for-byte pre-2026-08-27 path.
        let claims = crate::browser_profiles::active_profile_claims(&cfg);

        let binding = cfg.profiles.iter()
            .find(|p| p.name == pname)
            .and_then(|p| p.bindings.get(&key).cloned());

        // PROBLEM 105 — one spelling, shared with the delete-profile warning.
        let fallback = cfg.profiles.iter()
            .find(|p| p.name == crate::config::schema::FALLBACK_PROFILE)
            .and_then(|p| p.bindings.get(&key).cloned());

        (pname, binding, fallback, claims)
    };

    // A key that is UNASSIGNED in the active profile must still honour the
    // Founders binding. Before this, a brand-new profile with no bindings did
    // nothing at all for every key (user report 2026-08-10) — the fallback
    // only ever ran when an ASSIGNED binding failed to launch.
    let primary_mapped = binding.as_ref().is_some_and(|b| b.is_mapped());
    let (bind, substituted) = if primary_mapped {
        (binding.unwrap(), false)
    } else {
        match fallback.clone().filter(|f| f.is_mapped()) {
            Some(fb) => {
                log::info!(
                    "engine: Space+{ch} unassigned in '{profile_name}' → using the Founders binding"
                );
                (fb, true)
            }
            None => {
                log::debug!(
                    "engine: Space+{ch} unassigned in '{profile_name}' and no Founders fallback"
                );
                return;
            }
        }
    };

    // NOTE: a pre-flight "absolute path missing" guard used to return here.
    // It PREVENTED the Founders fallback from ever running for a broken path
    // — exactly the case the user asked to see reported (a missing game in
    // Gamers should fall back and SAY SO). smart_cascade now handles the
    // missing path, falls back, and reports the outcome for the toast.
    if let Some(ref app) = bind.app {
        let p = std::path::Path::new(app);
        if p.is_absolute() && !p.exists() {
            log::warn!("engine: absolute path missing: {app} — cascade will try the fallback");
        }
    }

    // NOTE: this used to unminimize/show/focus the "settings" dashboard on
    // EVERY Space+key press, so summoning your browser also threw the
    // SpaceToggle window in your face. It was not needed for foreground
    // rights either — smart_cascade already does the AttachThreadInput dance.
    let app_handle = {
        let s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
        s.app_handle.clone()
    };

    let label = bind.label.clone().unwrap_or_else(|| ch.to_string().to_uppercase());
    // When we already substituted the Founders binding, don't pass it again
    // as the fallback — it IS the primary now.
    let outcome = actions::smart_cascade::smart_cascade(
        &bind,
        if substituted { None } else { fallback.as_ref() },
        &claims,
        Some(app_handle),
    );

    let s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
    if substituted {
        s.emit_toast(&match outcome {
            actions::smart_cascade::CascadeOutcome::Failed => {
                format!("❌ {label} could not be opened")
            }
            _ => format!("↩ {label} · Founders (unassigned in {profile_name})"),
        });
    } else {
        s.emit_toast(&cascade_toast(outcome, &label, fallback.as_ref()));
    }

    // PROBLEM 242 — tell the dashboard a shortcut REALLY fired. Gated on the
    // same outcome the toast is: `Failed` means nothing was focused, launched
    // or minimized, so a key that did nothing must not read as a success on
    // the other side. That gate is what makes the tour's "wrong key does
    // nothing, no error, it waits" behaviour fall out for free.
    if outcome != actions::smart_cascade::CascadeOutcome::Failed {
        s.emit_launched(ch, &label);
    }
}

fn handle_boss_key(state_arc: &Arc<Mutex<EngineState>>) {
    let (boss_state, app_handle) = {
        let s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
        (Arc::clone(&s.boss_key), s.app_handle.clone())
    };

    let msg = actions::boss_key::toggle_boss_key(&boss_state, None);
    crate::show_toast(&app_handle, msg);
}

fn handle_pip(state_arc: &Arc<Mutex<EngineState>>) {
    let (pip_cache, app_handle) = {
        let s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
        (s.pip_cache.clone(), s.app_handle.clone())
    };

    let msg = actions::pip::toggle_pip(&pip_cache);
    crate::show_toast(&app_handle, &msg);
}

/// Space+Tab — fullscreen-preserving PiP (pip.rs §9, PROBLEM 219).
///
/// The same three lines as `handle_pip`, against the same cache HANDLE — but
/// since PROBLEM 220 the two keys own SEPARATE namespaces inside it, keyed by
/// `(hwnd, mode)`. Neither key can see the other's bounds, corner index or
/// fullscreen capture, and pressing one on a window the other is holding takes
/// the window over: the first key's tile is restored, then this key enters
/// fresh. They shared one entry per window until 1.0.93, which the owner
/// reported as *"it did behave oddly."*
fn handle_fullscreen_pip(state_arc: &Arc<Mutex<EngineState>>) {
    let (pip_cache, app_handle) = {
        let s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
        (s.pip_cache.clone(), s.app_handle.clone())
    };

    let msg = actions::pip::toggle_fullscreen_pip(&pip_cache);
    crate::show_toast(&app_handle, &msg);
}

fn handle_focus(state_arc: &Arc<Mutex<EngineState>>) {
    let msg = actions::focus_engine::focus_input_engine();
    let s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
    s.emit_toast(&msg);
}

fn handle_profile_cycle(state_arc: &Arc<Mutex<EngineState>>) {
    // PROBLEM 100 — this path logged NOTHING, so the debug log could not
    // answer "did Space+RightAlt reach the engine?". When the user reported
    // profile switching failing while the Spaceadom window itself was
    // focused, the log was silent either way and a grep for profile cycles
    // returned zero — which reads as "never worked" when it actually means
    // "never recorded". Every dispatched combo must leave a trace; alpha keys
    // already do ("engine: combo Space+X received").
    log::info!("engine: combo Space+RightAlt received (profile cycle)");

    let (new_name, app_handle) = {
        let mut s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
        let name = s.cycle_profile();
        let ah = s.app_handle.clone();
        (name, ah)
    };
    log::info!("engine: profile cycled to '{new_name}' — emitting profile-changed");

    let msg = format!("👤 OS Layer: {new_name}");
    crate::show_toast(&app_handle, &msg);
    let _ = app_handle.emit("profile-changed", &new_name);
}

fn handle_force_close(state_arc: &Arc<Mutex<EngineState>>) {
    // Alt+F4 to the foreground window. Inputs carry the hook cookie (via
    // make_input) so our own hook passes them straight through.
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::{KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP};
        const VK_MENU: u16 = 0x12;
        const VK_F4: u16 = 0x73;
        let inputs = [
            make_input(VK_MENU, KEYBD_EVENT_FLAGS(0)),
            make_input(VK_F4, KEYBD_EVENT_FLAGS(0)),
            make_input(VK_F4, KEYEVENTF_KEYUP),
            make_input(VK_MENU, KEYEVENTF_KEYUP),
        ];
        // PROBLEM 227 — checked. A partial insert of `Alt↓ F4↓` leaves ALT
        // latched, and a latched Alt is the one that opens menu bars and
        // KeyTips in every app the owner uses.
        let _ = crate::hook::send_keys_checked(&inputs, "force close: Alt+F4");
    }
    log::info!("force_close: sent Alt+F4 to foreground window");
    let s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
    s.emit_toast("⌧ Closed App");
}

fn handle_double_tap_up(state_arc: &Arc<Mutex<EngineState>>) {
    let now = tick_count();
    let mut s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
    let last = s.last_up_ts;
    s.last_up_ts = now;

    if now - last < 400 {
        s.last_up_ts = 0;
        // Send Ctrl+Home (scroll to top)
        send_ctrl_key(0x24); // VK_HOME
        s.emit_toast("⤒ Scrolled to Top");
    }
}

fn handle_double_tap_down(state_arc: &Arc<Mutex<EngineState>>) {
    let now = tick_count();
    let mut s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
    let last = s.last_down_ts;
    s.last_down_ts = now;

    if now - last < 400 {
        s.last_down_ts = 0;
        // Send Ctrl+End (scroll to bottom)
        send_ctrl_key(0x23); // VK_END
        s.emit_toast("⤓ Scrolled to Bottom");
    }
}

fn handle_bypass_toggle(state_arc: &Arc<Mutex<EngineState>>) {
    use crate::hook::{BYPASS_MODE, MODIFIER_ACTIVE};
    use std::sync::atomic::Ordering;
    
    let new_state = !BYPASS_MODE.load(Ordering::Relaxed);
    BYPASS_MODE.store(new_state, Ordering::Relaxed);

    // If we just engaged bypass mode, the keyboard hook will skip the SPACE_UP event.
    // We must manually reset MODIFIER_ACTIVE to prevent the Space key from getting permanently stuck.
    if new_state {
        MODIFIER_ACTIVE.store(false, Ordering::Relaxed);
    }

    let state = state_arc.lock().unwrap_or_else(|p| p.into_inner());
    state.emit_bypass_toggled(new_state);
    
    if new_state {
        state.emit_toast("⏸ Spaceadom Paused");
    } else {
        state.emit_toast("▶ Spaceadom Active");
    }
}

fn send_ctrl_key(vk: u16) {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::*;
        let inputs = [
            make_input(VK_CONTROL.0, KEYBD_EVENT_FLAGS(0)),
            make_input(vk, KEYEVENTF_EXTENDEDKEY),
            make_input(vk, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP),
            make_input(VK_CONTROL.0, KEYEVENTF_KEYUP),
        ];
        // PROBLEM 227 — checked: a partial insert leaves CTRL latched.
        let _ = crate::hook::send_keys_checked(&inputs, "media/tab: Ctrl+key");
    }
}

#[cfg(windows)]
fn make_input(
    vk: u16,
    flags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS,
) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                // Hook cookie: without it our own hook re-processes this event —
                // e.g. as a Space+combo if the user is still holding Space.
                dwExtraInfo: 0x7A7A7A7A,
            },
        },
    }
}

fn tick_count() -> u64 {
    #[cfg(windows)]
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
    #[cfg(not(windows))]
    0
}

/// The system-wide shortcuts the HUD's INNER ring lists, in ring order.
///
/// A `const` and not a literal inside the builder so the truth table below can
/// be tested against the real list rather than a copy of it.
///
/// NINE since 2026-08-29 (pip.rs §9, PROBLEM 219). The label is deliberately
/// SHORT: specials render at their full label and a long one widens the inner
/// ring for every chip on it, so "Fullscreen PiP" — shorter than the
/// "Multi-Corner PiP Mode" already sitting next to it — cannot be the entry
/// that decides the ring's size. `toast.ts` measures the labels it is given
/// and has a documented ladder that passes at twelve specials, so the count
/// itself is inside what the overlay was built for.
/// Declared as a SLICE, not a fixed-size array, so the Tab row below can be
/// commented in or out without also editing a length that would then be the
/// one thing left to get wrong.
const HUD_SPECIALS: &[(&str, &str)] = &[
    ("Esc", "Boss Key (Hide All + Mute)"),
    ("`", "Multi-Corner PiP Mode"),
    // SPACE+TAB SPLIT — 1.0.91 ships this line COMMENTED OUT on purpose.
    // The feature's code stays in the tree (pip.rs §9, PROBLEM 219); only its
    // advertisement and its key registration (hook/mod.rs, `VK_TAB`) are
    // disabled, pending the owner's verdict after testing. 1.0.92 = this exact
    // tree with BOTH lines uncommented. Do not delete either one.
    ("Tab", "Fullscreen PiP"), // ← 1.0.92 ON / 1.0.91 commented out
    ("⌫", "Force Close App"),
    ("RAlt", "Cycle OS Profiles"),
    (",", "Contextual Search/Input"),
    (".", "Pause Spaceadom"),
    ("Scroll", "Layer Opacity"),
    ("Up/Dn ×2", "Scroll Top/Bottom"),
];

/// Which specials go into the `GuideHudPayload` — the DETERMINISTIC half of
/// the rows/specials system, and nothing else.
///
/// `hud_band_count` and `hud_show_specials` are one system because the
/// specials occupy the HUD's INNER band, so they only exist when the apps need
/// just the outer one. The full table, and who resolves each row:
///
/// ```text
///   rows   specials   sent from here   who decides the final look
///   one    on         the eight        Rust — inner ring + apps outer
///   one    off        empty            Rust — one app band, no inner ring
///   two    on         empty            Rust — two app bands win outright
///   two    off        empty            Rust
///   auto   on         the eight        THE PAGE — it drops them if the
///                                      measured labels need two bands
///   auto   off        empty            Rust
/// ```
///
/// So a NON-EMPTY return means "show these if one band is enough", not "show
/// these". `"auto"` cannot be resolved here: the band count falls out of
/// measured label widths, which exist only in the overlay document. Anything
/// that is not `"one"` or `"two"` is treated as `"auto"` — an old config that
/// somehow carries `""` or a typo must behave like every previous build did,
/// never like a layout the user did not choose.
pub(crate) fn specials_for_hud(show_specials: bool, band_count: &str) -> Vec<(String, String)> {
    if !show_specials || band_count == "two" {
        return Vec::new();
    }
    HUD_SPECIALS
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

/// The APP half of the `GuideHudPayload`: one `(KEY, label)` pair per MAPPED
/// binding in `profile_name`, sorted by key.
///
/// This was inline in the SpaceDown arm until 1.0.96 and was lifted out for one
/// reason: the Settings preview must show the SAME chips the real ring shows.
/// A preview built from its own copy of this walk would drift the first time
/// either copy was edited, and the drift would be invisible — the preview would
/// simply be subtly wrong about the thing it exists to demonstrate.
///
/// STILL ON THE SPACE-HOLD LATENCY PATH, so the rules that governed it inline
/// still govern it here: no I/O, no locks of its own, and the browser-profile
/// name is READ from the binding rather than resolved.
pub(crate) fn hud_apps_for(
    cfg: &crate::config::AppConfig,
    profile_name: &str,
) -> Vec<(String, String)> {
    let mut binds: Vec<(String, String)> = Vec::new();
    let Some(profile) = cfg.profiles.iter().find(|p| p.name == profile_name) else {
        return binds;
    };
    let mut keys: Vec<_> = profile.bindings.iter().filter(|(_, b)| b.is_mapped()).collect();
    keys.sort_by(|a, b| a.0.cmp(b.0));
    for (key, bind) in keys {
        let label = bind
            .label
            .clone()
            .or_else(|| bind.app.clone())
            .or_else(|| bind.web_url.clone())
            .unwrap_or_default();
        // 2026-08-26 — a key pinned to a browser profile reads "Brave —
        // Studies", not just "Brave", because "Brave" on three different keys
        // tells the user nothing.
        //
        // The human name is READ FROM THE BINDING, never resolved here: this
        // runs on the Space-hold path, which must produce a HUD inside the
        // user's configured delay, and translating "Profile 1" into a name
        // means opening and JSON-parsing the browser's ~96 KB `Local State` on
        // every hold. `browser_profile_name` exists so that I/O never touches
        // this path — see the field's comment in schema.rs.
        //
        // A binding with no profile gets its label back byte-for-byte
        // (`hud_label`'s own test), and the chip already truncates with an
        // ellipsis at 118px, so a long pair cannot disturb the ring.
        let label =
            crate::browser_profiles::hud_label(&label, bind.browser_profile_name.as_deref());
        binds.push((key.to_uppercase(), label));
    }
    binds
}

/// PROBLEM 267 round 3 — the Space ring's pill icons: one entry per
/// `hud_apps_for` row, same order (both walk the mapped bindings sorted by
/// key). The SAME sources the icon ring uses (`middle_ring::build_entries`),
/// minus the shell call: a link → its `site_icon`; otherwise the picker's
/// `icon_override`, else `lookup(target)` — the caller's CACHE-ONLY lookup,
/// so nothing here fetches or extracts on the Space-hold path. `None` = the
/// letter disc, exactly as before. Pure; `hud_icon_tests` pin the rules.
pub(crate) fn hud_icons_for(
    cfg: &crate::config::AppConfig,
    profile_name: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Vec<Option<String>> {
    let Some(profile) = cfg.profiles.iter().find(|p| p.name == profile_name) else {
        return Vec::new();
    };
    let mut keys: Vec<_> = profile.bindings.iter().filter(|(_, b)| b.is_mapped()).collect();
    keys.sort_by(|a, b| a.0.cmp(b.0));
    keys.into_iter()
        .map(|(_, bind)| {
            if bind.web_url.is_some() {
                return bind.site_icon.clone().filter(|s| !s.is_empty());
            }
            bind.icon_override
                .clone()
                .filter(|s| !s.is_empty())
                .map(|b64| crate::middle_ring::as_png_data_url(&b64))
                .or_else(|| {
                    bind.app
                        .as_deref()
                        .and_then(lookup)
                        .map(|b64| crate::middle_ring::as_png_data_url(&b64))
                })
        })
        .collect()
}

/// The active profile's emoji, for the glyph beside the SPACE pill.
///
/// `None` is the NORMAL state, not a degraded one: it is the correct answer for
/// every profile the user has not given an emoji, and `Profile::emoji`'s own
/// comment states the rule every surface owes it — *render your existing look
/// when it is None*. The page honours that literally: with `None` it appends no
/// element to the pill at all, so the wordmark's box, its centring and its
/// `st-space-pop` are byte-identical to every build before this one.
///
/// THE `filter` IS NOT DEFENSIVE PADDING. `emoji_is_valid` already rejects an
/// empty string on the way IN, but this reads a file that can be hand-edited
/// and that predates the field, and `Some("")` would put an empty span inside
/// the pill: nothing visible, and yet not the same DOM as `None`. One condition
/// here is cheaper than a second "is it really absent" rule in the renderer.
///
/// Trimmed rather than tested as-is because a whitespace-only value is the same
/// non-answer as an empty one — and `emoji_is_valid` rejects internal
/// whitespace too, so nothing legitimate is lost by it.
pub(crate) fn profile_emoji_for(
    cfg: &crate::config::AppConfig,
    profile_name: &str,
) -> Option<String> {
    cfg.profiles
        .iter()
        .find(|p| p.name == profile_name)
        .and_then(|p| p.emoji.clone())
        .filter(|e| !e.trim().is_empty())
}

/* ===========================================================================
   THE SETTINGS PREVIEW — payload half. `commands::preview_hud_layout` is the
   command; everything about WHAT gets drawn lives here, beside the real
   builder, so the two can be read against each other on one screen.

   WHAT A PREVIEW IS: the REAL ring, with the user's REAL bindings, drawn in a
   layout he has not chosen yet. It is not a mock-up and it is not a
   screenshot — a mock-up cannot tell him whether HIS twenty-six labels fit,
   which is the only question he is actually asking.

   WHAT IT IS NOT: a config write. The override travels in the payload and
   expires with the show (`guide_hud::HudPreview`). A preview he cancels must
   leave his HUD exactly as he found it.
   =========================================================================== */

/// The three previewable layouts, named as the owner names them in Settings.
///
/// They are NOT a third setting. Each one is a (layout, band-count) PAIR drawn
/// from the two settings that already exist, which is why the mapping lives in
/// one function instead of being spelled out at the call site three times:
///
/// ```text
///   compact  magnetic + auto   the shipped default — bands only if needed
///   wide     classic           the 1.0.88 ring; band count does not apply
///   double   magnetic + two    two app bands, so no specials by construction
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PreviewLayout {
    Compact,
    Wide,
    Double,
}

impl PreviewLayout {
    /// Parse the command's string. UNKNOWN IS AN ERROR, NOT A DEFAULT — and
    /// that is the opposite of how every config field in this app is
    /// normalised, on purpose. A config value arrives from a file that may
    /// predate the field, so falling back to the shipped default is the only
    /// safe reading. This value arrives from a button that was just clicked, so
    /// a value we do not recognise means the dashboard and the backend disagree
    /// about what the buttons are — and silently previewing "compact" for a
    /// click on "double" would teach the owner something false about his own
    /// app.
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "compact" => Some(PreviewLayout::Compact),
            "wide" => Some(PreviewLayout::Wide),
            "double" => Some(PreviewLayout::Double),
            _ => None,
        }
    }

    /// `(layout, bands)` in the PAGE's vocabulary — `hud-layout.ts`'s
    /// `HudLayout` and `hud-band-count.ts`'s `HudBandCount`.
    pub(crate) fn overrides(self) -> (&'static str, &'static str) {
        match self {
            PreviewLayout::Compact => ("magnetic", "auto"),
            PreviewLayout::Wide => ("classic", "auto"),
            PreviewLayout::Double => ("magnetic", "two"),
        }
    }
}

/// Build the payload for one preview.
///
/// THE SPECIALS ARE GATED ON THE OVERRIDE, NOT ON THE SAVED SETTING, and that
/// is the one place a preview could quietly become wrong. `specials_for_hud`'s
/// truth table says two app bands and the specials ring cannot coexist — the
/// specials ARE the inner band. So `double` must send an empty vec, exactly as
/// a real hold with `hud_band_count == "two"` does; passing the saved
/// `hud_band_count` here would send eight specials into a two-band ring and the
/// page would draw three rings on top of each other. The user's own
/// `hud_show_specials` is still honoured — it answers "does he want them at
/// all", which the preview has no business overriding.
pub(crate) fn preview_payload(
    cfg: &crate::config::AppConfig,
    layout: PreviewLayout,
) -> crate::guide_hud::GuideHudPayload {
    let (mode, bands) = layout.overrides();
    let name = cfg.active_profile.clone();
    crate::guide_hud::GuideHudPayload {
        profile_emoji: profile_emoji_for(cfg, &name),
        apps: hud_apps_for(cfg, &name),
        // A preview carries no shell icons (no cache at hand); the stored
        // site/override icons still show, as they would on a real hold.
        app_icons: hud_icons_for(cfg, &name, &|_| None),
        specials: specials_for_hud(cfg.hud_show_specials, bands),
        profile: name,
        preview: Some(crate::guide_hud::HudPreview {
            layout: mode.to_string(),
            bands: bands.to_string(),
        }),
    }
}

#[cfg(test)]
mod band_gate_tests {
    use super::*;

    /// All SIX rows of the rows x specials table, in one place, because the
    /// bug this guards against is silent: a HUD that renders two app bands AND
    /// an inner ring has no room for both and the page would simply overlap
    /// them. There is no error, no log line and nothing to see except a mess.
    #[test]
    fn the_six_rows_and_specials_combinations() {
        let n = HUD_SPECIALS.len();

        // rows = one — the only shape where Rust itself says "draw the ring".
        assert_eq!(
            specials_for_hud(true, "one").len(),
            n,
            "one row + specials ON must send the whole list"
        );
        assert!(
            specials_for_hud(false, "one").is_empty(),
            "one row + specials OFF must send an empty list"
        );

        // rows = two — the new gate. The specials cannot coexist with two app
        // bands and Rust knows that WITHOUT measuring anything, so it decides.
        assert!(
            specials_for_hud(true, "two").is_empty(),
            "two rows must drop the specials even with the setting ON — the \
             inner band is spoken for"
        );
        assert!(
            specials_for_hud(false, "two").is_empty(),
            "two rows + specials OFF is empty for both reasons at once"
        );

        // rows = auto — Rust keeps sending; the PAGE drops them if the
        // measured labels turn out to need two bands. Sending an empty vec
        // here would make "auto" mean "never show specials", which is not what
        // the owner asked for.
        assert_eq!(
            specials_for_hud(true, "auto").len(),
            n,
            "auto + specials ON must still SEND them — only the page can know \
             whether one band fits"
        );
        assert!(
            specials_for_hud(false, "auto").is_empty(),
            "auto + specials OFF must send an empty list"
        );
    }

    /// An unrecognised value must behave like `"auto"`, never like `"two"`.
    /// `""` is what a bare `#[serde(default)]` on a String would have produced
    /// for every config on disk — the failure schema.rs's named default exists
    /// to prevent — and it must not be the value that silently hides the ring.
    #[test]
    fn an_unknown_band_count_falls_back_to_auto_not_to_two() {
        for v in ["", "AUTO", "1", "one row", "three", "auto"] {
            assert_eq!(
                specials_for_hud(true, v).len(),
                HUD_SPECIALS.len(),
                "{v:?} must behave like auto — only the literal \"two\" hides the ring"
            );
        }
    }

    /// THE PREVIEW'S THREE NAMES MAP ONTO THE TWO SETTINGS THAT EXIST, and
    /// nothing else. If this ever drifts, the owner clicks "Double" and is
    /// shown "Compact" with no error anywhere — the preview lying about the
    /// only thing it does.
    #[test]
    fn the_three_preview_layouts_map_to_the_settings_that_exist() {
        assert_eq!(
            PreviewLayout::parse("compact").unwrap().overrides(),
            ("magnetic", "auto"),
            "compact is the shipped default: the magnetic ring, bands only if needed"
        );
        assert_eq!(
            PreviewLayout::parse("wide").unwrap().overrides(),
            ("classic", "auto"),
            "wide is the 1.0.88 ring; classic ignores the band count entirely, so the \
             value paired with it must be the one that changes nothing"
        );
        assert_eq!(
            PreviewLayout::parse("double").unwrap().overrides(),
            ("magnetic", "two"),
            "double is two APP bands"
        );
    }

    /// An unknown name must be REFUSED, not defaulted. This is the one place in
    /// the app where that is right: the string comes from a button in our own
    /// dashboard, so an unrecognised value means the two halves disagree.
    #[test]
    fn an_unknown_preview_layout_is_refused_rather_than_defaulted() {
        for v in ["", "COMPACT", "magnetic", "two", "compact ", "classic"] {
            assert!(
                PreviewLayout::parse(v).is_none(),
                "{v:?} must not parse — a preview that silently draws the wrong shape is \
                 worse than one that does not appear"
            );
        }
    }

    /// THE ROW THAT MATTERS: `double` must send NO specials, because the
    /// specials ARE the inner band and two app bands leave no room for it.
    /// Gated on the OVERRIDE, never on the saved `hud_band_count` — a preview
    /// built from the saved value would send eight specials into a two-band
    /// ring and the page would draw three rings over each other.
    #[test]
    fn the_preview_gates_specials_on_the_override_not_on_the_saved_setting() {
        let n = HUD_SPECIALS.len();
        for (name, layout) in [
            ("compact", PreviewLayout::Compact),
            ("wide", PreviewLayout::Wide),
            ("double", PreviewLayout::Double),
        ] {
            let (_, bands) = layout.overrides();
            let with_setting_on = specials_for_hud(true, bands);
            let with_setting_off = specials_for_hud(false, bands);
            if layout == PreviewLayout::Double {
                assert!(
                    with_setting_on.is_empty(),
                    "{name}: two app bands must drop the specials even with the setting ON"
                );
            } else {
                assert_eq!(
                    with_setting_on.len(),
                    n,
                    "{name}: the user's own 'show specials' setting is still honoured"
                );
            }
            assert!(
                with_setting_off.is_empty(),
                "{name}: specials OFF is the user's decision and no preview may override it"
            );
        }
    }

    /// `hud_apps_for` is the SpaceDown arm's own walk, lifted out verbatim so
    /// the preview cannot show a different chip set from the real ring. The
    /// properties worth pinning are the ones the ring depends on: only MAPPED
    /// keys, sorted, and the key upper-cased for the badge.
    #[test]
    fn hud_apps_for_returns_only_mapped_keys_sorted_and_upper_cased() {
        use crate::config::{KeyBinding, Profile};
        let mut cfg = crate::config::AppConfig::default();
        let mut bindings = crate::config::BindingMap::new();
        bindings.insert(
            "c".to_string(),
            KeyBinding { label: Some("Chrome".into()), app: Some("chrome.exe".into()),
                         ..Default::default() },
        );
        bindings.insert(
            "a".to_string(),
            KeyBinding { label: Some("Afterburner".into()), app: Some("ab.exe".into()),
                         ..Default::default() },
        );
        // Present but UNMAPPED — the ring must not draw a chip for it.
        bindings.insert("z".to_string(), KeyBinding::default());
        cfg.profiles = vec![Profile { name: "Preview Test".into(), bindings, emoji: None }];
        cfg.active_profile = "Preview Test".into();

        let apps = hud_apps_for(&cfg, "Preview Test");
        assert_eq!(
            apps,
            vec![
                ("A".to_string(), "Afterburner".to_string()),
                ("C".to_string(), "Chrome".to_string()),
            ],
            "mapped keys only, sorted by key, badge upper-cased"
        );
        assert!(
            hud_apps_for(&cfg, "A Profile That Does Not Exist").is_empty(),
            "an unknown profile must produce an empty ring, never a panic"
        );
    }

    /// The preview payload is a PROJECTION: same profile, same chips as the
    /// real path, plus the override. If these two ever diverge the preview
    /// stops being evidence about the user's own config.
    #[test]
    fn the_preview_payload_carries_the_same_chips_as_a_real_hold() {
        use crate::config::{KeyBinding, Profile};
        let mut cfg = crate::config::AppConfig::default();
        let mut bindings = crate::config::BindingMap::new();
        bindings.insert(
            "b".to_string(),
            KeyBinding { label: Some("Brave".into()), app: Some("brave.exe".into()),
                         ..Default::default() },
        );
        cfg.profiles = vec![Profile {
            name: "Live".into(),
            bindings,
            emoji: Some("🎯".into()),
        }];
        cfg.active_profile = "Live".into();
        cfg.hud_show_specials = true;

        let p = preview_payload(&cfg, PreviewLayout::Compact);
        assert_eq!(p.profile, "Live");
        assert_eq!(p.apps, hud_apps_for(&cfg, "Live"), "the same chips, from the same builder");
        assert_eq!(
            p.profile_emoji.as_deref(),
            Some("🎯"),
            "the preview shows the profile's own emoji — it is a projection of the real ring, \
             not a sample of one"
        );
        let pv = p.preview.expect("a preview payload must carry its override");
        assert_eq!((pv.layout.as_str(), pv.bands.as_str()), ("magnetic", "auto"));

        // And the real path's payload must carry NO override, or the page
        // would honour a stale preview on an ordinary Space-hold.
        let d = preview_payload(&cfg, PreviewLayout::Double);
        assert!(d.specials.is_empty(), "double sends no specials");
        assert_eq!(d.preview.map(|p| p.bands), Some("two".to_string()));
    }

    /// THE THREE STATES THE SPACE PILL HAS TO SURVIVE. The absent case is the
    /// one that matters most: `None` is what every existing user has, and the
    /// page's contract is that `None` draws the pill EXACTLY as it drew before
    /// this feature existed. A blank string must reach the page as `None` too,
    /// or the renderer would need a second "is it really absent" rule.
    #[test]
    fn profile_emoji_is_absent_blank_or_real_and_blank_reads_as_absent() {
        use crate::config::Profile;
        let mk = |emoji: Option<&str>| {
            let mut cfg = crate::config::AppConfig::default();
            cfg.profiles = vec![Profile {
                name: "P".into(),
                bindings: crate::config::BindingMap::new(),
                emoji: emoji.map(str::to_string),
            }];
            cfg.active_profile = "P".into();
            cfg
        };
        assert_eq!(profile_emoji_for(&mk(None), "P"), None, "absent stays absent");
        assert_eq!(
            profile_emoji_for(&mk(Some("")), "P"),
            None,
            "an empty string is a non-answer, and must not become an empty element in the pill"
        );
        assert_eq!(
            profile_emoji_for(&mk(Some("   ")), "P"),
            None,
            "whitespace-only is the same non-answer"
        );
        assert_eq!(
            profile_emoji_for(&mk(Some("🎯")), "P").as_deref(),
            Some("🎯"),
            "a real emoji reaches the payload unchanged"
        );
        assert_eq!(
            profile_emoji_for(&mk(Some("🎯")), "Some Other Profile"),
            None,
            "an unknown profile must produce None, never a panic and never another \
             profile's emoji"
        );
    }

    /// The gate must not quietly edit the list it is gating.
    #[test]
    fn the_sent_list_is_the_real_one_in_ring_order() {
        let sent = specials_for_hud(true, "one");
        // Counted from the real list, NOT a literal, because Space+Tab is
        // deliberately commented out of `HUD_SPECIALS` in 1.0.91 and back in
        // for 1.0.92 — the same test has to pass for both builds.
        assert_eq!(sent.len(), HUD_SPECIALS.len());
        assert_eq!(sent[0].0, "Esc");
        assert_eq!(sent[0].1, "Boss Key (Hide All + Mute)");
        assert_eq!(sent[sent.len() - 1].0, "Up/Dn ×2");
        assert_eq!(sent[1].0, "`");
        // Tab sits next to the backtick, because the two PiPs are the pair a
        // user has to tell apart and the ring is the only place that says so.
        // Asserted only WHEN PRESENT: absent is the legitimate 1.0.91 shape.
        if let Some(i) = sent.iter().position(|(k, _)| k == "Tab") {
            assert_eq!(i, 2, "Tab must sit immediately after the backtick");
            assert_eq!(
                sent[i].1, "Fullscreen PiP",
                "the label must stay SHORT — specials render at their full label and a long one \
                 widens the inner ring for every chip on it"
            );
        }
    }
}

/// PROBLEM 267 — the owner's `middle_ring_style` switch, pinned at the one
/// place `dispatch` consults it. `GuideHud` must reproduce phase 1 exactly,
/// which means the SAME `SpaceDown` normalisation PROBLEM 263 shipped; the
/// default must be the new ring.
#[cfg(test)]
mod middle_route_tests {
    use super::*;
    use crate::config::MiddleRingStyle;

    #[test]
    fn guide_hud_normalises_to_space_down_and_icon_ring_keeps_its_own_arm() {
        assert!(matches!(routed_middle_event(MiddleRingStyle::GuideHud), HookEvent::SpaceDown));
        assert!(matches!(routed_middle_event(MiddleRingStyle::IconRing), HookEvent::MiddleButtonDown));
        assert!(
            matches!(routed_middle_event(MiddleRingStyle::default()), HookEvent::MiddleButtonDown),
            "the default is the icon ring (owner, 2026-09-13)"
        );
    }

    /// `ring_icon_for` answers from the cache without touching the shell, and
    /// a cache hit is returned byte-for-byte. (The extraction leg needs a
    /// desktop; `picker_worker::tests::icons_extract_on_a_non_main_sta_thread`
    /// already proves the extractor on a non-main thread.)
    /// PROBLEM 267 round 3 — the HUD's pill icons follow the ring's rules,
    /// one per `hud_apps_for` row in the same order, cache-only.
    #[test]
    fn hud_icons_follow_the_rings_sources_and_never_extract() {
        use crate::config::{KeyBinding, Profile};
        let mut cfg = crate::config::AppConfig::default();
        let mut map = crate::config::BindingMap::new();
        map.insert("b".into(), KeyBinding { label: Some("Brave".into()), app: Some("brave.exe".into()), ..Default::default() });
        map.insert("g".into(), KeyBinding { label: Some("GitHub".into()), web_url: Some("https://github.com".into()), site_icon: Some("data:image/x-icon;base64,AAEC".into()), ..Default::default() });
        map.insert("p".into(), KeyBinding { label: Some("Pinned".into()), app: Some("pinned.exe".into()), icon_override: Some("QUJD".into()), ..Default::default() });
        map.insert("x".into(), KeyBinding { label: Some("Cold".into()), app: Some("cold.exe".into()), ..Default::default() });
        cfg.profiles = vec![Profile { name: "P".into(), bindings: map, emoji: None }];
        cfg.active_profile = "P".into();
        let lookup = |t: &str| -> Option<String> { (t == "brave.exe").then(|| "iVBOR".to_string()) };
        let apps = hud_apps_for(&cfg, "P");
        let icons = hud_icons_for(&cfg, "P", &lookup);
        assert_eq!(apps.len(), icons.len());
        assert_eq!(apps.iter().map(|a| a.0.as_str()).collect::<Vec<_>>(), vec!["B", "G", "P", "X"]);
        assert_eq!(icons[0].as_deref(), Some("data:image/png;base64,iVBOR"), "shell icon from the cache");
        assert_eq!(icons[1].as_deref(), Some("data:image/x-icon;base64,AAEC"), "a link's site_icon");
        assert_eq!(icons[2].as_deref(), Some("data:image/png;base64,QUJD"), "icon_override wins");
        assert_eq!(icons[3], None, "not cached: the letter disc, no extraction");
        assert!(hud_icons_for(&cfg, "Nope", &lookup).is_empty());
    }

    #[test]
    fn ring_icon_for_answers_from_the_cache_first() {
        let cache = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        cache.lock().unwrap().insert("brave.exe".to_string(), "QUJD".to_string());
        assert_eq!(ring_icon_for("brave.exe", Some(&cache)).as_deref(), Some("QUJD"));
    }
}
