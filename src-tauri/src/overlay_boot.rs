//! PROBLEM 265 — the Guide HUD must be able to DRAW within about a second of
//! logon, without re-opening PROBLEM 59, 76 or 215.
//!
//! # What the owner measured (2026-09-10, his own `debug.log`, autostart)
//!
//! ```text
//! 09:55:16.425  hook installed (27 ms in)
//! 09:55:16.442  autostart launch — hook and engine are LIVE now; only the
//!               window/webview creation waits 10s for the shell to settle
//! 09:55:16.718  hold start (hold #1)          <- 320 ms after launch
//! 09:55:17.177  guide_hud: still starting — the overlay webview is not built yet
//! 09:55:20.523  guide_hud: still starting — the overlay webview is not built yet
//! 09:55:32.200  engine: combo Space+c received   <- the first LETTER, 16 s in
//! ```
//!
//! Four Space holds in the first sixteen seconds and not one letter pressed.
//! The shortcuts were live the whole time — the owner could not TELL, because
//! nothing was drawn, and he read that as "my shortcuts don't work". PROBLEM
//! 215 called the trade "silent but functional, which is the accepted trade";
//! that acceptance is what this entry withdraws.
//!
//! # Why this does not re-open PROBLEM 59
//!
//! PROBLEM 59's hazard is a WEBVIEW hazard, so it genuinely covers the overlay
//! as well as the dashboard: at a cold logon `CreateCoreWebView2Controller`
//! fails with `HRESULT(0x80070490) ERROR_NOT_FOUND` and Tauri destroys the
//! host window. What has changed since PROBLEM 59 was written is not the
//! hazard — it is that the overlay, and ONLY the overlay, now has three
//! independent recoveries from exactly that outcome:
//!
//! 1. `create_app_windows`'s PROBLEM 59 existence check still runs at the full
//!    `AUTOSTART_SETTLE` mark and rebuilds any webview that is missing with an
//!    explicit builder — including an overlay whose early attempt failed.
//! 2. `display_watch`'s self-heal poll (PROBLEM 117/118/214) rebuilds a missing
//!    or `OVERLAY_DISABLED` overlay every poll, for the rest of the session.
//! 3. `guide_hud` calls `display_watch::heal_now()` the moment a hold finds no
//!    overlay after the windows have been created.
//!
//! The dashboard has none of those: nothing in the app rebuilds a dashboard
//! after `create_app_windows` has run. So **the dashboard keeps the full 10 s
//! and the overlay does not** — which is PROBLEM 215's own rule ("a delay added
//! to protect one subsystem must be scoped to that subsystem") applied one
//! level further down, to the two windows the wait was protecting as a pair.
//!
//! The worst case of trying early is therefore TODAY'S BEHAVIOUR: an overlay
//! that fails to attach at 1.2 s is rebuilt at 10 s by the path that already
//! exists, and the owner sees exactly what he sees now. The best case is a ring
//! on the first hold.
//!
//! PROBLEM 76 is not re-opened either — it CUT the wait (30 s → 10 s) because a
//! long wait made the app look dead to the user. This is the same decision
//! taken again for the one window whose absence is what he can see. Its other
//! two halves (the Run key, the tray-icon promotion) are untouched.
//!
//! PROBLEM 215 is not re-opened: the hook and the engine still come up ahead of
//! every window, nothing moved back in front of them, and the settle thread
//! still exists — it now has two phases instead of one.
//!
//! # What is deliberately NOT changed
//!
//! * Safe mode (PROBLEM 253) still wins: a safe-mode launch has no overlay, and
//!   this path must not put one back. That check is FIRST in [`plan`].
//! * Every overlay guard is reapplied through the one shared function
//!   (`crate::configure_overlay_window`): transparent, click-through,
//!   NoActivate, always-on-top, DWM border cleared, and
//!   `set_ignore_cursor_events` still FAILS CLOSED.
//! * `display_watch` still starts from `create_app_windows`, at the same point
//!   it always did. An overlay created early is simply already there when the
//!   watcher begins fingerprinting.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// PROBLEM 265 — how long an `--autostart` launch waits before creating the
/// OVERLAY. The dashboard keeps `crate::AUTOSTART_SETTLE` (10 s) unchanged.
///
/// 1.2 s, not 0: the point of the wait is that the shell is mid-logon, and the
/// evidence for what "settled enough" means is the owner's own log — the hook
/// was installed 27 ms in and his first Space hold landed at 320 ms. A window
/// created at 1.2 s is comfortably ahead of any hold a human can produce after
/// noticing the tray icon, and it is still a wait rather than a race with
/// `tauri::Builder`'s own startup. It is also the same order of magnitude as
/// `display_watch`'s 1.2 s post-display-change settle, which is the only other
/// "let Windows finish moving" constant in this app.
pub const OVERLAY_SETTLE: Duration = Duration::from_millis(1200);

