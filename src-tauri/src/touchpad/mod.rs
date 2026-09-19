//! TOUCHPAD T2 — the edge-gesture engine (docs/TOUCHPAD-BRIEF-T2.md).
//!
//! `raw.rs` (T1, proven) reads Precision Touchpad HID reports; `gesture.rs`
//! (pure) turns contacts into `Enter/Move/Exit`; `actions.rs` applies volume,
//! brightness and scrub in-process; `bands.rs` publishes the one atomic the
//! LL mouse hook reads to freeze the pointer. This module wires them:
//!
//! * a SUPERVISOR thread, from boot, that detects the pad (`detect_presence`)
//!   every 1.5 s, emits `touchpad-caps` when it changes, and starts/stops the
//!   READER thread as a Precision pad appears/vanishes and bands are
//!   enabled/disabled;
//! * a READER thread that runs `raw::run_sink`, feeds the gesture machine,
//!   drives the actions, sets `BAND_LIVE`, and emits `touchpad-live` at
//!   ≤ 30 Hz.
//!
//! Events (global `emit`, the only arrangement that works here):
//!   `touchpad-caps  { presence, pad_mm: [w,h] }`
//!   `touchpad-live  { edge, action, value_pct, travel }`  (edge=null on end)
//!
//! Presence is `"precision"` or `"none"`. A non-Precision touchpad (one whose
//! driver exposes it only as a mouse) is NOT reliably distinguishable from an
//! ordinary mouse through Raw Input, so it is reported as `"none"` — see the
//! ship report. Only a Precision pad (0x0D/0x05) ever drives the feature or
//! shows the home thumbnail.
#![cfg(windows)]

pub mod actions;
pub mod bands;
pub mod gesture;
pub mod raw;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::config::schema::{AppConfig, BandAction, TouchEdge, Touchpad};
use bands::BAND_LIVE;
use gesture::{FracContact, Gesture, GestureEvent};

// --- shared state -----------------------------------------------------------

static APP: OnceLock<AppHandle> = OnceLock::new();
/// The current touchpad settings, refreshed by `apply_config`. The reader
/// reloads it only while its gesture is idle (never mid-slide).
static CONFIG: OnceLock<RwLock<Touchpad>> = OnceLock::new();
/// Bumped on every `apply_config`, so the reader knows to reload.
static CONFIG_GEN: AtomicU64 = AtomicU64::new(0);
/// Whether the reader thread is currently running.
static READER_RUNNING: AtomicBool = AtomicBool::new(false);
/// The reader thread's id, for `PostThreadMessageW(WM_QUIT)` on stop.
static READER_TID: AtomicU32 = AtomicU32::new(0);

fn config() -> &'static RwLock<Touchpad> {
    CONFIG.get_or_init(|| RwLock::new(Touchpad::default()))
}

fn snapshot() -> Touchpad {
    config().read().map(|c| c.clone()).unwrap_or_default()
}

// --- presence ---------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Precision,
    None,
}

impl Presence {
    fn wire(self) -> &'static str {
        match self {
            Presence::Precision => "precision",
            Presence::None => "none",
        }
    }
}

/// The first Precision Touchpad on the machine, if any.
fn find_pad() -> Option<raw::DeviceInfo> {
    raw::enumerate().into_iter().find(|d| d.is_touch_pad())
}

/// What kind of touchpad this machine has, right now.
pub fn detect_presence() -> Presence {
    if find_pad().is_some() {
        Presence::Precision
    } else {
        Presence::None
    }
}

#[derive(Serialize, Clone)]
pub struct CapsPayload {
    pub presence: &'static str,
    /// `[width_mm, height_mm]`, or `null` when the descriptor gives no size.
    pub pad_mm: Option<[f64; 2]>,
}

