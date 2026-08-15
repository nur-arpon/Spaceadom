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
use tauri::Emitter;
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

    fn cancel_hud(&mut self) {
        if let Some(tx) = self.hud_cancel_tx.take() {
            let _ = tx.send(true);
        }
        guide_hud::hide_guide_hud();
    }
}

/// Start the async engine actor. Call once on app startup.
pub fn start_engine(
    rx: Receiver<HookEvent>,
    state_arc: Arc<Mutex<EngineState>>,
) {
    tauri::async_runtime::spawn(async move {
        log::info!("engine: actor started");

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

            let state_clone = Arc::clone(state_arc);
            tauri::async_runtime::spawn(async move {
                tokio::select! {
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(hud_delay_ms)) => {
                        // Show HUD if not cancelled
                        if !*cancel_rx.borrow() {
                            let (profile_name, bindings, specials) = {
                                let s = state_clone.lock().unwrap_or_else(|p| p.into_inner());
                                let cfg = s.config.read().unwrap_or_else(|p| p.into_inner());
                                let name = cfg.active_profile.clone();
                                let mut binds: Vec<(String, String)> = Vec::new();

                                // CORE_AIM: the HUD must show the CURRENT
                                // PROFILE's shortcuts — the user's actual app
                                // keys first, system shortcuts after.
                                if let Some(profile) =
                                    cfg.profiles.iter().find(|p| p.name == name)
                                {
                                    let mut keys: Vec<_> = profile
                                        .bindings
                                        .iter()
                                        .filter(|(_, b)| b.is_mapped())
                                        .collect();
                                    keys.sort_by(|a, b| a.0.cmp(b.0));
                                    for (key, bind) in keys {
                                        let label = bind
                                            .label
                                            .clone()
                                            .or_else(|| bind.app.clone())
                                            .or_else(|| bind.web_url.clone())
                                            .unwrap_or_default();
                                        binds.push((key.to_uppercase(), label));
                                    }
                                }

                                // System-wide shortcuts — separate list; the
                                // HUD renders these FIRST (user's direction:
                                // specials are the hard-to-remember part).
                                let specials: Vec<(String, String)> = vec![
                                    ("Esc".to_string(), "Boss Key (Hide All + Mute)".to_string()),
                                    ("`".to_string(), "Multi-Corner PiP Mode".to_string()),
                                    ("⌫".to_string(), "Force Close App".to_string()),
                                    ("RAlt".to_string(), "Cycle OS Profiles".to_string()),
                                    (",".to_string(), "Contextual Search/Input".to_string()),
                                    (".".to_string(), "Pause Spaceadom".to_string()),
                                    ("Scroll".to_string(), "Layer Opacity".to_string()),
                                    ("Up/Dn ×2".to_string(), "Scroll Top/Bottom".to_string()),
                                ];

                                (name, binds, specials)
                            };
                            guide_hud::show_guide_hud(&profile_name, bindings, specials);
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
            s.cancel_hud();
        }

        // ---------------------------------------------------------------
        // Combo key while Space held
        // ---------------------------------------------------------------
        HookEvent::KeyCombo(combo) => {
            // Cancel guide HUD immediately on any combo
            {
                let mut s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
                s.cancel_hud();
            }

            match combo {
                KeyCombo::Alpha(ch)       => handle_alpha(ch, state_arc),
                KeyCombo::Special(name)   => handle_special(name, state_arc),
                KeyCombo::Escape          => handle_boss_key(state_arc),
                KeyCombo::Backtick        => handle_pip(state_arc),
                KeyCombo::Backspace       => handle_force_close(state_arc),
                KeyCombo::Comma           => handle_focus(state_arc),
                KeyCombo::RightAlt        => handle_profile_cycle(state_arc),
                KeyCombo::UpArrow         => handle_double_tap_up(state_arc),
                KeyCombo::DownArrow       => handle_double_tap_down(state_arc),
                KeyCombo::Period          => handle_bypass_toggle(state_arc),
            }
        }

        // ---------------------------------------------------------------
        // Mouse wheel scroll with Space held
        // ---------------------------------------------------------------
        HookEvent::WheelUp => {
            {
                let mut s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
                s.cancel_hud();
            }
            actions::opacity::increase_opacity();
        }
        HookEvent::WheelDown => {
            {
                let mut s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
                s.cancel_hud();
            }
            actions::opacity::decrease_opacity();
        }
    }
}

// ---------------------------------------------------------------------------
// Action handlers
// ---------------------------------------------------------------------------

/// Handle Space + F1–F12 / Enter / Tab / Left / Right.
/// Routes through AppConfig.special_keys if user has configured a binding.
fn handle_special(key_name: String, state_arc: &Arc<Mutex<EngineState>>) {
    println!("Received modifier: Space, Trigger: Key({})", key_name);

    let (binding, fallback) = {
        let s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
        let cfg = s.config.read().unwrap_or_else(|p| p.into_inner());
        let bind = cfg.special_keys.get(&key_name).cloned();
        // Founders-profile fallback not applicable for special keys, but keep API consistent
        (bind, None::<crate::config::KeyBinding>)
    };

    let Some(bind) = binding else {
        log::debug!("engine: no special_key binding for Space+{key_name}");
        return; // key passes through — not configured
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
    
    let outcome = actions::smart_cascade::smart_cascade(&bind, fallback.as_ref(), Some(app_handle));

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

    let (profile_name, binding, fallback) = {
        let s = state_arc.lock().unwrap_or_else(|p| p.into_inner());
        let cfg = s.config.read().unwrap_or_else(|p| p.into_inner());
        let pname = cfg.active_profile.clone();
        let key = ch.to_string();

        let binding = cfg.profiles.iter()
            .find(|p| p.name == pname)
            .and_then(|p| p.bindings.get(&key).cloned());

        // PROBLEM 105 — one spelling, shared with the delete-profile warning.
        let fallback = cfg.profiles.iter()
            .find(|p| p.name == crate::config::schema::FALLBACK_PROFILE)
            .and_then(|p| p.bindings.get(&key).cloned());

        (pname, binding, fallback)
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
        use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP};
        const VK_MENU: u16 = 0x12;
        const VK_F4: u16 = 0x73;
        let inputs = [
            make_input(VK_MENU, KEYBD_EVENT_FLAGS(0)),
            make_input(VK_F4, KEYBD_EVENT_FLAGS(0)),
            make_input(VK_F4, KEYEVENTF_KEYUP),
            make_input(VK_MENU, KEYEVENTF_KEYUP),
        ];
        SendInput(&inputs, std::mem::size_of::<windows::Win32::UI::Input::KeyboardAndMouse::INPUT>() as i32);
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
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
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