/// Set once, as the first statement of `crate::run()`. Read by the "overlay
/// usable" log line so "how long after process start could the HUD draw?" is
/// one grep instead of subtracting two timestamps by hand.
static STARTED: OnceLock<Instant> = OnceLock::new();

/// A main-thread creation attempt is already queued. Prevents one hold per
/// second from posting one closure per hold; cleared by the closure itself, so
/// a later hold can still ask again if the attempt did not succeed.
static ATTEMPT_QUEUED: AtomicBool = AtomicBool::new(false);

/// Call once, as early in `run()` as possible.
pub fn mark_process_start() {
    let _ = STARTED.set(Instant::now());
}

/// How long ago `mark_process_start` ran. Zero if it never did (tests, and any
/// future entry point that forgets) — a zero in the log reads as "unmeasured"
/// rather than as a wrong number.
pub fn since_start() -> Duration {
    STARTED.get().map_or(Duration::ZERO, Instant::elapsed)
}

/// Everything the decision depends on, gathered by the caller so the decision
/// itself is pure and can be tested without a window, a webview or a clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootFacts {
    /// `--autostart` was on the command line (PROBLEM 64's Run value).
    pub autostart: bool,
    /// PROBLEM 253 — this launch deliberately has no overlay.
    pub safe_mode: bool,
    /// `create_app_windows` has already run (or is running).
    pub windows_created: bool,
    /// A window with the label `overlay` exists right now.
    pub overlay_exists: bool,
    /// A Space hold is asking for the ring RIGHT NOW. PROBLEM 215 already
    /// established that asking for the app is asking for its UI — the tray's
    /// "Open Settings" and the single-instance handler both skip the settle for
    /// exactly this reason. A hold is that same request, aimed at the overlay.
    pub demanded: bool,
    /// Time since `mark_process_start`.
    pub since_start: Duration,
}

/// Why there is nothing to do. Named rather than a bare `false`, because three
/// of the four reasons are states a reader has to be able to tell apart in a
/// log — "safe mode chose not to" and "it failed to build" are the two PROBLEM
/// 253 spent a whole entry separating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// PROBLEM 253 — safe mode has no overlay, by design.
    SafeMode,
    /// It is already there. The normal outcome of the second and later calls.
    AlreadyThere,
    /// `create_app_windows` owns the overlay from here; do not race it.
    FullCreationDone,
    /// A manual launch builds both windows inline, at the same instant Tauri
    /// itself would have. There is no early phase to run.
    NotAnAutostartLaunch,
}

/// The decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayBoot {
    /// Build it, on the main thread, now.
    CreateNow,
    /// Not yet — this much of [`OVERLAY_SETTLE`] is still to run.
    WaitLonger(Duration),
    /// Nothing to do, for this reason.
    NothingToDo(Skip),
}

/// PROBLEM 265 — should the overlay be created right now?
///
/// The order of these checks is load-bearing:
///
/// 1. **Safe mode first.** PROBLEM 253's whole point is that a launch which has
///    already died three times comes up with no overlay; a "make it faster"
///    path that quietly put one back would undo that, and would do it on
///    exactly the machine that could least afford it.
/// 2. **Already there.** Cheapest true answer, and the one every repeat call
///    hits.
/// 3. **Full creation done.** `create_app_windows` is idempotent but it is not
///    re-entrant from two threads at once; once it has run, it owns both
///    windows and the PROBLEM 59 recovery inside it is the thing that rebuilds
///    a missing overlay.
/// 4. **Manual launch.** Nothing to do early — the inline call already did it.
/// 5. **Demand beats the clock.** A hold that has arrived is better evidence
///    that the user is present and the shell is usable than any timer.
pub fn plan(f: BootFacts) -> OverlayBoot {
    if f.safe_mode {
        return OverlayBoot::NothingToDo(Skip::SafeMode);
    }
    if f.overlay_exists {
        return OverlayBoot::NothingToDo(Skip::AlreadyThere);
    }
    if f.windows_created {
        return OverlayBoot::NothingToDo(Skip::FullCreationDone);
    }
    if !f.autostart {
        return OverlayBoot::NothingToDo(Skip::NotAnAutostartLaunch);
    }
    if f.demanded || f.since_start >= OVERLAY_SETTLE {
        return OverlayBoot::CreateNow;
    }
    OverlayBoot::WaitLonger(OVERLAY_SETTLE - f.since_start)
}

// ---------------------------------------------------------------------------
// The impure half: gather the facts, and build the window if the plan says so.
// ---------------------------------------------------------------------------