/// The caps RIGHT NOW, for the page to ASK on load. The `touchpad-caps`
/// event alone was not enough: it fires once at boot — before the dashboard
/// page has a listener — and then only on CHANGE, so a freshly loaded page
/// never heard it and showed "none detected" on a machine whose log said
/// `presence=precision` (owner, 2026-09-19 16:55, 1.0.120). Same class as the
/// overlay's theme seeding (CLAUDE.md: an event that only fires on change
/// leaves a freshly-opened page in the wrong state — seed from a command).
pub fn current_caps() -> CapsPayload {
    let pad = find_pad();
    let presence = if pad.is_some() { Presence::Precision } else { Presence::None };
    let pad_mm = pad
        .as_ref()
        .and_then(|d| d.pad.as_ref())
        .and_then(|p| p.size_mm())
        .map(|(w, h)| [w, h]);
    CapsPayload { presence: presence.wire(), pad_mm }
}

fn emit_caps(app: &AppHandle, presence: Presence, pad: Option<&raw::DeviceInfo>) {
    let pad_mm = pad
        .and_then(|d| d.pad.as_ref())
        .and_then(|p| p.size_mm())
        .map(|(w, h)| [w, h]);
    let _ = app.emit(
        "touchpad-caps",
        CapsPayload { presence: presence.wire(), pad_mm },
    );
    log::info!(
        "touchpad: caps — presence={} pad_mm={:?} (touchpad-edge-gestures-t2-spaceadom-120)",
        presence.wire(),
        pad_mm
    );
}

// --- public entry points ----------------------------------------------------

/// Called once from `lib.rs setup()`. Seeds the config snapshot and starts the
/// supervisor. Safe on a machine with no touchpad (the supervisor simply
/// reports `presence=none` and never starts a reader).
pub fn init(app: AppHandle, cfg: &AppConfig) {
    let _ = APP.set(app.clone());
    *config().write().unwrap_or_else(|p| p.into_inner()) = cfg.touchpad.clone();
    CONFIG_GEN.fetch_add(1, Ordering::Relaxed);
    spawn_supervisor(app);
}

/// Called from `config::save` (the one funnel every mutation goes through), so
/// enabling a band starts the reader and disabling the last one stops it, with
/// no restart. Cheap and idempotent.
pub fn apply_config(cfg: &AppConfig) {
    if APP.get().is_none() {
        return; // init() has not run yet (very early boot); nothing to do.
    }
    *config().write().unwrap_or_else(|p| p.into_inner()) = cfg.touchpad.clone();
    CONFIG_GEN.fetch_add(1, Ordering::Relaxed);
}

// --- supervisor -------------------------------------------------------------

fn spawn_supervisor(app: AppHandle) {
    std::thread::Builder::new()
        .name("st-touchpad-supervisor".into())
        .spawn(move || {
            let mut last_presence: Option<Presence> = None;
            loop {
                let pad = find_pad();
                let presence = if pad.is_some() { Presence::Precision } else { Presence::None };
                if last_presence != Some(presence) {
                    emit_caps(&app, presence, pad.as_ref());
                    last_presence = Some(presence);
                }

                let want_reader =
                    presence == Presence::Precision && snapshot().any_enabled();
                let running = READER_RUNNING.load(Ordering::Relaxed);
                if want_reader && !running {
                    if let Some(pad) = pad {
                        start_reader(app.clone(), pad);
                    }
                } else if !want_reader && running {
                    stop_reader();
                }

                std::thread::sleep(Duration::from_millis(1500));
            }
        })
        .map(|_| ())
        .unwrap_or_else(|e| log::warn!("touchpad: could not start the supervisor: {e}"));
}

fn stop_reader() {
    let tid = READER_TID.load(Ordering::Relaxed);
    if tid == 0 {
        return;
    }
    use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
    // SAFETY: post WM_QUIT to end the reader's GetMessageW pump.
    unsafe {
        let _ = PostThreadMessageW(
            tid,
            WM_QUIT,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(0),
        );
    }
    log::info!("touchpad: reader stop requested (no band enabled or pad gone)");
}

// --- reader -----------------------------------------------------------------

fn start_reader(app: AppHandle, pad: raw::DeviceInfo) {
    if READER_RUNNING.swap(true, Ordering::Relaxed) {
        return; // already running
    }
    std::thread::Builder::new()
        .name("st-touchpad-reader".into())
        .spawn(move || {
            reader_loop(app, pad);
            READER_RUNNING.store(false, Ordering::Relaxed);
            READER_TID.store(0, Ordering::Relaxed);
            BAND_LIVE.store(false, Ordering::Relaxed);
            log::info!("touchpad: reader loop ended");
        })
        .map(|_| ())
        .unwrap_or_else(|e| {
            READER_RUNNING.store(false, Ordering::Relaxed);
            log::warn!("touchpad: could not start the reader: {e}");
        });
}

