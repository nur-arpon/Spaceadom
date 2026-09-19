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
/// value; scrub shows its own speed as a fraction of the maximum tap rate;
/// chords show no percent (the page shows the chord name + a step counter).
fn live_value_pct(action: &BandAction, travel: f32, sensitivity: u8) -> u8 {
    match action {
        BandAction::Volume => actions::get_volume_pct().unwrap_or(0),
        BandAction::Brightness => actions::get_brightness().unwrap_or(0),
        BandAction::Scrub => {
            let r = actions::scrub_rate(travel, sensitivity);
            ((r / actions::SCRUB_MAX_RATE) * 100.0).round().clamp(0.0, 100.0) as u8
        }
        BandAction::None | BandAction::Chords { .. } => 0,
    }
}

/// Travel in units of the pad's SHORT side (what `chord_steps` wants): a
/// vertical edge's travel already is; a horizontal edge's is a fraction of
/// the long side, so divide by `aspect` (short/long). Pure.
fn travel_short_side(edge: TouchEdge, travel: f32, aspect: f32) -> f32 {
    if edge.is_vertical() || aspect <= 0.0 || !aspect.is_finite() {
        travel
    } else {
        travel / aspect
    }
}

#[derive(Serialize, Clone)]
struct LivePayload {
    edge: Option<&'static str>,
    action: Option<&'static str>,
    value_pct: Option<u8>,
    travel: Option<f32>,
    /// `Chords` only: the forward chord's name ("Ctrl+Tab") for the readout.
    chord: Option<String>,
    /// `Chords` only: the signed step count sent so far this gesture.
    steps: Option<i32>,
}

fn edge_wire(e: TouchEdge) -> &'static str {
    match e {
        TouchEdge::Left => "left",
        TouchEdge::Right => "right",
        TouchEdge::Top => "top",
        TouchEdge::Bottom => "bottom",
    }
}

fn action_wire(a: &BandAction) -> &'static str {
    a.wire()
}

/// The readout name for a `Chords` band: its forward chord, or "(no keys)".
fn chord_label(a: &BandAction) -> Option<String> {
    match a {
        BandAction::Chords { forward, .. } => Some(crate::engine::specials::chord_name(forward)),
        _ => None,
    }
}

// --- the live slide toast (owner, 2026-09-19, 1.0.123) -----------------------

/// The one key every edge's live toast shares: only one band can be live at a
/// time, so one pill is the whole story.
const LIVE_TOAST_KEY: &str = "touchpad-slide";

/// Minimum spacing between two in-place updates of the live toast: 125 ms is
/// ≤ 8 Hz, the owner's ceiling. Enter always shows at once; Exit always
/// flushes the last value. Move updates are what this throttles.
pub(crate) const LIVE_TOAST_MIN_MS: u64 = 125;

/// The rate limiter for the live toast — PURE (milliseconds in, bool out), so
/// it can be tested without a clock. `reset` on Enter; `allow(now)` on every
/// Move says whether an update may go out now and records it if so.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiveToastLimiter {
    last_ms: Option<u64>,
}

impl LiveToastLimiter {
    pub(crate) fn reset(&mut self) {
        self.last_ms = None;
    }

    /// True when an update may go out at `now_ms`, recording it. The first
    /// call after `reset` always passes; after that, only once per
    /// `LIVE_TOAST_MIN_MS`. A clock that went backwards is treated as "too
    /// soon" (saturating), never as a burst.
    pub(crate) fn allow(&mut self, now_ms: u64) -> bool {
        match self.last_ms {
            Some(last) if now_ms.saturating_sub(last) < LIVE_TOAST_MIN_MS => false,
            _ => {
                self.last_ms = Some(now_ms);
                true
            }
        }
    }
}

