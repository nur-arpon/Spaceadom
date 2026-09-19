//! TOUCHPAD 1.0.130 — "Video seek": the band IS the seek bar, anchored where
//! the finger lands (owner decision 2026-09-20 00:50).
//!
//! HOW. Windows' Global System Media Transport Controls
//! (`Windows.Media.Control`) expose the session every media player registers
//! with the system media overlay — Chrome/Edge/Brave pages with a `<video>`,
//! VLC, Spotify, Media Player, Films & TV. On Enter the reader asks
//! `GlobalSystemMediaTransportControlsSessionManager::RequestAsync().get()`
//! → `GetCurrentSession()` → `GetPlaybackInfo().Controls().
//! IsPlaybackPositionEnabled()` → `GetTimelineProperties()` and records the
//! position `P0` and the length `L` (EndTime − StartTime). On every Move with
//! signed travel `t` (fraction of the band's LENGTH along its axis, −1..1):
//!
//!     target = clamp(P0 + t × L × span(sensitivity), 0, L)
//!
//! `span` is 1.0 at sensitivity 5 — the whole band spans the whole video — and
//! runs 0.25 … 2.0 over 1..10 (`seek_span`, table below). NO acceleration:
//! pure proportion, so sliding back to where the finger landed puts the video
//! back where it was (CLAUDE.md's reversible rule). `TryChangePlaybackPositionAsync`
//! is called at ≤ 10 Hz and only when the target moved ≥ 1 s (`SeekLimiter`),
//! fire-and-forget (the `IAsyncOperation` is dropped, never awaited), so the
//! raw-input pump is never blocked longer than one WinRT call. The reader
//! thread is an MTA COM thread, which is what `.get()` on Enter needs.
//!
//! FALLBACK. No session, position control not enabled, or any call failing →
//! `SeekSession::open` answers `None` and the reader runs that gesture exactly
//! as `Scrub` (←/→ taps), the pill saying "⏩ Scrubbing (no seek bar here)".
//! Scrub stays a separate, user-selectable action; this fallback never
//! replaces the user's choice.
//!
//! The Track preset's `skip_track` lives here too: the same session's
//! `TrySkipNextAsync` / `TrySkipPreviousAsync` when one exists, else the
//! `VK_MEDIA_NEXT_TRACK` / `VK_MEDIA_PREV_TRACK` keys.
//!
//! NOTHING here is ever run by a non-`#[ignore]` test: the only machine test
//! is READ-ONLY (it prints the session, the timeline and the flag).
#![cfg(windows)]

// ---------------------------------------------------------------------------
// Pure — the span table, the target, the limiter, the clock text
// ---------------------------------------------------------------------------

/// 100-nanosecond ticks per second — WinRT `TimeSpan.Duration` units.
pub const TICKS_PER_SEC: i64 = 10_000_000;
/// Send a new position only when the target moved at least this far.
pub const SEEK_MIN_MOVE_TICKS: i64 = TICKS_PER_SEC;
/// Minimum spacing between two `TryChangePlaybackPositionAsync` calls: 100 ms
/// is ≤ 10 Hz, the owner's ceiling.
pub const SEEK_MIN_MS: u64 = 100;

/// Piecewise-linear 1..=10 → `lo` at 1, `mid` at 5, `hi` at 10. Shared by the
/// seek span and the scrub cap so both read "5 is the default, 1 is a
/// quarter, 10 is double-ish". Pure.
pub fn sens_scale(sensitivity: u8, lo: f32, mid: f32, hi: f32) -> f32 {
    let s = sensitivity.clamp(1, 10) as f32;
    if s <= 5.0 {
        lo + (mid - lo) * ((s - 1.0) / 4.0)
    } else {
        mid + (hi - mid) * ((s - 5.0) / 5.0)
    }
}

/// How much of the video the whole band spans, by sensitivity. Pure.
///
/// | sens | 1    | 2      | 3     | 4      | 5   | 6   | 7   | 8   | 9   | 10  |
/// |------|------|--------|-------|--------|-----|-----|-----|-----|-----|-----|
/// | span | 0.25 | 0.4375 | 0.625 | 0.8125 | 1.0 | 1.2 | 1.4 | 1.6 | 1.8 | 2.0 |
///
/// 5 = the band spans the whole video (the default); 1 = a quarter of it
/// (fine control); 10 = twice the video, so half the band reaches either end.
pub fn seek_span(sensitivity: u8) -> f32 {
    sens_scale(sensitivity, 0.25, 1.0, 2.0)
}