/// Build the `overlay` window NOW, from its own `tauri.conf.json` declaration,
/// and configure it through the same function every other overlay path uses.
///
/// **MAIN THREAD ONLY** — window creation is not thread-safe anywhere in Win32,
/// and this is the same rule `create_app_windows` and `display_watch`'s rebuild
/// already follow.
///
/// `from_config`, not a hand-written builder: PROBLEM 81 is what a hand-copied
/// builder produced ("an opaque, decorated, focus-stealing rectangle"), and
/// PROBLEM 215 chose `from_config` for the same reason. A failure here is a
/// WARN, not an ERROR, and it is not fatal: `create_app_windows`'s PROBLEM 59
/// existence check runs at the full settle and rebuilds it, and `display_watch`
/// self-heals it after that.
pub fn create_overlay_now(app: &tauri::AppHandle, demanded: bool) {
    use tauri::Manager;

    let facts = BootFacts {
        autostart: crate::autostart_launch(),
        safe_mode: crate::safe_mode::active(),
        windows_created: crate::windows_created(),
        overlay_exists: app.get_webview_window("overlay").is_some(),
        demanded,
        since_start: since_start(),
    };
    match plan(facts) {
        OverlayBoot::CreateNow => {}
        OverlayBoot::NothingToDo(Skip::SafeMode) => {
            log::info!(
                "overlay-early: SAFE MODE — not creating the overlay ahead of the settle. \
                 This launch has no Guide HUD by design (PROBLEM 253); nothing tried and \
                 failed."
            );
            return;
        }
        OverlayBoot::NothingToDo(_) | OverlayBoot::WaitLonger(_) => return,
    }

    let Some(wc) = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "overlay")
        .cloned()
    else {
        log::error!(
            "overlay-early: tauri.conf.json declares no window labelled 'overlay' — the Guide \
             HUD cannot be built early or late"
        );
        return;
    };

    match tauri::WebviewWindowBuilder::from_config(app, &wc).and_then(|b| b.build()) {
        Ok(w) => {
            log::info!(
                "overlay-early: window 'overlay' created {} ms after app start, ahead of the \
                 {}s dashboard settle ({}). PROBLEM 265.",
                since_start().as_millis(),
                crate::AUTOSTART_SETTLE.as_secs(),
                if demanded {
                    "a Space hold asked for it"
                } else {
                    "on the overlay settle timer"
                }
            );
            // Every guard the overlay has ever needed, through the one function
            // both other creation paths use: hidden, NoActivate, DWM border
            // cleared, corners un-rounded, and click-through that FAILS CLOSED.
            crate::configure_overlay_window(&w);
            // PROBLEM 86 — register it with the opacity action before anything
            // can fade it. `create_app_windows` does this too, ten seconds from
            // now; doing it here as well closes the gap rather than reasoning
            // about whether the gap is reachable.
            #[cfg(windows)]
            if let Ok(h) = w.hwnd() {
                crate::engine::actions::opacity::register_own_hwnd(h.0 as isize);
            }
        }
        Err(e) => log::warn!(
            "overlay-early: the overlay could not be created {} ms after app start ({e}) — \
             this is survivable and expected to be rare: it is most likely PROBLEM 59's \
             cold-boot WebView2 race, and the {}s window creation below rebuilds it, after \
             which display_watch self-heals it for the rest of the session. The app is in \
             exactly the state it was in before this optimisation existed.",
            since_start().as_millis(),
            crate::AUTOSTART_SETTLE.as_secs()
        ),
    }
}

/// PROBLEM 265 — a Space hold arrived and there is no overlay to draw into.
///
/// Called from `guide_hud`, on the engine thread, so it cannot build anything
/// itself; it hops to the main thread. **This does not rescue the hold that
/// called it** — a webview takes far longer than a hold to boot — and it is not
/// meant to: it is the difference between "the first hold pays a one-off cost"
/// and "every hold until the timer fires draws nothing". With
/// [`OVERLAY_SETTLE`] at 1.2 s this branch should be all but unreachable; it
/// stays because a main thread that is busy at logon is exactly the condition
/// the settle exists for, and a user holding Space is the one signal that
/// outranks a timer.
pub fn request_now(app: &tauri::AppHandle) {
    if ATTEMPT_QUEUED.swap(true, Ordering::SeqCst) {
        return;
    }
    let h = app.clone();
    if let Err(e) = app.run_on_main_thread(move || {
        create_overlay_now(&h, true);
        ATTEMPT_QUEUED.store(false, Ordering::SeqCst);
    }) {
        ATTEMPT_QUEUED.store(false, Ordering::SeqCst);
        log::warn!(
            "overlay-early: a Space hold asked for the overlay but the main thread could not \
             be reached ({e}) — the settle thread will still create it"
        );
    }
}

