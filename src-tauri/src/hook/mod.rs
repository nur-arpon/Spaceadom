/// hook/mod.rs — Dedicated Win32 keyboard hook thread.
///
/// Architecture guarantees:
/// • Runs on a completely isolated OS thread with its own Win32 message pump.
/// • Uses `GetMessage` (blocking) → 0 % CPU when idle.
/// • Communicates events to the engine via a `crossbeam_channel` SPSC sender.
/// • Never touches Tauri/WebView2 on the critical path.

pub mod fullscreen;
pub mod exclusions;
pub mod conflicts;
pub mod conflict_close;
pub mod pointer;

use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Public event type sent to the async engine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum KeyCombo {
    Alpha(char),      // Space + a–z
    Special(String),  // Space + F1–F12, Enter, Tab, Left, Right (user-configurable)
    Escape,           // Space + Esc   → Boss Key
    Backtick,         // Space + `     → PiP
    Comma,            // Space + ,     → Focus Engine
    RightAlt,         // Space + RAlt  → Profile Cycle
    UpArrow,          // Space + Up    → Scroll-Top (double-tap)
    DownArrow,        // Space + Down  → Scroll-Bottom (double-tap)
    Period,           // Space + .     → Bypass Toggle
    Backspace,        // Space + ⌫     → Force Close (Alt+F4)
    /// Space + Tab → FULLSCREEN-PRESERVING PiP (pip.rs §9, PROBLEM 219).
    ///
    /// A FIXED special, like Esc and the backtick — not one of the optional
    /// user-bindable ones any more. Tab used to be bit 13 of
    /// `BOUND_SPECIALS`, i.e. a key the user could point at an app through
    /// `special_keys`, which nothing in the UI has ever been able to write
    /// (see the `BOUND_SPECIALS` comment). The owner claimed it for this
    /// feature on 2026-08-29 precisely because it was unbound in practice.
    ///
    /// `allow(dead_code)` because 1.0.91 comments out the only place this
    /// variant is CONSTRUCTED (the `VK_TAB` arm of the combo match); the
    /// engine still matches on it. 1.0.92 constructs it again.
    #[allow(dead_code)]
    Tab,
}

#[derive(Debug, Clone)]
pub enum HookEvent {
    SpaceDown,
    SpaceUp { modifier_fired: bool },
    KeyCombo(KeyCombo),
    WheelUp,
    WheelDown,
    /// PROBLEM 206 — pointer activation on the Guide HUD: the cursor was
    /// resting on this binding's chip when Space was released (gesture A) or
    /// the left button went down (gesture B). Carries the chip's key char;
    /// the engine dispatches it exactly like `KeyCombo(Alpha)` — the same
    /// `cancel_hud(true)` handover (PROBLEM 135), the same `handle_alpha` →
    /// `smart_cascade` → toast. This variant is the ONLY route from the
    /// overlay page to a launch: `handle_alpha` is private and `EngineState`
    /// is never `.manage()`d, so no `#[tauri::command]` could reach it.
    PointerActivate(char),
}

// ---------------------------------------------------------------------------
// Shared atomic state (written by hook thread, read by engine)
// ---------------------------------------------------------------------------

/// `true` when Space is held down as a modifier.
pub static MODIFIER_ACTIVE: AtomicBool = AtomicBool::new(false);
/// `true` when Bypass mode is active (hook passes Space through without interception).
pub static BYPASS_MODE: AtomicBool = AtomicBool::new(false);
/// `true` when a fullscreen game is active — hook passes everything through.
pub static FULLSCREEN_ACTIVE: AtomicBool = AtomicBool::new(false);
/// `true` when the foreground app is on the user's App-exceptions list — hook
/// passes everything through, so Space is stock in there (Photoshop's
/// hold-Space panning, a game's Space key, anything).
///
/// Written ONLY by the `st-exclusion-watcher` poller in hook/exclusions.rs.
/// The hook callback may do lock-free atomics only, and asking Windows which
/// window is in front is a win32k call (PROBLEM 58/134/184).
pub static EXCLUDED_ACTIVE: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Suppression counters — LOCK-FREE ATOMICS ONLY (PROBLEM 58).
//
// The hook callback must never touch the logger: log4rs writes synchronously
// to disk, and disk I/O inside a WH_KEYBOARD_LL callback makes Windows evict
// the hook once it overruns LowLevelHooksTimeout. These counters cost a single
// atomic increment; `drain_hook_diagnostics()` turns them into log lines from
// the ENGINE thread, where blocking is harmless.
// ---------------------------------------------------------------------------
use std::sync::atomic::AtomicU32;
static SUPPRESS_FULLSCREEN: AtomicU32 = AtomicU32::new(0);
static SUPPRESS_BYPASS: AtomicU32 = AtomicU32::new(0);
static ROLLOVER_HITS: AtomicU32 = AtomicU32::new(0);
static STUCK_MODIFIER: AtomicU32 = AtomicU32::new(0);
static UNMAPPED_KEYS: AtomicU32 = AtomicU32::new(0);

/// PROBLEM 104 — the counter that answers "does the hook see ANY key?".
///
/// The user reports that with the Spaceadom window focused, nothing works at
/// all: no Guide HUD, no toasts, no launches. Every existing counter only
/// records keys the hook DECIDED something about, so a hook that never fires
/// and a hook that fires and passes everything through look identical — both
/// leave zeros everywhere. This is incremented on entry, before any branch,
/// so a still-zero value is proof the callback is not being invoked.
static KB_EVENTS_SEEN: AtomicU32 = AtomicU32::new(0);
/// Of those, how many arrived while OUR OWN window held the foreground. If
/// this stays 0 while the total climbs, Windows is not delivering our own
/// window's keystrokes to our hook — which is the user's exact symptom.
static KB_EVENTS_OWN_FG: AtomicU32 = AtomicU32::new(0);
/// PROBLEM 218 — how many 1s watchdog ticks happened in this window, and how
/// many of them found OUR OWN window in the foreground. The denominator
/// `KB_EVENTS_OWN_FG` never had. See the sampling site in `watchdog_check`.
static FG_SAMPLES: AtomicU32 = AtomicU32::new(0);
static FG_SELF_SAMPLES: AtomicU32 = AtomicU32::new(0);
/// Of the watchdog alarms raised in this window, how many found our own
/// window in the foreground. `BLIND_WHILE_OWN_FG` already counts this, but it
/// is drained only on ESCALATION — which happens a handful of times a day, so
/// the ratio that matters was never printed beside the exposure that explains
/// it.
static WD_ALARMS: AtomicU32 = AtomicU32::new(0);
static WD_ALARMS_OWN_FG: AtomicU32 = AtomicU32::new(0);
/// Which cooldown window the hold-off line has already been printed for, so
/// one 60s hold-off produces one line instead of sixty.
static HOLDOFF_LOGGED_FOR: AtomicU64 = AtomicU64::new(0);
static DROPPED_EVENTS: AtomicU32 = AtomicU32::new(0);

/// What one 60-second diagnostics window actually observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookWindow {
    /// The primary hook saw keys. Working.
    Working,
    /// Nobody typed. NOT evidence about the hook (PROBLEM 101) — stay quiet.
    Quiet,
    /// Genuine reference-hook callbacks happened and the primary hook saw NONE
    /// of them. Keys reached this thread's chain and ours missed them. This is
    /// the only state that may be called deafness, and the only one that may
    /// reach Sentry.
    Deaf,
}

/// PROBLEM 228 — the deafness test, as arithmetic on COUNTERS.
///
/// `primary_seen` — `KB_EVENTS_SEEN` for the window (callback-only).
/// `genuine_ref_events` — the increase in `REF_KB_EVENTS` across the SAME
/// window. Callback-only by construction: `install_hooks()` cannot reach it.
///
/// The old test asked `ref_silence < 60_000` off `LAST_REF_KB_EVENT`, a clock
/// the re-hook itself stamped — so a window in which NOTHING happened except a
/// watchdog repair came out "deaf", which is how 281 of 282 DEAF lines were
/// produced. A count that only the hook can increment cannot be forged by the
/// repair, which is the entire fix: **the instrument must not be writable by
/// the thing it is measuring.**
pub(crate) fn classify_hook_window(primary_seen: u32, genuine_ref_events: u32) -> HookWindow {
    if primary_seen > 0 {
        HookWindow::Working
    } else if genuine_ref_events > 0 {
        HookWindow::Deaf
    } else {
        HookWindow::Quiet
    }
}