/// The percent shown while a band is live. Volume/brightness read the real
/// value; scrub shows its own speed as a fraction of the maximum tap rate.
fn live_value_pct(action: BandAction, travel: f32, sensitivity: u8) -> u8 {
    match action {
        BandAction::Volume => actions::get_volume_pct().unwrap_or(0),
        BandAction::Brightness => actions::get_brightness().unwrap_or(0),
        BandAction::Scrub => {
            let r = actions::scrub_rate(travel, sensitivity);
            ((r / actions::SCRUB_MAX_RATE) * 100.0).round().clamp(0.0, 100.0) as u8
        }
        BandAction::None => 0,
    }
}

#[derive(Serialize, Clone)]
struct LivePayload {
    edge: Option<&'static str>,
    action: Option<&'static str>,
    value_pct: Option<u8>,
    travel: Option<f32>,
}

fn edge_wire(e: TouchEdge) -> &'static str {
    match e {
        TouchEdge::Left => "left",
        TouchEdge::Right => "right",
        TouchEdge::Top => "top",
        TouchEdge::Bottom => "bottom",
    }
}

fn action_wire(a: BandAction) -> &'static str {
    match a {
        BandAction::Brightness => "brightness",
        BandAction::Volume => "volume",
        BandAction::Scrub => "scrub",
        BandAction::None => "none",
    }
}