/// The live toast's text for one tick — PURE. Volume/brightness carry the
/// real percent, scrub a fixed verb, chords the forward chord's name and the
/// signed step count (0 steps shows the name alone). `None` for a band that
/// does nothing: no toast for a slide that changes nothing.
pub(crate) fn live_toast_text(
    action: &BandAction,
    value_pct: u8,
    chord: Option<&str>,
    steps: i32,
) -> Option<String> {
    match action {
        BandAction::Volume => Some(format!("🔊 Volume {value_pct}%")),
        BandAction::Brightness => Some(format!("☀ Brightness {value_pct}%")),
        BandAction::Scrub => {
            // Live: direction + how far this slide has scrubbed, as a count of
            // arrow taps (each is the player's own step: 5 s in YouTube, 10 s
            // in VLC), plus a speed bar from the current rate.
            let bars = ((value_pct as usize) / 20).min(5);
            let speed = "▮".repeat(bars.max(1));
            if steps == 0 {
                Some(format!("⏩ Scrub {speed}"))
            } else if steps > 0 {
                Some(format!("⏩ +{steps} {speed}"))
            } else {
                Some(format!("⏪ {steps} {speed}"))
            }
        }
        BandAction::Chords { .. } => {
            let name = chord.unwrap_or("(no keys)");
            if steps == 0 {
                Some(format!("⌨ {name}"))
            } else {
                let n = steps.unsigned_abs();
                let sign = if steps < 0 { "\u{2212}" } else { "" };
                let unit = if n == 1 { "step" } else { "steps" };
                Some(format!("⌨ {name} · {sign}{n} {unit}"))
            }
        }
        BandAction::None => None,
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
    let mut chord_sent = 0i32;
    let mut last_travel = 0.0f32;
    let mut entered_at = Instant::now();
    let mut last_tick = Instant::now();
    let mut last_bright = Instant::now() - Duration::from_secs(1);
    let mut bright_worker: Option<actions::BrightnessWorker> = None;
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    // The live slide toast (1.0.123): the ≤ 8 Hz limiter, the text last SENT
    // and the text last COMPUTED, so Exit can flush a throttled final value.
    let t0 = Instant::now();
    let mut toast_limit = LiveToastLimiter::default();
    let mut toast_sent: Option<String> = None;
    let mut toast_pending: Option<String> = None;
    // The 5-second proof line (coordinator, 2026-09-19: the owner's 1.0.121
    // slides did nothing and the log could not say why — the reader logged
    // nothing per gesture). Same shape as the probe's summary.
    let mut summary_at = Instant::now();
    let mut reports_since = 0u64;
    let mut max_contacts = 0usize;
    let mut band_entries = 0u64;

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

        reports_since += 1;
        max_contacts = max_contacts.max(contacts.len());
        if summary_at.elapsed() >= Duration::from_secs(5) {
            let secs = summary_at.elapsed().as_secs_f32().max(1e-3);
            let first = contacts.first().map(|c| format!("({:.3},{:.3})", c.fx, c.fy)).unwrap_or_else(|| "-".into());
            log::info!(
                "touchpad: {:.0} reports/s, max contacts {}, band entries {}, live={} enabled={:?} last contact {} (touchpad-proof-line-spaceadom-122)",
                reports_since as f32 / secs,
                max_contacts,
                band_entries,
                g.is_live(),
                cfg.enabled_edges(),
                first
            );
            summary_at = Instant::now();
            reports_since = 0;
            max_contacts = 0;
        }

        let Some(ev) = g.feed(&contacts) else { return };
        match ev {
            GestureEvent::Enter(edge) => {
                let action = cfg.band(edge).action.clone();
                prev_travel = 0.0;
                scrub_pending = 0.0;
                chord_sent = 0;
                if matches!(action, BandAction::Brightness) {
                    bright_worker = actions::BrightnessWorker::start();
                }
                last_travel = 0.0;
                entered_at = Instant::now();
                last_tick = Instant::now();
                band_entries += 1;
                BAND_LIVE.store(true, Ordering::Relaxed);
                let (fx, fy) = contacts.first().map(|c| (c.fx, c.fy)).unwrap_or((0.0, 0.0));
                log::info!(
                    "touchpad: band LIVE edge={} action={} at ({fx:.3},{fy:.3}) width={:.2} length={:.2}",
                    edge_wire(edge),
                    action.wire(),
                    cfg.band(edge).width,
                    cfg.band(edge).length
                );
                let value = live_value_pct(&action, 0.0, cfg.band(edge).sensitivity);
                let label = chord_label(&action);
                emit_live(&app, edge, &action, value, 0.0, label.clone(), 0);
                last_emit = Instant::now();
                // The live toast appears on Enter, with the first value. The
                // setting is read from `cfg` — the snapshot already taken
                // above — never from the config lock on this callback.
                toast_limit.reset();
                toast_sent = None;
                toast_pending = None;
                if cfg.slide_toast {
                    if let Some(text) = live_toast_text(&action, value, label.as_deref(), 0) {
                        toast_limit.allow(t0.elapsed().as_millis() as u64);
                        crate::show_live_toast(&app, LIVE_TOAST_KEY, &text);
                        toast_sent = Some(text);
                    }
                }
            }
            GestureEvent::Move { edge, travel } => {
                let band = cfg.band(edge).clone();
                let now = Instant::now();
                let dt = now.duration_since(last_tick).as_secs_f32().clamp(0.0, 0.2);
                last_tick = now;
                last_travel = travel;
                let mut value = None;
                match &band.action {
                    BandAction::Volume => {
                        let delta = actions::analogue_gain(band.sensitivity) * (travel - prev_travel);
                        value = actions::add_volume(delta);
                    }
                    BandAction::Brightness => {
                        // One hidden PowerShell WORKER per slide (started on Enter,
                        // fed values, closed on Exit): the in-process WMI COM read
                        // failed silently on the owner's laptop (1.0.120–123) while
                        // PowerShell's WMI worked every time. Rate-limit ~20 Hz.
                        if now.duration_since(last_bright) >= Duration::from_millis(50) {
                            let delta = actions::analogue_gain(band.sensitivity) * (travel - prev_travel);
                            let step = (delta * 100.0).round() as i32;
                            if step != 0 {
                                if let Some(w) = bright_worker.as_mut() {
                                    value = w.add(step);
                                }
                                prev_travel = travel;
                            }
                            last_bright = now;
                        } else {
                            value = bright_worker.as_ref().map(|w| w.current());
                        }
                    }
                    BandAction::Scrub => {
                        let rate = actions::scrub_rate(travel, band.sensitivity);
                        scrub_pending += rate * dt;
                        let forward = travel >= 0.0;
                        while scrub_pending >= 1.0 {
                            actions::scrub_tap(forward);
                            scrub_pending -= 1.0;
                            // Signed tap count for the live pill (owner, 2026-09-19:
                            // "make the toast live for scrubbing too").
                            chord_sent += if forward { 1 } else { -1 };
                        }
                        value = Some(live_value_pct(&BandAction::Scrub, travel, band.sensitivity));
                    }
                    BandAction::Chords { forward, backward } => {
                        // Step-based: bring `chord_sent` up (or down) to the
                        // quantised target, one chord per step, so a slide
                        // back sends the other chord and undoes the slide out.
                        let target = actions::chord_steps(
                            travel_short_side(edge, travel, aspect),
                            band.sensitivity,
                        );
                        while chord_sent < target {
                            actions::send_chord(forward);
                            chord_sent += 1;
                        }
                        while chord_sent > target {
                            actions::send_chord(backward);
                            chord_sent -= 1;
                        }
                    }
                    BandAction::None => {}
                }
                if !matches!(band.action, BandAction::Brightness) {
                    prev_travel = travel;
                }
                // Emit at ≤ 30 Hz.
                if now.duration_since(last_emit) >= Duration::from_millis(33) {
                    emit_live(&app, edge, &band.action, value.unwrap_or(0), travel, chord_label(&band.action), chord_sent);
                    last_emit = now;
                }
                // The live toast, in place, at ≤ 8 Hz. Only when the text
                // changed: a finger resting still sends nothing.
                if cfg.slide_toast {
                    let label = chord_label(&band.action);
                    toast_pending = live_toast_text(&band.action, value.unwrap_or(0), label.as_deref(), chord_sent);
                    if toast_pending.is_some()
                        && toast_pending != toast_sent
                        && toast_limit.allow(t0.elapsed().as_millis() as u64)
                    {
                        if let Some(text) = toast_pending.as_deref() {
                            crate::show_live_toast(&app, LIVE_TOAST_KEY, text);
                        }
                        toast_sent = toast_pending.clone();
                    }
                }
            }
            GestureEvent::Exit(edge) => {
                if let Some(w) = bright_worker.take() {
                    w.stop();
                }
                BAND_LIVE.store(false, Ordering::Relaxed);
                log::info!(
                    "touchpad: band DEAD edge={} travel={:.3} steps={} after {} ms",
                    edge_wire(edge),
                    last_travel,
                    chord_sent,
                    entered_at.elapsed().as_millis()
                );
                emit_end(&app);
                // Flush a throttled final value, then let the pill fade
                // (~600 ms on the page). `toast_sent` is the witness that a
                // pill exists: nothing was shown, nothing to end.
                if toast_pending.is_some() && toast_pending != toast_sent {
                    if let Some(text) = toast_pending.as_deref() {
                        crate::show_live_toast(&app, LIVE_TOAST_KEY, text);
                    }
                    toast_sent = toast_pending.take();
                }
                if toast_sent.take().is_some() {
                    crate::end_live_toast(&app, LIVE_TOAST_KEY);
                }
            }
        }
    });

    if let Err(e) = raw::run_sink(&pad, on_report) {
        log::warn!("touchpad: sink failed: {e}");
    }
    // Whatever happens, do not leave the pointer frozen.
    BAND_LIVE.store(false, Ordering::Relaxed);
}