/// Report and reset the hook's suppression counters. Called from the ENGINE
/// thread (never the hook thread). Silent when everything is zero, so a
/// healthy log stays clean.
/// PROBLEM 183 — WHY THIS MUST ALSO BE CALLED ON A TIMER.
///
/// For its whole life this had exactly ONE caller: the engine's `SpaceUp` arm.
/// A `SpaceUp` only reaches the engine if the hook received Space-down AND
/// Space-up AND the channel send succeeded — so **every line it has ever
/// printed was written at an instant when the hook was demonstrably working.**
///
/// That makes it blind to the only episode anyone cares about. When the owner
/// reports *"it works only if I minimise the app"*, there is no successful
/// Space release during the failure, so there is no drain and no line at all.
/// The next line you read is a 60-second summary published from the far side of
/// the outage, by which time the counters describe the recovery.
///
/// I built an argument on those zeros — "0 of them while the Spaceadom window
/// had focus, twelve samples running" — and it was worthless: the samples could
/// not have contained the failure. `KB_EVENTS_OWN_FG` has in fact read non-zero
/// 35 times historically (`2026-08-16 20:30:05.439 … 341 of them`), which alone
/// disproves the reading I gave it.
///
/// Called from a tokio interval on the ENGINE thread as well now, so a minute
/// with no successful Space still produces a line. Never call it from the hook
/// CALLBACK — it logs, and PROBLEM 58 is what that costs.
pub fn drain_hook_diagnostics() {
    let fs = SUPPRESS_FULLSCREEN.swap(0, Ordering::Relaxed);
    let by = SUPPRESS_BYPASS.swap(0, Ordering::Relaxed);
    let ro = ROLLOVER_HITS.swap(0, Ordering::Relaxed);
    let st = STUCK_MODIFIER.swap(0, Ordering::Relaxed);
    let un = UNMAPPED_KEYS.swap(0, Ordering::Relaxed);
    let dr = DROPPED_EVENTS.swap(0, Ordering::Relaxed);
    let rh = HOOK_REINSTALLS.swap(0, Ordering::Relaxed);
    let os = PASSED_TO_OS.swap(0, Ordering::Relaxed);
    let ex = SUPPRESS_EXCLUDED.swap(0, Ordering::Relaxed);
    // MUST-FIX 2 — drained alongside `os` (passed-to-os): both counters are
    // "a modifier was held", but this one is the Space-UP side that DROPPED
    // the pending space injection rather than passing a keystroke through.
    let dm = SPACE_DROPPED_MODIFIER.swap(0, Ordering::Relaxed);
    // PROBLEM 104 — reported at most once a minute. This function drains on
    // every Space RELEASE, so logging unconditionally wrote a line every few
    // seconds while typing: the same log-noise problem the watchdog had, in a
    // line that was added to diagnose it. The counters keep accumulating
    // between reports, so nothing is lost — only the printing is throttled.
    {
        static LAST_SEEN_REPORT: AtomicU64 = AtomicU64::new(0);
        let now = tick_count();
        let last = LAST_SEEN_REPORT.load(Ordering::Relaxed);
        // MUST-FIX 1 — `LAST_SEEN_REPORT` starts at 0 and `tick_count()` is
        // `GetTickCount64`: MACHINE UPTIME, not an elapsed window. On the
        // very first drain of a process `now - 0` equals however long the
        // box has been on, so the `>= 60_000` gate below was satisfied
        // instantly and the FIRST line printed every single launch lied
        // about what it measured. Measured live:
        //     08:09:20.439  hook: saw 70 key event(s) in the last 38539s ...
        // — on a machine that had booted 38538s earlier. Seed the clock (and
        // zero the two counters, so pre-launch accumulation can't leak into
        // the first real window) and print NOTHING here, so the first line
        // that ever prints measures a window it actually covered.
        // PROBLEM 228 — the reference counter's baseline for THIS window. It
        // must be seeded and advanced in lockstep with `LAST_SEEN_REPORT`, or
        // the two halves of the deafness test would describe different windows
        // — which is the same class of mistake as the one being fixed.
        static LAST_REF_COUNT: AtomicU32 = AtomicU32::new(0);
        if last == 0 {
            LAST_SEEN_REPORT.store(now, Ordering::Relaxed);
            KB_EVENTS_SEEN.store(0, Ordering::Relaxed);
            KB_EVENTS_OWN_FG.store(0, Ordering::Relaxed);
            LAST_REF_COUNT.store(REF_KB_EVENTS.load(Ordering::Relaxed), Ordering::Relaxed);
        } else if now.saturating_sub(last) >= 60_000 {
            let seen = KB_EVENTS_SEEN.swap(0, Ordering::Relaxed);
            let own = KB_EVENTS_OWN_FG.swap(0, Ordering::Relaxed);
            // PROBLEM 218 — the exposure and the alarm rate that go with the
            // `own` numerator. Drained together so the three can never
            // describe different windows.
            let fg_n = FG_SAMPLES.swap(0, Ordering::Relaxed);
            let fg_self = FG_SELF_SAMPLES.swap(0, Ordering::Relaxed);
            let al_n = WD_ALARMS.swap(0, Ordering::Relaxed);
            let al_self = WD_ALARMS_OWN_FG.swap(0, Ordering::Relaxed);
            // ALWAYS advance the window, even when there is nothing to print.
            // This used to advance only inside `if seen > 0`, so a drain that
            // found the counters empty swapped them to zero, printed nothing,
            // and left LAST_SEEN_REPORT where it was — quietly discarding an
            // accumulated `own` count that had never been reported. A counter
            // that can be zeroed without ever being read is not a measurement.
            LAST_SEEN_REPORT.store(now, Ordering::Relaxed);
            let elapsed_s = now.saturating_sub(last) / 1000;
            // SHOULD-FIX 3 — `millis_since_last_input()` (GetLastInputInfo)
            // counts MOUSE movement too, so `seen == 0 && idle_ms < 60_000`
            // is ALSO the signature of someone reading a page with their
            // hands off the keyboard — the exact ambiguity PROBLEM 101
            // deleted a whole detector over, and this line was reinstating
            // it at INFO. Use the REFERENCE hook instead (PROBLEM 181): a
            // second do-nothing WH_KEYBOARD_LL that cannot be evicted for
            // being slow, so `ref_silence` answers "did any key reach this
            // thread's hook chain" definitively, not "was the mouse
            // touched". Computed the same way `watchdog_check` computes it.
            //
            // PROBLEM 228 — and `ref_silence` is NOT how to ask it. That clock
            // was stamped by `install_hooks()` on every watchdog re-hook, so
            // the question "did any key reach the chain?" was being answered by
            // the repair. Ask the COUNTER instead: how many times was the
            // reference hook genuinely CALLED during this window. Nothing but
            // the callback can move it.
            let ref_now = REF_KB_EVENTS.load(Ordering::Relaxed);
            let ref_events = ref_now.wrapping_sub(LAST_REF_COUNT.swap(ref_now, Ordering::Relaxed));
            // Kept only to say WHEN, never whether — and honest now that the
            // install no longer writes it.
            let ref_silence = now.saturating_sub(LAST_REF_KB_EVENT.load(Ordering::Relaxed));
            let ref_ever = ref_now > 0;
            // PROBLEM 218 — printed on EVERY window, including a silent one.
            // The question this answers ("is the app deafer while its own
            // window is focused?") is a rate, and a rate needs the windows
            // where nothing was typed as much as the ones where something was.
            if fg_self > 0 || al_self > 0 {
                log::info!(
                    "hook focus exposure — the Spaceadom window itself held the foreground \
                     for {fg_self} of the last {fg_n} one-second samples, and {al_self} of \
                     the {al_n} watchdog alarm(s) in that window were raised while it did. \
                     Compare the two ratios: if alarms are over-represented against exposure, \
                     deafness IS specific to our own window being focused; if they track, it \
                     is not (PROJECT_STATUS 2026-08-25 recorded this as open for want of a \
                     denominator)."
                );
            }
            match classify_hook_window(seen, ref_events) {
                HookWindow::Working => {
                    log::info!(
                        "hook: saw {seen} key event(s) in the last {elapsed_s}s, {own} of them \
                         while the Spaceadom window itself had focus"
                    );
                }
                HookWindow::Deaf => {
                    // Genuine reference callbacks happened and the primary saw
                    // none of them: keys reached the chain and this hook missed
                    // them. Name it, so it can't be misread as "nobody typed" a
                    // year from now — and say WHICH measurement says so, because
                    // the previous version of this line said "the reference hook
                    // fired 16ms ago" when what had happened 16 ms earlier was a
                    // re-hook (PROBLEM 228).
                    let last_genuine = if ref_ever {
                        format!("the last genuine key reached it {ref_silence}ms ago")
                    } else {
                        "no genuine key has reached it since launch".to_string()
                    };
                    log::warn!(
                        "hook: DEAF for the last {elapsed_s}s — the reference hook was \
                         genuinely CALLED {ref_events} time(s) in that window ({last_genuine}) \
                         but the primary hook saw 0 of them. Counted, not inferred from a \
                         timestamp the re-hook could have stamped. This is NOT 'nobody typed'."
                    );
                    // PROBLEM 217 — promoted to the crash reporter. This is the
                    // app silently ceasing to do the one thing it exists for,
                    // with nothing on screen to say so; at WARN it never left the
                    // machine. Rate-limited per condition: this fired 38 times in
                    // one session on the owner's machine and that must be ONE
                    // event. (Called from the ENGINE thread — see this function's
                    // header — never from the hook callback.)
                    //
                    // PROBLEM 228 — and it now fires only on COUNTED reference
                    // events. Seventeen of these events were sent on the old
                    // test, of which the log says at most one could have been
                    // real; a report that is wrong 281 times out of 282 is worse
                    // than no report, because it teaches its reader to ignore it.
                    crate::telemetry::report_degraded(
                        crate::telemetry::Degraded::HookDeaf,
                        &format!(
                            "the primary keyboard hook saw 0 events in {elapsed_s}s while the \
                             reference hook was genuinely called {ref_events} time(s) in the \
                             same window — keys are reaching the chain and this app is not \
                             seeing them, so every shortcut is dead"
                        ),
                    );
                }
                // The reference hook was ALSO silent this window, so nobody
                // typed. Idle time is not evidence about the hook (PROBLEM 101)
                // — stay quiet rather than reinstate the ambiguity.
                HookWindow::Quiet => {}
            }
        }
    }
    // PROBLEM 218 — a non-zero value here is the fingerprint of a lost
    // Space-UP, which until now left no trace at all: the hold simply never
    // ended and the log said nothing about why.
    let sr = STALE_HOLDS_REAPED.swap(0, Ordering::Relaxed);
    // PROBLEM 227 — two of the injection sites live in the hook callback, where
    // a log call is forbidden (PROBLEM 58), so a short `SendInput` can only
    // report itself through a counter drained here.
    let pi = PARTIAL_INJECTIONS.swap(0, Ordering::Relaxed);
    let ck = CORRECTIVE_KEYUPS.swap(0, Ordering::Relaxed);
    if pi > 0 {
        log::warn!(
            "hook: {pi} SendInput batch(es) were only PARTIALLY inserted — another thread \
             blocked injection partway (BlockInput, or UIPI while an elevated window has \
             focus). {ck} corrective KEYUP(s) were sent so nothing was left latched \
             (NATIVE_SAFETY.md §3). If a modifier or Space felt stuck at that moment, this \
             is the line that explains it."
        );
    }
    if fs == 0 && by == 0 && ro == 0 && st == 0 && un == 0 && dr == 0 && rh == 0 && os == 0
        && dm == 0 && ex == 0 && sr == 0 && pi == 0
    {
        return;
    }
    log::info!(
        "hook diagnostics — fullscreen-suppressed:{fs} bypass-suppressed:{by} \
         typed-not-command(rollover):{ro} stuck-modifier-resets:{st} unmapped-keys:{un} \
         dropped-events:{dr} watchdog-reinstalls:{rh} passed-to-os(ctrl/alt/win held):{os} \
         space-dropped(modifier still held on release):{dm} excluded-app:{ex} \
         stale-holds-reaped(lost Space-UP):{sr}"
    );
    if ro > 0 {
        // The advice here used to say "set a SLOWER typing speed (a slower
        // setting narrows the window)". Both halves were backwards: a slower
        // setting WIDENS the window (16800/wpm), which produces MORE of these,
        // so following it made the reported problem worse.
        log::info!(
            "hook: {ro} key(s) landed INSIDE the rollover window and were typed instead of \
             treated as a shortcut. Hold Space slightly longer before the letter, or set a \
             FASTER 'Typing speed' in Settings — a faster setting NARROWS the window, so a \
             shorter hold counts as a command."
        );
    }

    // PROBLEM 95 — the Space-down→key-down delay distribution from REAL typing.
    // This is the measurement that decides whether the window is safe for THIS
    // person's hands; it cannot be obtained by simulating keystrokes.
    let window = ROLLOVER_MS.load(Ordering::Relaxed);
    let typed: Vec<u32> = MARGIN_TYPED.iter().map(|b| b.swap(0, Ordering::Relaxed)).collect();
    let cmd: Vec<u32> = MARGIN_COMMAND.iter().map(|b| b.swap(0, Ordering::Relaxed)).collect();
    if typed.iter().chain(cmd.iter()).any(|&n| n > 0) {
        let fmt = |v: &[u32]| {
            v.iter()
                .enumerate()
                .filter(|(_, &n)| n > 0)
                .map(|(i, &n)| {
                    let lo = i as u64 * MARGIN_BUCKET_MS;
                    if i == MARGIN_BUCKETS - 1 {
                        format!("{lo}+ms:{n}")
                    } else {
                        format!("{lo}-{}ms:{n}", lo + MARGIN_BUCKET_MS - 1)
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        log::info!(
            "hook margins (window {window}ms) — TYPED [{}] | COMMAND [{}]",
            fmt(&typed),
            fmt(&cmd)
        );
        // The danger sign: ordinary typing arriving within one bucket of the
        // threshold. One heavier-thumbed day and those become commands.
        let near = window.saturating_sub(MARGIN_BUCKET_MS) / MARGIN_BUCKET_MS;
        let close: u32 = typed.iter().skip(near as usize).sum();
        if close > 0 {
            log::warn!(
                "hook: {close} keystroke(s) came within {}ms of being treated as a command \
                 while typing. If shortcuts ever fire mid-sentence, set a SLOWER 'Typing \
                 speed' in Settings — that WIDENS the window and pushes ordinary typing \
                 further from the threshold.",
                MARGIN_BUCKET_MS
            );
        }
    }
}
/// `true` when the current Space-down has already been aborted (another key hit in rollover window).
static SPACE_ABORTED: AtomicBool = AtomicBool::new(false);
/// `true` when we actually swallowed the current Space-down. If we passed it
/// through (Ctrl/Alt/Win held, bypass, fullscreen), the matching Space-up must
/// pass through too — otherwise we inject a phantom space the user never typed.
static SPACE_INTERCEPTED: AtomicBool = AtomicBool::new(false);
/// Timestamp (ms) when Space was pressed down.
static SPACE_DOWN_TS: AtomicU64 = AtomicU64::new(0);

// --- PROBLEM 218 — the stale-hold reaper ----------------------------------
//
// THE FAILURE THIS EXISTS FOR. `HookEvent::SpaceUp` is the only thing that
// reaches `engine::cancel_hud(false)`, and it can only be sent by a callback
// that is still installed. On this machine the hooks are evicted 15-40 times
// a day (PROBLEM 173/181/182), so a hold that straddles an eviction loses its
// Space-UP outright: `MODIFIER_ACTIVE` stays latched, `HUD_VISIBLE` stays
// true, the chips stay published, and the ring stays on screen with nothing
// left in the system able to take it down.
//
// It is worse than a stuck picture. The MOUSE hook is a separate hook and
// often survives, so `note_cursor` keeps running, `st-hud-pointer` keeps
// arming chips against the stranded ring, and the next left-click activates
// whichever chip the cursor happens to point at. That is the owner's report
// of 2026-08-28, in his words: *"it interacted, but even after I left my hand
// from the space it stayed. And it ultimately opened whatever my cursor was
// towards."*
//
// `watchdog_check` already repairs this — but ONLY on the tick where it
// decides to re-hook, and it declines to reach that decision while
// `millis_since_last_input()` says the user has been idle for 2s, or while
// the 60s cooldown is holding. So the repair is not bounded by anything the
// user can feel.
//
// WHY NOT `GetAsyncKeyState(VK_SPACE)`. Because we SUPPRESS Space-down,
// Windows never marks it pressed, and the API reports a physically-held Space
// as UP. Building a failsafe on it is what broke every shortcut in the app
// once already — the FAILSAFE comment in the combo branch is the record of
// it, and CLAUDE.md's keyboard laws forbid it outright.
//
// WHAT IS USED INSTEAD. Windows auto-repeat. A physically-held Space produces
// a fresh WM_KEYDOWN every repeat period for as long as it is held, and every
// one of them enters this callback (that is precisely why the down branch
// says "always suppress Space down to prevent auto-repeat leaking to the OS").
// So auto-repeat IS the liveness signal for a hold, it costs one relaxed store
// on a branch that already runs, and it stops arriving the instant the hook
// stops being called — which is the condition we cannot otherwise observe.
/// GetTickCount64 ms of the most recent Space-down of the CURRENT hold — the
/// initial press AND every auto-repeat.
static SPACE_TICK_TS: AtomicU64 = AtomicU64::new(0);
/// How many auto-repeats the current hold has produced. Zeroed at Space-down
/// and again at Space-up.
static SPACE_REPEATS: AtomicU32 = AtomicU32::new(0);

/// PROBLEM 219 (gate leak, 2026-08-29) — has ANOTHER key gone down during the
/// current Space-hold?
///
/// THE MEASUREMENT THAT FORCED THIS. Windows auto-repeats only the
/// MOST-RECENTLY-PRESSED key. The instant a second key goes down while Space
/// is held, Space stops repeating — and it does NOT resume when that key is
/// released. So the liveness signal `SPACE_TICK_TS` depends on is destroyed by
/// the very thing this app exists to do.
///
/// Straight from the owner's `debug.log`, 2026-08-29 03:19:50-52 (Space held
/// throughout, Space+Tab tapped four times):
/// ```text
/// 03:19:50.651  fs-pip: … held its tile at (0,0)      ← tap 1
/// 03:19:50.972  pip: … → corner 1 at (1280,0)         ← tap 2
/// 03:19:51.453  pip: … → corner 2 at (1280,800)       ← tap 3
/// 03:19:51.939  pip: … → corner 3 at (0,800)          ← tap 4
/// 03:19:52.444  hook: a Space-hold has been latched for 2016ms with no
///               auto-repeat after 5 of them … Reaping it
/// ```
/// The last Space auto-repeat landed at ~03:19:50.43 — BEFORE tap 1 — and
/// none arrived across the next four taps, because Tab owned the repeat slot.
/// 2016 ms later the reaper declared a physically-held Space dead, cleared
/// `MODIFIER_ACTIVE`, and the owner's FIFTH tap fell through the combo branch
/// to `CallNextHookEx` — a real Tab into Brave, moving focus around the page.
/// His words: *"It has to understand that I am still holding Space."*
///
/// WHY DISARM RATHER THAN RE-STAMP. Stamping this flag's timestamp on every
/// key-down was tried on paper first and does not fix it: it only moves the
/// deadline to 2 s after the LAST tap, so any pause for thought between taps
/// still reaps a live hold. After a combo there is **no liveness signal for a
/// held Space at all** — held-quietly and hook-is-dead are indistinguishable
/// from inside the hook, and no threshold can separate them. CLAUDE.md's rule
/// applies directly: a check that cannot produce a negative is not a check, so
/// the honest move is to stop asking rather than to guess.
///
/// WHAT IS GIVEN UP, EXACTLY. The reaper keeps full power over the hold shape
/// PROBLEM 218 was written for — Space held, HUD on screen, pointer arming
/// chips, no combo pressed — which is every stuck-HUD report in the log. It
/// stands down only for a hold that has already fired a key, where the HUD has
/// been taken down by `cancel_hud` on that very combo, and where the latch is
/// still bounded by `MAX_MODIFIER_HOLD_MS` (30 s, in the combo branch) and
/// cleared outright by the next Space press/release.
///
/// Cleared at every fresh Space-down, at Space-up, by the reaper and by the
/// watchdog's eviction reset. One relaxed store on a branch that already
/// loaded `MODIFIER_ACTIVE`; the callback pays nothing new (PROBLEM 58).
static SPACE_COMBO_SEEN: AtomicBool = AtomicBool::new(false);

/// How many auto-repeats must be OBSERVED before the reaper is allowed to act.
///
/// This is the check that can produce a negative (CLAUDE.md: *"a check that
/// cannot produce a negative result is not a check"*). A keyboard, driver or
/// accessibility setting with auto-repeat disabled produces zero repeats, the
/// reaper never arms, and a legitimate long hold is never torn down — the app
/// simply falls back to the behaviour it has today. Two, not one, so a single
/// stray repeat cannot arm it.
const MIN_OBSERVED_REPEATS: u32 = 2;

/// How long after the last observed Space auto-repeat a latched hold is
/// declared dead.
///
/// Windows' slowest configured repeat is ~2/second and its longest repeat
/// DELAY is 1000 ms, so the widest legitimate gap between two repeats is about
/// 500 ms. 2000 ms is four times that, and the reaper is additionally gated on
/// having already seen two repeats of THIS hold, so the machine's real cadence
/// is known to be inside the budget before the clock is ever consulted.
const STALE_HOLD_GRACE_MS: u64 = 2_000;

/// Is a latched Space-hold demonstrably dead? Pure, so it can be tested.
///
/// All three conditions are required, and each rules out a specific way of
/// being wrong:
///   * `modifier_active`  — there is a hold to reap at all.
///   * `repeats >= MIN_OBSERVED_REPEATS` — this keyboard is known to repeat,
///     so an absence of repeats is evidence rather than the normal case.
///   * `since_last_tick_ms > grace_ms` — the repeats have stopped arriving,
///     which means either the key came up and we missed it, or the callback
///     is no longer being called. Both mean the same thing to the HUD.
///   * `!combo_seen` — PROBLEM 219. Nothing above is evidence once another key
///     has gone down during this hold, because Windows moved auto-repeat to
///     that key and Space will never repeat again for the rest of the hold.
///     The three conditions then describe a perfectly healthy Space+key user
///     exactly as well as they describe a dead hook. See `SPACE_COMBO_SEEN`.
pub(crate) fn hold_is_stale(
    modifier_active: bool,
    repeats: u32,
    since_last_tick_ms: u64,
    grace_ms: u64,
    combo_seen: bool,
) -> bool {
    modifier_active
        && !combo_seen
        && repeats >= MIN_OBSERVED_REPEATS
        && since_last_tick_ms > grace_ms
}

/// Tear down a Space-hold whose auto-repeat has stopped arriving.
///
/// Returns true when it reaped. Called from `st-hud-pointer` (an independent
/// thread, so it still runs when the hook thread is the thing that is stuck)
/// and from the hook pump's WM_TIMER branch (so it still runs when the pointer
/// watcher failed to spawn). Two homes, two different failure modes; both are
/// off the hook CALLBACK, so logging here is legal.
///
/// FAIL SAFE, deliberately. CLAUDE.md: a missing HUD is recoverable, a stuck
/// one is not. Every reset below is the same set `watchdog_check` already
/// performs after an eviction — this simply reaches them on a bounded clock
/// instead of only when the watchdog happens to re-hook.
pub fn reap_stale_hold() -> bool {
    let repeats = SPACE_REPEATS.load(Ordering::Relaxed);
    let since = tick_count().saturating_sub(SPACE_TICK_TS.load(Ordering::Relaxed));
    if !hold_is_stale(
        MODIFIER_ACTIVE.load(Ordering::Relaxed),
        repeats,
        since,
        STALE_HOLD_GRACE_MS,
        SPACE_COMBO_SEEN.load(Ordering::Relaxed),
    ) {
        return false;
    }
    // Claim it before doing anything else: both callers race each other, and
    // a double teardown would emit two `guide-hud-hide` events.
    if !MODIFIER_ACTIVE.swap(false, Ordering::SeqCst) {
        return false;
    }
    SPACE_REPEATS.store(0, Ordering::Relaxed);
    SPACE_COMBO_SEEN.store(false, Ordering::Relaxed);
    let hud_was_up = crate::guide_hud::is_visible();
    log::warn!(
        "hook: a Space-hold has been latched for {since}ms with no auto-repeat after \
         {repeats} of them — the Space-UP was never delivered, so this hold is over and \
         nothing else was going to end it. Reaping it (HUD was up: {hud_was_up}). Left \
         standing this is a ring on screen that no later hide can reach, with the pointer \
         still arming chips behind it (PROBLEM 218)."
    );
    STALE_HOLDS_REAPED.fetch_add(1, Ordering::Relaxed);
    // We ate the Space-down and can no longer honour the tap-types-a-space
    // contract for it — the up is gone. Clear the latch rather than leave it
    // to be mistaken for the NEXT hold's down (which is how a stale
    // `SPACE_INTERCEPTED` turns one release into a phantom double space).
    SPACE_INTERCEPTED.store(false, Ordering::Relaxed);
    SPACE_ABORTED.store(false, Ordering::Relaxed);
    // PROBLEM 206 — an armed chip or a half-eaten click must not outlive the
    // hold that created it. Without this the stranded arm is exactly what
    // "opened whatever my cursor was towards".
    pointer::reset_on_eviction();
    // And the visible part, last, because it is the part the user can see.
    crate::guide_hud::hide_guide_hud();
    true
}

/// How many stale holds the reaper has taken down. Drained into the 60s
/// diagnostics line; a non-zero value is the fingerprint of a lost Space-UP.
static STALE_HOLDS_REAPED: AtomicU32 = AtomicU32::new(0);
/// PROBLEM 95 — how close does REAL typing come to the command threshold?
///
/// Simulated keystrokes could not answer this: injected input never reliably
/// reached the hook from the test harness, and a thumb is exactly the thing a
/// simulation guesses at. So the app measures it on live typing instead.
///
/// One bucket per 40 ms of Space-down→key-down delay, counted separately for
/// the two verdicts. A single `fetch_add` per event: no allocation, no lock,
/// no logging — the hook callback must still return in microseconds.
///
/// Read it in debug.log as `hook margins`. What to look for: TYPED counts
/// piling up in the buckets just under the window mean the user's ordinary
/// typing is skimming the threshold, and one heavier day would tip it into
/// firing commands mid-sentence.
pub const MARGIN_BUCKET_MS: u64 = 40;
pub const MARGIN_BUCKETS: usize = 10; // 0-39 … 360+
pub static MARGIN_TYPED: [AtomicU32; MARGIN_BUCKETS] =
    [const { AtomicU32::new(0) }; MARGIN_BUCKETS];
pub static MARGIN_COMMAND: [AtomicU32; MARGIN_BUCKETS] =
    [const { AtomicU32::new(0) }; MARGIN_BUCKETS];

#[inline(always)]
fn record_margin(bucketed: &[AtomicU32; MARGIN_BUCKETS], held_ms: u64) {
    let i = ((held_ms / MARGIN_BUCKET_MS) as usize).min(MARGIN_BUCKETS - 1);
    bucketed[i].fetch_add(1, Ordering::Relaxed);
}

/// Timestamp (ms) of last alpha-key press (for rollover detection).
static LAST_ALPHA_TS: AtomicU64 = AtomicU64::new(0);
/// Rollover window in milliseconds (configurable, default 50).
pub static ROLLOVER_MS: AtomicU64 = AtomicU64::new(50);
/// The hook's Win32 thread ID — needed to post WM_QUIT on teardown.
static HOOK_THREAD_ID: AtomicU64 = AtomicU64::new(0);

// --- PROBLEM 65/66 — hook liveness (the eviction watchdog) -----------------
/// GetTickCount64 ms of the last genuine event seen by the KEYBOARD hook.
/// Stamped before any filtering so a fully-bypassed keystroke still counts.
///
/// **THIS CLOCK IS SEEDED, and every reader must know it.** Three writers:
/// the callback (`kb_hook_proc`), `install_hooks()` on every re-hook, and the
/// idle early-return in `watchdog_check`. The last two are not events — they
/// are there so the ALARM does not fire on silence that means nothing
/// (PROBLEM 101's 260 false alarms). That makes this the right input for the
/// alarm and the WRONG input for any sentence claiming a key arrived. Use
/// `LAST_KB_CALLBACK` for that.
static LAST_KB_EVENT: AtomicU64 = AtomicU64::new(0);
/// PROBLEM 228 — the same clock with NO seeded writers: stamped only from
/// inside `kb_hook_proc`, i.e. only when Windows actually called us.
///
/// It exists because the app spent 20 days reporting things it had not
/// observed. `LAST_KB_EVENT` above is stamped by the repair itself, so
/// "the last repair DID deliver events" was satisfied by the idle re-stamp
/// two seconds after any re-hook, and printed 100 times in the current log
/// while nothing had been delivered at all. The alarm still uses the seeded
/// clock (that is what stops the false alarms); the SENTENCES use this one.
///
/// Cost on the hot path: one relaxed store beside the one already there.
static LAST_KB_CALLBACK: AtomicU64 = AtomicU64::new(0);
/// PROBLEM 173 — how long the app can be deaf before the watchdog notices.
///
/// These were a 3000 ms timer against an 8000 ms silence threshold, so the
/// worst case was ~11 SECONDS during which holding Space did nothing at all:
/// no HUD, no shortcuts, no sign anything was wrong. The owner's 2026-08-24
/// log has that happening **17 times in one day**, every entry reading
/// `kb 9000ms / mouse 9000ms`, with spacedesk AND PowerToys both running (two
/// more low-level keyboard hooks on the same machine, which is the classic
/// cause of Windows evicting ours). That is the measured explanation for
/// *"space hud doesn't appear all the time"* — for those seconds the app is
/// not slow or hidden, it is simply not receiving the keystroke.
///
/// 1000/3000 puts the worst case at ~4 s instead of ~11 s.
///
/// WHY NOT LOWER. The threshold is what separates "our hook is deaf" from
/// "this person just is not typing right now", and PROBLEM 101 is the record
/// of getting that wrong: 260 false alarms in two days from a test that could
/// not tell those apart. Three seconds of BOTH hooks silent while
/// `GetLastInputInfo` says the user was active within the last two seconds is
/// still genuinely anomalous — a mouse move alone stamps the mouse hook, so
/// silence on both means input is going somewhere we cannot see. Going much
/// below this starts measuring the gap between two keystrokes.
///
/// The cost of a false positive here is one hook reinstall, bounded by the
/// 60-second cooldown below. The cost of a miss is the app being dead in the
/// user's hands. The asymmetry is what justifies the tightening.
const WATCHDOG_TICK_MS: u32 = 1000;
const BLIND_MS: u64 = 3_000;

/// GetTickCount64 ms of the last keyboard event the hook saw.
///
/// LIVENESS ONLY. This is stamped for EVERY callback — before the injected-input
/// cookie test, and again by `install_hooks` and `watchdog_check` off the hook
/// path entirely — because "were we called at all?" is the question the eviction
/// watchdog asks. That makes it exactly the wrong signal for "is a human
/// typing?", and PROBLEM 225 is the record of what borrowing it cost. Use
/// `last_user_typing_tick()` for that; do not reach for this one again.
pub fn last_keyboard_event_tick() -> u64 {
    LAST_KB_EVENT.load(Ordering::Relaxed)
}

/// PROBLEM 225 — tick of the last key-DOWN that was THE USER TYPING.
///
/// Not a combo's own releases, not our injections, not the watchdog's idle
/// re-stamp. `smart_cascade`'s post-launch raise (PROBLEM 170) stands down when
/// it believes the user has moved on, and it used to read
/// `last_keyboard_event_tick()` for that. In the owner's 2026-08-25..31 log that
/// belief was wrong **29 times**, because every one of the following stamps
/// LAST_KB_EVENT and none of them is a person typing:
///
///   * the Space-UP of the very combo that fired the launch (the whole
///     0.62–1.00 s cluster of false stand-downs, measured);
///   * the letter released inside that same combo;
///   * `inject_space()`'s own synthetic Space, which carries our
///     `0x7A7A7A7A` cookie and is stamped BEFORE the cookie test;
///   * `install_hooks()` (line ~1196) and `watchdog_check()` (line ~1386),
///     which re-stamp it from the message pump to arm their own
///     "did the repair deliver anything?" comparison.
///
/// The four conditions below exclude all four by construction rather than by
/// timing luck — which is what the old `SETTLE_MS` baseline was, and it lost the
/// race whenever the owner held Space longer than half a second to read the HUD.
///
/// COST, and why it is legal on the callback (PROBLEM 58 / NATIVE_SAFETY): one
/// predictable branch and one relaxed store, on the key-down path only. No
/// allocation, no logging, no Win32, no lock. `SPACE_TICK_TS` (PROBLEM 218) and
/// `record_margin` (PROBLEM 95) already pay the same envelope.
static LAST_USER_TYPING: AtomicU64 = AtomicU64::new(0);

/// The virtual-key code that produced `LAST_USER_TYPING`.
///
/// Kept because of what the FIX BRIEF called for as a temporary probe and this
/// file's own history says should be permanent: the stand-down message used to
/// assert *"you started typing"*, and there was no way to check it. With the vk
/// in the line, "the watcher stood down on the combo's own Space" and "the
/// owner really did start typing" stop being the same log entry. One extra
/// relaxed store on a branch that is already taken (PROBLEM 58 envelope).
static LAST_USER_TYPING_VK: AtomicU32 = AtomicU32::new(0);

/// See `LAST_USER_TYPING`. One relaxed load; safe from any thread.
pub fn last_user_typing_tick() -> u64 {
    LAST_USER_TYPING.load(Ordering::Relaxed)
}

/// The vk behind `last_user_typing_tick()`. Read it only to EXPLAIN a decision
/// that tick already made — the two are not sampled atomically together.
pub fn last_user_typing_vk() -> u32 {
    LAST_USER_TYPING_VK.load(Ordering::Relaxed)
}

/// PROBLEM 78 — tick of the watchdog's last reinstall, for its 60s cooldown.
static WATCHDOG_LAST_REINSTALL: AtomicU64 = AtomicU64::new(0);
/// PROBLEM 182 — tick of the last hook-THREAD rebuild, rate-limited separately
/// from re-hooking because the supervisor that performs it gives up for good
/// after 5 rebuilds in 10 minutes.
static LAST_ESCALATION: AtomicU64 = AtomicU64::new(0);
/// Same for the MOUSE hook. Kept separate: the two hooks are evicted
/// independently, and our keyboard callback is the heavy one — a dead
/// keyboard hook with a live mouse hook is the realistic failure.
static LAST_MS_EVENT: AtomicU64 = AtomicU64::new(0);
/// PROBLEM 132 — consecutive watchdog reinstalls with NO hook event in
/// between. The 2026-08-17 outage ran 20 unbroken minutes at one reinstall a
/// minute, each logging `reinstall ok: true`, because re-hooking is the only
/// move the watchdog had. A hook proc only fires on the thread that installed
/// it, so if THAT THREAD's message pump is the thing that is wedged,
/// SetWindowsHookEx on it can never help — it succeeds and delivers nothing.
/// PROBLEM 134 - "is OUR window the foreground?", sampled OFF the hook path.
/// Refreshed on the watchdog's 3s timer, read by the callback as one atomic.
static FG_IS_SELF: AtomicBool = AtomicBool::new(false);
static BLIND_REINSTALLS: AtomicU32 = AtomicU32::new(0);
/// PROBLEM 132 — how many alarms fired while OUR OWN window held the
/// foreground. This is the owner's exact repeated report ("shortcuts do not
/// work inside the app"), and until now it was the ONE case the watchdog could
/// not describe: the UIPI discriminator skips self-focus, so it fell straight
/// through to the eviction verdict with no evidence either way.
static BLIND_WHILE_OWN_FG: AtomicU32 = AtomicU32::new(0);
/// Set by the watchdog, read by the message pump: tear this whole thread down
/// so the PROBLEM 82 supervisor rebuilds it with a fresh pump and fresh hooks.
static ESCALATE_RESTART: AtomicBool = AtomicBool::new(false);

/// PROBLEM 161 — let the USER ask for the repair the watchdog performs.
///
/// The dashboard shows a banner when `HOOK_INSTALLED` is false, and its "Try
/// again" button lands here. It sets the same escalation flag the watchdog
/// uses after two failed re-hooks, so the hook THREAD is rebuilt rather than
/// the hook merely re-installed on a thread that may itself be wedged — which
/// is the distinction PROBLEM 132 was about: re-hooking from a jammed thread
/// produces something that looks healthy and receives nothing.
pub fn request_hook_rebuild() {
    log::info!("hook: rebuild requested by the user from the dashboard banner");
    ESCALATE_RESTART.store(true, Ordering::Relaxed);
}
/// `true` while the WH_KEYBOARD_LL hook is believed installed. Set by the
/// hook thread; read by get_hook_status so the dashboard tells the truth
/// (it used to hardcode `installed: true`).
pub static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
/// Count of watchdog reinstalls, drained into the log by the engine thread.
pub static HOOK_REINSTALLS: AtomicU32 = AtomicU32::new(0);
/// PROBLEM 180 — which of the OPTIONAL special keys the user has actually
/// bound. Bits 0..11 = F1..F12, 12 = Enter, 14 = Left, 15 = Right.
///
/// **BIT 13 WAS TAB AND IS NOW UNUSED** (2026-08-29, pip.rs §9, PROBLEM 219).
/// Space+Tab became the FIXED fullscreen-preserving PiP key, so Tab is
/// dispatched unconditionally like Esc and the backtick and has no bit to
/// gate. The bit is retired rather than reassigned: its number is quoted in
/// the log line this function emits and throughout PROBLEM 180's write-up, and
/// renumbering the survivors would make every historical log entry read wrong.
/// The paragraph below is preserved as it was written, including its Space+Tab
/// example, because it is the record of the bug — see the note after it.
///
/// THE BUG THIS FIXES HAS SHIPPED IN EVERY BUILD SINCE 1.0.27, and nobody
/// noticed because its symptom is *nothing happening*.
///
/// The combo match swallowed all sixteen of these unconditionally:
///
///     VK_RETURN => Some(KeyCombo::Special("enter".into())),
///     VK_TAB    => Some(KeyCombo::Special("tab".into())),
///     v if (VK_F1..=VK_F12).contains(&v) => …
///
/// directly under a comment claiming the opposite — *"only dispatch if the
/// user has bound them in special_keys config"*. The config check does exist,
/// but it lives downstream in `engine::handle_special`, which runs AFTER the
/// keystroke has already been destroyed by `return LRESULT(1)`. Its own
/// comment, `// key passes through — not configured`, describes something that
/// cannot happen: by then there is no key left to pass through.
///
/// `special_keys` is `{}` in the owner's config and **nothing in the UI can
/// write it**, so for every user of every build: hold Space and Enter, Tab,
/// the Left/Right arrows and all twelve F-keys are eaten and do nothing.
/// Space+Tab cannot alt-tab, Space+Enter cannot confirm a dialog.
///
/// (The Space+Tab half of that example is HISTORY as of 2026-08-29: Tab is
/// deliberately eaten now, because it is the fullscreen-PiP key. That Tab sat
/// here unbindable-in-practice for so long is exactly why it was the one free
/// to claim. Everything the paragraph says about Enter, the arrows and the
/// F-keys still stands.)
///
/// A bitmask rather than a config read because this is consulted on the hook
/// path, where PROBLEM 58's rule is absolute: atomics only, no allocation, no
/// lock, no logging. One relaxed load and a shift.
pub static BOUND_SPECIALS: AtomicU32 = AtomicU32::new(0);

/// Is this optional special key bound? Hook-path safe: one relaxed load.
#[inline]
fn special_bound(bit: u16) -> bool {
    BOUND_SPECIALS.load(Ordering::Relaxed) & (1u32 << bit) != 0
}

/// Bit index for a `special_keys` config name, or `None` if that key is a
/// FIXED shortcut rather than an optional one (esc = Boss Key, up/down =
/// scroll) and therefore always dispatched.
fn special_bit(name: &str) -> Option<u32> {
    match name {
        "enter" => Some(12),
        // "tab" IS DELIBERATELY ABSENT since 2026-08-29 (pip.rs §9, PROBLEM
        // 210). Space+Tab is now the fixed fullscreen-preserving PiP key, so
        // it is dispatched unconditionally like Esc and the backtick and has
        // no bit to gate. Bit 13 is left unused rather than reassigned: the
        // numbering is quoted in the `BOUND_SPECIALS` log line and in
        // PROBLEM 180's write-up, and renumbering the survivors would make
        // every historical log entry read wrong.
        //
        // A user's existing `special_keys["tab"]` binding is NOT deleted —
        // config is never rewritten to suit a code change — it is simply no
        // longer reachable, and `publish_bound_specials` says so once.
        "left" => Some(14),
        "right" => Some(15),
        _ => name
            .strip_prefix('f')
            .and_then(|n| n.parse::<u32>().ok())
            .filter(|n| (1..=12).contains(n))
            .map(|n| n - 1),
    }
}

/// Publish which optional special keys are bound, so the hook can stop eating
/// the ones that are not.
///
/// MUST be called from BOTH the startup config load and every save. The atomic
/// starts at 0, so without the startup call every bit is clear from launch
/// until the user happens to save something — which is the same dead-keys bug
/// with a smaller window.
pub fn publish_bound_specials(cfg: &crate::config::AppConfig) {
    let mut mask = 0u32;
    let mut shadowed_tab = false;
    for (name, bind) in &cfg.special_keys {
        if !bind.is_mapped() {
            continue;
        }
        if name == "tab" {
            // A binding that can no longer fire. Say so ONCE rather than
            // deleting it: the config is the user's, and a key that silently
            // stopped working is exactly the class of failure PROBLEM 180
            // was — "its symptom is nothing happening".
            shadowed_tab = true;
            continue;
        }
        if let Some(bit) = special_bit(name) {
            mask |= 1 << bit;
        }
    }
    if shadowed_tab {
        log::warn!(
            "hook: config still maps Space+Tab to an app in special_keys, but Tab became the \
             FIXED fullscreen-preserving PiP key on 2026-08-29 (pip.rs §9). That binding can \
             no longer fire. It has NOT been deleted — remove it from special_keys in \
             config.json if you want it gone."
        );
    }
    let prev = BOUND_SPECIALS.swap(mask, Ordering::Relaxed);
    if prev != mask {
        log::info!(
            "hook: optional special keys bound = {mask:#06x} (bits 0-11 F1-F12, 12 Enter, \
             14 Left, 15 Right; bit 13 was Tab and is retired — Tab is the fixed \
             fullscreen-PiP key since 2026-08-29, PROBLEM 219). Unbound ones now pass \
             through to Windows instead of being swallowed (PROBLEM 180)."
        );
    }
}

/// PROBLEM 206 — is pointer activation on the Guide HUD enabled?
///
/// Consulted on the hook path (`pointer::take_armed_key`, one relaxed load)
/// and by the `st-hud-pointer` poller, so it is an atomic, never a config
/// read — the callback may do lock-free atomics only (PROBLEM 58/134/184).
/// Starts FALSE even though the SETTING now defaults to ON (PROBLEM 209 —
/// the owner's decision, see `AppConfig::pointer_hud_activation`). That is
/// deliberate and must not be "corrected" to `true`: this atomic is the
/// runtime mirror, not the default, and it is false only for the few
/// milliseconds between process start and `publish_pointer_hud_activation`
/// running in `lib.rs`. Seeding it `true` would mean a config that says OFF
/// is briefly honoured as ON — the one window in which a stray Space release
/// could launch something the user turned off.
pub static POINTER_HUD_ACTIVATION: AtomicBool = AtomicBool::new(false);

/// Publish the pointer-activation setting for the hook and the poller.
///
/// MUST be called from BOTH the startup config load (lib.rs) and
/// `config::save` — the single funnel every mutation path goes through
/// (`reset_config` and friends included; publishing at the `save_config`
/// COMMAND would miss them, PROBLEM 180's exact bug). The atomic starts
/// false, so skipping the startup call is the silent failure where the
/// feature works all session and then reads OFF for the entire next launch
/// until the user touches any setting.
pub fn publish_pointer_hud_activation(cfg: &crate::config::AppConfig) {
    let on = cfg.pointer_hud_activation;
    let prev = POINTER_HUD_ACTIVATION.swap(on, Ordering::Relaxed);
    if prev != on {
        log::info!(
            "hook: pointer HUD activation (cursor-on-chip launch) is now {}",
            if on { "ON" } else { "OFF" }
        );
    }
}

/// PROBLEM 176 — keys handed straight back to Windows because a REAL modifier
/// (Ctrl/Alt/Win) was held alongside Space. Drained into the diagnostics line
/// so "my system shortcut did nothing" and "Spaceadom ate my key" stay
/// distinguishable in a log, which is the only place the difference is visible.
pub static PASSED_TO_OS: AtomicU32 = AtomicU32::new(0);

/// Key-DOWN events handed straight back to Windows because the foreground app
/// is on the user's App-exceptions list. Counted DOWN events only, mirroring
/// `SUPPRESS_BYPASS`, so the number reads as keystrokes, not edges.
pub static SUPPRESS_EXCLUDED: AtomicU32 = AtomicU32::new(0);

/// MUST-FIX 2 — Space-UP dropped injecting a synthetic space because a REAL
/// modifier (Ctrl/Alt/Win) was STILL physically held at release. PROBLEM 176
/// added a pass-through on the combo path (see the big comment at the
/// Space-held branch below) that lets a key escape to the OS WITHOUT setting
/// `SPACE_ABORTED` — deliberately, so a plain hold still types a space on
/// release. But that leaves `!SPACE_ABORTED` alone unable to tell "nothing
/// happened, inject a space" apart from "a chord fired while Space was held,
/// and the modifier may STILL be down right now" — and injecting a bare
/// VK_SPACE into a live Alt/Win/Ctrl composes Alt+Space (window menu),
/// Win+Space (layout switch) or Ctrl+Space (IME/IntelliSense), not a space.
/// Drained alongside `passed-to-os`: without this counter, "my space went
/// missing" and "Spaceadom ate my key" read identically in a log.
pub static SPACE_DROPPED_MODIFIER: AtomicU32 = AtomicU32::new(0);

/// The same count, but for the whole session and **never drained**.
///
/// PROBLEM 173 — `HOOK_REINSTALLS` above is `swap(0)`-ed on every Space
/// release (`drain_hook_diagnostics`), which is right for a log line and
/// useless for a UI: anything asking "how often has this happened?" would
/// almost always read zero, because the user pressed Space between the
/// eviction and the question. A counter that is reset by an unrelated event
/// cannot answer a question about history.
pub static HOOK_EVICTIONS_TOTAL: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Win32 Virtual Key constants we care about
// ---------------------------------------------------------------------------
const VK_SPACE: u16 = 0x20;
const VK_ESCAPE: u16 = 0x1B;
const VK_OEM_3: u16 = 0xC0;  // backtick / ~
const VK_OEM_COMMA: u16 = 0xBC;
const VK_OEM_PERIOD: u16 = 0xBE;
const VK_RMENU: u16 = 0xA5;  // Right Alt
const VK_UP: u16 = 0x26;
const VK_DOWN: u16 = 0x28;
const VK_BACK: u16 = 0x08;   // Backspace → Force Close
const VK_LEFT: u16 = 0x25;
const VK_RIGHT: u16 = 0x27;
const VK_RETURN: u16 = 0x0D;   // Enter
// `allow` because 1.0.91 comments out the ONLY use of this constant (the
// Space+Tab split — see the combo match). Unused in that build, used in 1.0.92.
#[allow(dead_code)]
const VK_TAB: u16 = 0x09;
// F1–F12
const VK_F1: u16 = 0x70;
const VK_F12: u16 = 0x7B;
// WM message values
const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_SYSKEYUP: u32 = 0x0105;
// Hook type
const WH_KEYBOARD_LL: i32 = 13;
const WH_MOUSE_LL: i32 = 14;
// SendInput constants
const _INPUT_KEYBOARD: u32 = 1;
const _KEYEVENTF_KEYUP: u32 = 0x0002;
// Mouse hook WM values
const WM_MOUSEWHEEL: u32 = 0x020A;
const _WHEEL_DELTA: i32 = 120;
// PROBLEM 206 — pointer activation needs the move and the left button too.
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;

// ---------------------------------------------------------------------------
// Thread-local sender (set once when the hook thread starts)
// ---------------------------------------------------------------------------
thread_local! {
    static EVENT_TX: std::cell::RefCell<Option<Sender<HookEvent>>> =
        const { std::cell::RefCell::new(None) };
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Spawn the isolated keyboard + mouse hook thread.
/// Returns immediately; the hook runs until `stop_hook()` is called.
/// Deliberate shutdown flag — set by stop_hook() so the respawn supervisor
/// can tell "the app is exiting" from "the hook thread DIED" (PROBLEM 82).
pub static HOOK_SHUTDOWN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn spawn_hook_thread(tx: Sender<HookEvent>, rollover_ms: u64) {
    ROLLOVER_MS.store(rollover_ms, Ordering::Relaxed);
    // PROBLEM 95 — say which window is in force. Without this line the log
    // cannot answer "why did a shortcut not fire" or "why did one fire while
    // typing": the single number that decides both was invisible.
    log::info!(
        "hook: rollover window {rollover_ms}ms — Space must be held at least this long \
         before a letter for it to count as a command; anything quicker is typed"
    );

    // PROBLEM 82 — the hook thread is the app. If it panics (a driver feeds a
    // malformed event, an OS call fails somewhere unexpected), Space+key is
    // dead until the user restarts the process, and NOTHING says so. The
    // supervisor loop below catches the panic, logs it loudly, and restarts
    // the whole hook thread body — with a 2s pause and a 5-restart/10-min cap
    // so a persistent crash cannot become a spin loop.
    std::thread::Builder::new()
        .name("st-hook-supervisor".into())
        .spawn(move || {
            let mut restarts: Vec<std::time::Instant> = Vec::new();
            loop {
                let tx2 = tx.clone();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    hook_thread_main(tx2);
                }));
                if HOOK_SHUTDOWN.load(Ordering::Relaxed) {
                    return; // clean exit (tray Exit / self-restart)
                }
                match result {
                    Ok(()) => {
                        // hook_thread_main returned without shutdown — the
                        // message pump ended unexpectedly (WM_QUIT from a
                        // foreign source). Treat like a crash: restart.
                        log::error!("hook: thread exited unexpectedly — restarting it");
                    }
                    Err(_) => {
                        log::error!(
                            "hook: THREAD PANICKED (payload in the panic-hook entry above) — \
                             restarting it so Space+key keeps working"
                        );
                    }
                }
                let now = std::time::Instant::now();
                restarts.retain(|t| now.duration_since(*t).as_secs() < 600);
                restarts.push(now);
                if restarts.len() > 5 {
                    log::error!(
                        "hook: 5 restarts inside 10 minutes — giving up to avoid a crash loop. \
                         Space+key is DEAD until the app is restarted."
                    );
                    return;
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        })
        .expect("failed to spawn hook supervisor");
}

/// Signal the hook thread to uninstall hooks and exit. Sets HOOK_SHUTDOWN
/// first so the supervisor (PROBLEM 82) knows this exit is deliberate and
/// does not restart the thread.
pub fn stop_hook() {
    HOOK_SHUTDOWN.store(true, Ordering::SeqCst);
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW;
        let tid = HOOK_THREAD_ID.load(Ordering::Relaxed) as u32;
        if tid != 0 {
            let _ = PostThreadMessageW(tid, 0x0012 /*WM_QUIT*/, None, None);
        }
    }
}

// ---------------------------------------------------------------------------
// Hook thread main body
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn hook_thread_main(tx: Sender<HookEvent>) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, KillTimer, SetTimer, TranslateMessage,
        UnhookWindowsHookEx, MSG, WM_TIMER,
    };
    use windows::Win32::System::Threading::GetCurrentThreadId;

    // Store sender in thread-local
    EVENT_TX.with(|cell| *cell.borrow_mut() = Some(tx));

    unsafe {
        HOOK_THREAD_ID.store(GetCurrentThreadId() as u64, Ordering::Relaxed);

        // PROBLEM 134 - raise this thread above the UI.
        //
        // Windows evicts a low-level hook whose callback does not RETURN inside
        // LowLevelHooksTimeout (1000ms cap since Win10 1709). That deadline is
        // wall-clock: a callback that is merely waiting for a CPU slice misses
        // it exactly like a slow one. At normal priority this thread competes with
        // WebView2's renderer, and on this owner's machine the overlay runs in
        // SOFTWARE mode (--disable-gpu, GPU composition is dead here), so the
        // dashboard is composited on the CPU - a 1766x964 window since PROBLEM
        // 123 grew it to 92% of the work area. The busiest moment is precisely
        // when that window is focused, which is precisely the owner's report:
        // "while I am using the app, nothing fires."
        //
        // ABOVE_NORMAL, deliberately not TIME_CRITICAL: this thread must beat a
        // rendering pass, not the kernel. The callback is bounded work (atomics
        // and a channel send), so it cannot monopolise anything even if it is
        // scheduled aggressively.
        {
            use windows::Win32::System::Threading::{
                GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL,
            };
            match SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL) {
                Ok(()) => log::info!(
                    "hook: thread priority raised to ABOVE_NORMAL so a WebView2 render                      pass cannot starve the callback past LowLevelHooksTimeout                      (PROBLEM 134)"
                ),
                Err(e) => log::warn!(
                    "hook: could not raise thread priority ({e}) - continuing at normal                      priority; a heavy UI frame may still evict the hook (PROBLEM 134)"
                ),
            }
        }

        // PROBLEM 66 — SetWindowsHookExW used to be .expect()ed: on a machine
        // where install fails (AV/policy blocking global hooks), the hook
        // thread PANICKED silently and the app sat in the tray doing nothing,
        // with the dashboard still claiming everything was fine.
        let (mut kb_hook, mut ms_hook) = install_hooks();
        if kb_hook.is_invalid() {
            log::error!(
                "hook: SetWindowsHookExW(WH_KEYBOARD_LL) FAILED — Space+key cannot work. \
                 Usually security software or policy blocking global hooks. \
                 The watchdog will keep retrying."
            );
        } else {
            log::info!("hook: WH_KEYBOARD_LL + WH_MOUSE_LL installed");
        }

        // PROBLEM 65 — the eviction watchdog. Windows silently EVICTS a
        // low-level hook whose callback overruns LowLevelHooksTimeout (300ms
        // default); nothing tells us, GetMessageW keeps pumping an empty
        // queue, and Space+key just dies while the log looks healthy. A hook
        // proc only fires on the thread that installed it, so the reinstall
        // must happen HERE — a thread-queue timer wakes the blocking pump.
        //
        // NULL-hwnd SetTimer IGNORES the id you pass and returns a fresh
        // system id; WM_TIMER carries THAT id. Compare against the RETURN
        // VALUE or the watchdog silently never fires.
        let timer_id = SetTimer(None, 0, WATCHDOG_TICK_MS, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_TIMER && msg.wParam.0 == timer_id {
                watchdog_check(&mut kb_hook, &mut ms_hook);
                // PROBLEM 132 - returning here IS the repair, not a failure.
                // HOOK_SHUTDOWN stays false, and that is precisely what tells
                // the supervisor this exit was not deliberate, so it rebuilds
                // the thread immediately with a fresh message queue.
                if ESCALATE_RESTART.swap(false, Ordering::Relaxed) {
                    let _ = KillTimer(None, timer_id);
                    let _ = UnhookWindowsHookEx(kb_hook);
                    let _ = UnhookWindowsHookEx(ms_hook);
                    HOOK_INSTALLED.store(false, Ordering::Relaxed);
                    return;
                }
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = KillTimer(None, timer_id);
        UnhookWindowsHookEx(kb_hook).ok();
        UnhookWindowsHookEx(ms_hook).ok();
        HOOK_INSTALLED.store(false, Ordering::Relaxed);
        log::info!("hook: hooks removed, thread exiting");
    }
}

/// Install (or re-install) both hooks. Never panics: a failed install leaves
/// an invalid HHOOK for the watchdog to retry. Updates HOOK_INSTALLED so the
/// dashboard reports the truth.
#[cfg(windows)]
/// PROBLEM 181 — the REFERENCE keyboard hook, and the reason it exists.
///
/// The watchdog's only eviction test is `both_dead`: BOTH the keyboard and the
/// mouse hook silent past a threshold while the user is active. That cannot see
/// the failure that actually happens on this machine — the KEYBOARD hook alone
/// being evicted — because the mouse hook keeps firing and holds `both_dead`
/// false. Measured in the owner's log for 2026-08-24/25: **24 of 46 alarms had
/// mouse silence under 5s while keyboard silence ran to 5–29 SECONDS**, e.g.
///
///     23:31:07.802  kb 20000ms / mouse 3906ms   Foreground: Discord.exe
///     23:35:17.905  kb 19000ms / mouse 3515ms   Foreground: spaceadom.exe
///
/// Twenty seconds of a dead app, and the watchdog only noticed when the mouse
/// happened to go quiet too.
///
/// The obvious detector — "keyboard silent while mouse is alive" — is exactly
/// the branch PROBLEM 101 DELETED, and deleting it was right: it cannot tell an
/// evicted hook from a person who is reading rather than typing. Both look
/// identical from inside this process. The removal left no detector at all.
///
/// THE DISCRIMINATOR. Install a SECOND `WH_KEYBOARD_LL` that does nothing but
/// stamp a timestamp and call the next hook. Windows evicts the hook whose
/// callback overran `LowLevelHooksTimeout`, not every hook in the chain — and
/// this one cannot overrun, because it does one relaxed store. So:
///
///     reference firing + primary silent  =  the primary was evicted. Certain.
///     both silent                        =  nobody is typing. Ambiguous, ignore.
///
/// That turns an unanswerable question into an arithmetic one.
///
/// It lives on the SAME thread as the primary deliberately. If the thread's
/// message pump is what wedged, both stop together and this correctly reports
/// nothing — that case is already handled by `ESCALATE_RESTART` rebuilding the
/// whole thread, and a reference on another thread would have muddied the two
/// failure modes back together.
///
/// ═══ PROBLEM 228 — THIS STATIC USED TO BE WRITTEN BY THE REPAIR ═══
///
/// `install_hooks()` stamped it, and `install_hooks()` runs on every watchdog
/// re-hook. So the DEAF message's load-bearing clause — *"the reference hook
/// fired Nms ago (keys ARE reaching the chain)"* — was citing the repair as
/// proof that keys were flowing. Measured across `debug.log` + `debug.log.0`
/// (41,675 lines, 2026-08-11 → 08-31): **281 of 282 DEAF lines have their
/// implied reference instant within 10 ms of a watchdog re-hook.** The cleanest
/// sample:
///
///     09:52:39.420 [WARN] hook: WATCHDOG — ... Re-hooking. reinstall ok: true
///     09:52:39.435 [WARN] hook: DEAF ... the reference hook fired 16ms ago
///                                       (keys ARE reaching the chain)
///
/// The reference hook did not fire. The install stamped it 15 ms earlier.
/// Seventeen Sentry events rest on that sentence.
///
/// It is now GENUINE-ONLY: the callback below is the only writer. The install
/// time went to `REF_HOOK_INSTALLED_AT`, which is a different fact and is now
/// printed as one.
static LAST_REF_KB_EVENT: AtomicU64 = AtomicU64::new(0);

/// How many times the reference hook has ACTUALLY been called since launch.
///
/// A COUNTER, not a clock, and that is the point: a timestamp can be forged by
/// anything holding a `store`, but a counter that only the callback increments
/// cannot be made to say a key arrived. Every deafness test reads this; the
/// clock above is only ever used to say *when*.
///
/// Wraps at u32 (~4 billion keystrokes). Consumers compare deltas with
/// `wrapping_sub`, so a wrap costs one window, not a wrong verdict.
static REF_KB_EVENTS: AtomicU32 = AtomicU32::new(0);

/// GetTickCount64 ms of the last `install_hooks()` — i.e. when the reference
/// hook was last (re)installed. NOT evidence that a key was seen; it is the
/// fact that used to masquerade as one.
static REF_HOOK_INSTALLED_AT: AtomicU64 = AtomicU64::new(0);

/// Reference hook. Do NOT add anything to this function. Its entire value is
/// that it cannot be evicted for being slow, and every line added erodes that.
///
/// The `fetch_add` (PROBLEM 228) is the same cost as the store beside it — one
/// lock-free relaxed RMW, no allocation, no syscall, no branch that can grow.
/// That is the only addition this function may ever accept.
#[cfg(windows)]
unsafe extern "system" fn ref_kb_hook_proc(
    n_code: i32,
    w_param: windows::Win32::Foundation::WPARAM,
    l_param: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::CallNextHookEx;
    if n_code >= 0 {
        LAST_REF_KB_EVENT.store(tick_count(), Ordering::Relaxed);
        REF_KB_EVENTS.fetch_add(1, Ordering::Relaxed);
    }
    CallNextHookEx(None, n_code, w_param, l_param)
}

unsafe fn install_hooks() -> (
    windows::Win32::UI::WindowsAndMessaging::HHOOK,
    windows::Win32::UI::WindowsAndMessaging::HHOOK,
) {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowsHookExW, WINDOWS_HOOK_ID};
    let kb = SetWindowsHookExW(WINDOWS_HOOK_ID(WH_KEYBOARD_LL), Some(kb_hook_proc), None, 0)
        .unwrap_or_default();
    let ms = SetWindowsHookExW(WINDOWS_HOOK_ID(WH_MOUSE_LL), Some(ms_hook_proc), None, 0)
        .unwrap_or_default();
    // PROBLEM 181 — the liveness reference. Kept in its own static rather than
    // returned, because every caller of install_hooks() treats its two return
    // values as "the hooks to unhook", and this one must be replaced on the
    // same schedule without any of them having to remember it.
    let old_ref = REF_KB_HOOK.swap(0, Ordering::SeqCst);
    if old_ref != 0 {
        use windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx;
        let _ = UnhookWindowsHookEx(windows::Win32::UI::WindowsAndMessaging::HHOOK(
            old_ref as *mut _,
        ));
    }
    let rf = SetWindowsHookExW(WINDOWS_HOOK_ID(WH_KEYBOARD_LL), Some(ref_kb_hook_proc), None, 0)
        .unwrap_or_default();
    REF_KB_HOOK.store(rf.0 as isize as u64, Ordering::SeqCst);

    HOOK_INSTALLED.store(!kb.is_invalid(), Ordering::Relaxed);
    // PROBLEM 184 — a reinstall means we may have missed key-ups while unhooked,
    // so take the OS's word for the modifier state rather than our own stale
    // bookkeeping. This runs on the hook THREAD, not in the callback.
    resync_modifiers();
    let now = tick_count();
    // These two are the ALARM's baseline, and re-stamping them here is
    // deliberate: without it every re-hook would be followed by an instant
    // second alarm computed against silence that predates the repair.
    LAST_KB_EVENT.store(now, Ordering::Relaxed);
    LAST_MS_EVENT.store(now, Ordering::Relaxed);
    // PROBLEM 228 — `LAST_REF_KB_EVENT.store(now)` USED TO BE HERE, and it is
    // the single line that made SPACEADOM-2 a broken instrument: it let the
    // repair stamp the evidence, and the DEAF message then read that stamp back
    // as "keys ARE reaching the chain". 281 of 282 DEAF lines in the owner's
    // 20-day log were produced this way.
    //
    // The install time is a real and useful fact, so it is kept — under its own
    // name, where nothing can mistake it for a keystroke. Note there is no
    // alarm to protect here: `kb_only_dead` needs the reference to have fired
    // RECENTLY, so an un-seeded reference clock makes that test harder to
    // satisfy, never easier.
    REF_HOOK_INSTALLED_AT.store(now, Ordering::Relaxed);
    (kb, ms)
}

/// Handle of the reference hook, so it can be replaced/removed alongside the
/// primary. Stored as a raw `u64` because `HHOOK` is not `Send`/`Sync`.
static REF_KB_HOOK: AtomicU64 = AtomicU64::new(0);

/// Milliseconds since the OS last saw ANY user input (keyboard or mouse).
/// GetLastInputInfo reports in 32-bit GetTickCount space — compare there,
/// never against GetTickCount64.
#[cfg(not(windows))]
fn millis_since_last_input() -> u64 {
    u64::MAX
}

#[cfg(windows)]
fn millis_since_last_input() -> u64 {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    unsafe {
        let mut lii = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut lii).as_bool() {
            (GetTickCount() as u64).wrapping_sub(lii.dwTime as u64) & 0xFFFF_FFFF
        } else {
            u64::MAX
        }
    }
}