fn reader_loop(app: AppHandle, pad: raw::DeviceInfo) {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    use windows::Win32::System::Threading::GetCurrentThreadId;

    // SAFETY: COM for this thread (Core Audio + WMI). MTA matches boss_key.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        READER_TID.store(GetCurrentThreadId(), Ordering::Relaxed);
    }

    let caps = match pad.pad.clone() {
        Some(c) => c,
        None => {
            log::warn!("touchpad: reader — pad has no caps; nothing to normalise against");
            return;
        }
    };
    let (Some(x), Some(y)) = (caps.x.clone(), caps.y.clone()) else {
        log::warn!("touchpad: reader — pad has no X/Y caps");
        return;
    };
    // Aspect = short/long, so a band's physical thickness is uniform on every
    // edge. Assume X (usually 119 mm here) is the long side.
    let (long_mm, short_mm) = match caps.size_mm() {
        Some((w, h)) => (w.max(h), w.min(h)),
        None => (x.logical_span().max(1) as f64, y.logical_span().max(1) as f64),
    };
    let aspect = if long_mm > 0.0 { (short_mm / long_mm) as f32 } else { 1.0 };

    let mut g = Gesture::new(snapshot(), aspect);
    let mut seen_gen = CONFIG_GEN.load(Ordering::Relaxed);

    // Per-gesture action state.
    let mut prev_travel = 0.0f32;
    let mut scrub_pending = 0.0f32;
    let mut last_tick = Instant::now();
    let mut last_bright = Instant::now() - Duration::from_secs(1);
    let mut last_emit = Instant::now() - Duration::from_secs(1);

    log::info!(
        "touchpad: reader started — pad {}x{} logical, aspect {:.3}",
        x.logical_span(),
        y.logical_span(),
        aspect
    );

    let on_report = Box::new(move |rep: &raw::Report| {
        // Reload settings only while idle, so a change never rewrites a slide.
        let now_gen = CONFIG_GEN.load(Ordering::Relaxed);
        if now_gen != seen_gen && !g.is_live() {
            g.set_config(snapshot(), aspect);
            seen_gen = now_gen;
        }

        let cfg = snapshot();
        if !cfg.any_enabled() {
            if BAND_LIVE.swap(false, Ordering::Relaxed) {
                emit_end(&app);
            }
            return;
        }

        let contacts: Vec<FracContact> = rep
            .tips()
            .map(|c| FracContact { id: c.id, fx: x.fraction(c.x) as f32, fy: y.fraction(c.y) as f32 })
            .collect();

        let Some(ev) = g.feed(&contacts) else { return };
        match ev {
            GestureEvent::Enter(edge) => {
                let action = cfg.band(edge).action;
                prev_travel = 0.0;
                scrub_pending = 0.0;
                last_tick = Instant::now();
                BAND_LIVE.store(true, Ordering::Relaxed);
                let value = live_value_pct(action, 0.0, cfg.band(edge).sensitivity);
                emit_live(&app, edge, action, value, 0.0);
                last_emit = Instant::now();
            }
            GestureEvent::Move { edge, travel } => {
                let band = *cfg.band(edge);
                let now = Instant::now();
                let dt = now.duration_since(last_tick).as_secs_f32().clamp(0.0, 0.2);
                last_tick = now;
                let mut value = None;
                match band.action {
                    BandAction::Volume => {
                        let delta = actions::analogue_gain(band.sensitivity) * (travel - prev_travel);
                        value = actions::add_volume(delta);
                    }
                    BandAction::Brightness => {
                        // Rate-limit sets to ~20 Hz; accumulate travel between.
                        if now.duration_since(last_bright) >= Duration::from_millis(50) {
                            let delta = actions::analogue_gain(band.sensitivity) * (travel - prev_travel);
                            let step = (delta * 100.0).round() as i32;
                            if step != 0 {
                                value = actions::add_brightness(step);
                                prev_travel = travel;
                            }
                            last_bright = now;
                        } else {
                            // Hold prev_travel so the next set sees the full delta.
                            value = actions::get_brightness();
                        }
                    }
                    BandAction::Scrub => {
                        let rate = actions::scrub_rate(travel, band.sensitivity);
                        scrub_pending += rate * dt;
                        let forward = travel >= 0.0;
                        while scrub_pending >= 1.0 {
                            actions::scrub_tap(forward);
                            scrub_pending -= 1.0;
                        }
                        value = Some(live_value_pct(BandAction::Scrub, travel, band.sensitivity));
                    }
                    BandAction::None => {}
                }
                if !matches!(band.action, BandAction::Brightness) {
                    prev_travel = travel;
                }
                // Emit at ≤ 30 Hz.
                if now.duration_since(last_emit) >= Duration::from_millis(33) {
                    emit_live(&app, edge, band.action, value.unwrap_or(0), travel);
                    last_emit = now;
                }
            }
            GestureEvent::Exit(_edge) => {
                BAND_LIVE.store(false, Ordering::Relaxed);
                emit_end(&app);
            }
        }
    });

    if let Err(e) = raw::run_sink(&pad, on_report) {
        log::warn!("touchpad: sink failed: {e}");
    }
    // Whatever happens, do not leave the pointer frozen.
    BAND_LIVE.store(false, Ordering::Relaxed);
}

fn emit_live(app: &AppHandle, edge: TouchEdge, action: BandAction, value_pct: u8, travel: f32) {
    let _ = app.emit(
        "touchpad-live",
        LivePayload {
            edge: Some(edge_wire(edge)),
            action: Some(action_wire(action)),
            value_pct: Some(value_pct),
            travel: Some(travel),
        },
    );
}

fn emit_end(app: &AppHandle) {
    let _ = app.emit(
        "touchpad-live",
        LivePayload { edge: None, action: None, value_pct: None, travel: None },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_wire_strings() {
        assert_eq!(Presence::Precision.wire(), "precision");
        assert_eq!(Presence::None.wire(), "none");
    }

    #[test]
    fn edge_and_action_wire_strings() {
        assert_eq!(edge_wire(TouchEdge::Top), "top");
        assert_eq!(action_wire(BandAction::Scrub), "scrub");
        assert_eq!(action_wire(BandAction::None), "none");
    }

    #[test]
    fn live_value_for_scrub_is_a_fraction_of_the_cap() {
        // Full travel → 100 %; dead zone → 0.
        assert_eq!(live_value_pct(BandAction::Scrub, 1.0, 6), 100);
        assert_eq!(live_value_pct(BandAction::Scrub, 0.0, 6), 0);
    }
}