/// The position to ask for, in ticks: `clamp(p0 + travel × len × span, 0,
/// len)`. `travel` is the signed fraction of the band's length (−1..1);
/// anything non-finite reads as 0. A zero or negative `len` (no timeline)
/// pins the answer to `p0` clamped at 0. Pure.
pub fn seek_target(p0: i64, len: i64, travel: f32, span: f32) -> i64 {
    let len = len.max(0);
    let t = if travel.is_finite() { travel } else { 0.0 };
    let span = if span.is_finite() { span } else { 1.0 };
    let delta = (t as f64) * (len as f64) * (span as f64);
    let raw = (p0 as f64) + delta;
    raw.round().clamp(0.0, len as f64) as i64
}

/// The rate limiter for `TryChangePlaybackPositionAsync` — PURE. `reset` on
/// Enter; `allow(now_ms, target)` on every Move says whether to send `target`
/// now and records it if so: never within `SEEK_MIN_MS` of the last send, and
/// never for a target within `SEEK_MIN_MOVE_TICKS` of the last one SENT (the
/// first send after reset always passes when the target moved ≥ 1 s from the
/// anchor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeekLimiter {
    last_ms: Option<u64>,
    /// The last position SENT (or the anchor after `reset`).
    last_sent: i64,
}

impl SeekLimiter {
    pub fn new(anchor: i64) -> Self {
        SeekLimiter { last_ms: None, last_sent: anchor }
    }

    pub fn reset(&mut self, anchor: i64) {
        self.last_ms = None;
        self.last_sent = anchor;
    }

    pub fn last_sent(&self) -> i64 {
        self.last_sent
    }

    pub fn allow(&mut self, now_ms: u64, target: i64) -> bool {
        if (target - self.last_sent).abs() < SEEK_MIN_MOVE_TICKS {
            return false;
        }
        if let Some(last) = self.last_ms {
            if now_ms.saturating_sub(last) < SEEK_MIN_MS {
                return false;
            }
        }
        self.last_ms = Some(now_ms);
        self.last_sent = target;
        true
    }
}

/// `12:34`, or `1:02:03` at an hour or more. Negative or non-finite ticks
/// read as 0. Pure.
pub fn fmt_time(ticks: i64) -> String {
    let secs = (ticks.max(0) / TICKS_PER_SEC) as u64;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The live pill's clock: "12:34 / 45:00" (the prefix "▶ " is added by
/// `touchpad::live_toast_text`, which owns every pill's icon).
pub fn clock_text(position: i64, len: i64) -> String {
    format!("{} / {}", fmt_time(position), fmt_time(len))
}

/// The pill when a Seek gesture fell back to scrubbing.
pub const SEEK_FALLBACK_TEXT: &str = "⏩ Scrubbing (no seek bar here)";

/// The percent of the video the position is at, for the page's meter.
pub fn position_pct(position: i64, len: i64) -> u8 {
    if len <= 0 {
        return 0;
    }
    ((position.clamp(0, len) as f64 / len as f64) * 100.0).round() as u8
}

// ---------------------------------------------------------------------------
// The WinRT session (reader thread only; MTA COM already initialised there)
// ---------------------------------------------------------------------------

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
};

/// READ-ONLY: the current session, or `None` (no manager, no session).
fn current_session() -> Option<Session> {
    let manager = Manager::RequestAsync()
        .and_then(|op| op.get())
        .map_err(|e| log::info!("touchpad: seek — the media session manager is unavailable: {e}"))
        .ok()?;
    // `GetCurrentSession` answers an error (E_POINTER-ish null) when nothing
    // is registered; that is the everyday "no video open" case, logged at
    // info, never warn.
    manager
        .GetCurrentSession()
        .map_err(|e| log::info!("touchpad: seek — no current media session: {e}"))
        .ok()
}