/// PROBLEM 65 — decide whether the hooks were silently evicted, and reinstall.
///
/// Runs in the message pump on the WM_TIMER branch (every 3s), NEVER inside a
/// hook callback, so the Win32 calls and any logging here cannot trip
/// LowLevelHooksTimeout. Logging happens only AFTER the dead hooks are
/// unhooked, so a slow disk write can never delay a still-live callback.
///
/// Two rules, because the hooks die independently:
/// 1. BOTH silent >8s while the OS saw input <2s ago → both evicted (or the
///    callback overran and took the pair down). Fast reinstall.
/// 2. Keyboard silent >120s while the MOUSE hook is provably alive (<8s) and
///    the OS saw input <2s ago → the keyboard hook alone was evicted. The
///    long window exists because "mouse active, no typing" is a normal way
///    to read a page; the price of the occasional false positive is a sub-ms
///    unhook/rehook, which is harmless.
/// PROBLEM 132 — WHICH window has the foreground, by name.
///
/// The watchdog used to log "Usually means an elevated window has focus
/// (UIPI)" on every alarm. That sentence is wrong by construction: the code
/// immediately above it RULES ELEVATION OUT before it can be reached. Weeks of
/// investigation went past this line and believed it. A log that asserts a
/// cause the code already excluded is worse than one that says nothing —
/// it is a signpost pointing away from the answer.
///
/// Safe here and ONLY here: this runs on the WM_TIMER branch of the pump, not
/// in a hook callback, so an OpenProcess round-trip cannot trip
/// LowLevelHooksTimeout.
#[cfg(windows)]
fn foreground_desc() -> String {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return "<none - secure desktop or desktop switch>".to_string();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        if pid == 0 {
            return "<foreground window reports no pid>".to_string();
        }
        let is_self = pid == std::process::id();
        let mut name = String::new();
        // LIMITED, not PROCESS_QUERY_INFORMATION: this one SUCCEEDS against an
        // elevated process from medium integrity, which is the point — we want
        // the name even when the window is the reason we are deaf.
        if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut buf = [0u16; 260];
            let mut len = buf.len() as u32;
            if QueryFullProcessImageNameW(
                h,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok()
            {
                let full = String::from_utf16_lossy(&buf[..len as usize]);
                name = full.rsplit(std::path::MAIN_SEPARATOR).next().unwrap_or("").to_string();
            }
            let _ = CloseHandle(h);
        }
        if name.is_empty() {
            name = format!("pid {pid}");
        }
        if is_self {
            format!("{name} <- SPACEADOM'S OWN WINDOW")
        } else {
            format!("{name} (pid {pid})")
        }
    }
}