// ---------------------------------------------------------------------------
// Tests — the pure decision only. There is no honest unit test for creating a
// WebView2 window, and PROBLEM 118 is what pretending otherwise costs.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    /// A normal autostart launch, one second in, nothing built yet.
    fn autostart_at(ms: u64) -> BootFacts {
        BootFacts {
            autostart: true,
            safe_mode: false,
            windows_created: false,
            overlay_exists: false,
            demanded: false,
            since_start: Duration::from_millis(ms),
        }
    }

    #[test]
    fn waits_before_the_overlay_settle() {
        assert_eq!(
            plan(autostart_at(0)),
            OverlayBoot::WaitLonger(OVERLAY_SETTLE)
        );
        assert_eq!(
            plan(autostart_at(200)),
            OverlayBoot::WaitLonger(Duration::from_millis(1000))
        );
    }

    #[test]
    fn creates_at_the_overlay_settle() {
        assert_eq!(
            plan(autostart_at(OVERLAY_SETTLE.as_millis() as u64)),
            OverlayBoot::CreateNow
        );
        assert_eq!(plan(autostart_at(5_000)), OverlayBoot::CreateNow);
    }

    /// The whole point of the entry: the overlay must not be made to wait the
    /// dashboard's ten seconds.
    #[test]
    fn the_overlay_does_not_wait_the_full_autostart_settle() {
        assert!(OVERLAY_SETTLE < crate::AUTOSTART_SETTLE);
        assert_eq!(plan(autostart_at(1_500)), OverlayBoot::CreateNow);
    }

    /// PROBLEM 215's rule, applied to the ring: a hold IS a request for the UI.
    #[test]
    fn a_hold_beats_the_timer() {
        let mut f = autostart_at(50);
        assert!(matches!(plan(f), OverlayBoot::WaitLonger(_)));
        f.demanded = true;
        assert_eq!(plan(f), OverlayBoot::CreateNow);
    }

    /// PROBLEM 253 — and it outranks the demand, not the other way round.
    #[test]
    fn safe_mode_wins_over_everything() {
        let mut f = autostart_at(9_000);
        f.safe_mode = true;
        assert_eq!(plan(f), OverlayBoot::NothingToDo(Skip::SafeMode));
        f.demanded = true;
        assert_eq!(plan(f), OverlayBoot::NothingToDo(Skip::SafeMode));
        f.overlay_exists = true;
        assert_eq!(plan(f), OverlayBoot::NothingToDo(Skip::SafeMode));
    }

    #[test]
    fn an_existing_overlay_is_never_rebuilt() {
        let mut f = autostart_at(9_000);
        f.overlay_exists = true;
        assert_eq!(plan(f), OverlayBoot::NothingToDo(Skip::AlreadyThere));
        f.demanded = true;
        assert_eq!(plan(f), OverlayBoot::NothingToDo(Skip::AlreadyThere));
    }

    /// Once `create_app_windows` has run it owns both windows, and its own
    /// PROBLEM 59 recovery is what rebuilds a missing overlay.
    #[test]
    fn the_full_creation_path_takes_over() {
        let mut f = autostart_at(20_000);
        f.windows_created = true;
        assert_eq!(plan(f), OverlayBoot::NothingToDo(Skip::FullCreationDone));
        f.demanded = true;
        assert_eq!(plan(f), OverlayBoot::NothingToDo(Skip::FullCreationDone));
    }

    /// A manual launch never had the problem: PROBLEM 215 creates both windows
    /// inline, at the instant Tauri itself would have.
    #[test]
    fn a_manual_launch_has_no_early_phase() {
        let mut f = autostart_at(0);
        f.autostart = false;
        assert_eq!(
            plan(f),
            OverlayBoot::NothingToDo(Skip::NotAnAutostartLaunch)
        );
        f.demanded = true;
        assert_eq!(
            plan(f),
            OverlayBoot::NothingToDo(Skip::NotAnAutostartLaunch)
        );
    }

    /// The remaining wait must never overflow past zero — `WaitLonger` is fed
    /// straight to `sleep`.
    #[test]
    fn the_remaining_wait_is_always_within_the_settle() {
        for ms in [0_u64, 1, 599, 1_199] {
            match plan(autostart_at(ms)) {
                OverlayBoot::WaitLonger(d) => {
                    assert!(d > Duration::ZERO && d <= OVERLAY_SETTLE, "ms={ms} d={d:?}");
                }
                other => panic!("ms={ms} gave {other:?}"),
            }
        }
    }

    /// `since_start` must be honest when nothing marked the start, rather than
    /// panicking or inventing a number.
    #[test]
    fn since_start_is_zero_until_marked() {
        // `STARTED` is a process-wide OnceLock, so this only asserts the
        // no-panic contract; the value is whatever this test binary has done.
        let _ = since_start();
    }
}