fn emit_live(
    app: &AppHandle,
    edge: TouchEdge,
    action: &BandAction,
    value_pct: u8,
    travel: f32,
    chord: Option<String>,
    steps: i32,
) {
    let is_chords = matches!(action, BandAction::Chords { .. });
    let _ = app.emit(
        "touchpad-live",
        LivePayload {
            edge: Some(edge_wire(edge)),
            action: Some(action_wire(action)),
            value_pct: Some(value_pct),
            travel: Some(travel),
            chord,
            steps: if is_chords { Some(steps) } else { None },
        },
    );
}

fn emit_end(app: &AppHandle) {
    let _ = app.emit(
        "touchpad-live",
        LivePayload { edge: None, action: None, value_pct: None, travel: None, chord: None, steps: None },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_live_toast_limiter_passes_the_first_tick_then_at_most_8_hz() {
        let mut l = LiveToastLimiter::default();
        assert!(l.allow(0), "the first update after reset always goes out");
        assert!(!l.allow(1));
        assert!(!l.allow(LIVE_TOAST_MIN_MS - 1), "one ms early is still too soon");
        assert!(l.allow(LIVE_TOAST_MIN_MS), "exactly the spacing passes");
        assert!(!l.allow(LIVE_TOAST_MIN_MS + 60));
        assert!(l.allow(2 * LIVE_TOAST_MIN_MS + 5));
        // One second of 100 Hz reports lets through no more than 8 + the first.
        let mut l = LiveToastLimiter::default();
        let sent = (0..1000u64).step_by(10).filter(|&ms| l.allow(ms)).count();
        assert!(sent <= 9, "{sent} updates in a second is more than 8 Hz");
        assert!(sent >= 8, "{sent} updates in a second is far under 8 Hz");
        // reset() makes the next tick immediate again, and a clock that goes
        // backwards is "too soon", never a burst.
        l.reset();
        assert!(l.allow(5));
        assert!(!l.allow(0));
    }

    #[test]
    fn the_live_toast_text_per_action() {
        assert_eq!(live_toast_text(&BandAction::Volume, 62, None, 0).as_deref(), Some("🔊 Volume 62%"));
        assert_eq!(live_toast_text(&BandAction::Brightness, 40, None, 0).as_deref(), Some("☀ Brightness 40%"));
        assert_eq!(live_toast_text(&BandAction::Scrub, 77, None, 0).as_deref(), Some("⏩ Scrub ▮▮▮"));
        assert_eq!(live_toast_text(&BandAction::Scrub, 100, None, 12).as_deref(), Some("⏩ +12 ▮▮▮▮▮"));
        assert_eq!(live_toast_text(&BandAction::Scrub, 0, None, -3).as_deref(), Some("⏪ -3 ▮"));
        let chords = BandAction::Chords { forward: vec![0x11, 0x09], backward: vec![0x11, 0x10, 0x09] };
        assert_eq!(live_toast_text(&chords, 0, Some("Ctrl+Tab"), 0).as_deref(), Some("⌨ Ctrl+Tab"));
        assert_eq!(live_toast_text(&chords, 0, Some("Ctrl+Tab"), 1).as_deref(), Some("⌨ Ctrl+Tab · 1 step"));
        assert_eq!(live_toast_text(&chords, 0, Some("Ctrl+Tab"), 3).as_deref(), Some("⌨ Ctrl+Tab · 3 steps"));
        assert_eq!(live_toast_text(&chords, 0, Some("Ctrl+Tab"), -2).as_deref(), Some("⌨ Ctrl+Tab · \u{2212}2 steps"));
        assert_eq!(live_toast_text(&chords, 0, None, 1).as_deref(), Some("⌨ (no keys) · 1 step"));
        assert_eq!(live_toast_text(&BandAction::None, 50, None, 0), None, "a band that does nothing shows nothing");
    }

    #[test]
    fn presence_wire_strings() {
        assert_eq!(Presence::Precision.wire(), "precision");
        assert_eq!(Presence::None.wire(), "none");
    }

    #[test]
    fn edge_and_action_wire_strings() {
        assert_eq!(edge_wire(TouchEdge::Top), "top");
        assert_eq!(edge_wire(TouchEdge::Bottom), "bottom");
        assert_eq!(action_wire(&BandAction::Scrub), "scrub");
        assert_eq!(action_wire(&BandAction::None), "none");
        assert_eq!(action_wire(&BandAction::empty_chords()), "chords");
    }

    #[test]
    fn live_value_for_scrub_is_a_fraction_of_the_cap() {
        // Full travel → 100 %; dead zone → 0.
        assert_eq!(live_value_pct(&BandAction::Scrub, 1.0, 6), 100);
        assert_eq!(live_value_pct(&BandAction::Scrub, 0.0, 6), 0);
        assert_eq!(live_value_pct(&BandAction::empty_chords(), 1.0, 6), 0, "chords show no percent");
    }

    #[test]
    fn a_chords_band_names_its_forward_chord_and_a_horizontal_edge_travels_in_short_side_units() {
        let a = BandAction::Chords { forward: vec![0x11, 0x09], backward: vec![0x11, 0x10, 0x09] };
        assert_eq!(chord_label(&a).as_deref(), Some("Ctrl+Tab"));
        assert_eq!(chord_label(&BandAction::Scrub), None);
        // Pad aspect 0.62 (short/long): 0.062 of the long side is 0.1 of the short.
        assert!((travel_short_side(TouchEdge::Top, 0.062, 0.62) - 0.1).abs() < 1e-5);
        assert!((travel_short_side(TouchEdge::Bottom, -0.062, 0.62) + 0.1).abs() < 1e-5);
        assert_eq!(travel_short_side(TouchEdge::Left, 0.1, 0.62), 0.1, "vertical edges already are");
        assert_eq!(travel_short_side(TouchEdge::Top, 0.1, 0.0), 0.1, "a bad aspect is ignored");
    }
}