#[cfg(windows)]
unsafe fn watchdog_check(
    kb: &mut windows::Win32::UI::WindowsAndMessaging::HHOOK,
    ms: &mut windows::Win32::UI::WindowsAndMessaging::HHOOK,
) {
    use windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx;

    // PROBLEM 218 — the reaper's SECOND home, and it must sit above every
    // early return in this function.
    //
    // `st-hud-pointer` is the primary caller, but that thread's spawn is
    // allowed to fail (PROBLEM 124) and it logs "keyboard shortcuts are
    // unaffected" when it does — which would have been a lie once the HUD
    // teardown depended on it. This is a 1s cadence on the pump's WM_TIMER
    // branch, where logging and Win32 are already legal.
    //
    // ABOVE the idle early-return below, deliberately: a stranded HUD is
    // most likely to be noticed by a user who has STOPPED touching the
    // keyboard, and `millis_since_last_input() >= 2000` is exactly that
    // person. Putting the reap under that gate would skip the case it is for.
    let _ = reap_stale_hold();

    // PROBLEM 134 - sample the foreground here, on the timer, so the hook
    // callback never has to. This branch already runs Win32 calls safely.
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        let fg = GetForegroundWindow();
        let mut is_self = false;
        if !fg.0.is_null() {
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut pid));
            is_self = pid == std::process::id();
        }
        FG_IS_SELF.store(is_self, Ordering::Relaxed);
        // PROBLEM 218 — THE DENOMINATOR.
        //
        // `KB_EVENTS_OWN_FG` has always been a numerator with nothing to
        // divide by, and PROBLEM 183 is the written record of what that cost:
        // an argument was built on "0 of them while the Spaceadom window had
        // focus" that turned out to be worthless, because the reading cannot
        // distinguish "the app works fine when our window is focused" from
        // "our window is almost never focused". `PROJECT_STATUS.md` still
        // carries the resulting verdict as *"Focus-specificity is NOT proven
        // and is recorded as open"*.
        //
        // These two counters close it. This tick already computed `is_self`,
        // so the exposure costs one relaxed add and no syscall — and the 60s
        // line can then report deafness-per-second-of-focus instead of
        // deafness-per-keystroke, which is the number the question is actually
        // about.
        FG_SAMPLES.fetch_add(1, Ordering::Relaxed);
        if is_self {
            FG_SELF_SAMPLES.fetch_add(1, Ordering::Relaxed);
        }
    }

    // PROBLEM 184 — heal a modifier bit latched by a lost key-up (an eviction
    // that straddled a held Ctrl/Alt/Win). Free here; forbidden in the
    // callback, which is the whole reason the mask exists.
    resync_modifiers();

    let user_input_ms = millis_since_last_input();
    if user_input_ms >= 2_000 {
        // PROBLEM 101 — THE ROOT CAUSE OF 260 FALSE ALARMS.
        //
        // Returning early is not enough. The silence CLOCKS keep running while
        // the user is away — asleep, reading, out of the room — so the moment
        // they touch the mouse again the watchdog compares a stale keyboard
        // timer against a fresh "user is active" signal and concludes the hook
        // must be dead. Measured: "kb hook silent 1825375ms / mouse 547ms" —
        // 30 minutes of not typing, mouse alive half a second ago.
        //
        // Idle time is not evidence about the hook, so it must not accumulate.
        // Re-stamping here means silence is only ever counted while the user
        // was actually PRESENT, which is the only silence that means anything.
        let now = tick_count();
        LAST_KB_EVENT.store(now, Ordering::Relaxed);
        LAST_MS_EVENT.store(now, Ordering::Relaxed);
        return;
    }

    // PROBLEM 78 — while an ELEVATED window has focus (UAC prompt, admin
    // terminal, an installer), Windows UIPI delivers NOTHING to a
    // non-elevated hook, but GetLastInputInfo still updates — the user is
    // typing into the elevated window. That is the app's documented,
    // accepted limitation, NOT an eviction, and it produced a storm of
    // reinstalls at ERROR level during this machine's own install sessions
    // ("kb silent 9000ms / mouse 9000ms, user active 0ms ago" × 7).
    // Discriminator: OpenProcess(PROCESS_QUERY_INFORMATION) on the
    // foreground process FAILS with access denied from medium integrity
    // against an elevated process (the LIMITED flavour would succeed —
    // deliberately not used here).
    {
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION};
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        let fg = GetForegroundWindow();
        // PROBLEM 101 — a NULL foreground window is the UAC secure desktop (or
        // a desktop switch). The hook is deaf there BY DESIGN, and this block
        // had no `else`, so null fell straight through to the eviction verdict
        // — turning every UAC prompt into a false alarm plus a pointless
        // reinstall. Treat it like elevated focus: expected silence.
        if fg.0.is_null() {
            return;
        }
        {
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut pid));
            // PROBLEM 132 — when the foreground IS us, the elevation test below
            // is skipped and we fall through to "evicted". That is the owner's
            // exact repeated symptom, so COUNT it rather than losing it.
            if pid != 0 && pid == std::process::id() {
                BLIND_WHILE_OWN_FG.fetch_add(1, Ordering::Relaxed);
            }
            if pid != 0 && pid != std::process::id() {
                match OpenProcess(PROCESS_QUERY_INFORMATION, false, pid) {
                    Ok(h) => {
                        let _ = windows::Win32::Foundation::CloseHandle(h);
                    }
                    Err(_) => return, // elevated focus — UIPI silence is normal
                }
            }
        }
    }

    let now = tick_count();

    // PROBLEM 78 — cooldown. A real eviction is fixed by ONE reinstall; if
    // silence persists after that, reinstalling again 3s later cannot help
    // and a repeating cause (UIPI edge, another hook ahead of us swallowing
    // events) turns the watchdog into an ERROR-spam loop. One reinstall per
    // minute is fast enough for real evictions and bounds the noise.
    // MEASURE FIRST, THROTTLE SECOND. PROBLEM 182.
    //
    // This early return used to sit ABOVE the silence computation, so after any
    // repair the app was UNREPAIRABLE for a full minute and the log had nothing
    // to say about it. The cooldown's own length is visible in the owner's
    // measurements: six alarms on 2026-08-24/25 report silences of 58000,
    // 58016, 60000, 60015 and 60016 ms. A 58–60 second eviction is not
    // plausible — that is the throttle showing up in its own instrument.
    //
    // On a machine that evicts this hook 15–40 times a day, a fixed 60s hold is
    // a bigger source of deafness than the fault it throttles.
    let kb_silence = now.saturating_sub(LAST_KB_EVENT.load(Ordering::Relaxed));
    let ms_silence = now.saturating_sub(LAST_MS_EVENT.load(Ordering::Relaxed));
    // PROBLEM 228 — GENUINE reference silence. `install_hooks()` no longer
    // writes this clock, so for the first time this number means what the log
    // line says it means. `ref_seen` is the guard for "it has never fired at
    // all", where the clock is still 0 and the subtraction would otherwise
    // report machine uptime.
    let ref_seen = REF_KB_EVENTS.load(Ordering::Relaxed);
    let ref_silence = now.saturating_sub(LAST_REF_KB_EVENT.load(Ordering::Relaxed));
    // What the messages below may honestly say about the reference hook. Built
    // once so the three log lines cannot drift apart.
    let ref_desc = if ref_seen == 0 {
        let installed = now.saturating_sub(REF_HOOK_INSTALLED_AT.load(Ordering::Relaxed));
        format!(
            "the reference hook has NEVER fired since launch (installed {installed}ms ago), \
             so nothing here says a key reached the chain"
        )
    } else {
        format!("the reference hook last genuinely fired {ref_silence}ms ago ({ref_seen} total)")
    };

    // PROBLEM 101 — the `kb_dead` branch is DELETED. It read:
    //     kb_dead = kb_silence > 120_000 && ms_silence < 8_000
    // i.e. "the mouse hook is delivering but nobody has typed for 2 minutes",
    // which it treated as proof the keyboard hook had died. That is simply a
    // description of reading a page. It accounted for 95 of 255 alarms; for
    // those, the MEASURED median mouse silence was 79ms and 78 of 95 had a
    // mouse event within one second — the hooks were provably installed and
    // delivering at the instant it declared them dead. The branch has zero
    // power to distinguish an evicted hook from a person not typing, so no
    // threshold could have rescued it.
    //
    // `both_dead` survives: neither hook seeing anything for 8s while the user
    // is demonstrably active IS anomalous. Note it usually means UIPI deafness
    // (an elevated window has focus and this app runs unelevated) rather than
    // eviction — a reinstall cannot cure that, which is why the log line below
    // no longer claims it can.
    let both_dead = kb_silence > BLIND_MS && ms_silence > BLIND_MS;

    // PROBLEM 181 — the case `both_dead` structurally cannot see: the KEYBOARD
    // hook alone evicted. The reference hook is firing, so keys ARE moving and
    // hooks in this chain CAN still be called; our primary is simply no longer
    // among them. That is not ambiguous the way the deleted `kb_dead` branch
    // was, and it is the failure the owner actually lives with — 24 of 46
    // alarms on 2026-08-24/25 had the mouse alive within 5s while the keyboard
    // had been silent for 5–29s.
    //
    // PROBLEM 228 — `ref_seen > 0` is not redundant. The reference clock is 0
    // until the hook has genuinely fired once, and `now - 0` is machine uptime,
    // which is under `BLIND_MS` for the first three seconds after a boot. A
    // test that can be satisfied by a freshly booted clock is not a test.
    let kb_only_dead = kb_silence > BLIND_MS && ref_seen > 0 && ref_silence < BLIND_MS;

    if !both_dead && !kb_only_dead {
        // Events are arriving: whatever was wrong has cleared. Reset the
        // streak so escalation only ever fires for CONTINUOUS blindness.
        BLIND_REINSTALLS.store(0, Ordering::Relaxed);
        return;
    }
    // PROBLEM 218 — count the alarm HERE, where it is confirmed, and count it
    // before any throttle can return: a throttled alarm is still an alarm, and
    // excluding those is how the rate would come out flattering.
    WD_ALARMS.fetch_add(1, Ordering::Relaxed);
    if FG_IS_SELF.load(Ordering::Relaxed) {
        WD_ALARMS_OWN_FG.fetch_add(1, Ordering::Relaxed);
    }

    // PROBLEM 182 — the cooldown, now ADAPTIVE and applied after measuring.
    //
    // A fixed 60s hold assumes the previous reinstall did something. Often it
    // did not: ten alarms on 2026-08-24/25 read exactly `kb 4000ms / mouse
    // 4000ms` — identical clocks, which is the signature of `install_hooks()`
    // stamping both and then NEITHER hook firing. `reinstall ok: true` only
    // means `SetWindowsHookExW` returned a handle (PROBLEM 132's lesson,
    // still reproducing).
    //
    // So: if the last repair demonstrably produced NO events, do not wait —
    // retrying immediately is the whole point, and `BLIND_REINSTALLS` bounds it
    // by escalating to a full thread rebuild on the second failure. Reserve the
    // 60s hold for the case where events DID resume and then stopped again,
    // which is the repeating-cause scenario the cooldown was written for.
    let last = WATCHDOG_LAST_REINSTALL.load(Ordering::Relaxed);
    if last != 0 && now.saturating_sub(last) < 60_000 {
        // PROBLEM 218 — ASK THE HOOK THAT IS ACTUALLY DEAD.
        //
        // This used to accept an event from EITHER hook as proof the last
        // repair worked. In the `kb_only_dead` failure the mouse hook being
        // alive is the PREMISE — it is how we know the keyboard hook alone was
        // evicted — so a mouse event was being read as evidence against the
        // very condition it helps establish. With the mouse in the user's hand
        // (which it is, whenever they are working in a window rather than
        // typing into one) `previous_worked` was true on every tick, and the
        // watchdog held off for the FULL 60 seconds, every time, while the
        // keyboard stayed dead. PROBLEM 181 measured that shape directly: 24 of
        // 46 alarms on 2026-08-24/25 had the mouse alive within 5s while the
        // keyboard had been silent for 5-29s.
        //
        // A keyboard repair is proven by a KEYBOARD event and by nothing else.
        let previous_worked = if kb_only_dead {
            LAST_KB_EVENT.load(Ordering::Relaxed) > last
        } else {
            LAST_KB_EVENT.load(Ordering::Relaxed) > last
                || LAST_MS_EVENT.load(Ordering::Relaxed) > last
        };
        if previous_worked {
            // PROBLEM 218 — this was `log::debug!`, and release builds run at
            // Info (logger.rs), so the busiest decision in the watchdog has
            // never once appeared in the owner's log. A 60-second hold-off and
            // "the watchdog never noticed" are indistinguishable from outside,
            // which is precisely the gap that made this take three sessions to
            // find. Printed at Info, ONCE per cooldown window — the tick is 1s
            // and the window is 60s, so an unthrottled line would be 60 of
            // them per episode.
            let already = HOLDOFF_LOGGED_FOR.swap(last, Ordering::Relaxed);
            if already != last {
                // PROBLEM 228 — WHAT THIS SENTENCE USED TO CLAIM, AND WHY IT
                // WAS FALSE. It said "the last repair DID deliver events", but
                // the test above reads `LAST_KB_EVENT`, which the watchdog's own
                // idle early-return re-stamps every tick the user has been quiet
                // for 2 s. So after ANY pause following a re-hook the claim was
                // satisfied by the watchdog writing to its own instrument —
                // 100 such lines in the current log, none of them evidence.
                //
                // The DECISION is left exactly as it was on purpose: changing it
                // changes how often the app re-hooks, and that is a behaviour
                // question to answer with a fortnight of honest data, not in the
                // same pass that fixes the instrument. What changes is that the
                // line now prints the CALLBACK-ONLY clock beside the seeded one,
                // so the next fortnight of logs can settle it. Look for
                // "genuine keyboard callback: none since the repair" — that is
                // the hold-off happening on no evidence.
                let genuine = LAST_KB_CALLBACK.load(Ordering::Relaxed);
                let genuine_desc = if genuine > last {
                    format!("a genuine keyboard callback arrived {}ms after it", genuine - last)
                } else {
                    "genuine keyboard callback: NONE since the repair (the test above was \
                     satisfied by the idle re-stamp, not by a key)"
                        .to_string()
                };
                log::info!(
                    "hook: WATCHDOG would re-hook (kb {kb_silence}ms / mouse {ms_silence}ms; \
                     {ref_desc}) but the cooldown test says the last repair delivered events \
                     — {genuine_desc}. Holding off for the rest of the 60s cooldown. If \
                     shortcuts are dead right now, THIS is why, and it will clear on its own \
                     within a minute."
                );
            }
            return;
        }
        // A FLOOR, NOT ZERO. Retrying with no delay at all would spin, and —
        // far worse — it would burn the supervisor's restart budget. `lib.rs`'s
        // hook-thread supervisor keeps `restarts.retain(|t| … < 600)` and gives
        // up permanently above 5, logging *"Space+key is DEAD until the app is
        // restarted."* Escalation currently needs 2 blind reinstalls, which at
        // the old 60s cooldown took ~2 minutes and never reached that cap. Take
        // the cooldown to zero and the same path escalates every few seconds,
        // trips the cap inside a minute, and converts INTERMITTENT deafness
        // into PERMANENT deafness. That is the fix being worse than the bug.
        //
        // 5s is fast enough that a real eviction costs seconds instead of a
        // minute, and slow enough that the escalation rate stays inside budget
        // — together with the separate escalation floor below.
        const BLIND_RETRY_MS: u64 = 5_000;
        if now.saturating_sub(last) < BLIND_RETRY_MS {
            return;
        }
        log::warn!(
            "hook: the previous re-hook delivered NOTHING (kb {kb_silence}ms / mouse \
             {ms_silence}ms; {ref_desc}) — retrying after {BLIND_RETRY_MS}ms instead \
             of sitting out the rest of the 60s cooldown deaf (PROBLEM 182)."
        );
    }
    // NOTE: WATCHDOG_LAST_REINSTALL is stamped AFTER install_hooks() below,
    // not here. Stamping before was a logic bug found on re-review: the
    // `previous_worked` test above compares LAST_KB_EVENT against this stamp,
    // and install_hooks() itself stores LAST_KB_EVENT a few ms LATER than a
    // stamp taken here — so the reinstall's own bookkeeping satisfied
    // "an event arrived after the repair" and the blind-retry path could
    // never fire. The whole adaptive cooldown was dead on arrival.

    // Unhook FIRST (dead handles unhook harmlessly), log after — the old
    // hooks are gone by the time the disk write happens.
    let _ = UnhookWindowsHookEx(*kb);
    let _ = UnhookWindowsHookEx(*ms);
    // A mid-hold eviction must not leave the Space latch stuck.
    MODIFIER_ACTIVE.store(false, Ordering::Relaxed);
    SPACE_INTERCEPTED.store(false, Ordering::Relaxed);
    SPACE_ABORTED.store(false, Ordering::Relaxed);
    // PROBLEM 219 — the hold is over, so its combo evidence must not survive
    // into the next one and leave the reaper standing down for a hold that
    // never pressed a key.
    SPACE_COMBO_SEEN.store(false, Ordering::Relaxed);
    // PROBLEM 206 — nor the pointer latches: an armed chip whose release was
    // lost with the hook, or a suppressed click whose up never arrived.
    pointer::reset_on_eviction();

    // PROBLEM 177, second half — and it must not leave the HUD stuck either.
    //
    // The three flags above were reset because an eviction mid-hold loses the
    // Space-UP: the hook is gone when the key comes back up, so `SpaceUp` is
    // never delivered and the engine's `cancel_hud(false)` never runs. Every
    // piece of KEY state was repaired here; the HUD, which is the only part of
    // that state the USER can see, was not. So the ring stayed on screen with
    // nothing left in the system able to take it down — the owner's *"I'm not
    // holding the space but the space hud is still stuck"*.
    //
    // This machine evicts the hook 17 times a day (PROBLEM 173), so a hold
    // that straddles an eviction is routine here, not exotic.
    if crate::guide_hud::is_visible() {
        log::warn!(
            "hook: the HUD was up when the hook was evicted — the Space-UP for that hold is \
             gone for good, so hiding it here. Without this it stays on screen forever."
        );
    }
    crate::guide_hud::hide_guide_hud();

    let fg = foreground_desc();
    let (nkb, nms) = install_hooks();
    // Stamped with a FRESH tick, strictly >= the clock stamps install_hooks()
    // just wrote — so `LAST_KB_EVENT > WATCHDOG_LAST_REINSTALL` is true only
    // when a GENUINE event arrived after the repair completed (PROBLEM 182).
    WATCHDOG_LAST_REINSTALL.store(tick_count(), Ordering::Relaxed);
    *kb = nkb;
    *ms = nms;
    HOOK_REINSTALLS.fetch_add(1, Ordering::Relaxed);
    HOOK_EVICTIONS_TOTAL.fetch_add(1, Ordering::Relaxed);
    // PROBLEM 101 — WARN, not ERROR, and it no longer asserts a cause it
    // cannot know. 260 of these were logged at ERROR in two days with not one
    // demonstrable eviction among them, which made the log's error channel
    // useless for finding real faults. It also claimed "silent eviction" as
    // fact; the likelier cause is UIPI deafness (an elevated window has focus
    // while this app runs unelevated), which a reinstall cannot fix. Say what
    // was OBSERVED and leave the diagnosis open.
    // PROBLEM 181 — say WHICH failure this is. "Neither hook saw anything" and
    // "the keyboard hook alone was evicted while keys were demonstrably still
    // being delivered to the chain" are different faults with different causes,
    // and until the reference hook existed they were indistinguishable in this
    // line.
    if kb_only_dead && !both_dead {
        log::warn!(
            "hook: WATCHDOG — the KEYBOARD hook alone was evicted. {ref_desc}, so keys ARE \
             reaching the chain, but ours has been silent for \
             {kb_silence}ms (mouse {ms_silence}ms, user active {user_input_ms}ms ago). \
             Foreground: {fg}. This is the failure `both_dead` could never see — it needs BOTH \
             hooks quiet, and the mouse hook keeps it false. Re-hooking. reinstall ok: {}",
            !nkb.is_invalid()
        );
    } else {
        // PROBLEM 228 — `kb`/`mouse` here are the SEEDED clocks (the callback,
        // `install_hooks()` and the idle re-stamp all write them), which is
        // correct for the alarm and is why 54% of these lines print the two
        // numbers as exactly equal — that is one non-hook writer setting both,
        // not two hooks falling silent in the same millisecond. The reference
        // half is now the only unforgeable measurement in this line; read it
        // first.
        log::warn!(
            "hook: WATCHDOG — user active {user_input_ms}ms ago but NEITHER hook saw anything \
             (kb {kb_silence}ms / mouse {ms_silence}ms — both are re-stamped clocks; \
             {ref_desc}). Foreground: {fg}. \
             Elevation was ALREADY ruled out above, so this is NOT UIPI. Re-hooking. \
             reinstall ok: {}",
            !nkb.is_invalid()
        );
    }

    // PROBLEM 132 - ESCALATE. Re-hooking was the ONLY repair this watchdog
    // had, and on 2026-08-17 it ran for 20 unbroken minutes: one alarm a
    // minute, every one reporting `reinstall ok: true`, while the owner had no
    // shortcuts at all. That "ok" only means SetWindowsHookEx returned a
    // handle. It says nothing about whether events will ARRIVE, because a hook
    // proc fires on the thread that INSTALLED it - so if this thread's message
    // pump is what is wedged, a fresh hook on the same wedged pump is a fresh
    // hook that never fires. Repeating it every minute forever is a repair
    // that cannot work, logging success each time.
    //
    // After two consecutive blind reinstalls (~2 min) take the bigger move and
    // end this thread. The PROBLEM 82 supervisor reads an unexpected pump exit
    // as a crash and rebuilds it from scratch - new thread, new message queue,
    // new hooks - which is the only repair that survives a wedged pump. That
    // supervisor's own 5-restarts-per-10-minutes cap bounds this, so escalation
    // cannot become a spin loop.
    let streak = BLIND_REINSTALLS.fetch_add(1, Ordering::Relaxed) + 1;
    // PROBLEM 182 — escalation is rate-limited SEPARATELY from re-hooking.
    //
    // Re-hooking is cheap and can now retry every 5s. Escalation is not: it
    // kills the hook thread so the supervisor rebuilds it, and that supervisor
    // gives up FOREVER after 5 rebuilds in 10 minutes. Letting the faster retry
    // cadence drive escalation would spend that budget in under a minute and
    // leave the app permanently deaf — measured against the seven rebuilds at
    // 00:13:03 through 00:25:03 on 2026-08-25, which stayed inside the cap only
    // because they were two minutes apart.
    //
    // 120s preserves exactly that spacing, so the budget behaves as it always
    // has while the ordinary repair got twelve times faster.
    const ESCALATE_EVERY_MS: u64 = 120_000;
    let last_esc = LAST_ESCALATION.load(Ordering::Relaxed);
    let escalation_allowed = last_esc == 0 || now.saturating_sub(last_esc) >= ESCALATE_EVERY_MS;
    if streak >= 2 && !escalation_allowed {
        log::warn!(
            "hook: {streak} blind reinstalls, but the last hook-thread rebuild was only {}ms \
             ago — holding off. The supervisor gives up permanently after 5 rebuilds in 10 \
             minutes, and spending that budget turns intermittent deafness into permanent.",
            now.saturating_sub(last_esc)
        );
    }
    if streak >= 2 && escalation_allowed {
        LAST_ESCALATION.store(now, Ordering::Relaxed);
        BLIND_REINSTALLS.store(0, Ordering::Relaxed);
        let own = BLIND_WHILE_OWN_FG.swap(0, Ordering::Relaxed);
        log::error!(
            "hook: {streak} reinstalls in a row and STILL no events - re-hooking has failed, \
             so the ENTIRE hook thread is being restarted (fresh message pump). \
             Foreground: {fg}. Alarms while Spaceadom's OWN window had focus: {own}."
        );
        ESCALATE_RESTART.store(true, Ordering::Relaxed);
    }
}