/// What a READ of the current session looks like — for the ignored machine
/// test and the Enter log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReadout {
    pub app_id: String,
    pub start: i64,
    pub end: i64,
    pub position: i64,
    pub position_enabled: bool,
    pub next_enabled: bool,
    pub prev_enabled: bool,
    pub status: i32,
}

/// READ-ONLY: everything `SeekSession::open` looks at, without opening one.
pub fn read_current() -> Option<SessionReadout> {
    let s = current_session()?;
    let app_id = s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
    let tl = s.GetTimelineProperties().ok()?;
    let info = s.GetPlaybackInfo().ok()?;
    let controls = info.Controls().ok()?;
    Some(SessionReadout {
        app_id,
        start: tl.StartTime().map(|t| t.Duration).unwrap_or(0),
        end: tl.EndTime().map(|t| t.Duration).unwrap_or(0),
        position: tl.Position().map(|t| t.Duration).unwrap_or(0),
        position_enabled: controls.IsPlaybackPositionEnabled().unwrap_or(false),
        next_enabled: controls.IsNextEnabled().unwrap_or(false),
        prev_enabled: controls.IsPreviousEnabled().unwrap_or(false),
        status: info.PlaybackStatus().map(|s| s.0).unwrap_or(-1),
    })
}

/// One live Seek gesture: the session, its timeline origin, the anchor
/// position and the length, all captured on Enter.
pub struct SeekSession {
    session: Session,
    app_id: String,
    /// The timeline's `StartTime` — positions are sent as `start + target`.
    start: i64,
    /// The position at Enter, relative to `start`.
    p0: i64,
    /// `EndTime − StartTime`.
    len: i64,
    sends: u32,
    last_target: i64,
}

impl SeekSession {
    /// Open the current session for seeking. `None` — with an info line
    /// saying which gate failed — means "fall back to scrub for this gesture".
    pub fn open() -> Option<SeekSession> {
        let session = current_session()?;
        let app_id = session.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
        let enabled = session
            .GetPlaybackInfo()
            .and_then(|i| i.Controls())
            .and_then(|c| c.IsPlaybackPositionEnabled())
            .map_err(|e| log::info!("touchpad: seek — {app_id}: playback info unreadable: {e}"))
            .ok()?;
        if !enabled {
            log::info!("touchpad: seek — {app_id}: position control is not enabled; scrubbing instead");
            return None;
        }
        let tl = session
            .GetTimelineProperties()
            .map_err(|e| log::info!("touchpad: seek — {app_id}: timeline unreadable: {e}"))
            .ok()?;
        let start = tl.StartTime().map(|t| t.Duration).unwrap_or(0);
        let end = tl.EndTime().map(|t| t.Duration).unwrap_or(0);
        let position = tl.Position().map(|t| t.Duration).unwrap_or(0);
        let len = end - start;
        if len <= 0 {
            log::info!("touchpad: seek — {app_id}: timeline has no length (start {start}, end {end}); scrubbing instead");
            return None;
        }
        let p0 = (position - start).clamp(0, len);
        Some(SeekSession { session, app_id, start, p0, len, sends: 0, last_target: p0 })
    }

    pub fn app_id(&self) -> &str {
        &self.app_id
    }
    pub fn p0(&self) -> i64 {
        self.p0
    }
    pub fn len(&self) -> i64 {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len <= 0
    }
    pub fn sends(&self) -> u32 {
        self.sends
    }
    pub fn last_target(&self) -> i64 {
        self.last_target
    }

    /// Ask the player to move to `target` (relative to the timeline start).
    /// Fire-and-forget: the operation is dropped, never awaited, so this is
    /// one WinRT call and no wait. A refused call is logged once per gesture.
    pub fn seek_to(&mut self, target: i64) {
        let abs = self.start + target.clamp(0, self.len);
        match self.session.TryChangePlaybackPositionAsync(abs) {
            Ok(_op) => {
                self.sends += 1;
                self.last_target = target;
            }
            Err(e) => {
                if self.sends == 0 {
                    log::warn!("touchpad: seek — {}: TryChangePlaybackPositionAsync refused: {e}", self.app_id);
                }
            }
        }
    }
}