#[cfg(not(windows))]
fn hook_thread_main(_tx: Sender<HookEvent>) {
    log::warn!("hook: non-Windows platform — keyboard hook is a no-op");
}

// ---------------------------------------------------------------------------
// Keyboard HOOKPROC
// ---------------------------------------------------------------------------

#[cfg(windows)]
unsafe extern "system" fn kb_hook_proc(
    n_code: i32,
    w_param: windows::Win32::Foundation::WPARAM,
    l_param: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{CallNextHookEx, KBDLLHOOKSTRUCT};
    use windows::Win32::Foundation::LRESULT;

    if n_code < 0 {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    let ks = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
    let vk = ks.vkCode as u16;
    let msg = w_param.0 as u32;
    let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
    let now = tick_count();
    // PROBLEM 65 — liveness stamp for the eviction watchdog. One lock-free
    // store; runs for EVERY callback (even injected/bypassed events count —
    // being called at all is the proof of life).
    LAST_KB_EVENT.store(now, Ordering::Relaxed);
    // PROBLEM 228 — the same instant, in the clock NOTHING ELSE MAY WRITE.
    // `LAST_KB_EVENT` above is also stamped by `install_hooks()` and by the
    // watchdog's idle re-stamp, so it cannot answer "did the hook actually
    // fire?". This one can, because this line is its only writer.
    LAST_KB_CALLBACK.store(now, Ordering::Relaxed);

    const MAGIC_INJECTED: usize = 0x7A7A7A7A;
    // --- Ignore OUR OWN synthetic inputs to prevent infinite loops ---
    //
    // NOTE: we deliberately test only our magic cookie, NOT the generic
    // LLKHF_INJECTED flag. Blanket-ignoring LLKHF_INJECTED silently disables
    // SpaceToggle for anyone using AutoHotkey, macro keyboards, the on-screen
    // keyboard, Remote Desktop, or laptop drivers that stamp INJECTED onto
    // genuinely physical keystrokes. The magic cookie is sufficient to break
    // the feedback loop, because every key we synthesise carries it.
    if ks.dwExtraInfo == MAGIC_INJECTED {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    // ===================================================================
    // NO log:: CALLS BEYOND THIS POINT — PROBLEM 58, and it broke the app.
    //
    // log4rs writes SYNCHRONOUSLY to a file. Doing that inside a
    // WH_KEYBOARD_LL callback puts disk I/O on the hook path, and Windows
    // enforces `LowLevelHooksTimeout` (300ms default, HKCU\Control Panel\
    // Desktop): a callback that overruns it gets the hook SILENTLY EVICTED.
    // The process keeps running, the startup line still says "hooks
    // installed", and every keystroke simply stops arriving.
    //
    // That is exactly what happened: PROBLEM 48 added eight log::info! calls
    // here to explain suppressions. On a fast SSD dev machine the writes
    // absorbed fine; on a tester's Vostro 5471 they did not, and Space+key
    // and the Guide HUD both died while the log looked perfectly healthy.
    // logger.rs line 43 already warned about this in writing.
    //
    // Diagnostics here MUST be lock-free atomics only. The engine thread
    // reads them and does the logging safely off the hook path.
    // ===================================================================

    // PROBLEM 104 — count EVERY key before any decision. Two atomics and one
    // GetForegroundWindow/GetWindowThreadProcessId pair; no allocation, no
    // lock, no logging, so the callback still returns in microseconds.
    KB_EVENTS_SEEN.fetch_add(1, Ordering::Relaxed);

    // PROBLEM 184 — maintain the Ctrl/Alt/Win mask from the events themselves,
    // BEFORE the fullscreen and bypass early returns below, because a modifier
    // released while bypassed must not stay latched. Two atomics at worst, no
    // syscall.
    //
    // DELIBERATELY *BELOW* THE COOKIE RETURN, and the code is right where the
    // old comment was wrong (review finding, 2026-08-31). The previous wording
    // listed "the cookie check" among the early returns this line comes before;
    // it does not, and it must not. `handle_force_close` injects Alt+F4 with our
    // 0x7A7A7A7A cookie, so if that Alt reached `track_modifier` the mask would
    // read our OWN injection as a physically-held Alt — and the mask is what
    // decides whether Space is passed through to the OS. "Fixing" the order to
    // match that sentence would break Space for the moment after every force
    // close.
    track_modifier(vk, is_down);

    // PROBLEM 225 — "the user is typing", as a signal that can be trusted.
    //
    // Read the doc comment on `LAST_USER_TYPING` for the four false signals
    // this replaces. Each condition below kills one of them, and the order is
    // cheapest-first:
    //
    //   `is_down`        — a key-UP is the END of something, never the start of
    //                      typing. This alone kills the measured 0.62–1.00 s
    //                      cluster: the Space-up and letter-up of the combo
    //                      that fired the launch.
    //   `vk != VK_SPACE` — kills the combo's own Space release directly, and
    //                      keeps a plain Space tap (which types a space and is
    //                      therefore genuinely typing) out of this too. That is
    //                      deliberate: a Space tap is how you DISMISS the HUD,
    //                      and treating it as "the user moved on" is the same
    //                      false positive one layer down.
    //   `!MODIFIER_ACTIVE` — a key pressed while Space is held is a COMMAND,
    //                      not prose. Kills the combo's letter.
    //
    // The fourth condition — "never our own injection" — is satisfied by
    // POSITION: `dwExtraInfo == MAGIC_INJECTED` already returned above, so
    // `inject_space()` and `force_foreground`'s synthetic tap cannot reach
    // this line. **If this block is ever moved above that early return, the
    // cookie test has to come with it.**
    //
    // Never stamped from `install_hooks` or `watchdog_check`: those run on the
    // pump thread, not here, so their re-stamps of LAST_KB_EVENT cannot touch
    // this static at all. That is by construction, not by convention.
    //
    // NATIVE_SAFETY / PROBLEM 58: one relaxed atomic load and one relaxed
    // atomic store, on the key-down path. Do NOT add `GetAsyncKeyState` here to
    // ask whether Space is held — it reports a key we SUPPRESS as UP, which is
    // the exact lie that once broke every shortcut in the app. `MODIFIER_ACTIVE`
    // is our own bookkeeping and is the only honest answer.
    if is_down && vk != VK_SPACE && !MODIFIER_ACTIVE.load(Ordering::Relaxed) {
        LAST_USER_TYPING.store(now, Ordering::Relaxed);
        LAST_USER_TYPING_VK.store(vk as u32, Ordering::Relaxed);
    }

    {
        // PROBLEM 134 - this used to call GetForegroundWindow +
        // GetWindowThreadProcessId HERE, on every keystroke. Both are Win32
        // window queries, and the rule for this callback (skill reference
        // win32-keyboard-hook.md, section 2) is absolute: "read the event,
        // check your dwExtraInfo tag, consult an atomic or lock-free
        // structure, decide pass-or-suppress, return." Window queries are not
        // on that list. They enter win32k and contend on USER32 state that the
        // foreground application's UI thread also touches - so the cost is
        // paid exactly when that thread is busiest, which is when OUR OWN
        // dashboard is focused and rendering.
        //
        // The old comment claimed "no allocation, no lock, no logging, so the
        // callback still returns in microseconds" and was believed for that
        // reason. It counted the wrong costs: the lock it takes is inside the
        // window manager, not in our code.
        //
        // Now a plain atomic read. FG_IS_SELF is refreshed off the hook path,
        // on the watchdog's WM_TIMER branch - stale by up to 3s, which is
        // irrelevant for a per-minute diagnostic counter and free here.
        if FG_IS_SELF.load(Ordering::Relaxed) {
            KB_EVENTS_OWN_FG.fetch_add(1, Ordering::Relaxed);
        }
    }

    // ===================================================================
    // SPACE UP — PROBLEM 218: this block sits ABOVE the three stand-down
    // gates, and that position is load-bearing.
    //
    // WHOEVER EATS THE DOWN OWES THE UP. That rule was already written for
    // the mouse (`CLICK_EATEN`, pointer.rs) and for the space injection
    // (`SPACE_INTERCEPTED`, right below), and this branch is the only place
    // that discharges it for the keyboard. It used to sit BELOW the
    // fullscreen / app-exception / bypass gates, all three of which
    // `return CallNextHookEx` for every event — so if any of them went TRUE
    // during a hold, the Space-UP for a hold we had already swallowed never
    // reached this code at all.
    //
    // Three flags, three 500 ms pollers and one user-facing toggle can each
    // flip mid-hold, and every one of them then cost the user:
    //   * the space they typed (we ate the down and never injected the up), and
    //   * `HookEvent::SpaceUp`, which is the ONLY thing that reaches
    //     `engine::cancel_hud(false)` — i.e. the ONLY thing that takes the
    //     Guide HUD down on an ordinary release. No SpaceUp, no teardown, and
    //     the ring stays on screen with `MODIFIER_ACTIVE` still latched: a HUD
    //     that no later hide can reach, which is the exact class CLAUDE.md's
    //     window rules and PROBLEM 135/177 already warn about.
    //
    // Moving it up cannot change behaviour for a hold that started inside a
    // stand-down: those never set `SPACE_INTERCEPTED`, so the first line
    // below passes them to the OS byte-identically to before.
    // ===================================================================
    if vk == VK_SPACE && is_up {
        // If we never swallowed the matching down-stroke, this up-stroke
        // belongs to the OS. Injecting here would duplicate the space.
        if !SPACE_INTERCEPTED.swap(false, Ordering::Relaxed) {
            return CallNextHookEx(None, n_code, w_param, l_param);
        }
        // PROBLEM 218 — the hold is over, so the staleness evidence must not
        // outlive it. Zeroing the repeat count here is what stops the reaper
        // (`reap_stale_hold`) from ever looking at a finished hold.
        SPACE_REPEATS.store(0, Ordering::Relaxed);
        // PROBLEM 219 — and the hold's combo evidence dies with the hold.
        SPACE_COMBO_SEEN.store(false, Ordering::Relaxed);

        let modifier_fired = MODIFIER_ACTIVE.load(Ordering::Relaxed);
        MODIFIER_ACTIVE.store(false, Ordering::Relaxed);

        // If no modifier action was taken, pass a real Space through.
        // MUST-FIX 2 — but NOT into a physically-held modifier. This is an
        // ASYMMETRY fix, not a new rule: the Space-DOWN gate above already
        // declines to intercept when Ctrl/Alt/Win arrives FIRST ("Ctrl+Space,
        // Alt+Space and Win+Space are real OS/app shortcuts... never swallow
        // them"). The up-path never learned the same rule, so hold Space →
        // press Alt → Tab → release Space injected a bare VK_SPACE while Alt
        // was STILL physically down, and the OS composed it as Alt+Space
        // (opens the window menu) instead of a space. `other_modifier_down()`
        // is a single relaxed atomic load (PROBLEM 184) — safe on the
        // callback. Do NOT reintroduce `GetAsyncKeyState` here; see the
        // FAILSAFE comment further down for why it lies about suppressed keys.
        if !SPACE_ABORTED.load(Ordering::Relaxed) {
            if !other_modifier_down() {
                inject_space();
            } else {
                SPACE_DROPPED_MODIFIER.fetch_add(1, Ordering::Relaxed);
            }
        }

        // PROBLEM 206 — pointer activation, gesture A: the cursor was parked
        // on a chip when Space came up. The injection decision ABOVE is
        // untouched: arming already set SPACE_ABORTED (the wheel's exact
        // mechanic), so an armed release injects no space, and a hold that
        // never armed goes through the branch above byte-identically.
        // `take_armed_key` is relaxed atomic loads/stores only (PROBLEM 58).
        match pointer::take_armed_key() {
            Some(ch) => send_event(HookEvent::PointerActivate(ch)),
            None => send_event(HookEvent::SpaceUp { modifier_fired }),
        }
        return LRESULT(1);
    }

    // --- Fullscreen: pass everything through immediately ---
    if FULLSCREEN_ACTIVE.load(Ordering::Relaxed) {
        SUPPRESS_FULLSCREEN.fetch_add(1, Ordering::Relaxed);
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    // --- App exceptions: pass everything through immediately ---
    //
    // Structured identically to the fullscreen gate above, and placed straight
    // after it on purpose. Note what is NOT here: the Space + . bypass escape
    // hatch that the bypass branch below keeps. Full stock behaviour means
    // full stock behaviour — inside an excluded app the hook decides nothing
    // at all, so a Space + . in Photoshop is a full stop, not a toggle.
    if EXCLUDED_ACTIVE.load(Ordering::Relaxed) {
        if is_down {
            SUPPRESS_EXCLUDED.fetch_add(1, Ordering::Relaxed);
        }
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    // --- Bypass Mode: pass everything through immediately (except Space + .) ---
    if BYPASS_MODE.load(Ordering::Relaxed) {
        if is_down {
            SUPPRESS_BYPASS.fetch_add(1, Ordering::Relaxed);
        }
        // Still allow Space + . to toggle bypass mode OFF!
        if vk == VK_OEM_PERIOD && is_down && (windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(VK_SPACE as i32) as u16 & 0x8000) != 0 {
            send_event(HookEvent::KeyCombo(KeyCombo::Period));
            return LRESULT(1);
        }
        return CallNextHookEx(None, n_code, w_param, l_param);
    }


    // --- Track last alpha-key timestamp for rollover window ---
    if is_down && is_alpha_or_digit(vk) {
        LAST_ALPHA_TS.store(now, Ordering::Relaxed);
        if MODIFIER_ACTIVE.load(Ordering::Relaxed) {
            // Modifier is active — this key should be intercepted as a combo
            // (handled in the combo dispatch below)
        }
    }

    // ===================================================================
    // SPACE DOWN
    // ===================================================================
    if vk == VK_SPACE && is_down {
        // Ctrl+Space, Alt+Space and Win+Space are real OS/app shortcuts
        // (IME switch, IDE autocomplete, window menu, layout switcher).
        // Never swallow them — hand them straight to the OS.
        if other_modifier_down() {
            return CallNextHookEx(None, n_code, w_param, l_param);
        }
        if !MODIFIER_ACTIVE.load(Ordering::Relaxed) {
            MODIFIER_ACTIVE.store(true, Ordering::Relaxed);
            SPACE_ABORTED.store(false, Ordering::Relaxed);
            SPACE_DOWN_TS.store(now, Ordering::Relaxed);
            // PROBLEM 218 — a fresh hold starts with no evidence about this
            // keyboard's auto-repeat. See SPACE_REPEATS.
            SPACE_REPEATS.store(0, Ordering::Relaxed);
            // PROBLEM 219 — a fresh hold has fired no combo yet, so the reaper
            // starts this hold fully armed. Only a key pressed DURING this hold
            // stands it down, and only until this hold ends.
            SPACE_COMBO_SEEN.store(false, Ordering::Relaxed);
            // PROBLEM 206 — a fresh hold must not inherit the last hold's
            // armed chip or its wheel-block. Two relaxed stores.
            pointer::on_space_down();
            send_event(HookEvent::SpaceDown);
        } else {
            // PROBLEM 218 — an AUTO-REPEAT of a Space we are already holding.
            // Counting them is what makes the reaper self-validating: until a
            // hold has produced repeats we have no proof this keyboard repeats
            // at all, and a reaper armed without that proof would tear down
            // legitimate long holds on a machine with auto-repeat disabled.
            // One relaxed add, on the Space-down branch only.
            SPACE_REPEATS.fetch_add(1, Ordering::Relaxed);
        }
        // PROBLEM 218 — the liveness stamp the reaper measures against. One
        // relaxed store, on the Space-down branch only (never on other keys),
        // so the per-keystroke cost of the callback is unchanged.
        SPACE_TICK_TS.store(now, Ordering::Relaxed);
        SPACE_INTERCEPTED.store(true, Ordering::Relaxed);
        // Always suppress Space down to prevent auto-repeat leaking to the OS
        return LRESULT(1);
    }

    // ===================================================================
    // COMBO KEYS (only when modifier is active)
    // ===================================================================
    if MODIFIER_ACTIVE.load(Ordering::Relaxed) && is_down {
        // PROBLEM 219 — FIRST, above every branch that can return, because
        // this fact is true of the hold no matter what we decide to do with
        // the key. Space-down was handled and returned above, so this is
        // always some OTHER key going down while Space is held: the exact
        // event that hands Windows' auto-repeat slot to that key and silences
        // Space's repeat for the rest of the hold. One relaxed store on a
        // branch that already loaded `MODIFIER_ACTIVE` — no new callback cost
        // (PROBLEM 58). See `SPACE_COMBO_SEEN` for the log evidence.
        SPACE_COMBO_SEEN.store(true, Ordering::Relaxed);

        // --- FAILSAFE: has the modifier been latched on for an absurd time? ---
        //
        // THIS USED TO CALL GetAsyncKeyState(VK_SPACE) AND IT BROKE EVERY
        // SHORTCUT IN THE APP. Do not put it back. Reason:
        //
        // We suppress Space-down by returning LRESULT(1), so the keystroke
        // never propagates and Windows never marks Space as pressed in its
        // key-state table. GetAsyncKeyState therefore reports Space as UP even
        // while the user is physically holding it. The guard concluded
        // "modifier stuck", reset it, and let the letter through as plain
        // typing — so Space+F typed "f" instead of opening Explorer, every
        // single time. Confirmed on real hardware: the log filled with
        // "MODIFIER_ACTIVE stuck. Auto-correcting." on genuine keypresses.
        //
        // We are the only component that knows Space is down, because we are
        // the one hiding it. So trust our own bookkeeping and bound it by time
        // instead. The original worry was a dropped Space-UP event latching the
        // modifier on forever; a timeout covers that without lying about the
        // key state.
        // Generous on purpose: people hold Space and READ the guide HUD.
        // A latched modifier only mistypes until the user taps Space again,
        // so err on the side of never interrupting a real hold.
        const MAX_MODIFIER_HOLD_MS: u64 = 30_000;
        let latched_ms = now.saturating_sub(SPACE_DOWN_TS.load(Ordering::Relaxed));
        if latched_ms > MAX_MODIFIER_HOLD_MS {
            STUCK_MODIFIER.fetch_add(1, Ordering::Relaxed);
            MODIFIER_ACTIVE.store(false, Ordering::Relaxed);
            SPACE_INTERCEPTED.store(false, Ordering::Relaxed);
            return CallNextHookEx(None, n_code, w_param, l_param);
        }
        // Check rollover: if the alpha key hit within rollover_ms of Space↓,
        // treat it as normal typing — abort modifier and pass both keys through.
        let space_ts = SPACE_DOWN_TS.load(Ordering::Relaxed);
        let rollover = ROLLOVER_MS.load(Ordering::Relaxed);
        // saturating_sub: SPACE_DOWN_TS can legitimately be 0 or stale if
        // MODIFIER_ACTIVE was forced on by a path that never stamped it.
        let held_ms = now.saturating_sub(space_ts);
        let in_rollover = rollover > 0 && is_alpha_or_digit(vk) && held_ms < rollover;

        // --- A REAL OS SHORTCUT THAT OVERLAPS A HELD SPACE MUST WIN ---
        //
        // PROBLEM 176. The owner: *"when holding the space if I press on the
        // Print Screen button then Spotify comes up. For no reason. It doesn't
        // let me take screenshot while holding the space bar."*
        //
        // The screenshot shortcut on Windows 11 is **Win+Shift+S**, and the
        // chain is exact:
        //   * Space is held, so MODIFIER_ACTIVE is set.
        //   * The `S` of Win+Shift+S arrives here. Nothing below asks whether
        //     Win is also down, so `is_alpha_vk(0x53)` matches and it becomes
        //     KeyCombo::Alpha('s'), suppressed with LRESULT(1).
        //   * His active profile is Professionals, where Space+S is Slack.
        //     Slack does not launch, so `handle_alpha` falls back to the
        //     FOUNDERS binding for that key — which is **Spotify**.
        // Both halves of his report from one missing check: no screenshot,
        // and a music player instead.
        //
        // The identical rule is already applied to the Space-DOWN path a few
        // hundred lines above ("Ctrl+Space, Alt+Space and Win+Space are real
        // OS/app shortcuts... Never swallow them"). It was simply never
        // extended to the keys pressed WHILE Space is held, so the rule held
        // for Win+Space and not for Space-then-Win+S. This fixes Ctrl+C,
        // Alt+Tab, Win+L, Win+Shift+S and every other system chord that
        // happens to be pressed while a thumb is resting on the spacebar.
        //
        // VK_RMENU IS EXEMPT, and must stay exempt: Space+RightAlt cycles
        // profiles, and Right Alt IS Alt — so `other_modifier_down()` reports
        // true for the very keypress that shortcut is made of. Guarding it
        // without this exemption would silently delete profile cycling.
        //
        // Shift alone is deliberately NOT in `other_modifier_down` (Space+Shift
        // is not an OS shortcut and capitalising is not a chord), which is why
        // Win+Shift+S is caught by the Win, not the Shift.
        //
        // POSITION MATTERS, and getting it wrong made the first version of
        // this gate useless. It sat BELOW the rollover branch. `ROLLOVER_MS`
        // is 200 on this machine, and `is_alpha_or_digit(0x53)` is true — so
        // a Win+Shift+S pressed within 200ms of Space going down was
        // swallowed by `in_rollover` and retyped as a literal " s" before
        // execution ever reached the gate. It has to run before ANY branch
        // that can consume the key.
        //
        // Pass-through, not abort: the Space is still genuinely held, so
        // releasing it still types a space exactly as a plain hold always has.
        // Accepted knowingly — this returns before `SPACE_ABORTED` is set, so
        // Space-held + Ctrl+C + release now performs the copy AND types a
        // space, where previously the space was suppressed. The chord working
        // is worth the space.
        if vk != VK_RMENU && other_modifier_down() {
            PASSED_TO_OS.fetch_add(1, Ordering::Relaxed);
            return CallNextHookEx(None, n_code, w_param, l_param);
        }

        if in_rollover {
            // Typing rollover — this is prose, not a command.
            //
            // LOG IT (PROBLEM 48). This path silently turns an intended
            // shortcut into typed text. If a user's rollover_ms is set too
            // high for how fast they press, EVERY shortcut lands here and the
            // app looks completely dead while the log stays empty. Logged at
            // info with the actual numbers so the cause is a one-line read;
            // the first 5 and then every 20th, to bound the volume for a fast
            // typist while still showing the pattern.
            ROLLOVER_HITS.fetch_add(1, Ordering::Relaxed);
            record_margin(&MARGIN_TYPED, held_ms); // PROBLEM 95
            MODIFIER_ACTIVE.store(false, Ordering::Relaxed);
            SPACE_ABORTED.store(true, Ordering::Relaxed);

            // Emit BOTH keystrokes ourselves in a single atomic SendInput
            // batch, and suppress the original.
            //
            // The previous version injected the space and then let the real
            // key through via CallNextHookEx. That races: the real key is
            // already being delivered on this hook thread, while our injected
            // space goes to the BACK of the input queue — so fast typists got
            // "hte" instead of "the". Ordering is only guaranteed if we own
            // both events.
            inject_space_then_key(vk);
            return LRESULT(1);
        }

        // --- Map VK to combo variant ---
        let combo_opt: Option<KeyCombo> = match vk {
            VK_ESCAPE => Some(KeyCombo::Escape),
            VK_OEM_3 => Some(KeyCombo::Backtick),
            VK_OEM_COMMA => Some(KeyCombo::Comma),
            VK_OEM_PERIOD => Some(KeyCombo::Period),

            VK_RMENU => Some(KeyCombo::RightAlt),
            VK_UP    => Some(KeyCombo::UpArrow),
            VK_DOWN  => Some(KeyCombo::DownArrow),
            VK_BACK  => Some(KeyCombo::Backspace),
            v if is_alpha_vk(v) => {
                let ch = vk_to_char(v);
                ch.map(KeyCombo::Alpha)
            }
            // Special keys (F1–F12, Enter, Tab, Left, Right) — dispatched ONLY
            // if the user has actually bound them.
            //
            // PROBLEM 180: this comment has said "only dispatch if the user has
            // bound them" since 1.0.27 while the arms below dispatched
            // unconditionally, destroying the key with `return LRESULT(1)` and
            // leaving `engine::handle_special` to discover there was no binding
            // — far too late to pass anything through. `BOUND_SPECIALS` moves
            // that decision to where it can still matter.
            VK_RETURN if special_bound(12) => Some(KeyCombo::Special("enter".into())),
            // Tab is a FIXED special since 2026-08-29 (pip.rs §9, PROBLEM
            // 210) — fullscreen-preserving PiP. It is no longer gated on
            // `special_bound(13)` because it is no longer optional; it sits
            // with Esc, `, ⌫ and the rest, all of which are dispatched
            // unconditionally. `special_bit` no longer maps "tab", so bit 13
            // can never be set and gating on it would make this key dead.
            //
            // SPACE+TAB SPLIT — 1.0.91 ships this line COMMENTED OUT on
            // purpose. With no mapping here the hook never emits
            // `KeyCombo::Tab`, so `handle_fullscreen_pip` is unreachable and
            // Space+Tab types a normal Tab exactly as it did in 1.0.90. The
            // feature's code stays in the tree; 1.0.92 = this exact tree with
            // this line and the `("Tab", "Fullscreen PiP")` row in
            // `engine::HUD_SPECIALS` uncommented. Pending the owner's verdict
            // after testing 1.0.92 — do not delete either line.
            VK_TAB    => Some(KeyCombo::Tab), // ← 1.0.92 ON / 1.0.91 commented out
            VK_LEFT   if special_bound(14) => Some(KeyCombo::Special("left".into())),
            VK_RIGHT  if special_bound(15) => Some(KeyCombo::Special("right".into())),
            v if (VK_F1..=VK_F12).contains(&v) && special_bound(v - VK_F1) => {
                let n = v - VK_F1 + 1;
                Some(KeyCombo::Special(format!("f{n}")))
            }
            _ => None,
        };

        if let Some(combo) = combo_opt {
            // NO LOGGING HERE — see the PROBLEM 58 banner above. The engine
            // logs "combo Space+X received" the moment it handles this event.
            record_margin(&MARGIN_COMMAND, held_ms); // PROBLEM 95
            SPACE_ABORTED.store(true, Ordering::Relaxed);
            send_event(HookEvent::KeyCombo(combo));
            return LRESULT(1); // suppress key
        }

        if is_alpha_or_digit(vk) {
            UNMAPPED_KEYS.fetch_add(1, Ordering::Relaxed);
        }
    }

    // Default: pass through
    CallNextHookEx(None, n_code, w_param, l_param)
}

// ---------------------------------------------------------------------------
// Mouse HOOKPROC (wheel events)
// ---------------------------------------------------------------------------

#[cfg(windows)]
unsafe extern "system" fn ms_hook_proc(
    n_code: i32,
    w_param: windows::Win32::Foundation::WPARAM,
    l_param: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{CallNextHookEx, MSLLHOOKSTRUCT};
    use windows::Win32::Foundation::LRESULT;

    if n_code < 0 {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }
    // PROBLEM 65 — liveness stamp (see kb_hook_proc). Must be BEFORE the
    // MODIFIER_ACTIVE early-return or the watchdog only sees mouse life
    // while Space is held.
    LAST_MS_EVENT.store(tick_count(), Ordering::Relaxed);
    let msg = w_param.0 as u32;

    // PROBLEM 206 — the second half of a suppressed click, and it must run
    // BEFORE every gate below. Gesture B swallows a WM_LBUTTONDOWN; if its
    // matching WM_LBUTTONUP slipped through (Space released first drops the
    // MODIFIER_ACTIVE gate, or the launched app put an excluded window in
    // front during the ~100ms the button was held), the app underneath would
    // receive an up with no down — unbalanced button state: a stuck drag, a
    // phantom selection. Same rule as SPACE_INTERCEPTED: whoever eats the
    // down owes the up. Cost: one relaxed load per mouse-up system-wide.
    if msg == WM_LBUTTONUP && pointer::eat_click_up() {
        return LRESULT(1);
    }

    // --- App exceptions: same gate as kb_hook_proc, before anything is eaten.
    // MODIFIER_ACTIVE can still be TRUE from a Space held just before the
    // switch, and without this the wheel would stay swallowed for the first
    // scroll inside an excluded app.
    if EXCLUDED_ACTIVE.load(Ordering::Relaxed) {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }
    if !MODIFIER_ACTIVE.load(Ordering::Relaxed) {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    // PROBLEM 206 — while Space is held, remember where the cursor is.
    // THREE relaxed stores and NOTHING else: no hit-test, no chip scan, no
    // win32k call. The overlay is WS_EX_TRANSPARENT so the page can never see
    // a mousemove; this is the entire source of cursor truth, and the
    // `st-hud-pointer` poller does all thinking off this thread. This hook is
    // already evicted 15-40 times a day on this machine (PROBLEM 173/181) —
    // the callback has no budget to spend.
    if msg == WM_MOUSEMOVE {
        let ms = &*(l_param.0 as *const MSLLHOOKSTRUCT);
        pointer::note_cursor(ms.pt.x, ms.pt.y);
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    // PROBLEM 206 — gesture B: left-click while a chip is armed launches it.
    // Suppressed, with the up latched above; an unarmed click passes through
    // untouched so ordinary clicking while holding Space keeps working.
    if msg == WM_LBUTTONDOWN {
        if let Some(ch) = pointer::take_armed_key() {
            pointer::latch_click_eaten();
            send_event(HookEvent::PointerActivate(ch));
            return LRESULT(1);
        }
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    if msg == WM_MOUSEWHEEL {
        let ms = &*(l_param.0 as *const MSLLHOOKSTRUCT);
        let delta = (ms.mouseData >> 16) as i16;
        SPACE_ABORTED.store(true, Ordering::Relaxed);
        // PROBLEM 206, guard 6 — Space+scroll already aborts the space and
        // changes opacity; it must not ALSO leave a chip armed to launch on
        // release. Disarm and stand down for the rest of this hold — the
        // poller sees the block and deliberately does NOT clear SPACE_ABORTED
        // (the wheel owns it now).
        pointer::block_for_hold();
        if delta > 0 {
            send_event(HookEvent::WheelUp);
        } else {
            send_event(HookEvent::WheelDown);
        }
        return LRESULT(1); // suppress scroll
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn send_event(event: HookEvent) {
    EVENT_TX.with(|cell| {
        if let Some(tx) = cell.borrow().as_ref() {
            if let Err(e) = tx.try_send(event) {
                // Counter, not a log call: send_event() runs ON THE HOOK PATH,
                // and this fires precisely when the system is already under
                // load — the worst possible moment to add disk I/O and get the
                // hook evicted (PROBLEM 58). Reported by drain_hook_diagnostics.
                let _ = e;
                DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// PROBLEM 227 — EVERY INJECTION GOES THROUGH ONE FUNCTION, AND IT CHECKS
// ---------------------------------------------------------------------------
//
// `SendInput` inserts its events ONE AT A TIME and stops at the first one
// another thread blocks, returning a SHORT COUNT. Nine call sites across four
// files discarded that return value, and seven of them send real modifiers:
// Win↓ M↓ M↑ Win↑ (boss key), Win↓ Shift↓ M↓ M↑ Shift↑ Win↑ (restore), Ctrl↓ vk↓
// vk↑ Ctrl↑, Alt↓ F4↓ F4↑ Alt↑, Shift↓ vk↓ vk↑ Shift↑, and the hook's own
// Space↓ Space↑. A partial insert of `Win↓ Shift↓ M↓` leaves LWIN and LSHIFT
// physically latched in the OS with no corrective KEYUP anywhere in the tree —
// and a partial insert of `Space↓` leaves SPACE latched, which on this app is
// the worst failure there is (auto-repeat into whatever has focus).
//
// NATIVE_SAFETY.md §3 names the recovery — *"Keys acting held-down → send
// corrective KEYUPs"* — and nothing implemented it. This does.
//
// `force_foreground`'s VK_NONAME tap is immune BY CONSTRUCTION (a reserved key
// that is not a modifier, PROBLEM 225) and stays where it is; that reasoning is
// what was never carried across to the sites that send real modifiers.

/// How many `SendInput` batches came back short, and how many corrective
/// KEYUPs that cost. Counters rather than logs because two of the call sites
/// are INSIDE the hook callback, where PROBLEM 58 forbids disk I/O; reported
/// from `drain_hook_diagnostics` on the engine thread like every other
/// hook-path counter.
static PARTIAL_INJECTIONS: AtomicU32 = AtomicU32::new(0);
static CORRECTIVE_KEYUPS: AtomicU32 = AtomicU32::new(0);

/// Which virtual keys a SHORT insert left physically DOWN.
///
/// `batch` is the batch in order as `(vk, is_keyup)`; `inserted` is what
/// `SendInput` actually returned. Only the inserted PREFIX reached the OS, so
/// the answer is "every key whose down is in the prefix and whose up is not".
///
/// Returned newest-first: the batches this app sends are nested (Win↓ Shift↓ M↓
/// M↑ Shift↑ Win↑), so releasing in reverse press order is the same shape the
/// batch would have produced had it completed.
///
/// `vk == 0` is a `KEYEVENTF_UNICODE` event — it carries a character in
/// `wScan`, latches nothing, and must never be "released".
///
/// Pure, and tested: this is arithmetic on an ordering, done on the path that
/// only ever runs after something else has already gone wrong — exactly the
/// shape CLAUDE.md says to write a test for rather than to hope about.
/// The core, written to be callable FROM THE HOOK CALLBACK: no allocation, no
/// `Vec`, no logging. `get(i)` yields `(vk, is_keyup)` for event `i`, so the
/// caller can decode `INPUT`s in place instead of building a list first
/// (PROBLEM 58 — "lock-free atomics only" on that path, and a `Vec` is neither).
///
/// Writes the still-held keys into `out`, newest-first, and returns how many.
/// `out` is expected to be at least as long as the batch; anything beyond it is
/// dropped, which cannot happen for the batches this app sends (six events at
/// most) and is bounded rather than panicking if that ever changes.
pub(crate) fn unreleased_keys_into(
    len: usize,
    inserted: usize,
    get: impl Fn(usize) -> (u16, bool),
    out: &mut [u16],
) -> usize {
    let mut n = 0usize;
    for i in 0..inserted.min(len) {
        let (vk, is_up) = get(i);
        if vk == 0 {
            continue;
        }
        if is_up {
            if let Some(pos) = out[..n].iter().position(|&h| h == vk) {
                out.copy_within(pos + 1..n, pos);
                n -= 1;
            }
        } else if !out[..n].contains(&vk) && n < out.len() {
            out[n] = vk;
            n += 1;
        }
    }
    out[..n].reverse();
    n
}

/// `Vec` convenience over `unreleased_keys_into`, for tests and for readers.
/// It DELEGATES rather than reimplementing: two copies of this ordering rule
/// that could disagree is the failure mode half of PROBLEM 227 is about.
pub(crate) fn unreleased_keys(batch: &[(u16, bool)], inserted: usize) -> Vec<u16> {
    let mut buf = [0u16; 32];
    let n = unreleased_keys_into(batch.len(), inserted, |i| batch[i], &mut buf);
    buf[..n].to_vec()
}

/// Send one keyboard batch and repair the OS state if it came back short.
///
/// Returns `true` when every event was inserted. NEVER LOGS: it is called from
/// the hook callback (PROBLEM 58). Callers on the engine thread that want a log
/// line use `send_keys_checked`, which wraps this.
#[cfg(windows)]
pub(crate) unsafe fn send_keys_raw(
    inputs: &[windows::Win32::UI::Input::KeyboardAndMouse::INPUT],
) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP,
    };
    let sent = SendInput(inputs, std::mem::size_of::<INPUT>() as i32) as usize;
    if sent == inputs.len() {
        return true;
    }
    PARTIAL_INJECTIONS.fetch_add(1, Ordering::Relaxed);

    // NO ALLOCATION BEYOND THIS POINT. Two of the callers are inside the hook
    // callback, where the rule is atomics and stack only (PROBLEM 58) — so the
    // batch is decoded IN PLACE out of the union (the caller never has to
    // describe it twice, and two descriptions that could disagree is the exact
    // failure mode half of PROBLEM 227 is about) and the repair is built on the
    // stack. 16 is four times the longest batch this app sends.
    let mut stuck = [0u16; 16];
    let n = unreleased_keys_into(
        inputs.len(),
        sent,
        |i| {
            let inp = &inputs[i];
            if inp.r#type == INPUT_KEYBOARD {
                let ki = inp.Anonymous.ki;
                (ki.wVk.0, (ki.dwFlags.0 & KEYEVENTF_KEYUP.0) != 0)
            } else {
                (0, false) // mouse/hardware event: latches no key
            }
        },
        &mut stuck,
    );
    if n == 0 {
        return false;
    }
    let mut ups = [INPUT::default(); 16];
    for (slot, &vk) in ups.iter_mut().zip(stuck[..n].iter()) {
        *slot = kbd_input(vk, true);
    }
    // One batch, cookie-tagged like everything else we synthesise, so our own
    // hook passes it through instead of re-processing it. If THIS one is also
    // blocked there is nothing further to try — the keys are latched by
    // whatever is blocking injection, and it will be reported by the counters.
    let repaired = SendInput(&ups[..n], std::mem::size_of::<INPUT>() as i32) as usize;
    CORRECTIVE_KEYUPS.fetch_add(repaired as u32, Ordering::Relaxed);
    false
}

/// `send_keys_raw` plus a WARN naming the batch. Engine-thread callers only —
/// never from the hook callback.
#[cfg(windows)]
pub(crate) unsafe fn send_keys_checked(
    inputs: &[windows::Win32::UI::Input::KeyboardAndMouse::INPUT],
    what: &str,
) -> bool {
    if send_keys_raw(inputs) {
        return true;
    }
    log::warn!(
        "input: SendInput did not insert the whole '{what}' batch ({} events) — another \
         thread blocked it partway (BlockInput, or UIPI while an elevated window has \
         focus). Corrective KEYUPs were sent for anything the partial batch left down \
         (NATIVE_SAFETY.md §3). The shortcut itself did NOT happen.",
        inputs.len()
    );
    false
}

/// Build one synthetic keyboard INPUT stamped with our magic cookie so the
/// hook recognises it as self-generated and passes it through.
#[cfg(windows)]
fn kbd_input(vk: u16, keyup: bool) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: if keyup { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0x7A7A7A7A,
            },
        },
    }
}

/// Inject a synthetic Space key press+release via SendInput.
#[cfg(windows)]
unsafe fn inject_space() {
    let inputs = [kbd_input(VK_SPACE, false), kbd_input(VK_SPACE, true)];
    // PROBLEM 227 — `send_keys_raw`, not `SendInput`: this runs INSIDE the hook
    // callback, so it must not log (PROBLEM 58), but a partial insert here
    // latches SPACE ITSELF — auto-repeating into whatever has focus, with the
    // one key the whole app is built on stuck down. The corrective KEYUP is the
    // difference between a dropped space and a typing catastrophe.
    let _ = send_keys_raw(&inputs);
}

/// Emit Space followed by `vk` as ONE ordered SendInput batch.
/// Used by the typing-rollover path, where ordering must be guaranteed.
#[cfg(windows)]
unsafe fn inject_space_then_key(vk: u16) {
    let inputs = [
        kbd_input(VK_SPACE, false),
        kbd_input(VK_SPACE, true),
        kbd_input(vk, false),
        kbd_input(vk, true),
    ];
    // Same rule as `inject_space`: no logging on this path, and a short insert
    // must never leave Space (or the letter) down. This is the rollover path,
    // so it fires while the owner is typing at speed.
    let _ = send_keys_raw(&inputs);
}

#[cfg(not(windows))]
unsafe fn inject_space() {}

#[cfg(not(windows))]
unsafe fn inject_space_then_key(_vk: u16) {}

/// True if Ctrl, Alt or Win is physically held right now.
///
/// Shift is deliberately excluded: Shift+Space is a plain space in most apps,
/// and treating it as pass-through would break the modifier while capitalising.
/// Ctrl / Alt / Win currently held, tracked from the hook's OWN event stream.
/// Bit 0 = Ctrl, 1 = Alt, 2 = Win.
///
/// PROBLEM 184 — this replaces four-to-six `GetAsyncKeyState` calls that ran
/// **inside the keyboard callback**.
///
/// `GetAsyncKeyState` is a win32k syscall. This file already argues the case
/// against exactly this, at length, about a different pair of calls (PROBLEM
/// 134): *"Window queries are not on that list… They enter win32k and contend
/// on USER32 state that the foreground application's UI thread also touches —
/// so the cost is paid exactly when that thread is busiest."* That fix removed
/// `GetForegroundWindow` and left these behind.
///
/// It matters here more than it looks, for three reasons measured on this
/// machine:
///   * `LowLevelHooksTimeout` is **NOT SET** in HKCU, so Windows' 300 ms
///     default applies, and that deadline is WALL-CLOCK — a callback merely
///     waiting for a CPU slice misses it exactly like a slow one.
///   * The keyboard hook is evicted 15–40 times a day here while the MOUSE
///     hook, on the same thread and the same pump, survives. The one
///     structural difference between them is that `ms_hook_proc` makes no
///     win32k calls at all.
///   * PROBLEM 176 had just put this call on the path of EVERY combo key, not
///     only Space-down — i.e. it added syscalls to the hot path of a hook that
///     is already being evicted for overrunning.
///
/// A low-level keyboard hook SEES every Ctrl/Alt/Win transition, so the state
/// can be maintained from the events themselves and read back as one relaxed
/// load. No syscall, no contention, exact at the instant of the keystroke
/// rather than sampled near it.
///
/// SELF-HEALING: a missed key-up — from an eviction that straddles a held
/// modifier — would latch a bit and quietly pass every combo through. The
/// watchdog re-syncs the mask from `GetAsyncKeyState` on its timer, which is
/// OFF the callback path and where such a call is free.
static MODS_DOWN: AtomicU32 = AtomicU32::new(0);

const MOD_CTRL: u32 = 1;
const MOD_ALT: u32 = 2;
const MOD_WIN: u32 = 4;

/// Update `MODS_DOWN` from one hook event. Pure atomics; safe on the callback.
#[inline]
fn track_modifier(vk: u16, is_down: bool) {
    // LL hooks report the SPECIFIC side (VK_LCONTROL / VK_RCONTROL …); the
    // generic codes are accepted too because injected input may use them.
    let bit = match vk {
        0x11 | 0xA2 | 0xA3 => MOD_CTRL, // VK_CONTROL / VK_LCONTROL / VK_RCONTROL
        0x12 | 0xA4 | 0xA5 => MOD_ALT,  // VK_MENU / VK_LMENU / VK_RMENU
        0x5B | 0x5C => MOD_WIN,         // VK_LWIN / VK_RWIN
        _ => return,
    };
    if is_down {
        MODS_DOWN.fetch_or(bit, Ordering::Relaxed);
    } else {
        MODS_DOWN.fetch_and(!bit, Ordering::Relaxed);
    }
}

/// Re-sync the modifier mask from the OS. Watchdog thread only — never the
/// callback. Cheap there, and it heals a bit latched by a lost key-up.
#[cfg(windows)]
fn resync_modifiers() {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    unsafe {
        let mut mask = 0u32;
        if (GetAsyncKeyState(0x11) as u16 & 0x8000) != 0 { mask |= MOD_CTRL; }
        if (GetAsyncKeyState(0x12) as u16 & 0x8000) != 0 { mask |= MOD_ALT; }
        if (GetAsyncKeyState(0x5B) as u16 & 0x8000) != 0 { mask |= MOD_WIN; }
        if (GetAsyncKeyState(0x5C) as u16 & 0x8000) != 0 { mask |= MOD_WIN; }
        let prev = MODS_DOWN.swap(mask, Ordering::Relaxed);
        if prev != mask {
            log::debug!("hook: modifier mask re-synced {prev:#x} -> {mask:#x}");
        }
    }
}

#[cfg(not(windows))]
fn resync_modifiers() {}

/// Is a REAL modifier (Ctrl/Alt/Win) physically held? One relaxed load.
///
/// Shift is deliberately excluded: Space+Shift is not an OS chord and
/// capitalising is not a command. Win+Shift+S is caught by the Win.
fn other_modifier_down() -> bool {
    MODS_DOWN.load(Ordering::Relaxed) != 0
}

/// PROBLEM 99 — the monotonic clock, for callers outside this module (the
/// undo buffer needs an age). Deliberately the SAME source the hook uses, so
/// timings recorded here can be compared with hook timings directly.
pub fn tick_count_pub() -> u64 {
    tick_count()
}

fn tick_count() -> u64 {
    #[cfg(windows)]
    unsafe {
        windows::Win32::System::SystemInformation::GetTickCount64()
    }
    #[cfg(not(windows))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

fn is_alpha_or_digit(vk: u16) -> bool {
    (0x41..=0x5A).contains(&vk) // A–Z
        || (0x30..=0x39).contains(&vk) // 0–9
}

fn is_alpha_vk(vk: u16) -> bool {
    (0x41..=0x5A).contains(&vk)
}

fn vk_to_char(vk: u16) -> Option<char> {
    if (0x41..=0x5A).contains(&vk) {
        char::from_u32((vk as u32) + 32) // map A→a, etc.
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// TESTS — PROBLEM 218. The teardown-always-reached invariant, expressed as
// the one decision it depends on.
//
// House rule (CLAUDE.md): test the pure logic a user only reaches after
// something else has already gone wrong. Every case below is a state the
// owner's machine actually produces — it evicts these hooks 15-40 times a day.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod stale_hold_tests {
    use super::{hold_is_stale, MIN_OBSERVED_REPEATS, STALE_HOLD_GRACE_MS};

    /// The failure being closed: a hold whose Space-UP was lost with the hook.
    /// Auto-repeat stops, nothing else in the system will ever end this hold,
    /// and the HUD is on screen. It MUST be reaped.
    #[test]
    fn a_hold_whose_auto_repeat_stopped_is_reaped() {
        assert!(hold_is_stale(true, 40, 2_001, STALE_HOLD_GRACE_MS, false));
        assert!(hold_is_stale(true, MIN_OBSERVED_REPEATS, 60_000, STALE_HOLD_GRACE_MS, false));
    }

    /// The feature that must survive: the owner holds Space and READS the
    /// HUD. Measured in his log on 2026-08-29 — hold #272 stayed up for 26.8
    /// seconds and hid normally. Auto-repeat is arriving throughout, so no
    /// length of hold may ever be reaped.
    #[test]
    fn a_genuine_long_hold_is_never_reaped() {
        for held_ms in [0u64, 1_000, 27_000, 600_000] {
            let _ = held_ms;
            // Repeats keep landing: the gap since the LAST one stays small.
            assert!(
                !hold_is_stale(true, 900, 40, STALE_HOLD_GRACE_MS, false),
                "a hold that is still repeating must never be reaped"
            );
        }
        // Right up to the grace boundary, inclusive, it is still alive.
        assert!(!hold_is_stale(true, 900, STALE_HOLD_GRACE_MS, STALE_HOLD_GRACE_MS, false));
        assert!(hold_is_stale(true, 900, STALE_HOLD_GRACE_MS + 1, STALE_HOLD_GRACE_MS, false));
    }

    /// THE CHECK THAT CAN PRODUCE A NEGATIVE. On a keyboard, driver or
    /// accessibility setting with auto-repeat OFF, a real hold produces zero
    /// repeats — and a reaper armed without that evidence would tear the HUD
    /// down two seconds into every legitimate hold. Never arm without proof.
    #[test]
    fn without_observed_auto_repeat_the_reaper_never_arms() {
        for repeats in 0..MIN_OBSERVED_REPEATS {
            assert!(
                !hold_is_stale(true, repeats, 10 * 60_000, STALE_HOLD_GRACE_MS, false),
                "with only {repeats} observed repeat(s) there is no evidence this keyboard \
                 repeats at all, so silence is not evidence either"
            );
        }
        assert!(hold_is_stale(true, MIN_OBSERVED_REPEATS, 2_001, STALE_HOLD_GRACE_MS, false));
    }

    /// No hold, nothing to reap — whatever the clocks say. This is the state
    /// on every ordinary typed space, which is the path that must stay free.
    #[test]
    fn no_latched_hold_is_never_reaped() {
        assert!(!hold_is_stale(false, 900, 10 * 60_000, STALE_HOLD_GRACE_MS, false));
        assert!(!hold_is_stale(false, 0, 0, STALE_HOLD_GRACE_MS, false));
    }

    /// The grace must be wider than the slowest auto-repeat Windows can be
    /// configured to produce, or a slow-repeat machine reaps its own live
    /// holds. Longest repeat DELAY is 1000 ms and the slowest RATE is about
    /// 2/second, so the widest legitimate gap is ~500 ms after the delay and
    /// ~1000 ms before the first repeat.
    #[test]
    fn the_grace_clears_the_slowest_windows_auto_repeat() {
        const SLOWEST_WINDOWS_REPEAT_GAP_MS: u64 = 1_000;
        assert!(
            STALE_HOLD_GRACE_MS >= 2 * SLOWEST_WINDOWS_REPEAT_GAP_MS,
            "the grace must leave headroom over the slowest repeat cadence, or a live hold \
             on a slow-repeat machine reads as dead"
        );
        assert!(!hold_is_stale(
            true,
            900,
            SLOWEST_WINDOWS_REPEAT_GAP_MS,
            STALE_HOLD_GRACE_MS,
            false
        ));
    }

    // -----------------------------------------------------------------------
    // PROBLEM 219 — the gate leak. Reproduced from the owner's debug.log of
    // 2026-08-29 03:19:50-52: Space held, Space+Tab tapped four times, the
    // reaper fires 2016 ms after the LAST Space auto-repeat, and his fifth
    // tap becomes a real Tab inside Brave.
    // -----------------------------------------------------------------------

    /// THE BUG, as numbers. Every one of PROBLEM 218's three conditions is
    /// satisfied by a completely healthy Space+Tab user, because pressing Tab
    /// is what stopped Space from repeating. Without the fourth condition this
    /// hold is declared dead while the owner's thumb is still on the bar.
    #[test]
    fn a_hold_that_fired_a_combo_is_never_reaped() {
        // His measured numbers: 5 repeats seen, 2016 ms of silence since.
        assert!(
            !hold_is_stale(true, 5, 2_016, STALE_HOLD_GRACE_MS, true),
            "Space is still physically held; Tab merely took the auto-repeat slot"
        );
        // And no length of pause between taps may change that — there is no
        // liveness signal left to time, so no deadline is honest.
        for since in [2_001u64, 10_000, 60_000, 10 * 60_000] {
            assert!(!hold_is_stale(true, 40, since, STALE_HOLD_GRACE_MS, true));
        }
    }

    /// The same inputs WITHOUT a combo still reap — the fix narrows the
    /// reaper, it does not disable it. This is the pair that proves the new
    /// condition is the only thing that changed.
    #[test]
    fn the_combo_flag_is_the_only_difference() {
        let (active, repeats, since) = (true, 5u32, 2_016u64);
        assert!(hold_is_stale(active, repeats, since, STALE_HOLD_GRACE_MS, false));
        assert!(!hold_is_stale(active, repeats, since, STALE_HOLD_GRACE_MS, true));
    }

    /// The hold shape PROBLEM 218 was actually written for is untouched:
    /// Space held to READ the HUD, no key pressed, hook evicted mid-hold so
    /// the Space-UP never arrives. Still reaped, or the ring is stranded.
    #[test]
    fn a_silent_hold_with_no_combo_is_still_reaped() {
        assert!(hold_is_stale(true, 40, 2_001, STALE_HOLD_GRACE_MS, false));
        assert!(hold_is_stale(true, 900, 27_000, STALE_HOLD_GRACE_MS, false));
    }
}

/// PROBLEM 228 — the deafness instrument, and the lie it used to tell.
///
/// SPACEADOM-2 ("the primary keyboard hook saw 0 events while the reference hook
/// fired Nms ago") was measured against 20 days of the owner's own log: **281 of
/// 282 DEAF lines had their implied reference instant within 10 ms of a watchdog
/// re-hook**, because `install_hooks()` stamped `LAST_REF_KB_EVENT` and the
/// message read that stamp back as proof keys were flowing. The cleanest sample:
///
///     09:52:39.420 [WARN] hook: WATCHDOG — ... Re-hooking. reinstall ok: true
///     09:52:39.435 [WARN] hook: DEAF ... the reference hook fired 16ms ago
///
/// These tests pin the shape of the fix: the verdict is computed from a COUNTER
/// only the hook callback can increment, so the repair cannot forge it.
#[cfg(test)]
mod deaf_instrument_tests {
    use super::{classify_hook_window, HookWindow};

    /// THE 281 LINES. A window in which nobody typed and the watchdog re-hooked:
    /// the seeded timestamp said "a reference event 16 ms ago", the counter says
    /// nothing was called. Quiet, not deaf — and nothing reaches Sentry.
    #[test]
    fn a_re_hook_alone_is_not_evidence_that_keys_are_reaching_the_chain() {
        assert_eq!(classify_hook_window(0, 0), HookWindow::Quiet);
    }

    /// The one line the instrument exists for: the reference hook was genuinely
    /// CALLED and the primary saw none of it. Certain, by construction.
    #[test]
    fn counted_reference_calls_with_a_silent_primary_are_real_deafness() {
        assert_eq!(classify_hook_window(0, 1), HookWindow::Deaf);
        assert_eq!(classify_hook_window(0, 47), HookWindow::Deaf);
    }

    /// The primary saw keys: working, whatever the reference did. This is the
    /// ordinary case and it must never produce a warning.
    #[test]
    fn any_primary_traffic_at_all_means_working() {
        assert_eq!(classify_hook_window(29, 30), HookWindow::Working);
        assert_eq!(classify_hook_window(1, 0), HookWindow::Working);
    }

    /// A typing pause is not a fault. This is PROBLEM 101's deleted detector,
    /// which PROBLEM 217 reinstated at Sentry level through the seeded stamp —
    /// the surrounding minutes of the owner's log read
    /// `29 keys / 37 keys / DEAF / 31 keys / DEAF / 49 keys`, which is a person
    /// pausing, not a hook dying.
    #[test]
    fn silence_on_both_hooks_is_never_reported() {
        for window in 0..5u32 {
            let _ = window;
            assert_ne!(classify_hook_window(0, 0), HookWindow::Deaf);
        }
    }
}

/// PROBLEM 227 — what a SHORT `SendInput` insert leaves latched.
///
/// `SendInput` stops at the first event another thread blocks and returns a
/// short count. Nine call sites discarded that value; NATIVE_SAFETY.md §3 names
/// the recovery ("send corrective KEYUPs") and nothing implemented it. This is
/// the arithmetic that decides which keys to release, and it only ever runs
/// after something has already gone wrong — the exact shape CLAUDE.md says to
/// unit-test rather than hope about.
#[cfg(test)]
mod partial_injection_tests {
    use super::unreleased_keys;

    const WIN: u16 = 0x5B;
    const SHIFT: u16 = 0xA0;
    const M: u16 = 0x4D;
    const CTRL: u16 = 0x11;
    const ALT: u16 = 0x12;
    const F4: u16 = 0x73;
    const SPACE: u16 = 0x20;

    /// The batch went in whole: nothing to repair, and no corrective KEYUP may
    /// ever be sent on the happy path.
    #[test]
    fn a_complete_insert_leaves_nothing_down() {
        let batch = [(WIN, false), (M, false), (M, true), (WIN, true)];
        assert!(unreleased_keys(&batch, batch.len()).is_empty());
    }

    /// The boss key, cut off after `Win↓ Shift↓ M↓`. Without the repair, LWIN
    /// and LSHIFT stay physically latched in the OS with no KEYUP anywhere in
    /// the tree — every subsequent keystroke becomes a Win+Shift shortcut.
    /// Released newest-first, mirroring the nesting the batch would have had.
    #[test]
    fn a_partial_win_shift_m_releases_both_modifiers() {
        let batch = [
            (WIN, false), (SHIFT, false), (M, false),
            (M, true), (SHIFT, true), (WIN, true),
        ];
        assert_eq!(unreleased_keys(&batch, 3), vec![M, SHIFT, WIN]);
        assert_eq!(unreleased_keys(&batch, 4), vec![SHIFT, WIN]);
        assert_eq!(unreleased_keys(&batch, 5), vec![WIN]);
        assert_eq!(unreleased_keys(&batch, 6), Vec::<u16>::new());
    }

    /// Blocked outright: nothing reached the OS, so nothing is latched. Sending
    /// corrective KEYUPs here would be inventing input.
    #[test]
    fn a_batch_that_was_blocked_entirely_needs_no_repair() {
        let batch = [(ALT, false), (F4, false), (F4, true), (ALT, true)];
        assert!(unreleased_keys(&batch, 0).is_empty());
    }

    /// Ctrl+key, cut after `Ctrl↓ vk↓`: both are down.
    #[test]
    fn a_partial_ctrl_combo_releases_the_key_and_the_modifier() {
        let batch = [(CTRL, false), (M, false), (M, true), (CTRL, true)];
        assert_eq!(unreleased_keys(&batch, 2), vec![M, CTRL]);
    }

    /// The hook's own Space injection — the worst one to leave latched, because
    /// SPACE auto-repeats into whatever has focus and Space is what the entire
    /// app is built on.
    #[test]
    fn a_half_inserted_space_injection_releases_space() {
        let batch = [(SPACE, false), (SPACE, true)];
        assert_eq!(unreleased_keys(&batch, 1), vec![SPACE]);
    }

    /// A `KEYEVENTF_UNICODE` event carries its character in `wScan` and has
    /// `wVk == 0`. It latches nothing, and "releasing" vk 0 would inject a
    /// meaningless key-up.
    #[test]
    fn unicode_events_are_never_released() {
        let batch = [(0u16, false), (0u16, true)];
        assert!(unreleased_keys(&batch, 1).is_empty());
    }

    /// A key that went down AND came back up inside the inserted prefix is not
    /// held, and must not be released a second time.
    #[test]
    fn a_key_already_released_in_the_prefix_is_not_released_again() {
        let batch = [(M, false), (M, true), (WIN, false), (WIN, true)];
        assert_eq!(unreleased_keys(&batch, 3), vec![WIN]);
        assert!(unreleased_keys(&batch, 2).is_empty());
    }

    /// Defensive: a count larger than the batch (which `SendInput` cannot
    /// return, but an edit to the caller could produce) must not panic.
    #[test]
    fn an_impossible_count_is_clamped_rather_than_panicking() {
        let batch = [(WIN, false), (WIN, true)];
        assert!(unreleased_keys(&batch, 99).is_empty());
    }
}