/// The Track preset: skip through the media session when one exists (the
/// player's own next/previous, exactly what the keyboard's media keys reach),
/// else press `VK_MEDIA_NEXT_TRACK` / `VK_MEDIA_PREV_TRACK`. Returns which
/// path ran, for the log.
pub fn skip_track(forward: bool) -> &'static str {
    if let Some(s) = current_session() {
        let r = if forward { s.TrySkipNextAsync() } else { s.TrySkipPreviousAsync() };
        match r {
            Ok(_op) => return "media-session",
            Err(e) => log::info!("touchpad: track — the session refused the skip ({e}); pressing the media key"),
        }
    }
    let vk = if forward { super::presets::VK_MEDIA_NEXT_TRACK } else { super::presets::VK_MEDIA_PREV_TRACK };
    super::actions::send_chord(&[vk]);
    "media-key"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_span_table_is_the_documented_one() {
        let table = [
            (1u8, 0.25f32),
            (2, 0.4375),
            (3, 0.625),
            (4, 0.8125),
            (5, 1.0),
            (6, 1.2),
            (7, 1.4),
            (8, 1.6),
            (9, 1.8),
            (10, 2.0),
        ];
        for (s, want) in table {
            assert!((seek_span(s) - want).abs() < 1e-6, "sensitivity {s}: {} != {want}", seek_span(s));
        }
        assert_eq!(seek_span(0), seek_span(1), "clamped below");
        assert_eq!(seek_span(99), seek_span(10), "clamped above");
        for s in 1..10u8 {
            assert!(seek_span(s) < seek_span(s + 1), "monotonic at {s}");
        }
    }

    #[test]
    fn the_target_is_pure_proportion_clamped_at_both_ends_in_both_signs() {
        let len = 100 * TICKS_PER_SEC; // a 100 s video
        let p0 = 40 * TICKS_PER_SEC;
        // span 1.0: +0.25 of the band = +25 s; −0.25 = −25 s.
        assert_eq!(seek_target(p0, len, 0.25, 1.0), 65 * TICKS_PER_SEC);
        assert_eq!(seek_target(p0, len, -0.25, 1.0), 15 * TICKS_PER_SEC);
        assert_eq!(seek_target(p0, len, 0.0, 1.0), p0, "no travel, no move");
        // Beyond either end clamps to the end, not past it.
        assert_eq!(seek_target(p0, len, 0.9, 1.0), len, "past the end");
        assert_eq!(seek_target(p0, len, -0.9, 1.0), 0, "before the start");
        assert_eq!(seek_target(p0, len, 1.0, 2.0), len);
        assert_eq!(seek_target(p0, len, -1.0, 2.0), 0);
        // Span scales the proportion, never the shape (no acceleration): twice
        // the travel is twice the delta at every span.
        for span in [0.25f32, 1.0, 2.0] {
            let a = seek_target(p0, len, 0.1, span) - p0;
            let b = seek_target(p0, len, 0.2, span) - p0;
            // f32 0.1 is not exactly 0.1: allow one tick (100 ns) of rounding.
            assert!((b - 2 * a).abs() <= 1, "span {span}: {b} != 2×{a}");
            assert!(((seek_target(p0, len, -0.1, span) - p0) + a).abs() <= 1, "odd in travel at span {span}");
            assert!(a > 0, "span {span}");
        }
        // Span 0.25: the whole band is a quarter of the video.
        assert_eq!(seek_target(0, len, 1.0, 0.25), 25 * TICKS_PER_SEC);
        // Ends when the anchor is at an end.
        assert_eq!(seek_target(0, len, -0.5, 1.0), 0);
        assert_eq!(seek_target(len, len, 0.5, 1.0), len);
        // No timeline: pinned at the anchor clamped into 0..=0.
        assert_eq!(seek_target(p0, 0, 0.5, 1.0), 0);
        assert_eq!(seek_target(p0, -5, 0.5, 1.0), 0);
        // Garbage in: NaN travel reads as 0.
        assert_eq!(seek_target(p0, len, f32::NAN, 1.0), p0);
    }

    #[test]
    fn the_seek_limiter_needs_a_second_of_movement_and_at_most_10_hz() {
        let s = TICKS_PER_SEC;
        let mut l = SeekLimiter::new(50 * s);
        assert!(!l.allow(0, 50 * s + s / 2), "half a second from the anchor: too small");
        assert!(l.allow(0, 51 * s), "a full second: the first send");
        assert_eq!(l.last_sent(), 51 * s);
        assert!(!l.allow(50, 55 * s), "50 ms later: too soon even for a 4 s jump");
        assert!(!l.allow(100, 51 * s + s / 2), "100 ms later but under a second from the last SENT");
        assert!(l.allow(100, 53 * s), "100 ms later and 2 s away: sent");
        assert!(l.allow(200, 40 * s), "backwards counts too");
        // One second of 100 Hz reports with an ever-moving target lets
        // through no more than 10.
        let mut l = SeekLimiter::new(0);
        let sent = (0..1000u64).step_by(10).filter(|&ms| l.allow(ms, (ms as i64 + 1) * 2 * s)).count();
        assert!(sent <= 10, "{sent} sends in a second is more than 10 Hz");
        assert!(sent >= 9, "{sent} sends in a second is far under 10 Hz");
        // reset() re-anchors: the next tick is immediate again if it moved.
        l.reset(10 * s);
        assert!(!l.allow(5, 10 * s + 1));
        assert!(l.allow(5, 12 * s));
        // A clock that went backwards is "too soon", never a burst.
        assert!(!l.allow(0, 20 * s));
    }

    #[test]
    fn time_formats_as_mm_ss_or_h_mm_ss() {
        let s = TICKS_PER_SEC;
        assert_eq!(fmt_time(0), "0:00");
        assert_eq!(fmt_time(5 * s), "0:05");
        assert_eq!(fmt_time(754 * s), "12:34");
        assert_eq!(fmt_time(2700 * s), "45:00");
        assert_eq!(fmt_time(3599 * s), "59:59");
        assert_eq!(fmt_time(3600 * s), "1:00:00");
        assert_eq!(fmt_time(3723 * s), "1:02:03");
        assert_eq!(fmt_time(36000 * s + 61 * s), "10:01:01");
        assert_eq!(fmt_time(-5 * s), "0:00", "never negative");
        assert_eq!(fmt_time(s - 1), "0:00", "truncates, never rounds up");
        assert_eq!(clock_text(754 * s, 2700 * s), "12:34 / 45:00");
        assert_eq!(position_pct(50 * s, 200 * s), 25);
        assert_eq!(position_pct(500 * s, 200 * s), 100, "clamped");
        assert_eq!(position_pct(5, 0), 0, "no length");
        assert!(SEEK_FALLBACK_TEXT.starts_with("⏩ "));
    }

    /// READ-ONLY on the machine that runs it: prints the current media
    /// session's app id, timeline and whether position control is enabled.
    /// NEVER seeks, skips or changes anything.
    /// `cargo test --release --lib print_current_media_session -- --ignored --nocapture`
    #[test]
    #[ignore = "reads this machine's current media session; run by hand with --nocapture"]
    fn print_current_media_session_on_this_machine() {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        // SAFETY: MTA COM for this thread, as the reader thread does.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        match read_current() {
            None => eprintln!("media session: NONE (no player registered a session right now)"),
            Some(r) => {
                eprintln!("media session: app id        = {:?}", r.app_id);
                eprintln!("media session: playback status = {} (0 closed, 1 opened, 2 changing, 3 stopped, 4 playing, 5 paused)", r.status);
                eprintln!(
                    "media session: timeline      = start {} / position {} / end {}  (len {})",
                    fmt_time(r.start),
                    fmt_time(r.position),
                    fmt_time(r.end),
                    fmt_time(r.end - r.start)
                );
                eprintln!("media session: raw ticks     = start {} position {} end {}", r.start, r.position, r.end);
                eprintln!("media session: IsPlaybackPositionEnabled = {}", r.position_enabled);
                eprintln!("media session: IsNextEnabled = {}  IsPreviousEnabled = {}", r.next_enabled, r.prev_enabled);
                eprintln!(
                    "media session: seek would be {} for this gesture",
                    if r.position_enabled && r.end - r.start > 0 { "LIVE" } else { "the scrub FALLBACK" }
                );
            }
        }
    }
}
