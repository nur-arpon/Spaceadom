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
/// PROBLEM 263 — the BUILT-IN middle-button exclusion list (3D/CAD/design
/// programs where middle-drag already orbits or pans). Separate from the
/// user's App exceptions by design; see the module header.
pub mod orbit_apps;

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
    Semicolon,        // Space + ;     → Voice typing (Windows dictation, Win+H)
    Slash,            // Space + /     → Screenshot (Windows snip, Win+Shift+S)
    Quote,            // Space + '     → On-screen keyboard (Windows OSK, Win+Ctrl+O)
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
    /// PROBLEM 259 — a Space-down that the DASHBOARD PAGE observed, not the
    /// hook (`own_window_keys` fallback). The engine treats it exactly like
    /// `SpaceDown`; the ONLY difference is the sentence it logs, and that
    /// difference is load-bearing.
    ///
    /// `SpaceDown`'s per-hold line (PROBLEM 257) says *"the primary keyboard
    /// hook saw this Space-down"*, and CLAUDE.md keyboard-hook law 6 /
    /// `scripts/install-proof.ps1` read that line as the PROOF the hook is
    /// alive over our own window. If the fallback reused `SpaceDown` it would
    /// print that sentence about a Space the hook never saw, and the one
    /// witness law 6 has would start lying. Hence a separate variant whose
    /// line carries the `own-window fallback:` marker instead and deliberately
    /// does NOT contain `hold start (hold #N)` — a fallback hold can never
    /// satisfy the install proof.
    OwnWindowSpaceDown,
    /// PROBLEM 263 — the MIDDLE MOUSE BUTTON went down and this process
    /// swallowed it, so a ring is owed. The engine treats it exactly like
    /// `SpaceDown` — same HUD timer, same chips, same cascade, same toast —
    /// and the ONLY difference is the sentence it logs.
    ///
    /// A separate variant for the same reason `OwnWindowSpaceDown` is one: the
    /// per-hold `hold start (hold #N)` line is CLAUDE.md keyboard-hook law 6's
    /// proof that the KEYBOARD hook is alive, and `scripts/install-proof.ps1`
    /// reads it as such. A middle-button hold never touched the keyboard hook,
    /// so its line carries the `middle-button ring:` marker instead and
    /// deliberately does NOT contain the words `hold start`. After this change
    /// the ring has THREE possible triggers and the log can always say which
    /// one raised it.
    MiddleButtonDown,
    /// PROBLEM 263 — the middle button came back up before the hold threshold
    /// and nothing else claimed the press, so it was an ORDINARY MIDDLE CLICK
    /// and this process owes the world one.
    ///
    /// The replay is a `SendInput` and this variant is how it leaves the
    /// callback: `ms_hook_proc` may not make a win32k call (see the header
    /// there and PROBLEM 58/134/184), so the batch is composed and sent on the
    /// ENGINE thread, where it is also legal to log about it.
    MiddleButtonTap,
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
///
/// PROBLEM 230 — "before any branch" now includes the injected-cookie test, so
/// this counts our OWN synthetic keys too. That is deliberate and required:
/// this number is subtracted from `REF_KB_EVENTS`, which has always counted
/// them, and two counters compared against each other must count the same
/// events or the difference is an artefact rather than a measurement.
static KB_EVENTS_SEEN: AtomicU32 = AtomicU32::new(0);
/// Of those, how many arrived while OUR OWN window held the foreground. If
/// this stays 0 while the total climbs, Windows is not delivering our own
/// window's keystrokes to our hook — which is the user's exact symptom.
static KB_EVENTS_OWN_FG: AtomicU32 = AtomicU32::new(0);
/// PROBLEM 236 — of `KB_EVENTS_SEEN`, how many carried OUR OWN cookie.
///
/// `KB_EVENTS_SEEN` has counted this app's injections since PROBLEM 230 moved
/// its `fetch_add` above the `dwExtraInfo == MAGIC_INJECTED` test, and that was
/// right: the subtraction in `classify_hook_window` is only a measurement while
/// both sides count the same population. But it made the number PRINTED in
/// `saw N key event(s)` ambiguous in the other direction — "the primary is
/// seeing keys" and "the primary is seeing nothing but Spaceadom typing to
/// itself" became the same line, and the second one is a dead keyboard.
///
/// One relaxed add on a branch that is already taken (the cookie early return),
/// so the PROBLEM 58 envelope is unchanged. Subtract it from `KB_EVENTS_SEEN`
/// to get the REAL keyboard traffic; never subtract it before
/// `classify_hook_window`, which needs the whole population.
static KB_EVENTS_INJECTED: AtomicU32 = AtomicU32::new(0);
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
///
/// ═══ PROBLEM 230 — AND THE TWO ARGUMENTS MUST COUNT THE SAME POPULATION ═══
///
/// This is a subtraction between two counters, so it is a measurement only
/// while both sides count the same events. Two ways that broke, both fixed at
/// the counter rather than by a correction applied here:
///
///   * `primary_seen` skipped this app's OWN injected keys — the callback's
///     `dwExtraInfo` cookie test sat ABOVE the add — while
///     `genuine_ref_events` counted them. A window whose only keyboard traffic
///     was Spaceadom injecting to itself therefore read DEAF.
///   * the reference hook was installed in FRONT of the primary, which made it
///     the first hook Windows evicts, so the side of the subtraction that is
///     supposed to prove keys are flowing died BEFORE the side being tested.
///
/// Keep this function pure. Anything that has to be corrected for belongs
/// where the counter is written.
pub(crate) fn classify_hook_window(primary_seen: u32, genuine_ref_events: u32) -> HookWindow {
    if primary_seen > 0 {
        HookWindow::Working
    } else if genuine_ref_events > 0 {
        HookWindow::Deaf
    } else {
        HookWindow::Quiet
    }
}

/// PROBLEM 236 — THE LINE THAT TELLS THE FOUR HYPOTHESES APART.
///
/// The 1.0.96 log could not answer the one question that separates them,
/// because the only per-window number it printed was `saw N key event(s)` — a
/// single total, covering real keys and this app's own injections, with no
/// mouse or reference figure beside it. Reading the 2026-09-04 16:59:32–17:02:30
/// episode took a point-in-time watchdog clock (`the reference hook last
/// genuinely fired 178968ms ago`) and an aggregate from a DIFFERENT, earlier
/// window (`saw 60 key event(s) in the last 79s`), and those two do not overlap:
/// the 60 keys all landed before 16:59:32, which the log only reveals via a
/// third line 12 minutes away (`kb 7922ms` equalling `ref 7922ms` at
/// 16:59:39.641). Four counters, one window, one line — so the next reader
/// subtracts nothing.
///
/// All four numbers are CALLBACK-ONLY. Nothing but a hook proc can move them,
/// which is PROBLEM 228's law applied to the whole instrument panel rather than
/// to the reference hook alone.
///
///   * `primary_real` > 0 and `reference` == 0 — the primary is alive and the
///     witness is not: PROBLEM 230's inversion is back (check install order).
///   * `primary_real` == 0 and `primary_injected` > 0 — the keyboard is dead
///     and `saw N key event(s)` was counting Spaceadom typing to itself.
///   * `primary_real` == 0, `reference` > 0 — real deafness (see
///     `classify_hook_window`).
///   * all four 0 — nobody touched anything. Not evidence about the hooks.
///   * `mouse` > 0 with everything else 0 — the thread's pump is fine, so a
///     `both_dead` alarm raised in this window was measuring silence, not death.
pub(crate) fn format_liveness_split(
    primary_real: u32,
    primary_injected: u32,
    reference: u32,
    mouse: u32,
) -> String {
    format!(
        "primary_real:{primary_real} primary_injected:{primary_injected} \
         reference:{reference} mouse:{mouse}"
    )
}

/// PROBLEM 236 — does a `both_dead` alarm have any evidence behind it?
///
/// `both_dead` is `kb_silence > BLIND_MS && ms_silence > BLIND_MS`, and both of
/// those clocks are SEEDED — `install_hooks()` and the watchdog's own idle
/// early-return write them, so the alarm's premise can be, and demonstrably is,
/// satisfied by the repair and by the watchdog itself. In the owner's 1.0.96
/// session **6 of 16 alarms printed the pair as exactly equal round numbers**
/// (4000/4000 ×4, 5000/5000, 4000/4000) — the documented fingerprint of one
/// non-hook writer setting both, i.e. those alarms measured nothing at all
/// about the hooks.
///
/// This asks the same question of the clocks NOTHING but a callback may write.
/// An alarm is EVIDENCED only when all three unforgeable instruments — the
/// keyboard callback, the mouse callback and the reference hook — have each
/// been silent at least as long as the threshold the alarm is using. A hook
/// that has NEVER fired (`None`) can never make an alarm evidenced: never
/// having fired is not the same fact as having stopped, and PROBLEM 228 is the
/// record of what conflating those two costs.
///
/// **It deliberately does not change the decision.** Whether to re-hook on an
/// unevidenced alarm is a behaviour question, and this file's own law (PROBLEM
/// 228) is to fix the instrument in one pass and settle the behaviour with a
/// fortnight of honest data in the next. Grep `UNEVIDENCED` to collect it.
pub(crate) fn alarm_is_evidenced(
    kb_callback_silence_ms: Option<u64>,
    ms_callback_silence_ms: Option<u64>,
    ref_callback_silence_ms: Option<u64>,
    threshold_ms: u64,
) -> bool {
    match (kb_callback_silence_ms, ms_callback_silence_ms, ref_callback_silence_ms) {
        (Some(kb), Some(ms), Some(rf)) => {
            kb >= threshold_ms && ms >= threshold_ms && rf >= threshold_ms
        }
        _ => false,
    }
}

// ═══ PROBLEM 236, DECISION CHANGED (2026-09-04) ═══════════════════════════
//
// `alarm_is_evidenced` above deliberately gated NOTHING — it only worded the
// log, and the entry said to collect a fortnight of `EVIDENCED` vs
// `UNEVIDENCED` before touching the behaviour. The owner cannot wait a
// fortnight: every one of those alarms re-hooks, and the re-hook clears
// `MODIFIER_ACTIVE` / `SPACE_*`, calls `pointer::reset_on_eviction()` and
// hides the ring — so at one alarm every 2.4 minutes it kills a live Space
// hold roughly whenever he holds one. The data keeps accruing (the
// "would have alarmed" line below), but the DECISION moves now.
//
// The rule the decision uses from here on:
//
//   **An alarm may fire only when the keyboard, mouse and reference CALLBACK
//   clocks are ALL silent past the threshold, after each of them has fired at
//   least once since the last install.**
//
// Nothing that `install_hooks()` or this watchdog's own idle early-return
// writes may take part in it. `LAST_KB_EVENT` / `LAST_MS_EVENT` keep their
// re-stamps and keep raising the CANDIDATE alarm — removing those re-opens
// PROBLEM 101's 260 false alarms — but they can no longer authorise the
// destructive repair on their own.

/// The rule, in one sentence, printed on every line that acts on it AND on
/// every line that declines to. Nobody reading `debug.log` should have to open
/// this file to find out what decided.
///
/// It carries no numbers deliberately: the threshold, the grace and the unknown
/// bound are printed beside it from the constants themselves, so the sentence
/// can never drift away from the arithmetic.
const RULE_DESC: &str = "RULE (PROBLEM 236, decision changed 2026-09-04; reference vote corrected \
     on review the same day): only the PRIMARY keyboard callback or the mouse callback can prove \
     the hooks are alive. The REFERENCE hook cannot — \"the reference fires while the primary is \
     silent\" IS an evicted primary (PROBLEM 181/230), so a live reference only narrows the \
     verdict from both-hooks-dead to keyboard-only-dead; it never cancels the alarm. An alarm \
     fires when those callback clocks have been silent past the threshold, after each has fired \
     at least once since the last install; a hook that has never fired since the install is \
     UNKNOWN, not dead, until the unknown bound expires.";

/// How long after an install nothing said here means anything.
///
/// Chosen at 10 s, against a 3 s threshold and a 1 s tick. The alarm at
/// `16:28:53.412` in the owner's 1.0.96 session fired **6 seconds after
/// launch**, before any hook had been called once, and still printed "NEITHER
/// hook saw anything" — true, and about nothing. A grace shorter than that
/// window would have let the same line through again. Ten seconds is also
/// under the 5 s blind-retry floor doubled, so it costs at most one skipped
/// repair attempt on a genuinely dead install, which the bound below then
/// picks up.
const INSTALL_GRACE_MS: u64 = 10_000;

/// How long "no hook has fired since the install" is allowed to stay UNKNOWN
/// before it becomes a fact.
///
/// **This bound is not optional, and it is the reason the rule above is safe.**
/// Read literally, "never fired since install ⇒ unknown ⇒ no alarm" hands the
/// app a hole it can never climb out of: if `SetWindowsHookExW` returns a
/// handle that never fires (PROBLEM 132's wedged pump, or a failed install the
/// `is_invalid()` check missed), no callback can ever move, so the state stays
/// UNKNOWN forever and the watchdog never retries. That converts intermittent
/// deafness into permanent deafness, which is this file's standing definition
/// of the fix being worse than the bug.
///
/// So the "unknown" state is bounded. Every tick that reaches this decision has
/// already passed `millis_since_last_input() < 2000` (the user is demonstrably
/// active), a NULL foreground (UAC secure desktop), and the UIPI elevation
/// test. Thirty seconds of a demonstrably-present user producing not one
/// callback on ANY of the three hooks is not an absence of evidence — it is
/// evidence. At 30 s the retry cadence for that case is ten times quieter than
/// 1.0.96's, and it still self-heals.
const UNKNOWN_MAX_MS: u64 = 30_000;

/// What the CALLBACK-ONLY clocks say about the hooks. Three states, because
/// "we have not been told" and "we have been told nothing is happening" are
/// different facts and PROBLEM 228 is the record of what conflating them costs.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum DeadKind {
    /// The PRIMARY keyboard callback is silent past the threshold while the
    /// REFERENCE hook is still being called. Keys are demonstrably reaching the
    /// chain and ours is no longer among them — PROBLEM 181's shape, and the
    /// exact fingerprint of an evicted primary. A re-hook is the right repair.
    KbOnly,
    /// Every callback clock that can speak has gone quiet, the reference
    /// included. Nothing here says keys are reaching the chain at all.
    Both,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum CallbackLiveness {
    /// A PRIMARY callback-only clock — keyboard or mouse — moved inside the
    /// threshold. One of the hooks this app installed is demonstrably being
    /// called. **No alarm.**
    ///
    /// The reference hook is deliberately NOT among the clocks that can produce
    /// this; see `classify_callback_liveness`.
    Alive,
    /// Not enough has happened for anything here to mean anything: still inside
    /// the post-install grace, or a hook has not yet had its first callback
    /// since the install and the install is young enough that this is ordinary.
    /// **No alarm** — and this is the state that 6 of the owner's 16 alarms were
    /// really in when they printed `4000/4000`.
    Unknown,
    /// The callback-only clocks that can decide have been silent past the
    /// threshold — or the install has now had `UNKNOWN_MAX_MS` of a
    /// demonstrably-active user and produced no callback at all. **Alarm.**
    /// The payload says WHICH failure, because "our keyboard hook alone was
    /// evicted while keys kept flowing" and "nothing is being called at all"
    /// have different causes and only the first is certain to be repairable by
    /// re-hooking.
    Dead(DeadKind),
}

/// PROBLEM 236 — THE DECISION. One pure function; the watchdog does no
/// arithmetic of its own.
///
/// Every input is either a CALLBACK-ONLY clock or a wall-clock age. Note which
/// direction the one non-callback input can push: `since_install_ms` can only
/// ever SUPPRESS an alarm (through `grace_ms`) or, past `unknown_max_ms`, admit
/// one that no callback could have contradicted. A writer that can only
/// suppress is not the defect PROBLEM 228 named — that was a writer that
/// MANUFACTURED evidence.
///
/// `None` means "this hook has not fired since the last install". It is not
/// "this hook is dead": PROBLEM 228 is the twenty days of reports that
/// conflating those two produced. So a `None` can never, on its own, make an
/// alarm fire — only the `unknown_max_ms` bound can, and only after the user
/// has been continuously present for that long.
///
/// The caller supplies the user-activity premise, not this function: by the
/// time `watchdog_check` reaches here it has already returned early on an idle
/// user, a NULL foreground and an elevated foreground.
pub(crate) fn classify_callback_liveness(
    kb_callback_silence_ms: Option<u64>,
    ms_callback_silence_ms: Option<u64>,
    ref_callback_silence_ms: Option<u64>,
    since_install_ms: u64,
    threshold_ms: u64,
    grace_ms: u64,
    unknown_max_ms: u64,
) -> CallbackLiveness {
    // 1. Inside the grace, the hooks have not had a fair chance to speak.
    if since_install_ms < grace_ms {
        return CallbackLiveness::Unknown;
    }
    let recent = |v: Option<u64>| matches!(v, Some(ms) if ms < threshold_ms);

    // 2. ONLY A PRIMARY HOOK CAN VOTE "ALIVE" — REVIEW FIX, 2026-09-04.
    //
    //    This branch used to accept the REFERENCE clock as proof of life
    //    alongside the two primaries, and that single `||` made the whole
    //    `kb_only_dead` repair unreachable. The reference hook exists to
    //    witness that keys are reaching the chain; "the witness fires while our
    //    primary keyboard callback stays silent" is not evidence of health, it
    //    is the LITERAL DEFINITION of the fault this watchdog was built for
    //    (PROBLEM 181: 24 of 46 alarms; PROBLEM 230: the eviction the reorder
    //    fixed). So a live reference vetoed the alarm for exactly the shape the
    //    alarm was supposed to catch.
    //
    //    Generalise: **an instrument installed to witness a failure must never
    //    be allowed to vote that the failure did not happen.** Its reading is a
    //    premise of the diagnosis, not a rebuttal of it.
    if recent(kb_callback_silence_ms) || recent(ms_callback_silence_ms) {
        return CallbackLiveness::Alive;
    }

    // 3. The reference IS being called, and neither primary is. Keys are
    //    reaching the chain; ours is no longer among the hooks called for
    //    them. That is `kb_only_dead`, and it is repairable by re-hooking.
    //
    //    The reference may only ever narrow `Both` to `KbOnly` here — it can
    //    never cancel the verdict.
    if recent(ref_callback_silence_ms) {
        // PROBLEM 228's law still holds over the top of it: a primary that has
        // NEVER fired since the install is UNKNOWN, not dead, and only the
        // bound may promote that silence to a fact.
        if kb_callback_silence_ms.is_some() {
            return CallbackLiveness::Dead(DeadKind::KbOnly);
        }
        return if since_install_ms >= unknown_max_ms {
            CallbackLiveness::Dead(DeadKind::KbOnly)
        } else {
            CallbackLiveness::Unknown
        };
    }

    // 4. All three have fired since the install and all three have now been
    //    silent past the threshold. That is the alarm's own premise, stated
    //    from instruments the repair cannot write.
    if let (Some(_), Some(_), Some(_)) =
        (kb_callback_silence_ms, ms_callback_silence_ms, ref_callback_silence_ms)
    {
        return CallbackLiveness::Dead(DeadKind::Both);
    }
    // 5. A hook has never fired since the install. Unknown — until the install
    //    is old enough that the silence is itself the observation (see
    //    `UNKNOWN_MAX_MS`; without this branch a failed install is permanent).
    if since_install_ms >= unknown_max_ms {
        CallbackLiveness::Dead(DeadKind::Both)
    } else {
        CallbackLiveness::Unknown
    }
}

/// How long a live Space hold may hold off the watchdog's repair.
///
/// Ten seconds. Justification, in the order it matters:
///
///   * `reap_stale_hold()` runs at the TOP of `watchdog_check`, above every
///     early return, so a hold whose auto-repeat has stopped is already reaped
///     before this deferral is ever consulted. What is left to defer for is
///     therefore a hold that is still auto-repeating — i.e. one the keyboard
///     hook is still being called for — or one that has fired a combo, where
///     PROBLEM 219 stood the reaper down on purpose.
///   * the combo branch's own `MAX_MODIFIER_HOLD_MS` is 30 s, so 10 s cannot
///     be the longest thing latching this state.
///   * the blind-retry floor is 5 s and the escalation floor is 120 s, so a
///     10 s deferral perturbs neither cadence.
///   * a hold longer than ten seconds is not a shortcut; if the hook really is
///     dead the repair is only ten seconds late, and the alarm is re-evaluated
///     from scratch on the very next 1 s tick after the bound expires.
const MAX_HOLD_DEFER_MS: u64 = 10_000;

/// PROBLEM 236 — may this alarm's repair wait for the hold to finish?
///
/// The repair is destructive BY DESIGN: it clears `MODIFIER_ACTIVE`,
/// `SPACE_INTERCEPTED`, `SPACE_ABORTED`, `SPACE_COMBO_SEEN`, calls
/// `pointer::reset_on_eviction()` and hides the ring. Those resets are correct
/// for a REAL eviction — the Space-UP is genuinely lost and PROBLEM 177/218 are
/// what happens without them. Landing the same set on a hold that is still
/// being fed by the keyboard hook is the owner's *"it dies mid-press"*.
///
/// `primary_saw_key_within_hold` is `LAST_KB_CALLBACK >= SPACE_DOWN_TS`: the
/// keyboard callback was demonstrably entered at or after this hold began, so
/// the hook was not dead when the hold started. Callback-only on both sides —
/// nothing off the hook path writes either value.
pub(crate) fn hold_defers_rehook(
    modifier_active: bool,
    primary_saw_key_within_hold: bool,
    deferred_for_ms: u64,
    max_defer_ms: u64,
) -> bool {
    modifier_active && primary_saw_key_within_hold && deferred_for_ms < max_defer_ms
}

/// PROBLEM 262 item 2 — may a tick that did NOT alarm end the deferral episode?
///
/// ONLY when the hold that episode belongs to is over. This one-line predicate
/// is the whole of the 2026-09-07 wedge, so it is worth stating plainly.
///
/// `ALARM_DEFERRED_AT` is the start of the episode and `MAX_HOLD_DEFER_MS` is
/// measured from it, so the bound is only reachable if that stamp survives the
/// ticks in between. It did not. Three sites cleared it, and two of them fired
/// on ticks where nothing was wrong with the hold at all:
///
///   * `!both_dead && !kb_only_dead` — "events are arriving". `both_dead`
///     requires the MOUSE callback to have been silent past `BLIND_MS`, and the
///     mouse fires at 30-60 Hz whenever the user's hand is on it. So on every
///     tick the owner moved the mouse, the episode was thrown away.
///   * the `!Dead(_)` verdict return, for the same reason.
///
/// Measured consequence, installed 1.0.106 at 11:38:33 / 11:38:41 / 11:39:08 /
/// 11:39:30: four "Holds protected this session: 1 / 2 / 3 / 4" lines for ONE
/// hold. Each alarm found `started == 0`, restarted the clock at zero, deferred
/// again and logged again. **A 10 s bound that is reset by every quiet tick is
/// not a bound**; it required ten CONSECUTIVE seconds of alarm, which a moving
/// mouse makes impossible, so the deferral was unbounded in practice and the
/// repair never came.
///
/// The hold's own end is still the ordinary way an episode finishes — Space-up,
/// or `reap_stale_hold` — and that check lives at the top of `watchdog_check`,
/// above every early return.
pub(crate) fn defer_episode_ends_on_quiet_tick(hook_hold_latched: bool) -> bool {
    !hook_hold_latched
}

/// PROBLEM 257 — "the keyboard is DEAF, and here is the key that proves it."
///
/// Every earlier deafness test had to infer "somebody typed" from a clock
/// (`GetLastInputInfo`, which the mouse also moves) and was wrong about it
/// 260 times (PROBLEM 101). This one asks the OS whether SPACE IS DOWN RIGHT
/// NOW. Keyboard-hook law 3 says `GetAsyncKeyState` lies about keys we
/// SUPPRESS — and that is exactly what makes it honest here: a Space our hook
/// intercepted never reaches the OS key state, so the OS reports it UP while
/// `MODIFIER_ACTIVE` is true. A Space the OS reports DOWN with
/// `MODIFIER_ACTIVE` false is a Space our callback let through, and a callback
/// that let it through must have FIRED for it (and for its auto-repeats, which
/// start inside 1000 ms). So:
///
///     Space down per the OS  +  no hold latched  +  no deliberate pass-through
///     +  callback silent past `threshold_ms`  =  the callback was not called.
///
/// `stand_down` is the union of the three deliberate pass-through gates
/// (fullscreen, excluded app, bypass) and `other_modifier` is law 4's
/// Ctrl/Alt/Win pass-through; both are cases where the callback DID fire and
/// chose to return `CallNextHookEx`, so they are excluded rather than measured.
/// `kb_callback_silence_ms` is `None` when the callback has not fired since the
/// last install — that is UNKNOWN (PROBLEM 236), never proof, so it returns
/// false: a fresh install that has never been called cannot be declared deaf
/// by this test, only by the install-grace rules in `classify_callback_liveness`.
pub(crate) fn keyboard_deaf_with_space_down(
    space_physically_down: bool,
    modifier_active: bool,
    stand_down: bool,
    other_modifier: bool,
    kb_callback_silence_ms: Option<u64>,
    threshold_ms: u64,
) -> bool {
    if !space_physically_down || modifier_active || stand_down || other_modifier {
        return false;
    }
    matches!(kb_callback_silence_ms, Some(ms) if ms >= threshold_ms)
}

/// PROBLEM 260 — THE MECHANISM, NAMED ONCE SO NOBODY HAS TO REDISCOVER IT.
///
/// Printed on every forced repair. It is a sentence, not a number, for the same
/// reason `RULE_DESC` is: the arithmetic is printed beside it from the
/// constants, so the words can never drift away from the code.
const TIMEOUT_EVICTION_DESC: &str =
    "MECHANISM (PROBLEM 260): when a WH_KEYBOARD_LL callback overruns \
     LowLevelHooksTimeout (HKCU\\Control Panel\\Desktop, 1000ms by default) Windows STOPS \
     CALLING IT AND LEAVES THE HANDLE VALID — no message, no error, no return code says so, \
     and UnhookWindowsHookEx on it still succeeds. WH_MOUSE_LL is a SEPARATE hook with its \
     own timeout record on the same thread, so the mouse callback keeps firing at 30-60Hz \
     while not one keystroke arrives; that is why a timed-out keyboard hook makes the app \
     look alive from every clock except the keyboard's own. The ONLY cure a process has is \
     to change the chain: unhook and install afresh. Therefore (a) a live MOUSE callback may \
     never veto a keyboard-deaf verdict — it proves the pump, not the hook — and (b) a \
     repair is an UNHOOK plus a fresh SetWindowsHookExW, which is why the old and new HHOOK \
     values are printed below: an unchanged handle means no repair happened.";

/// PROBLEM 260 — which instrument proved the keyboard hook is not being called.
///
/// Both are CALLBACK-ONLY on the side that matters (`LAST_KB_CALLBACK`); they
/// differ only in how they establish the other half of the proof — that input
/// the OS accepted did not reach us.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum ProvenDeaf {
    /// PROBLEM 257's original test. `GetAsyncKeyState` says Space is physically
    /// DOWN right now, with no hold latched and no pass-through gate open, and
    /// the keyboard callback has been silent past the threshold. See
    /// `keyboard_deaf_with_space_down` for why law 3's lie makes this honest.
    SpaceHeld,
    /// The generalisation, and the one that would have caught the 2026-09-07
    /// episode 105 seconds sooner. SINCE PROBLEM 268 (2026-09-17) it is a
    /// MEASUREMENT, not an inference: the hook thread's raw-input sink saw a
    /// KEYBOARD event (`WM_INPUT`, a clock that owes nothing to the hook
    /// chain) inside `os_input_max_age_ms`, and our keyboard callback is
    /// older than that event by more than the attribution margin — a
    /// keystroke reached the OS and did not reach us.
    ///
    /// **WHAT IT WAS BEFORE, AND WHY IT HAD TO CHANGE.** From 1.0.105 to
    /// 1.0.111 this read `GetLastInputInfo` (any input) against the MOUSE
    /// callback's clock: "the OS saw input the mouse hook cannot account
    /// for, so it must have been keyboard." On this owner's laptop it
    /// produced 449 FORCED REPAIRS in the two days of 2026-09-16/17, 245 of
    /// them after less than three seconds of keyboard silence — precision-
    /// touchpad gestures arrive as pointer input the LL mouse hook never
    /// sees, and a person scrolling with two fingers while not typing looks
    /// exactly like a dead keyboard hook to that test. Each repair is a
    /// destructive re-install that can drop a live Space-UP. The raw clock
    /// cannot be fooled by the touchpad: it only ticks for keyboards.
    ///
    /// **THIS IS NOT PROBLEM 101'S DELETED `kb_dead` BRANCH.** That one read
    /// `kb_silence > 120_000 && ms_silence < 8_000` — "the mouse hook is
    /// delivering and nobody has typed for two minutes" — which is a
    /// description of reading a page, and it produced 95 of 255 false alarms.
    /// The difference is the direction of the mouse test. That branch fired
    /// when the mouse WAS delivering; this one fires only when the mouse is
    /// demonstrably NOT delivering while the OS says input happened anyway. A
    /// user reading a page moves the mouse, so the mouse callback accounts for
    /// the OS clock and this returns `None`. A user typing with their hand off
    /// the mouse into a hook that is no longer called is the only shape left.
    InputUnaccountedFor,
}

/// PROBLEM 260 — the proven-deaf verdict, as one pure function.
///
/// Everything here is either a CALLBACK-ONLY clock, a wall-clock age, or a fact
/// read from the OS on the watchdog thread. Note which way each non-callback
/// input can push: `since_install_ms` and `modifier_active` can only SUPPRESS;
/// `os_input_age_ms` can admit a verdict, and it is checked AGAINST the mouse
/// callback rather than trusted on its own, which is the whole of PROBLEM 101's
/// lesson applied here.
///
/// The caller supplies the premises this function does not check: by the time
/// `watchdog_check` reaches it, an idle user, a NULL foreground (UAC secure
/// desktop) and an elevated foreground (UIPI) have all already returned early.
/// A re-hook cannot cure any of those, so they must never reach this verdict.
#[allow(clippy::too_many_arguments)]
pub(crate) fn proven_keyboard_deaf(
    space_physically_down: bool,
    modifier_active: bool,
    stand_down: bool,
    other_modifier: bool,
    kb_callback_silence_ms: Option<u64>,
    raw_kb_age_ms: Option<u64>,
    os_input_age_ms: u64,
    since_install_ms: u64,
    threshold_ms: u64,
    grace_ms: u64,
    os_input_max_age_ms: u64,
    mouse_attribution_margin_ms: u64,
) -> Option<ProvenDeaf> {
    // 1. THE INSTALL GRACE STILL OUTRANKS EVERYTHING. A hook that has not had a
    //    fair chance to be called cannot be proven deaf, and PROBLEM 236's
    //    16:28:53.412 alarm — six seconds after launch, before any callback had
    //    run — is the record of what skipping this costs.
    if since_install_ms < grace_ms {
        return None;
    }
    // 2. NEVER WHILE A HOLD IS LATCHED. The forced repair below is destructive
    //    by design (it re-installs, which loses the Space-UP), and landing that
    //    on a live hold is the owner's "it dies mid-press" — PROBLEM 236. The
    //    other paths in `watchdog_check` have `hold_defers_rehook` for the case
    //    where a repair really must happen during a hold; this path simply does
    //    not run then. `reap_stale_hold()` runs at the TOP of `watchdog_check`,
    //    above every early return, so a hold the hook has stopped feeding is
    //    already cleared before this is consulted — the latch cannot wedge this
    //    verdict shut.
    if modifier_active {
        return None;
    }
    // 3. THE KEYBOARD CALLBACK MUST HAVE FIRED ONCE SINCE THIS INSTALL AND THEN
    //    STOPPED. `None` is UNKNOWN, never proof (PROBLEM 228) — a hook that has
    //    never been called since the install is the failed-install case, and it
    //    belongs to `classify_callback_liveness`'s `unknown_max_ms` bound, not
    //    here. Keeping this path narrow is what makes the word "PROVEN" honest:
    //    fired-then-stopped IS the timeout-eviction signature.
    let kb_silence = kb_callback_silence_ms?;
    if kb_silence < threshold_ms {
        return None;
    }
    // 4. PROOF A: a key is physically down right now and we were not called for
    //    it (PROBLEM 257). Delegated rather than re-implemented, so PROBLEM
    //    257's test and this one can never disagree about what "Space is down
    //    and we are deaf" means. The gates are excluded rather than measured
    //    because in each of them the callback DID fire and chose to pass the
    //    key on.
    if keyboard_deaf_with_space_down(
        space_physically_down,
        modifier_active,
        stand_down,
        other_modifier,
        kb_callback_silence_ms,
        threshold_ms,
    ) {
        return Some(ProvenDeaf::SpaceHeld);
    }
    // 5. PROOF B (PROBLEM 268): the OS DELIVERED A KEYSTROKE our callback did
    //    not see. `raw_kb_age_ms` is the raw-input sink's clock — keyboard
    //    devices only, chain-independent. `None` (no keystroke since launch,
    //    or no sink) proves nothing.
    //
    //    The gates are deliberately NOT excluded here. `LAST_KB_CALLBACK` is
    //    stamped in the first four lines of `kb_hook_proc`, ABOVE every gate, so
    //    a fullscreen/excluded/bypass window still stamps it on every keystroke.
    //    Silence past the threshold therefore means the callback was not entered
    //    at all, whatever the gates say.
    //
    //    The margin covers the gap between the LL callback (synchronous, in
    //    the input path) and the WM_INPUT for the same key reaching this
    //    thread's queue: the raw stamp is the LATER of the two, so for a key
    //    we did see, kb_silence <= raw_age + pump latency. It is small on
    //    purpose. `os_input_age_ms` stays as a belt to that brace: a raw
    //    event the OS input clock does not also know about is not one.
    let raw_age = raw_kb_age_ms?;
    if raw_age <= os_input_max_age_ms
        && os_input_age_ms <= os_input_max_age_ms
        && kb_silence > raw_age.saturating_add(mouse_attribution_margin_ms)
    {
        return Some(ProvenDeaf::InputUnaccountedFor);
    }
    None
}

/// PROBLEM 260 — how long the forced repair must wait, given how many
/// consecutive forced repairs delivered nothing.
///
/// Doubling from `base_ms`, capped at `max_ms`. The cap matters more than the
/// curve: a genuinely hostile environment (another process re-installing a hook
/// ahead of ours in a loop, a driver eating keys upstream of every hook) must
/// not be able to drive a repair storm, and a repair that keeps not working is
/// evidence the cure is not ours to apply.
///
/// The streak counts INEFFECTIVE repairs only — one keyboard callback after a
/// repair resets it to zero, so the fast cadence is always available again the
/// moment a repair demonstrably works.
pub(crate) fn forced_repair_backoff_ms(ineffective_streak: u32, base_ms: u64, max_ms: u64) -> u64 {
    // Saturating at 16 doublings keeps the shift in range whatever the streak.
    let shift = ineffective_streak.min(16);
    base_ms.saturating_mul(1u64 << shift).min(max_ms)
}

/// PROBLEM 260 — may a forced repair run right now?
///
/// `last_repair_age_ms` is `None` when this session has not forced one yet.
/// This is the ONLY throttle on the proven-deaf path: the 60 s cooldown and the
/// "last repair delivered events" test are deliberately not consulted — see the
/// forced-repair block in `watchdog_check` for why.
pub(crate) fn forced_repair_allowed(
    last_repair_age_ms: Option<u64>,
    ineffective_streak: u32,
    base_ms: u64,
    max_ms: u64,
) -> bool {
    match last_repair_age_ms {
        None => true,
        Some(age) => age >= forced_repair_backoff_ms(ineffective_streak, base_ms, max_ms),
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
        // PROBLEM 236 — the mouse counter's baseline, seeded and advanced in
        // exact lockstep with the reference one for the same reason: four
        // numbers printed on one line must describe ONE window, or the line
        // recreates the cross-window subtraction it was added to abolish.
        static LAST_MS_COUNT: AtomicU32 = AtomicU32::new(0);
        if last == 0 {
            LAST_SEEN_REPORT.store(now, Ordering::Relaxed);
            KB_EVENTS_SEEN.store(0, Ordering::Relaxed);
            KB_EVENTS_OWN_FG.store(0, Ordering::Relaxed);
            KB_EVENTS_INJECTED.store(0, Ordering::Relaxed);
            LAST_REF_COUNT.store(REF_KB_EVENTS.load(Ordering::Relaxed), Ordering::Relaxed);
            LAST_MS_COUNT.store(MS_EVENTS.load(Ordering::Relaxed), Ordering::Relaxed);
        } else if now.saturating_sub(last) >= 60_000 {
            let seen = KB_EVENTS_SEEN.swap(0, Ordering::Relaxed);
            // Swapped ADJACENTLY to `seen`, so the two describe the same
            // window to within one callback. `saturating_sub` below absorbs
            // the one event that can land between the two swaps.
            let injected = KB_EVENTS_INJECTED.swap(0, Ordering::Relaxed);
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
            // ═══ PROBLEM 236 — THE ONE LINE, PRINTED ONCE PER WINDOW ═══
            //
            // Four callback-only counters for the SAME 60-second window, so
            // "the primary sees keys but the witness is silent" stops being
            // something a reader has to assemble out of three lines minutes
            // apart. See `format_liveness_split` for how to read it.
            //
            // Printed whenever ANYTHING moved — a window in which the mouse
            // hook fired and nothing else did is exactly the window a
            // `both_dead` alarm gets raised in, so suppressing it would hide
            // the case this line exists for. A genuinely idle window (all four
            // zero) still prints nothing.
            let ms_now = MS_EVENTS.load(Ordering::Relaxed);
            let ms_events = ms_now.wrapping_sub(LAST_MS_COUNT.swap(ms_now, Ordering::Relaxed));
            let real = seen.saturating_sub(injected);
            // PROBLEM 268 — the raw-input keyboard clock beside the four hook
            // counters: raw keystrokes above 0 with primary_real 0 is the
            // deaf signature, measured rather than inferred.
            let raw_now = RAW_KB_EVENTS.load(Ordering::Relaxed);
            let raw_events = raw_now.wrapping_sub(LAST_RAW_COUNT.swap(raw_now, Ordering::Relaxed));
            if seen > 0 || ref_events > 0 || ms_events > 0 || raw_events > 0 {
                log::info!(
                    "hook liveness split — {} raw_keyboard:{raw_events} in the last {elapsed_s}s. All four are \
                     callback-only counters (PROBLEM 236): nothing but a hook proc can move \
                     them, so this line is the whole instrument panel for one window. \
                     primary_real 0 with primary_injected above it means the keyboard is dead \
                     and 'saw N key event(s)' was counting Spaceadom typing to itself; \
                     primary_real above 0 with reference 0 means the witness hook is being \
                     evicted again (PROBLEM 230 — check install order in install_hooks); \
                     mouse alone above 0 means the hook thread's pump is fine, so any \
                     WATCHDOG alarm in this window was measuring silence, not death.",
                    format_liveness_split(real, injected, ref_events, ms_events)
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
    // PROBLEM 257 — a non-zero value here is a keystroke the OS accepted that no
    // keyboard hook in this process saw. The WARN line that raised it names the
    // foreground window; this is the count.
    let od = OWN_DEAF_REHOOKS.swap(0, Ordering::Relaxed);
    // PROBLEM 261 — the fallback's own stranded-hold count. Separate from
    // `sr` on purpose: they are two different reapers watching two different
    // witnesses, and one number covering both would make "which path stranded
    // it?" unanswerable from the log.
    let fr = drain_own_holds_reaped();
    // PROBLEM 262 — the three new bounds, each with its own number. `dh` is the
    // deafness-aware reap (item 1), `lb` the last-resort latch bound (item 4)
    // and `rt` holds torn down by a repair (item 3). A non-zero `lb` in
    // particular is a bug report, not health: it means every earlier bound
    // missed and the ring was un-raisable until it fired.
    let dh = DEAF_HOLDS_REAPED.swap(0, Ordering::Relaxed);
    let lb = LATCH_BOUND_CLEARS.swap(0, Ordering::Relaxed);
    let rt = REPAIR_HOLD_TEARDOWNS.swap(0, Ordering::Relaxed);
    // PROBLEM 263 — the middle-button trigger's two numbers. `mt` is ordinary
    // middle clicks replayed through SendInput (health — it is what proves a
    // quick click still works); `mr` is middle holds the reaper had to tear
    // down, which is the fingerprint of a lost WM_MBUTTONUP and should be read
    // as a bug report.
    let (mt, mr) = drain_middle_counters();
    if fs == 0 && by == 0 && ro == 0 && st == 0 && un == 0 && dr == 0 && rh == 0 && os == 0
        && dm == 0 && ex == 0 && sr == 0 && pi == 0 && od == 0 && fr == 0 && dh == 0
        && lb == 0 && rt == 0 && mt == 0 && mr == 0
    {
        return;
    }
    log::info!(
        "hook diagnostics — fullscreen-suppressed:{fs} bypass-suppressed:{by} \
         typed-not-command(rollover):{ro} stuck-modifier-resets:{st} unmapped-keys:{un} \
         dropped-events:{dr} watchdog-reinstalls:{rh} passed-to-os(ctrl/alt/win held):{os} \
         space-dropped(modifier still held on release):{dm} excluded-app:{ex} \
         stale-holds-reaped(lost Space-UP):{sr} keyboard-deaf-rehooks(Space down, no callback):{od}          own-window-holds-reaped(page stopped talking mid-hold):{fr} \
         deaf-holds-reaped(hook not called at all):{dh} \
         modifier-latch-bound-clears(latched past {MAX_MODIFIER_HOLD_MS}ms with no callbacks):{lb} \
         repair-hold-teardowns(hold predated a re-hook):{rt} \
         middle-clicks-replayed(quick press, put back through SendInput):{mt} \
         middle-holds-reaped(lost WM_MBUTTONUP):{mr}"
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

// --- PROBLEM 262 — THE REAPER MUST WORK WHEN NO CALLBACK IS ARRIVING AT ALL --
//
// THE FAILURE THIS EXISTS FOR, measured on installed 1.0.106 (2026-09-07):
//
//   11:37:33.240  hold start (hold #11)                   <- a HOOK hold
//   11:37:34.340  engine: combo Space+RightAlt received   <- SPACE_COMBO_SEEN := true,
//                                                            and the LAST keyboard
//                                                            callback of the session
//   ... nothing ... for 166 seconds ...
//   11:38:33.351  WATCHDOG alarm confirmed, but a Space hold is LIVE ... protected: 1
//   11:38:41.352  ... protected: 2     11:39:08.352 ... 3     11:39:30.352 ... 4
//
// The keyboard hook was evicted one callback after the combo. The Space-UP was
// therefore never delivered, `MODIFIER_ACTIVE` stayed latched, and every
// instrument that could have ended the hold was keyed on a callback that was
// never going to run again:
//
//   * `hold_is_stale` needs Space AUTO-REPEAT, and PROBLEM 219 stands it down
//     entirely once a combo has been seen. Both halves are callback-fed.
//   * `proven_keyboard_deaf` returns `None` while `modifier_active` — so the
//     one path that BYPASSES every cooldown was itself held shut by the latch.
//   * `own_window_space_down_accepted`'s guard 2 refuses a fallback hold while
//     `MODIFIER_ACTIVE` is latched, so the page could not raise the ring
//     either. That is the owner's symptom exactly: *the ring stopped appearing
//     and only a restart cured it.*
//
// So the latch was unfalsifiable from inside the process, and it was cleared
// after 166 s only by luck — a stray keyboard callback got through and hit the
// combo branch's own 30 s `MAX_MODIFIER_HOLD_MS` bound, which is also
// callback-fed. Three bounds below close it, in the order they should fire.
//
// GENERALISE: **an instrument that can only be read by the thing that has
// failed is not an instrument.** Every liveness test for a hold must have at
// least one term that a dead hook cannot suppress.

/// PROBLEM 262 — how long the keyboard callback must have been silent before a
/// LATCHED hold may be torn down on deafness evidence alone.
///
/// 3000 ms — double `OWN_DEAF_SILENCE_MS`, and the same figure as `BLIND_MS`.
/// A hold is the one state where a false positive costs a live press (PROBLEM
/// 236's *"it dies mid-press"*), so this path is deliberately asked for twice
/// the silence the no-hold forced repair is. It is not the only term: the
/// verdict underneath it is `proven_keyboard_deaf`, which additionally requires
/// the OS to have accepted input our own mouse callback cannot account for.
const HOLD_DEAF_SILENCE_MS: u64 = 3_000;

/// The stuck-latch bound, hoisted out of `kb_hook_proc` (PROBLEM 262 item 4).
///
/// It was a `const` local to the combo branch, which meant the ONLY code that
/// could enforce it was the keyboard callback — the exact code that stops
/// running in the failure this bound is for. Same value, same meaning,
/// enforced from two places now: the callback (unchanged) and the reaper,
/// which runs on the pump and on `st-hud-pointer`.
///
/// Generous on purpose: people hold Space and READ the guide ring.
pub(crate) const MAX_MODIFIER_HOLD_MS: u64 = 30_000;

/// PROBLEM 262 — WHY a latched hold was torn down. Named rather than boolean so
/// the log says which instrument spoke and a test can assert the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HoldReap {
    /// PROBLEM 218's original, unchanged: the keyboard hook IS being called,
    /// Space's auto-repeat has stopped arriving, and no combo has stood the
    /// test down.
    AutoRepeatStopped,
    /// PROBLEM 262: the hook is not being called at all, so no callback-fed
    /// instrument can ever report on this hold again.
    KeyboardDeaf,
    /// PROBLEM 262 item 4 — belt and braces. The latch has outlived
    /// `MAX_MODIFIER_HOLD_MS` and the callback has been silent for that whole
    /// span, so nothing in the process is going to end it.
    LatchedPastBound,
}

/// PROBLEM 262 — may a latched hold be torn down because the KEYBOARD IS DEAF?
///
/// This is the answer to PROBLEM 219's one real cost. 219 stood the reaper down
/// after a combo for a correct reason: Windows auto-repeats only the
/// most-recently-pressed key, so after Space+Tab the Space never repeats again
/// and "no repeats" stops being evidence of anything. **That exemption is for a
/// hold that is still being FED — one where the callback is alive and simply
/// has nothing to say about Space.** It was never meant to protect a hold whose
/// hook has stopped being called, because for that hold the exemption is
/// permanent: there is no future event that could ever lift it.
///
/// So the two are separated by the one question 219 could not ask: *is the
/// callback running at all?* `proven_deaf` is `proven_keyboard_deaf`'s verdict
/// — the OS accepted input that our own mouse callback cannot account for while
/// our keyboard callback stayed silent — and `kb_callback_silence_ms` is the
/// callback-only clock, `None` when the hook has not fired since the last
/// install (UNKNOWN, never proof — PROBLEM 228).
///
/// A combo-seen hold that is STILL receiving auto-repeat has a small
/// `kb_callback_silence_ms` and no proven-deaf verdict, so it stays protected
/// exactly as PROBLEM 219 requires.
pub(crate) fn hold_is_deaf_stale(
    modifier_active: bool,
    proven_deaf: bool,
    kb_callback_silence_ms: Option<u64>,
    min_silence_ms: u64,
) -> bool {
    if !modifier_active || !proven_deaf {
        return false;
    }
    matches!(kb_callback_silence_ms, Some(ms) if ms >= min_silence_ms)
}

/// PROBLEM 262 item 4 — the last-resort bound, and the one that needs no
/// verdict at all.
///
/// `MODIFIER_ACTIVE` has been latched longer than `max_hold_ms` AND the
/// keyboard callback has been silent for longer than `max_hold_ms` — i.e. there
/// were no keyboard callbacks in that whole span. Nothing about that shape is
/// recoverable: the callback is the only writer of the Space-UP that would end
/// the hold, and it has not run.
///
/// `None` (never fired since the last install) is deliberately NOT accepted
/// here, for PROBLEM 228's reason: a hook that has never been called cannot be
/// measured. That case belongs to item 3 instead — a repair tears down any hold
/// that predates it, so a latch cannot survive an install either way.
pub(crate) fn modifier_latched_past_bound(
    modifier_active: bool,
    hold_age_ms: u64,
    kb_callback_silence_ms: Option<u64>,
    max_hold_ms: u64,
) -> bool {
    modifier_active
        && hold_age_ms > max_hold_ms
        && matches!(kb_callback_silence_ms, Some(ms) if ms > max_hold_ms)
}

/// PROBLEM 262 — the whole reaping decision, pure, in the order the bounds
/// should fire: cheapest and most specific first, last resort last.
#[allow(clippy::too_many_arguments)]
pub(crate) fn hold_reap_reason(
    modifier_active: bool,
    repeats: u32,
    since_last_tick_ms: u64,
    grace_ms: u64,
    combo_seen: bool,
    proven_deaf: bool,
    kb_callback_silence_ms: Option<u64>,
    deaf_silence_ms: u64,
    hold_age_ms: u64,
    max_hold_ms: u64,
) -> Option<HoldReap> {
    if hold_is_stale(modifier_active, repeats, since_last_tick_ms, grace_ms, combo_seen) {
        return Some(HoldReap::AutoRepeatStopped);
    }
    if hold_is_deaf_stale(modifier_active, proven_deaf, kb_callback_silence_ms, deaf_silence_ms) {
        return Some(HoldReap::KeyboardDeaf);
    }
    if modifier_latched_past_bound(modifier_active, hold_age_ms, kb_callback_silence_ms, max_hold_ms)
    {
        return Some(HoldReap::LatchedPastBound);
    }
    None
}

/// The deafness evidence the reaper needs, gathered OFF the hook callback.
///
/// Returns `(proven_deaf, kb_callback_silence_ms)`. Every clock here is scoped
/// to the current install and callback-only on the side that matters, exactly
/// as `watchdog_check`'s own proven-deaf block scopes them — the two must never
/// disagree about what "the keyboard callback is silent" means.
///
/// `modifier_active` is passed as `false` ON PURPOSE, and it is the only
/// deliberate divergence. `proven_keyboard_deaf` suppresses itself while a hold
/// is latched because ITS caller performs a destructive re-hook, and landing
/// that on a live hold is PROBLEM 236. Here the question is the opposite one —
/// *is this latch itself a lie?* — so the latch may not be the thing that
/// answers it. Feeding it back in is precisely how the 1.0.106 episode wedged.
#[cfg(windows)]
fn deaf_evidence_for_reap() -> (bool, Option<u64>) {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    let t = tick_count();
    let installed_at = HOOKS_INSTALLED_AT.load(Ordering::Relaxed);
    let since_install = t.saturating_sub(installed_at);
    let scoped = |cb: u64| (cb != 0 && cb >= installed_at).then(|| t.saturating_sub(cb));
    let kb_cb_silence = scoped(LAST_KB_CALLBACK.load(Ordering::Relaxed));
    // Legal on this thread and forbidden in the callback (keyboard law 3).
    let space_down = unsafe { (GetAsyncKeyState(VK_SPACE as i32) as u16 & 0x8000) != 0 };
    let stand_down = FULLSCREEN_ACTIVE.load(Ordering::Relaxed)
        || EXCLUDED_ACTIVE.load(Ordering::Relaxed)
        || BYPASS_MODE.load(Ordering::Relaxed);
    let proven = proven_keyboard_deaf(
        space_down,
        false,
        stand_down,
        other_modifier_down(),
        kb_cb_silence,
        raw_kb_age_ms(),
        millis_since_last_input(),
        since_install,
        HOLD_DEAF_SILENCE_MS,
        INSTALL_GRACE_MS,
        FORCED_INPUT_MAX_AGE_MS,
        FORCED_MOUSE_ATTRIBUTION_MS,
    )
    .is_some();
    (proven, kb_cb_silence)
}

#[cfg(not(windows))]
fn deaf_evidence_for_reap() -> (bool, Option<u64>) {
    (false, None)
}

/// How often the reaper may spend Win32 calls on the deafness evidence.
///
/// `st-hud-pointer` polls at 62 Hz while a hold is latched (`TICK_HELD_MS`),
/// and the bounds underneath this are 3 s and 30 s: four times a second is
/// ample and 62 is waste. Same figure and same reasoning as
/// `OWN_HOLD_FG_CHECK_MS`, which throttles the fallback reaper's foreground
/// probe for exactly this reason. PROBLEM 218's auto-repeat test is unthrottled
/// and still evaluated on every tick — it costs two relaxed loads.
const HOLD_DEAF_CHECK_MS: u64 = 250;
/// Tick of the last `deaf_evidence_for_reap()`. Raced by the pointer thread and
/// the pump; the worst outcome of losing that race is one skipped probe 250 ms
/// before the next.
static LAST_DEAF_PROBE: AtomicU64 = AtomicU64::new(0);

/// PROBLEM 262 — how many holds each bound took down. All three are DRAINED
/// into the 60 s `hook diagnostics` line, beside `STALE_HOLDS_REAPED`, and kept
/// apart on purpose: one number covering all of them would make "which
/// instrument had to save us?" unanswerable from the log, which is the question
/// the next session will need.
static DEAF_HOLDS_REAPED: AtomicU32 = AtomicU32::new(0);
/// Item 4's counter. A non-zero value here means every earlier bound missed and
/// the last-resort one fired — read it as a bug report, not as health.
static LATCH_BOUND_CLEARS: AtomicU32 = AtomicU32::new(0);
/// Item 3's counter: holds that were latched when a repair replaced the hook.
static REPAIR_HOLD_TEARDOWNS: AtomicU32 = AtomicU32::new(0);

/// PROBLEM 262 item 3 — a hold that predates a repair must not survive it.
///
/// A repair is an unhook plus a fresh `SetWindowsHookExW` (keyboard law 7). The
/// Space-UP of any hold that was latched when that happened belongs to the hook
/// that is now gone, so it can never be delivered and the latch is
/// **unfalsifiable** from that instant on. Left standing it is PROBLEM 218's
/// stranded ring with no reaper able to see it — and, since PROBLEM 259, it is
/// worse than a stuck picture: guard 2 of `own_window_space_down_accepted`
/// refuses a fallback hold while `MODIFIER_ACTIVE` is latched, so no NEW ring
/// can be raised either. That is the owner's 2026-09-07 report exactly.
///
/// The ordinary re-hook path in `watchdog_check` has always done this inline
/// and its log lines are unchanged. This is the same set for the PROVEN-deaf
/// forced repair, which had none of it: `proven_keyboard_deaf` returns `None`
/// while a hook hold is latched, so the hook-hold case looked impossible —
/// except a FALLBACK hold could be live there even then, and since item 1 the
/// reaper clears the hook latch on the same tick, which makes the ordering
/// something to state rather than to assume.
fn tear_down_hold_across_repair(what: &str) {
    let had_hook_hold = MODIFIER_ACTIVE.swap(false, Ordering::SeqCst);
    SPACE_INTERCEPTED.store(false, Ordering::Relaxed);
    SPACE_ABORTED.store(false, Ordering::Relaxed);
    // PROBLEM 219 — combo evidence may not survive into the next hold and leave
    // the reaper standing down for a hold that never pressed a key.
    SPACE_COMBO_SEEN.store(false, Ordering::Relaxed);
    SPACE_REPEATS.store(0, Ordering::Relaxed);
    if had_hook_hold {
        // PROBLEM 206 — an armed chip or a half-eaten click must not outlive
        // the hold that created it.
        pointer::reset_on_eviction();
    }
    // Does its own pointer reset, but ONLY if it owned a hold (see there).
    let had_own_hold = disarm_own_window_hold();
    // PROBLEM 263 — and the THIRD witness, on the identical argument. A middle
    // hold's `WM_MBUTTONUP` is delivered by the MOUSE hook, and `install_hooks`
    // replaces that one too, so a hold latched a moment ago belongs to a hook
    // that no longer exists and its release can never arrive. Left standing it
    // is a ring nothing can hide AND — because THE ARBITRATION's rules B and C
    // both refuse a Space hold while `MIDDLE_HOLD_ACTIVE` is set — no new ring
    // could be raised by any trigger. That is PROBLEM 262's wedge, reachable
    // from a third direction.
    let had_middle_hold = disarm_middle_hold();
    if !had_hook_hold && !had_own_hold && !had_middle_hold {
        return;
    }
    REPAIR_HOLD_TEARDOWNS.fetch_add(1, Ordering::Relaxed);
    let hud_was_up = crate::guide_hud::is_visible();
    log::warn!(
        "hook: repair-tore-down-a-hold-that-predated-it-spaceadom — {what} replaced the hook \
         chain while a hold was still latched (hook hold: {had_hook_hold}, own-window \
         fallback hold: {had_own_hold}, middle-button hold: {had_middle_hold}; HUD was up: \
         {hud_was_up}). That hold's release belongs \
         to a hook that no longer exists, so it can never arrive and the latch is unfalsifiable \
         from here on. Tearing it down: pointer latches reset, ring hidden. Left standing it is \
         a ring nothing can hide AND — because guard 2 of the own-window fallback refuses a new \
         hold while MODIFIER_ACTIVE is latched — no new ring can ever be raised, which is the \
         owner's 'the ring stopped appearing entirely and only a restart cured it'. PROBLEM 262 \
         item 3."
    );
    crate::guide_hud::hide_guide_hud();
}

/// Tear down a Space-hold that is over, by whichever of the three bounds can
/// see it (PROBLEM 218's auto-repeat, PROBLEM 262's deafness, PROBLEM 262's
/// last-resort latch bound).
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
///
/// ORDER MATTERS AT THE CALL SITE. This runs at the very TOP of
/// `watchdog_check`, above every early return, and that is now load-bearing in
/// a second way: clearing `MODIFIER_ACTIVE` here is what un-suppresses
/// `proven_keyboard_deaf` further down the same tick, so a genuinely deaf hook
/// is reaped and then repaired in one pass instead of neither.
pub fn reap_stale_hold() -> bool {
    let modifier_active = MODIFIER_ACTIVE.load(Ordering::Relaxed);
    // Cheapest possible exit, and it must stay first: with no hold latched
    // there is nothing to prove and no Win32 call worth making. This runs at
    // 4 Hz from the pointer thread and 1 Hz from the pump.
    if !modifier_active {
        return false;
    }
    let repeats = SPACE_REPEATS.load(Ordering::Relaxed);
    let now = tick_count();
    let since = now.saturating_sub(SPACE_TICK_TS.load(Ordering::Relaxed));
    let hold_age = now.saturating_sub(SPACE_DOWN_TS.load(Ordering::Relaxed));
    // Throttled: PROBLEM 218's auto-repeat test below is free and runs on every
    // tick; the deafness evidence costs two Win32 calls and is only useful at
    // the resolution of a 3 s bound. `(false, None)` on the ticks in between is
    // exactly the behaviour this function had before PROBLEM 262.
    let (proven_deaf, kb_cb_silence) =
        if now.saturating_sub(LAST_DEAF_PROBE.load(Ordering::Relaxed)) >= HOLD_DEAF_CHECK_MS {
            LAST_DEAF_PROBE.store(now, Ordering::Relaxed);
            deaf_evidence_for_reap()
        } else {
            (false, None)
        };
    let combo_seen = SPACE_COMBO_SEEN.load(Ordering::Relaxed);
    let Some(reason) = hold_reap_reason(
        modifier_active,
        repeats,
        since,
        STALE_HOLD_GRACE_MS,
        combo_seen,
        proven_deaf,
        kb_cb_silence,
        HOLD_DEAF_SILENCE_MS,
        hold_age,
        MAX_MODIFIER_HOLD_MS,
    ) else {
        return false;
    };
    // Claim it before doing anything else: both callers race each other, and
    // a double teardown would emit two `guide-hud-hide` events.
    if !MODIFIER_ACTIVE.swap(false, Ordering::SeqCst) {
        return false;
    }
    SPACE_REPEATS.store(0, Ordering::Relaxed);
    SPACE_COMBO_SEEN.store(false, Ordering::Relaxed);
    let hud_was_up = crate::guide_hud::is_visible();
    let kb_desc = match kb_cb_silence {
        Some(ms) => format!("{ms}ms ago"),
        None => "NEVER since the last install".to_string(),
    };
    match reason {
        HoldReap::AutoRepeatStopped => {
            STALE_HOLDS_REAPED.fetch_add(1, Ordering::Relaxed);
            log::warn!(
                "hook: a Space-hold has been latched for {since}ms with no auto-repeat after \
                 {repeats} of them — the Space-UP was never delivered, so this hold is over and \
                 nothing else was going to end it. Reaping it (HUD was up: {hud_was_up}). Left \
                 standing this is a ring on screen that no later hide can reach, with the pointer \
                 still arming chips behind it (PROBLEM 218)."
            );
        }
        HoldReap::KeyboardDeaf => {
            DEAF_HOLDS_REAPED.fetch_add(1, Ordering::Relaxed);
            log::warn!(
                "hook: stale-hold-reaped-because-the-keyboard-is-proven-deaf-spaceadom — a \
                 Space-hold has been latched {hold_age}ms and the keyboard callback last ran \
                 {kb_desc} (threshold {HOLD_DEAF_SILENCE_MS}ms) while the OS accepted input our \
                 own mouse callback cannot account for. So the hook is not being called at all, \
                 and NOTHING on the callback was ever going to end this hold: its auto-repeat \
                 cannot arrive from a hook nobody calls, and PROBLEM 219's combo stand-down \
                 (combo seen: {combo_seen}) would have protected it forever. Reaping it (HUD was \
                 up: {hud_was_up}). PROBLEM 219's exemption is KEPT for the case it was written \
                 for — a hold still being FED by auto-repeat, where the callback is alive and \
                 simply has nothing to say about Space; that hold has a small callback silence \
                 and no proven-deaf verdict, so it is untouched by this branch. PROBLEM 262."
            );
        }
        HoldReap::LatchedPastBound => {
            LATCH_BOUND_CLEARS.fetch_add(1, Ordering::Relaxed);
            log::warn!(
                "hook: modifier-active-latched-past-the-bound-with-no-keyboard-callbacks-spaceadom \
                 — MODIFIER_ACTIVE has been latched {hold_age}ms (bound {MAX_MODIFIER_HOLD_MS}ms) \
                 and the keyboard callback last ran {kb_desc}, so there were NO keyboard callbacks \
                 in that whole span. Clearing it (HUD was up: {hud_was_up}). This is the \
                 last-resort bound: every earlier one missed, which means the deafness verdict did \
                 not fire either (the user may simply not have touched anything), and until \
                 1.0.107 the only code that enforced this 30s bound was the keyboard callback \
                 itself — the exact code that had stopped running. If this line is in the log, \
                 read it as a bug report and not as health: with MODIFIER_ACTIVE latched, guard 2 \
                 of the own-window fallback refuses every new hold, so the ring cannot appear at \
                 all until this fires. PROBLEM 262 item 4."
            );
        }
    }
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

/// PROBLEM 236 — GetTickCount64 ms of the last `install_hooks()`, for ALL
/// three hooks (they are installed in one call, so it is one instant).
///
/// `REF_HOOK_INSTALLED_AT` already records the same number, but under a name
/// that says "the reference hook", and this value is now load-bearing for the
/// keyboard and mouse decisions too. Two names for one instant is cheaper than
/// a reader assuming the reference's install time also bounds the primary's.
///
/// It is the ONLY non-callback input to `classify_callback_liveness`, and it
/// can only ever move the verdict towards NO alarm (see that function).
static HOOKS_INSTALLED_AT: AtomicU64 = AtomicU64::new(0);

/// THE RAW-INPUT KEYBOARD CLOCK (PROBLEM 268, 2026-09-17). `GetTickCount` at
/// the last `WM_INPUT` the hook thread's sink window received for a KEYBOARD
/// device (`RegisterRawInputDevices`, usage page 1 / usage 6,
/// `RIDEV_INPUTSINK`). Raw input owes nothing to the hook chain: it is
/// delivered by the input stack to every registered window whatever the
/// low-level hooks did, so it is the one clock that can say "the OS delivered
/// a KEYSTROKE" — where `GetLastInputInfo` can only say "the OS delivered
/// something", and on a laptop that something is a touchpad gesture the
/// mouse hook never sees (precision-touchpad panning arrives as pointer
/// input). Stamped ONLY from `raw_sink_wndproc`, exactly like the two
/// callback clocks; zero until the first keystroke after launch.
static LAST_RAW_KB_EVENT: AtomicU64 = AtomicU64::new(0);
/// How many keyboard `WM_INPUT`s the sink has received since launch — a
/// counter for the same reason `REF_KB_EVENTS` is one.
static RAW_KB_EVENTS: AtomicU64 = AtomicU64::new(0);
/// `RAW_KB_EVENTS` as of the last 60-second liveness line (its window base).
static LAST_RAW_COUNT: AtomicU64 = AtomicU64::new(0);
/// The sink window exists and the registration succeeded. False means the
/// proven-deaf verdict has only proof A (a physically held Space) to go on.
static RAW_SINK_READY: AtomicBool = AtomicBool::new(false);

/// Age of the last raw keyboard event, or `None` before the first one (and
/// on a machine where the sink could not be registered).
fn raw_kb_age_ms() -> Option<u64> {
    let last = LAST_RAW_KB_EVENT.load(Ordering::Relaxed);
    (last != 0).then(|| tick_count().saturating_sub(last))
}

/// PROBLEM 268 — the sink window on the hook thread. A message-only window
/// (`HWND_MESSAGE`) whose only job is to receive `WM_INPUT` for keyboard
/// devices and stamp `LAST_RAW_KB_EVENT`; the thread's `GetMessageW` loop
/// already pumps it. `RIDEV_INPUTSINK` delivers even when this process is
/// not in the foreground. Registration is per process, so a thread restart
/// that creates a fresh sink simply re-points it; the old window dies with
/// its thread.
#[cfg(windows)]
unsafe fn register_raw_keyboard_sink() {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Input::{
        RegisterRawInputDevices, RAWINPUTDEVICE, RIDEV_INPUTSINK,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, RegisterClassW, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
        WNDCLASSW,
    };
    const CLASS: PCWSTR = w!("SpaceadomRawKbSink");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(raw_sink_wndproc),
        lpszClassName: CLASS,
        ..Default::default()
    };
    // A second registration of the same class name fails harmlessly (thread
    // restart); the class from the first one is still there.
    let _ = RegisterClassW(&wc);
    let hwnd = match CreateWindowExW(
        WINDOW_EX_STYLE(0),
        CLASS,
        w!("spaceadom-raw-kb-sink"),
        WINDOW_STYLE(0),
        0,
        0,
        0,
        0,
        HWND_MESSAGE,
        None,
        None,
        None,
    ) {
        Ok(h) if !h.is_invalid() => h,
        other => {
            RAW_SINK_READY.store(false, Ordering::Relaxed);
            log::warn!(
                "hook: raw-input keyboard sink window could not be created ({other:?}) — the \
                 proven-deaf verdict has only a physically held Space to go on this session \
                 (PROBLEM 268)"
            );
            return;
        }
    };
    let rid = [RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: 0x06,
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    }];
    match RegisterRawInputDevices(&rid, std::mem::size_of::<RAWINPUTDEVICE>() as u32) {
        Ok(()) => {
            RAW_SINK_READY.store(true, Ordering::Relaxed);
            log::info!(
                "hook: raw-input keyboard sink registered on the hook thread (hwnd {:#x}) — \
                 every keystroke the OS delivers now stamps a clock that owes nothing to the \
                 hook chain, and the proven-deaf verdict reads THAT instead of guessing from \
                 GetLastInputInfo (PROBLEM 268)",
                hwnd.0 as usize
            );
        }
        Err(e) => {
            RAW_SINK_READY.store(false, Ordering::Relaxed);
            log::warn!(
                "hook: RegisterRawInputDevices(keyboard, INPUTSINK) failed ({e}) — the \
                 proven-deaf verdict has only a physically held Space to go on this session \
                 (PROBLEM 268)"
            );
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn raw_sink_wndproc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, WM_INPUT};
    if msg == WM_INPUT {
        // Only keyboard devices are registered, so every WM_INPUT here IS a
        // keystroke (down or up; injected ones included, exactly as the LL
        // callback sees them). Nothing is parsed — the clock is the point.
        LAST_RAW_KB_EVENT.store(tick_count(), Ordering::Relaxed);
        RAW_KB_EVENTS.fetch_add(1, Ordering::Relaxed);
    }
    // WM_INPUT must reach DefWindowProc so the system can free the buffer.
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// PROBLEM 236 — tick at which the current alarm's repair was first deferred
/// for a live Space hold, or 0 when nothing is being deferred.
///
/// The bound (`MAX_HOLD_DEFER_MS`) is measured from THIS, not from the hold's
/// start: the question is how long the REPAIR has been waiting, not how long
/// the owner has been holding Space.
static ALARM_DEFERRED_AT: AtomicU64 = AtomicU64::new(0);
/// Whether the current deferral episode has already printed its line. "Log the
/// deferral once" — the tick is 1 s and a deferral can last 10 s, so without
/// this it would be ten identical lines.
static ALARM_DEFER_LOGGED: AtomicBool = AtomicBool::new(false);
/// How many live holds this build has protected from a watchdog teardown.
/// Printed in the deferral line itself so the number is never separated from
/// the sentence that explains it.
static HOLDS_PROTECTED: AtomicU32 = AtomicU32::new(0);

/// PROBLEM 236 — tick of the last "would have alarmed" line, and how many
/// watchdog ticks the current episode has suppressed.
///
/// The OLD rule (`both_dead || kb_only_dead` off the seeded clocks) is still
/// evaluated in full, and every time it fires while the new rule declines, a
/// line is printed saying so. That is what keeps the fortnight of data the
/// PROBLEM 236 entry asked for accruing — the behaviour changed, the
/// measurement did not.
///
/// Throttled to the rising edge of an episode plus one line a minute while it
/// persists, with the suppressed tick count in the line. `grep -c "would have
/// alarmed"` therefore counts EPISODES, not ticks; the old rule re-hooked and
/// re-stamped its own clocks, so it could not have produced one line per tick
/// either.
static WOULD_HAVE_ALARMED_AT: AtomicU64 = AtomicU64::new(0);
static WOULD_HAVE_ALARMED_TICKS: AtomicU32 = AtomicU32::new(0);
/// Same for the MOUSE hook. Kept separate: the two hooks are evicted
/// independently, and our keyboard callback is the heavy one — a dead
/// keyboard hook with a live mouse hook is the realistic failure.
///
/// **SEEDED, exactly like `LAST_KB_EVENT`** — `install_hooks()` and the
/// watchdog's idle early-return both write it. Right for the ALARM, wrong for
/// any sentence claiming the mouse hook fired. Use `LAST_MS_CALLBACK`.
static LAST_MS_EVENT: AtomicU64 = AtomicU64::new(0);
/// PROBLEM 236 — the mouse half of PROBLEM 228's fix, which was never applied.
///
/// `both_dead` — the branch that produced **16 of 16 watchdog alarms** in the
/// owner's 1.0.96 session (38 min, 2026-09-04 16:28→17:06) — is
/// `kb_silence > BLIND_MS && ms_silence > BLIND_MS`, and BOTH of those are
/// seeded clocks. The alarm line says so in its own text ("both are re-stamped
/// clocks") and then calls the result "NEITHER hook saw anything". It cannot
/// know that. Measured in that session, the ten alarms whose mouse clock was
/// NOT a round re-stamp value read
/// 3172, 3172, 3218, 3234, 3297, 3313, 3328, 3437, 3875, 5250 ms —
/// eight of the ten within 437 ms of the 3000 ms trip line, which is the shape
/// of a threshold being crossed by an ordinary pause, not of an eviction (an
/// evicted hook's clock keeps growing; these do not).
///
/// Stamped ONLY from inside `ms_hook_proc`. Nothing else may ever write it —
/// that is the whole value, and it is the same law `LAST_KB_CALLBACK` follows.
static LAST_MS_CALLBACK: AtomicU64 = AtomicU64::new(0);
/// How many times the MOUSE callback has actually been called since launch.
/// A counter for the same reason `REF_KB_EVENTS` is one: a clock can be
/// forged by anything holding a `store`, a callback-only counter cannot.
static MS_EVENTS: AtomicU32 = AtomicU32::new(0);
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

/// PROBLEM 257 — how many times the watchdog re-hooked because Space was
/// PHYSICALLY DOWN (per `GetAsyncKeyState`, read on the watchdog thread where
/// it is legal) while our keyboard callback had not fired for
/// `OWN_DEAF_SILENCE_MS`. Drained into the 60s `hook diagnostics` line as
/// `keyboard-deaf-rehooks`. Non-zero is the fingerprint of the 2026-09-06
/// regression: a keystroke the OS accepted (its key state changed) that never
/// reached ANY `WH_KEYBOARD_LL` hook in this process.
static OWN_DEAF_REHOOKS: AtomicU32 = AtomicU32::new(0);
/// PROBLEM 257 — tick of the last deafness re-hook.
///
/// PROBLEM 260 — **it no longer gates anything.** `LAST_FORCED_REPAIR` and
/// `forced_repair_allowed` are the rate limit now. This is kept, and still
/// stamped by the forced repair, purely as the historical name for "when did a
/// deafness repair last happen" — do not build a new gate on it, and do not
/// read its presence as evidence that it decides something.
static LAST_OWN_DEAF_REHOOK: AtomicU64 = AtomicU64::new(0);
/// PROBLEM 257 — how long the keyboard callback may be silent while Space is
/// physically held before that counts as proof. Windows' LONGEST auto-repeat
/// delay is 1000 ms, so a working hook holding Space sees a repeat inside this
/// window; 1500 ms leaves margin without waiting a whole hold.
const OWN_DEAF_SILENCE_MS: u64 = 1_500;
/// PROBLEM 257 — floor between two deafness re-hooks.
///
/// PROBLEM 260 — SUPERSEDED as the throttle on the proven-deaf path. A 15 s
/// floor is not itself a suppression bug (it never blocked a repair in the
/// measured log), but it is coarser than it needs to be and it counted only the
/// Space-held instance of the verdict. `FORCED_REPAIR_BASE_MS` below is the
/// floor now, with a backoff behind it; this constant is kept only so the
/// PROBLEM 257 entry and this file still agree about what 1.0.103 did.
#[allow(dead_code)]
const OWN_DEAF_REHOOK_COOLDOWN_MS: u64 = 15_000;

// ═══ PROBLEM 260 — THE FORCED REPAIR'S OWN BOOKKEEPING ═══════════════════
//
// Separate from every counter above, for one reason: `OWN_DEAF_REHOOKS` is
// DRAINED by `drain_hook_diagnostics` (`swap(0)`) every 60 s, and the
// PROBLEM 257 log line computed its `repair #N this session` from it. So the
// owner's 2026-09-06/07 logs printed **"repair #1 this session" three separate
// times**, and the obvious reading — "the counter never advanced, so that path
// never really reinstalled" — was wrong. The number was right; the word
// "session" was the lie. A counter that is drained cannot also be a session
// total, and a log line that says "session" must read one that is never drained.

/// PROBLEM 260 — forced repairs since process start. **NEVER DRAINED.** This is
/// the number the log means when it says "this session".
static FORCED_REPAIRS_TOTAL: AtomicU32 = AtomicU32::new(0);
/// PROBLEM 260 — `tick_count()` of the last forced repair, 0 for "none yet".
/// The rate limit and the effectiveness test both read it.
static LAST_FORCED_REPAIR: AtomicU64 = AtomicU64::new(0);
/// PROBLEM 260 — consecutive forced repairs after which the keyboard callback
/// still had not fired. Drives `forced_repair_backoff_ms`; reset to 0 the
/// moment a repair is followed by a genuine keyboard callback.
static FORCED_REPAIR_INEFFECTIVE: AtomicU32 = AtomicU32::new(0);
/// PROBLEM 260 — which backoff window the "holding off" line has already been
/// printed for, so one backoff produces one line instead of one per tick.
static FORCED_BACKOFF_LOGGED_FOR: AtomicU64 = AtomicU64::new(0);

/// PROBLEM 260 — floor between two forced repairs when the last one WORKED.
///
/// Five seconds, matching the blind-retry floor the other path already uses, so
/// the two cadences cannot fight. It is short because a proven-deaf verdict is
/// the app being dead at the one job it exists for: at a 1 s tick the repair
/// lands within a second of the proof, and 5 s bounds the worst case at twelve
/// repairs a minute rather than sixty.
const FORCED_REPAIR_BASE_MS: u64 = 5_000;
/// PROBLEM 260 — the cap on the backoff. Sixty seconds is the old fixed
/// cooldown: an environment where repairs keep failing is exactly the case that
/// cooldown was written for, so the backoff decays INTO it rather than past it.
const FORCED_REPAIR_MAX_MS: u64 = 60_000;
/// PROBLEM 260 — how fresh the OS input clock must be for `InputUnaccountedFor`.
///
/// `watchdog_check` has already returned early above `2000`, so this is the same
/// bound stated where the decision can see it rather than left implicit in a
/// caller — the pure function must be testable without the caller.
const FORCED_INPUT_MAX_AGE_MS: u64 = 2_000;
/// PROBLEM 260 — how much later than the OS's input stamp our mouse callback
/// may be and still be credited with having caused it.
///
/// 250 ms. `GetLastInputInfo` is stamped by the OS at the moment the event is
/// queued and `LAST_MS_CALLBACK` at the moment our callback runs, so the true
/// gap is single-digit milliseconds; the rest is slack for a busy pump. Widening
/// this is how the test decays back into PROBLEM 101's deleted branch, so it may
/// only ever move DOWN.
const FORCED_MOUSE_ATTRIBUTION_MS: u64 = 250;
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
/// `;` on a US layout (`VK_OEM_1`) — Space + ; is voice typing (2026-09-17).
const VK_OEM_1: u16 = 0xBA;
/// `/` on a US layout (`VK_OEM_2`) — Space + / is the screenshot (2026-09-17).
const VK_OEM_2: u16 = 0xBF;
/// `'` on a US layout (`VK_OEM_7`) — Space + ' is the on-screen keyboard (2026-09-18).
const VK_OEM_7: u16 = 0xDE;
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
/// PROBLEM 263 — the middle button, which is now a second ring trigger.
const WM_MBUTTONDOWN: u32 = 0x0207;
const WM_MBUTTONUP: u32 = 0x0208;

/// PROBLEM 263 — OUR cookie on the middle click we replay through `SendInput`.
///
/// The SAME `0x7A7A7A7A` the keyboard injections carry (keyboard law 1: filter
/// injected input ONLY by our own `dwExtraInfo` cookie, never by
/// `LLKHF_INJECTED` / `LLMHF_INJECTED` — blanket-ignoring the OS flag silently
/// disables the app for anyone using a macro mouse, an on-screen keyboard,
/// Remote Desktop or a laptop driver that stamps INJECTED onto physical input).
/// `MSLLHOOKSTRUCT::dwExtraInfo` is a `usize`, so it is declared once here
/// rather than re-typed at the two sites that use it.
const MAGIC_INJECTED_MOUSE: usize = 0x7A7A7A7A;

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
    // PROBLEM 259 — the own-window fallback needs the SAME channel, and this
    // is the only place in the process that holds it without touching lib.rs.
    // Registering it HERE (not at channel creation) also gives the fallback
    // the right lifetime for free: in SAFE MODE (PROBLEM 253) the hook thread
    // is never spawned, so no sender is registered and every fallback command
    // declines — safe mode means "Space is an ordinary space", and a webview
    // back-door into the engine would have quietly broken that promise.
    // `safe_mode`'s "Turn back on" calls this function, so the fallback comes
    // back with the hook and not before it.
    register_inject_sender(tx.clone());
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
        // PROBLEM 268 — the raw-input keyboard clock lives on this thread
        // because this thread pumps messages for as long as the hooks live.
        register_raw_keyboard_sink();

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
/// this one cannot overrun, because it does one relaxed store.
///
/// ═══ PROBLEM 230 — "CANNOT OVERRUN" WAS TRUE OF THE BODY AND FALSE OF THE
/// HOOK, FOR AS LONG AS IT SAT IN FRONT OF THE PRIMARY ═══
///
/// `CallNextHookEx` is synchronous, so a hook's measured duration includes
/// every hook BELOW it. Installed last, this one sat at the head of the chain
/// and its wall clock was `one relaxed store + the whole primary callback +
/// everything downstream` — the largest number in the chain, not the smallest.
/// It was evicted first, and the owner's 1.0.95 log has it frozen for 6¼
/// minutes at a time while the primary was demonstrably still counting keys.
/// It is now installed FIRST, so it sits at the TAIL; see `install_hooks()`
/// for why the tail costs nothing that any consumer reads. So:
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

/// Reference hook. Do NOT add anything to this function, and do NOT move it
/// back to being installed last. Its entire value is that it cannot be evicted
/// for being slow; a line added erodes that from the inside, and installing it
/// ahead of the primary erodes it from the outside by making it carry the
/// primary's duration on its own clock (PROBLEM 230).
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

    // ═══ PROBLEM 230 — INSTALL ORDER IS THE INSTRUMENT'S CORRECTNESS ═══
    //
    // `SetWindowsHookExW` puts the new hook at the HEAD of the chain, so the
    // hook installed LAST is called FIRST. The reference hook used to be
    // installed last. That put it in front of the primary — and because
    // `CallNextHookEx` is SYNCHRONOUS, the reference's own wall-clock duration
    // was its one relaxed add PLUS the entire primary callback PLUS every hook
    // downstream of us. `LowLevelHooksTimeout` is measured on that wall clock.
    //
    // So the hook documented as "cannot be evicted for being slow" was, by
    // construction, the SLOWEST hook in the chain and the first Windows drops.
    // Measured on the owner's machine, 1.0.95, 2026-09-01:
    //
    //     09:50:40  ref last genuinely fired 238843ms ago (2237 total)
    //     09:52:42  ref last genuinely fired 360859ms ago (2237 total)
    //     09:52:47  ref last genuinely fired 365859ms ago (2237 total)
    //     09:52:56  ref last genuinely fired 374859ms ago (2237 total)
    //
    // — the counter frozen for 6¼ MINUTES, across three re-hooks, while the
    // primary hook reported `saw 49 key event(s)` and `saw 13 key event(s)` for
    // windows inside that same span. The instrument was dead and the thing it
    // measures was alive: the exact inversion of what it was built to detect,
    // and the engine behind 514 watchdog alarms in one 3.8-hour session.
    //
    // INSTALLED FIRST NOW, so it lands at the TAIL, behind the primary. The
    // price of the tail is that a key the primary SUPPRESSES never reaches it —
    // and that price is zero, because of when the count is read:
    //
    //   * `classify_hook_window(primary_seen, ref_events)` consults the
    //     reference ONLY when `primary_seen == 0`, and a primary that saw
    //     nothing suppressed nothing, so everything passed down to the tail.
    //   * `kb_only_dead` has the same premise — the primary is silent.
    //   * an EVICTED primary is skipped by the system, so the tail still fires.
    //
    // The one case the tail undercounts is the case where the app is provably
    // working, which no consumer asks about. Do not "restore" the old order.
    //
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
    // PROBLEM 230 — and its install result was NEVER CHECKED. Every deafness
    // verdict this app has ever printed rests on this handle, and a failed
    // install was indistinguishable in the log from a hook that installed fine
    // and then went quiet — the two have opposite meanings and the same
    // symptom. `kb`'s failure has been logged since PROBLEM 66; this one was
    // not. Say it, at WARN, exactly once per install.
    if rf.is_invalid() {
        log::warn!(
            "hook: the REFERENCE keyboard hook failed to install — every 'DEAF' and \
             'keys ARE reaching the chain' verdict from here on is uninformed, because \
             the counter they read can no longer move. The primary hook is unaffected."
        );
    }
    // The real hooks go in AFTER the reference, so they sit in front of it.
    let kb = SetWindowsHookExW(WINDOWS_HOOK_ID(WH_KEYBOARD_LL), Some(kb_hook_proc), None, 0)
        .unwrap_or_default();
    let ms = SetWindowsHookExW(WINDOWS_HOOK_ID(WH_MOUSE_LL), Some(ms_hook_proc), None, 0)
        .unwrap_or_default();

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
    // PROBLEM 236 — the boundary the callback-only clocks are read against.
    // "Has the keyboard callback fired SINCE THE LAST INSTALL?" cannot be asked
    // of `LAST_KB_CALLBACK` alone, because that clock is deliberately never
    // reset (resetting it would make the install a writer of the instrument
    // again — PROBLEM 228). Recording the boundary separately keeps the clock
    // callback-only and still lets the decision scope it to this install.
    HOOKS_INSTALLED_AT.store(now, Ordering::Relaxed);
    // A fresh install ends any deferral that was waiting on the OLD hooks:
    // whatever hold it was protecting has just had its state torn down by the
    // caller, so there is nothing left to protect.
    ALARM_DEFERRED_AT.store(0, Ordering::Relaxed);
    ALARM_DEFER_LOGGED.store(false, Ordering::Relaxed);
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
    // PROBLEM 261 — the fallback's twin, on the same 1 s cadence and for the
    // same reason: two homes, two different failure modes. This one runs when
    // the pointer watcher failed to spawn (PROBLEM 124); the watcher's own
    // call is the fast one. `true` — the pump ticks once a second, so the
    // foreground probe here is already inside its own throttle.
    let _ = reap_own_window_hold(true);
    // PROBLEM 263 — and the middle button's, third in the row and for the third
    // time the same reason: two homes, two different failure modes. `true` for
    // the same reason as the line above — a 1 s pump tick is already inside the
    // deafness probe's own budget.
    let _ = reap_middle_hold(true);

    // PROBLEM 236 — a deferral belongs to ONE hold. The moment there is no hold
    // (Space-up, or the reaper immediately above) the episode is over, so the
    // next one starts its `MAX_HOLD_DEFER_MS` clock from zero rather than
    // inheriting a stale start and expiring instantly.
    if !MODIFIER_ACTIVE.load(Ordering::Relaxed)
        && ALARM_DEFERRED_AT.swap(0, Ordering::Relaxed) != 0
    {
        ALARM_DEFER_LOGGED.store(false, Ordering::Relaxed);
    }

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

    // ═══ PROBLEM 257 — THE PROVEN-DEAF TEST, AND THE ONE REPAIR WE OWN ═══
    //
    // 2026-09-06, installed 1.0.102, owner's hardware keys: with the dashboard
    // focused, holding Space showed no ring and launched nothing, while the
    // same keys worked over every other app. The log for 00:53:52→00:54:52
    // (our window foreground 60 of 60 samples): `mouse:2705`,
    // `primary_real:0`, `reference:0` — the mouse hook on THIS thread was
    // firing 45 times a second while neither keyboard hook fired once. That is
    // not a blocked pump, not a gate (every gate counter read 0), not
    // eviction (keys resumed the moment another window took focus, with no
    // re-hook). Keystrokes the OS accepted never reached ANY WH_KEYBOARD_LL
    // hook in this process while our own window had focus. See PROBLEM 257
    // for the bracket (last good 2026-09-05 14:11, first bad 15:35) and the
    // candidates; the mechanism is OUTSIDE this code and is not claimed here.
    //
    // What this block does is make the next occurrence PROVE ITSELF and try
    // the one repair a process owns against a keyboard hook it cannot see:
    // re-installing ours puts it back at the HEAD of the chain, ahead of
    // anything installed since. If keys arrive after the re-hook, something
    // ahead of us was swallowing them; if they still do not, the drop is
    // upstream of every hook in this process. Either way the next liveness
    // line says which — so this is an instrument first and a repair second.
    //
    // The evidence is `GetAsyncKeyState(VK_SPACE)`, legal on this thread and
    // forbidden in the callback (law 3) — see `keyboard_deaf_with_space_down`
    // for why the lie it tells about suppressed keys is what makes it honest
    // here. Placed AFTER the elevation early-return above on purpose: UIPI
    // silence is expected and a re-hook cannot cure it.
    // ═══ PROBLEM 260 — GENERALISED, AND IT BYPASSES EVERY THROTTLE BELOW ═══
    //
    // MEASURED 2026-09-07, installed 1.0.103, with an independent WH_KEYBOARD_LL
    // probe running beside the app (`_probe/ll-probe/events-run3.txt`):
    //
    //   10:10:24  the last keyboard callback of the episode
    //   10:11:49  hook liveness split — primary_real:0 primary_injected:0
    //                                   reference:0 mouse:217 in the last 60s
    //   10:12:34.246  the FIRST alarm of the episode — 130 SECONDS LATE
    //
    // Nothing suppressed a repair in those 130 seconds. **No alarm was ever
    // RAISED**, and that is a worse fault than a suppressed one, because every
    // throttle below is downstream of a candidate that never existed:
    //
    //   * `both_dead` is `kb_silence > BLIND_MS && ms_silence > BLIND_MS`. The
    //     mouse hook was firing 3-4 times a second throughout (mouse:217), so
    //     `ms_silence` never crossed 3000 ms. False on every tick.
    //   * `kb_only_dead` needs `ref_silence < BLIND_MS` — a LIVE reference hook.
    //     The reference had been silent for 129 seconds too (reference:0). False
    //     on every tick.
    //   * so the tick at 10:11:20, with the keyboard 56 s dead and the user
    //     typing, returned at `if !both_dead && !kb_only_dead { … return; }`
    //     without printing a word.
    //
    // The alarm at 10:12:34.246 fired only because the mouse happened to fall
    // quiet for 3032 ms at that instant. **The repair was waiting on the mouse
    // to stop moving.** That is the hole: the two candidate tests between them
    // cannot see "both keyboard hooks dead, mouse alive", which is precisely
    // what `LowLevelHooksTimeout` eviction produces (see TIMEOUT_EVICTION_DESC).
    //
    // This block is therefore not a throttle bypass bolted onto the old path —
    // it is a THIRD, INDEPENDENT candidate that reaches its own verdict from
    // instruments the repair cannot write, and repairs on it directly. It sits
    // above every throttle deliberately:
    //
    //   * the 60 s cooldown and the "last repair delivered events" test exist to
    //     stop churn on UNEVIDENCED alarms — PROBLEM 236, where 6 of 16 alarms
    //     printed `4000/4000` seeded clocks and each false repair killed a live
    //     hold. **They keep that job in full for the `both_dead`/`kb_only_dead`
    //     path below.** They must never apply to a PROVEN-deaf verdict: an
    //     instrument-backed proof that the app is deaf is the one case where
    //     waiting is strictly worse than repairing.
    //   * the install grace, PROBLEM 228's never-fired-is-UNKNOWN law, and the
    //     no-repair-during-a-live-hold rule all still apply — they are inside
    //     `proven_keyboard_deaf`, where a test can reach them.
    //   * the only throttle here is `forced_repair_allowed`: a 5 s floor with a
    //     doubling backoff to 60 s once repairs stop helping, so a hostile
    //     environment cannot produce a repair storm.
    //
    // `GetAsyncKeyState` is legal on this thread and forbidden in the callback
    // (law 3). Placed AFTER the elevation and NULL-foreground early-returns on
    // purpose: UIPI silence is expected and a re-hook cannot cure it.
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        let space_down = (GetAsyncKeyState(VK_SPACE as i32) as u16 & 0x8000) != 0;
        let stand_down = FULLSCREEN_ACTIVE.load(Ordering::Relaxed)
            || EXCLUDED_ACTIVE.load(Ordering::Relaxed)
            || BYPASS_MODE.load(Ordering::Relaxed);
        let t = tick_count();
        let installed_at = HOOKS_INSTALLED_AT.load(Ordering::Relaxed);
        let since_install = t.saturating_sub(installed_at);
        let kb_cb_raw = LAST_KB_CALLBACK.load(Ordering::Relaxed);
        let ms_cb_raw = LAST_MS_CALLBACK.load(Ordering::Relaxed);
        // Scoped to THIS install, exactly as the PROBLEM 236 path scopes them:
        // the clocks stay callback-only and the install boundary lives in its
        // own static, so no repair ever writes an instrument (PROBLEM 228).
        let scoped =
            |cb: u64| (cb != 0 && cb >= installed_at).then(|| t.saturating_sub(cb));
        let kb_cb_silence = scoped(kb_cb_raw);
        let ms_cb_silence = scoped(ms_cb_raw);
        let raw_age = raw_kb_age_ms();
        let proven = proven_keyboard_deaf(
            space_down,
            MODIFIER_ACTIVE.load(Ordering::Relaxed),
            stand_down,
            other_modifier_down(),
            kb_cb_silence,
            raw_age,
            user_input_ms,
            since_install,
            OWN_DEAF_SILENCE_MS,
            INSTALL_GRACE_MS,
            FORCED_INPUT_MAX_AGE_MS,
            FORCED_MOUSE_ATTRIBUTION_MS,
        );
        if let Some(reason) = proven {
            // DID THE LAST FORCED REPAIR WORK? Callback-only on both sides:
            // `LAST_KB_CALLBACK` is written by `kb_hook_proc` and by nothing
            // else, and `LAST_FORCED_REPAIR` is stamped after the install
            // completes. A keyboard callback strictly after the repair means
            // that repair restored delivery — so the backoff resets, and the
            // fast 5 s cadence is available again for this new episode.
            let last_forced = LAST_FORCED_REPAIR.load(Ordering::Relaxed);
            if last_forced != 0 && kb_cb_raw > last_forced {
                FORCED_REPAIR_INEFFECTIVE.store(0, Ordering::Relaxed);
            }
            let streak = FORCED_REPAIR_INEFFECTIVE.load(Ordering::Relaxed);
            let since_repair = (last_forced != 0).then(|| t.saturating_sub(last_forced));
            if !forced_repair_allowed(
                since_repair,
                streak,
                FORCED_REPAIR_BASE_MS,
                FORCED_REPAIR_MAX_MS,
            ) {
                // ONE line per backoff window, not one per tick.
                let wait = forced_repair_backoff_ms(
                    streak,
                    FORCED_REPAIR_BASE_MS,
                    FORCED_REPAIR_MAX_MS,
                );
                if FORCED_BACKOFF_LOGGED_FOR.swap(last_forced, Ordering::Relaxed) != last_forced
                {
                    log::warn!(
                        "hook: KEYBOARD DEAF, PROVEN ({reason:?}) — but {streak} consecutive \
                         forced repair(s) delivered no keyboard callback, so the backoff is \
                         engaged and the next repair waits {wait}ms (base \
                         {FORCED_REPAIR_BASE_MS}ms, cap {FORCED_REPAIR_MAX_MS}ms); the last \
                         one was {}ms ago. Repairs that keep not working are evidence the \
                         cure is not ours to apply — something upstream of every hook in \
                         this process is eating the keys. {TIMEOUT_EVICTION_DESC}",
                        since_repair.unwrap_or(0)
                    );
                }
                // Deliberately NOT a `return`: the ordinary `both_dead` /
                // `kb_only_dead` path below is unchanged and may still have
                // something to say about this tick.
            } else {
                let ms_desc = match ms_cb_silence {
                    Some(ms) => format!("{ms}ms ago"),
                    None => "NEVER since the last install".to_string(),
                };
                let raw_desc = match raw_age {
                    Some(ms) => format!("{ms}ms ago ({} raw keystrokes since launch)", RAW_KB_EVENTS.load(Ordering::Relaxed)),
                    None => "NEVER since launch".to_string(),
                };
                let fg = foreground_desc();
                // Handles BEFORE the repair. `install_hooks()` replaces the
                // reference hook too, from its own static, so all three are
                // printed and "repair #N" can never again be ambiguous about
                // whether anything actually changed.
                let old_kb = kb.0 as usize;
                let old_ms = ms.0 as usize;
                let old_ref = REF_KB_HOOK.load(Ordering::SeqCst);
                // A GENUINE repair: unhook both primaries, then install afresh.
                // `install_hooks()` unhooks and reinstalls the REFERENCE first
                // (law 5 — it must land at the TAIL of the chain, behind the
                // primary, or it becomes the slowest hook and the first Windows
                // evicts; PROBLEM 230). Do not reorder these three calls.
                let _ = UnhookWindowsHookEx(*kb);
                let _ = UnhookWindowsHookEx(*ms);
                let (nkb, nms) = install_hooks();
                let new_ref = REF_KB_HOOK.load(Ordering::SeqCst);
                *kb = nkb;
                *ms = nms;
                // PROBLEM 262 item 3 — the chain has just changed, so any hold
                // latched a moment ago belongs to a hook that no longer exists.
                // Its Space-UP can never arrive. Tear it down before anything
                // else reads the latch.
                tear_down_hold_across_repair("the PROVEN-deaf forced repair");
                let n = FORCED_REPAIRS_TOTAL.fetch_add(1, Ordering::Relaxed) + 1;
                // Kept as the DRAINED 60s diagnostic counter it always was —
                // `hook diagnostics … keyboard-deaf-rehooks:N`. It is no longer
                // the source of the "#N this session" number, which is why that
                // number used to read 1 three times over.
                OWN_DEAF_REHOOKS.fetch_add(1, Ordering::Relaxed);
                HOOK_REINSTALLS.fetch_add(1, Ordering::Relaxed);
                // Provisional: this repair counts as ineffective until a
                // keyboard callback arrives after it (checked at the top of
                // this block on the next proven-deaf verdict).
                FORCED_REPAIR_INEFFECTIVE.fetch_add(1, Ordering::Relaxed);
                let stamp = tick_count();
                LAST_FORCED_REPAIR.store(stamp, Ordering::Relaxed);
                LAST_OWN_DEAF_REHOOK.store(stamp, Ordering::Relaxed);
                // Strictly after the install, for the same reason the ordinary
                // path stamps it late: the cooldown's `previous_worked` test
                // compares callbacks against this instant.
                WATCHDOG_LAST_REINSTALL.store(stamp, Ordering::Relaxed);
                log::warn!(
                    "hook: KEYBOARD DEAF, PROVEN (PROBLEM 260, was 257) — reason {reason:?}. \
                     Our keyboard callback has not fired for {}ms (threshold \
                     {OWN_DEAF_SILENCE_MS}ms) while the OS delivered a KEYSTROKE (raw input, \
                     PROBLEM 268) {raw_desc}, the mouse callback fired {ms_desc} and the \
                     OS says the user was active {user_input_ms}ms ago; install {since_install}ms \
                     ago, no hold latched. Foreground: {fg}. FORCED REPAIR #{n} this session \
                     (this counter is never drained — the old line read a 60s counter and \
                     printed '#1' three times). Handles: keyboard {old_kb:#x} -> {:#x}, mouse \
                     {old_ms:#x} -> {:#x}, reference {old_ref:#x} -> {new_ref:#x}; a handle \
                     that did not change means the install failed. reinstall ok: {}. This \
                     repair BYPASSED the 60s cooldown and the 'last repair delivered events' \
                     test on purpose: those exist to stop churn on UNEVIDENCED alarms \
                     (PROBLEM 236) and a proven-deaf verdict is not one. {TIMEOUT_EVICTION_DESC} \
                     READ THE NEXT 'hook liveness split' LINE: primary_real above 0 means the \
                     re-install cured it; still 0 means the drop is upstream of every hook here.",
                    kb_cb_silence.unwrap_or(0),
                    nkb.0 as usize,
                    nms.0 as usize,
                    !nkb.is_invalid()
                );
                if nkb.is_invalid() {
                    log::error!(
                        "hook: the forced repair's SetWindowsHookExW returned an INVALID \
                         keyboard handle — the app has no keyboard hook at all right now. The \
                         backoff above will retry; if this repeats, the PROBLEM 82 supervisor's \
                         thread rebuild is the next move."
                    );
                }
                return;
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
        // PROBLEM 236 — the "would have alarmed" throttle counts one episode at
        // a time, so it resets here.
        //
        // PROBLEM 262 item 2 — THE DEFERRAL CLOCK NO LONGER DOES, WHILE A HOLD
        // IS LATCHED. It used to reset unconditionally, and this line is where
        // the 2026-09-07 wedge lived: `both_dead` needs the mouse callback to
        // fall silent past `BLIND_MS`, so every tick with the user's hand on the
        // mouse landed here and threw the episode away. The bound then required
        // ten CONSECUTIVE alarm seconds, never got them, and four separate
        // "Holds protected this session" lines were logged for one hold while
        // the repair never came. See `defer_episode_ends_on_quiet_tick`.
        if defer_episode_ends_on_quiet_tick(MODIFIER_ACTIVE.load(Ordering::Relaxed))
            && ALARM_DEFERRED_AT.swap(0, Ordering::Relaxed) != 0
        {
            ALARM_DEFER_LOGGED.store(false, Ordering::Relaxed);
        }
        WOULD_HAVE_ALARMED_TICKS.store(0, Ordering::Relaxed);
        return;
    }
    // PROBLEM 218 — count the alarm HERE, where it is confirmed, and count it
    // before any throttle can return: a throttled alarm is still an alarm, and
    // excluding those is how the rate would come out flattering.
    WD_ALARMS.fetch_add(1, Ordering::Relaxed);
    if FG_IS_SELF.load(Ordering::Relaxed) {
        WD_ALARMS_OWN_FG.fetch_add(1, Ordering::Relaxed);
    }

    // ═══ PROBLEM 236 — ASK THE CLOCKS THE REPAIR CANNOT WRITE ═══
    //
    // `kb_silence`/`ms_silence` above are SEEDED: `install_hooks()` and this
    // function's own idle early-return stamp both. The alarm keeps using them
    // (that is what stops PROBLEM 101's false alarms), but the SENTENCE it
    // prints — "NEITHER hook saw anything" — is a claim about the callbacks,
    // and until now nothing in the line could support it. Measured over the
    // owner's 1.0.96 session (2026-09-04 16:28→17:06): 16 alarms, all from
    // `both_dead`, ZERO from `kb_only_dead`, zero DEAF lines — and 6 of the 16
    // printed `kb`/`mouse` as identical round numbers, the fingerprint of one
    // non-hook writer setting both.
    //
    // Same law as PROBLEM 228: the alarm may read the seeded clock, the
    // sentence may not. Decision unchanged — see `alarm_is_evidenced`.
    // Each Option is "how long since this hook's callback last ran, SCOPED TO
    // THE CURRENT INSTALL". `None` = it has not fired since the hooks were last
    // installed, which is not the same fact as "it is dead" — see
    // `classify_callback_liveness`. The clocks themselves stay callback-only;
    // the install boundary is a separate static (`HOOKS_INSTALLED_AT`) so that
    // no repair ever writes an instrument again.
    let installed_at = HOOKS_INSTALLED_AT.load(Ordering::Relaxed);
    let since_install = now.saturating_sub(installed_at);
    let scoped = |cb: u64| (cb != 0 && cb >= installed_at).then(|| now.saturating_sub(cb));
    let kb_cb_silence = scoped(LAST_KB_CALLBACK.load(Ordering::Relaxed));
    let ms_cb_silence = scoped(LAST_MS_CALLBACK.load(Ordering::Relaxed));
    let ref_cb_silence = if ref_seen > 0 {
        scoped(LAST_REF_KB_EVENT.load(Ordering::Relaxed))
    } else {
        None
    };
    let since = |v: Option<u64>| match v {
        Some(ms) => format!("{ms}ms ago"),
        None => "NEVER since the last install".to_string(),
    };
    // The EVIDENCED/UNEVIDENCED word is UNCHANGED, on purpose: it is the tag the
    // PROBLEM 236 entry told the next session to grep, and the fortnight of data
    // is worth more if the word keeps meaning exactly what it meant in 1.0.97.
    // It no longer decides anything — `classify_callback_liveness` does.
    let cb_desc = format!(
        "{} — the callback-only clocks (nothing but a hook proc writes these) say keyboard {}, \
         mouse {}, reference {}, against a {BLIND_MS}ms threshold",
        if alarm_is_evidenced(kb_cb_silence, ms_cb_silence, ref_cb_silence, BLIND_MS) {
            "EVIDENCED"
        } else {
            "UNEVIDENCED"
        },
        since(kb_cb_silence),
        since(ms_cb_silence),
        since(ref_cb_silence)
    );

    // ═══ PROBLEM 236, DECISION CHANGED — THE GATE ═══
    //
    // Everything above this line is the OLD rule, computed in full and left
    // untouched so its verdict can still be logged. Below it, the seeded clocks
    // no longer authorise a repair on their own.
    let verdict = classify_callback_liveness(
        kb_cb_silence,
        ms_cb_silence,
        ref_cb_silence,
        since_install,
        BLIND_MS,
        INSTALL_GRACE_MS,
        UNKNOWN_MAX_MS,
    );
    if !matches!(verdict, CallbackLiveness::Dead(_)) {
        // The hooks are not dead by any instrument the repair cannot write, so
        // the destructive re-hook does NOT happen. Whatever was wrong is not
        // something re-hooking would fix, and re-hooking would kill a hold.
        BLIND_REINSTALLS.store(0, Ordering::Relaxed);
        // PROBLEM 262 item 2 — same correction as the `!both_dead` return
        // above: a tick that decides not to repair may not restart the bound of
        // a deferral episode whose hold is still latched.
        if defer_episode_ends_on_quiet_tick(MODIFIER_ACTIVE.load(Ordering::Relaxed))
            && ALARM_DEFERRED_AT.swap(0, Ordering::Relaxed) != 0
        {
            ALARM_DEFER_LOGGED.store(false, Ordering::Relaxed);
        }
        // Keep the fortnight of data the PROBLEM 236 entry asked for: say what
        // the OLD rule would have DONE, every time. Rising edge plus one line a
        // minute while the episode persists, with the tick count in the line.
        let ticks = WOULD_HAVE_ALARMED_TICKS.fetch_add(1, Ordering::Relaxed) + 1;
        let last_line = WOULD_HAVE_ALARMED_AT.load(Ordering::Relaxed);
        if ticks == 1 || now.saturating_sub(last_line) >= 60_000 {
            WOULD_HAVE_ALARMED_AT.store(now, Ordering::Relaxed);
            let which = if kb_only_dead && !both_dead {
                "kb_only_dead"
            } else {
                "both_dead"
            };
            log::info!(
                "hook: WATCHDOG would have alarmed ({which}) and re-hooked here, and 1.0.96 \
                 would have — kb {kb_silence}ms / mouse {ms_silence}ms, but those are the \
                 RE-STAMPED clocks (`install_hooks()` and this watchdog's own idle return \
                 write both). {cb_desc}. Verdict from the callback-only clocks: {verdict:?} \
                 (install {since_install}ms ago; grace {INSTALL_GRACE_MS}ms, unknown bound \
                 {UNKNOWN_MAX_MS}ms). {RULE_DESC} Standing down — {ticks} tick(s) in this \
                 episode. {ref_desc}."
            );
        }
        return;
    }
    WOULD_HAVE_ALARMED_TICKS.store(0, Ordering::Relaxed);

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
        //
        // REVIEW FIX 2026-09-04 — AND BY A CALLBACK, NOT BY A CLOCK THE
        // WATCHDOG WRITES ITSELF. This test read `LAST_KB_EVENT`, which
        // `install_hooks()` and this function's own idle early-return both
        // re-stamp (see the block above the elevation test). So after any pause
        // following a re-hook, "the last repair delivered events" was satisfied
        // by the watchdog writing to its own instrument — 100 such lines in the
        // owner's log, none of them evidence, and every one of them a full 60
        // seconds of holding off while the hook may genuinely have been dead.
        // The PROBLEM 228 note below already SAID the claim was false and left
        // the decision alone pending data; the data arrived, so the decision
        // moves onto the callback-only clocks that nothing but a hook proc
        // writes.
        let kb_cb = LAST_KB_CALLBACK.load(Ordering::Relaxed);
        let ms_cb = LAST_MS_CALLBACK.load(Ordering::Relaxed);
        let previous_worked = if kb_only_dead {
            kb_cb > last
        } else {
            kb_cb > last || ms_cb > last
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
                // WAS FALSE. It said "the last repair DID deliver events" off
                // `LAST_KB_EVENT`, which the watchdog's own idle early-return
                // re-stamps every tick the user has been quiet for 2 s. The
                // 2026-09-04 review moved the DECISION onto the callback-only
                // clocks (above), so the sentence and the test finally agree:
                // this hold-off cannot happen on a re-stamp any more.
                let genuine_desc = if kb_cb > last {
                    format!("a genuine keyboard callback arrived {}ms after it", kb_cb - last)
                } else if ms_cb > last {
                    format!(
                        "no keyboard callback since the repair, but a genuine MOUSE callback \
                         arrived {}ms after it (this alarm was not keyboard-only, so the mouse \
                         counts)",
                        ms_cb - last
                    )
                } else {
                    // Unreachable while `previous_worked` is what gates this
                    // block; kept so a future edit that widens the test cannot
                    // make the sentence lie without the log saying so.
                    "no callback of any kind since the repair — the test that let this line \
                     print has been widened and no longer matches what it claims"
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

    // ═══ PROBLEM 236 — LIVE-HOLD PROTECTION ═══
    //
    // Everything below this point is destructive by design, and correct for a
    // REAL eviction: the Space-UP is genuinely lost, so `MODIFIER_ACTIVE`,
    // `SPACE_INTERCEPTED`, `SPACE_ABORTED` and `SPACE_COMBO_SEEN` must be
    // cleared, the pointer latches reset (PROBLEM 206) and the ring hidden
    // (PROBLEM 177) or they outlive the hold forever. Landing that set on a
    // hold that is still ALIVE is the owner's *"the ring dies mid-press"*.
    //
    // The premise that the hold is alive is callback-only on both sides:
    // `LAST_KB_CALLBACK >= SPACE_DOWN_TS` says the keyboard callback was
    // entered at or after this hold began. Nothing off the hook path writes
    // either value.
    //
    // WHY THIS IS NOT A HOLE. `reap_stale_hold()` runs at the very TOP of this
    // function, above every early return: a hold whose auto-repeat has stopped
    // arriving has already been reaped before this line is reached. So the only
    // hold that can defer here is one still being fed by the keyboard callback,
    // or one that fired a combo (where PROBLEM 219 stood the reaper down on
    // purpose because Windows moved auto-repeat to the other key). Both of
    // those are holds a teardown would damage rather than repair. And the
    // deferral is BOUNDED — see `MAX_HOLD_DEFER_MS`.
    {
        let hold_active = MODIFIER_ACTIVE.load(Ordering::Relaxed);
        let space_down_at = SPACE_DOWN_TS.load(Ordering::Relaxed);
        let saw_key_within_hold = space_down_at != 0
            && LAST_KB_CALLBACK.load(Ordering::Relaxed) >= space_down_at;
        // The clock starts on the FIRST alarm tick that finds a deferrable
        // hold, not on the hold itself: the bound is "how long has the repair
        // been waiting", not "how long has he been holding Space".
        let deferrable = hold_active && saw_key_within_hold;
        let mut started = ALARM_DEFERRED_AT.load(Ordering::Relaxed);
        if deferrable && started == 0 {
            ALARM_DEFERRED_AT.store(now, Ordering::Relaxed);
            started = now;
        }
        let deferred_for = if started == 0 { 0 } else { now.saturating_sub(started) };
        if hold_defers_rehook(hold_active, saw_key_within_hold, deferred_for, MAX_HOLD_DEFER_MS) {
            let held_for = now.saturating_sub(space_down_at);
            // ONCE per deferral episode. The tick is 1s and the bound is 10s,
            // so an unthrottled line would be ten copies of the same sentence.
            if !ALARM_DEFER_LOGGED.swap(true, Ordering::Relaxed) {
                let n = HOLDS_PROTECTED.fetch_add(1, Ordering::Relaxed) + 1;
                log::info!(
                    "hook: WATCHDOG alarm confirmed, but a Space hold is LIVE — it has been \
                     latched {held_for}ms and the keyboard callback was entered inside it, so \
                     tearing it down here would clear MODIFIER_ACTIVE, reset the pointer \
                     latches and hide the ring mid-press (PROBLEM 236). Deferring the re-hook \
                     until the hold ends (Space-up, or PROBLEM 218's reaper), and no longer \
                     than {MAX_HOLD_DEFER_MS}ms. {cb_desc}. Holds protected this session: {n}."
                );
            }
            return;
        }
        // Not deferring: either there is no live hold, or the bound expired.
        // Clear the episode so the next one logs again, and say so exactly once
        // when the bound is what ended it — a deferral that timed out means the
        // hold outlived the alarm, which is the case this bound exists for.
        if started != 0 && deferred_for >= MAX_HOLD_DEFER_MS {
            let held_for = now.saturating_sub(space_down_at);
            let n = HOLDS_PROTECTED.load(Ordering::Relaxed);
            log::warn!(
                "hook: deferral-episode-bound-expired-proceeding-with-the-repair-spaceadom — the \
                 Space hold outlived the deferred WATCHDOG alarm. EPISODE DURATION \
                 {deferred_for}ms, measured from the FIRST deferred alarm of this episode (bound \
                 {MAX_HOLD_DEFER_MS}ms); the hold itself has been latched {held_for}ms, and \
                 {n} hold(s) have been protected this session. Proceeding with the re-hook, which \
                 tears this hold down a few lines below — a hold latched by a hook that is about \
                 to be replaced is unfalsifiable (PROBLEM 262 item 3). If the hold was real, this \
                 is the one press PROBLEM 236's protection cannot save; if the hook was dead, the \
                 repair is {deferred_for}ms late and no later. PROBLEM 262 item 2: before 1.0.107 \
                 this line could not print at all once the mouse was moving, because every quiet \
                 tick reset the clock this duration is measured from."
            );
        }
        ALARM_DEFERRED_AT.store(0, Ordering::Relaxed);
        ALARM_DEFER_LOGGED.store(false, Ordering::Relaxed);
    }

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
    // PROBLEM 261 — and not the FALLBACK's latch either. A re-hook hides the
    // ring a few lines below; a fallback hold left latched under a hidden ring
    // would keep the mouse callback's `note_cursor` running against a chip
    // snapshot nobody can see. The fallback's own reaper would catch it within
    // 250 ms, but the repair already knows the hold is over — say so now.
    if disarm_own_window_hold() {
        log::info!(
            "own-window fallback: the re-hook tore down a live fallback hold (PROBLEM 261) — \
             its ring is being hidden below, so its pointer latches go with it."
        );
    }
    // PROBLEM 263 — and not the MIDDLE BUTTON's latch either. `install_hooks()`
    // replaces the MOUSE hook as well, so the WM_MBUTTONUP that would end this
    // hold belongs to a hook that no longer exists. Leaving it latched would
    // also make THE ARBITRATION refuse every new Space hold (rules B and C both
    // read this flag), so the ring would stop appearing at all — PROBLEM 262's
    // symptom, reached from a third direction.
    if disarm_middle_hold() {
        log::info!(
            "middle-button ring: the re-hook tore down a live middle-button hold (PROBLEM 263) \
             — its ring is being hidden below, so its pointer latches go with it, and the \
             arbitration latch that would have refused every new hold is cleared with it."
        );
    }

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
    // REVIEW FIX 2026-09-04 — this branch is REACHABLE AGAIN, and it now keys
    // off the verdict that actually authorised the repair rather than off the
    // seeded clocks. It was dead code from the moment the gate above accepted a
    // live reference hook as proof of life: `kb_only_dead`'s premise IS a live
    // reference, so every alarm that would have printed this sentence was
    // vetoed one screen earlier. `Dead(KbOnly)` is exactly "the reference is
    // being called and our primary is not", which is what this line says.
    if matches!(verdict, CallbackLiveness::Dead(DeadKind::KbOnly)) {
        log::warn!(
            "hook: WATCHDOG — the KEYBOARD hook alone was evicted. {ref_desc}, so keys ARE \
             reaching the chain, but ours has been silent for \
             {kb_silence}ms (mouse {ms_silence}ms, user active {user_input_ms}ms ago). \
             Foreground: {fg}. This is the failure `both_dead` could never see — it needs BOTH \
             hooks quiet, and the mouse hook keeps it false. {cb_desc}. {RULE_DESC} This alarm \
             PASSED that rule (install {since_install}ms ago). Re-hooking. \
             reinstall ok: {}",
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
             Elevation was ALREADY ruled out above, so this is NOT UIPI. {cb_desc}. \
             {RULE_DESC} This alarm PASSED that rule (install {since_install}ms ago), which \
             is why it was allowed to re-hook at all — 1.0.96 re-hooked on the re-stamped \
             clocks alone and did it every ~2.4 minutes. Re-hooking. reinstall ok: {}",
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

    // PROBLEM 104 — count EVERY key before any decision.
    //
    // PROBLEM 230 — AND IT HAS TO BE *EVERY* KEY, BECAUSE OF WHAT IT IS
    // COMPARED AGAINST. This add used to sit ~50 lines below, underneath the
    // `dwExtraInfo == MAGIC_INJECTED` early return, while the reference hook
    // counts on `n_code >= 0` with no cookie test at all. The two counters
    // therefore measured DIFFERENT POPULATIONS — and
    // `classify_hook_window(primary_seen, genuine_ref_events)` subtracts one
    // from the other. A 60-second window whose only keyboard traffic was this
    // app's OWN injection (`inject_space`, `force_foreground`'s synthetic tap —
    // both reachable from a tray click or a dashboard button, with nobody
    // touching the keyboard) gave `ref_events > 0, primary_seen == 0`: a
    // verdict of DEAF, at WARN, with a Sentry event, produced by the app
    // typing to itself while the hook worked perfectly.
    //
    // Moved above the cookie test so the two halves of the subtraction count
    // the same events. This is what the doc comment on `KB_EVENTS_SEEN` has
    // claimed since PROBLEM 104 ("incremented on entry, before any branch") and
    // what the code did not do.
    KB_EVENTS_SEEN.fetch_add(1, Ordering::Relaxed);

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
        // PROBLEM 236 — the split `KB_EVENTS_SEEN` alone cannot report.
        // PROBLEM 230 was right to count injections in the total (both sides of
        // `classify_hook_window` must count the same population), and it left
        // `saw N key event(s)` unable to say whether those N were a person
        // typing or this app injecting to itself with the real keyboard dead.
        // Counting them SEPARATELY answers both questions from one callback:
        // the total stays honest for the subtraction, and `total - injected` is
        // the real traffic. One relaxed add on a branch already taken.
        KB_EVENTS_INJECTED.fetch_add(1, Ordering::Relaxed);
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

    // (PROBLEM 104's `KB_EVENTS_SEEN.fetch_add` used to live HERE, under the
    // injected-cookie early return. PROBLEM 230 moved it to the top of the
    // callback — see the comment there for why counting a different population
    // than the reference hook produced false DEAF verdicts.)

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
    //
    // PROBLEM 263 — and a key pressed while the MIDDLE BUTTON is holding the
    // ring is a command for the same reason, so it is excluded the same way.
    // One extra relaxed load, and only on keystrokes that have already passed
    // the three tests above (i.e. ordinary typing), which is the cheapest place
    // it could sit.
    if is_down
        && vk != VK_SPACE
        && !MODIFIER_ACTIVE.load(Ordering::Relaxed)
        && !MIDDLE_HOLD_ACTIVE.load(Ordering::Relaxed)
    {
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
        // PROBLEM 263 — THE ARBITRATION, RULE B (stated in full beside
        // `MIDDLE_TAP_MS`): a middle-button hold already owns a ring, so this
        // Space is an ORDINARY SPACE. Handed to the OS exactly like the
        // Ctrl/Alt/Win case directly above and for the same reason — a press
        // this branch declines sets no latch, swallows nothing and owes no up,
        // so the two triggers can never both serve one gesture. One relaxed
        // load, on the Space-down branch only.
        if MIDDLE_HOLD_ACTIVE.load(Ordering::Relaxed) {
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
    // PROBLEM 263 — a MIDDLE-BUTTON hold gets the combo branch too, because
    // "tapping a bound letter while the button is held launches that key, the
    // same as Space+letter" is half of what makes the second trigger worth
    // having. Two relaxed loads instead of one on the key-down path.
    //
    // `hook_hold` is kept as its own name and every Space-SPECIFIC piece below
    // is gated on it. Three of them, and each would be a real bug if it ran for
    // a middle hold:
    //   * `SPACE_COMBO_SEEN` is `reap_stale_hold`'s stand-down evidence. Setting
    //     it for a hold the Space reaper is not watching is harmless today and
    //     is exactly the kind of shared-flag drift PROBLEM 219 was about.
    //   * `MAX_MODIFIER_HOLD_MS` is measured from `SPACE_DOWN_TS`, which for a
    //     middle hold is some previous Space press or 0 — it would fire
    //     instantly and clear a latch it does not own.
    //   * the ROLLOVER window and `MARGIN_COMMAND` both measure Space-down →
    //     key-down, i.e. "was this typing?". The middle button is not a typing
    //     key, so there is no rollover to apply and no margin to record; doing
    //     either would inject a phantom space and poison PROBLEM 95's histogram
    //     with a delay that means nothing.
    let hook_hold = MODIFIER_ACTIVE.load(Ordering::Relaxed);
    if (hook_hold || MIDDLE_HOLD_ACTIVE.load(Ordering::Relaxed)) && is_down {
        // PROBLEM 219 — FIRST, above every branch that can return, because
        // this fact is true of the hold no matter what we decide to do with
        // the key. Space-down was handled and returned above, so this is
        // always some OTHER key going down while Space is held: the exact
        // event that hands Windows' auto-repeat slot to that key and silences
        // Space's repeat for the rest of the hold. One relaxed store on a
        // branch that already loaded `MODIFIER_ACTIVE` — no new callback cost
        // (PROBLEM 58). See `SPACE_COMBO_SEEN` for the log evidence.
        //
        // PROBLEM 263 — `hook_hold` only. A middle hold produces no Space
        // auto-repeat for this flag to stand down, and it is not the hold
        // `reap_stale_hold` is watching.
        if hook_hold {
            SPACE_COMBO_SEEN.store(true, Ordering::Relaxed);
        }

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
        //
        // PROBLEM 262 item 4 — `MAX_MODIFIER_HOLD_MS` used to be declared HERE,
        // as a `const` local to this branch, which meant the only code in the
        // process that could enforce it was this callback. That is the exact
        // code that stops running when the hook is evicted, so the bound could
        // not fire in the one failure it was written for: on 2026-09-07 a hold
        // stayed latched 166 s and was cleared only when a stray callback
        // happened to reach this line. Same constant, same value, now at module
        // scope so `reap_stale_hold` enforces it too — from the pump and from
        // `st-hud-pointer`, neither of which needs the hook to be alive.
        //
        // PROBLEM 263 — `hook_hold` only, and this one would be a live bug
        // otherwise: `SPACE_DOWN_TS` belongs to the Space hold, so under a
        // middle hold `latched_ms` is the age of some previous Space press (or
        // of the epoch), the bound fires on the first key, and the branch then
        // clears a latch it does not own.
        let latched_ms = now.saturating_sub(SPACE_DOWN_TS.load(Ordering::Relaxed));
        if hook_hold && latched_ms > MAX_MODIFIER_HOLD_MS {
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
        // PROBLEM 263 — `hook_hold` only. The rollover window exists because
        // Space is a TYPING key and a fast typist's "the" must not become a
        // command; the middle button types nothing, so there is no prose to
        // protect and no space to re-inject. Without this term a middle hold
        // would fall into `inject_space_then_key` and type a space the user
        // never asked for.
        let in_rollover =
            hook_hold && rollover > 0 && is_alpha_or_digit(vk) && held_ms < rollover;

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
            VK_OEM_1 => Some(KeyCombo::Semicolon),
            VK_OEM_2 => Some(KeyCombo::Slash),
            VK_OEM_7 => Some(KeyCombo::Quote),

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
            // PROBLEM 263 — `hook_hold` only. PROBLEM 95's histogram measures
            // Space-down → key-down so the owner can see how close real typing
            // comes to the command threshold; a middle hold's `held_ms` is not
            // that measurement and would silently poison the data.
            if hook_hold {
                record_margin(&MARGIN_COMMAND, held_ms); // PROBLEM 95
            }
            // NOT gated: `SPACE_ABORTED` means "something else claimed this
            // press", which is true of both holds and is exactly what stops a
            // middle release from ALSO replaying a click (see
            // `middle_press_was_a_click`).
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
    let ms_now = tick_count();
    LAST_MS_EVENT.store(ms_now, Ordering::Relaxed);
    // PROBLEM 236 — the same instant and the same event, in a clock and a
    // counter NOTHING ELSE MAY WRITE. `LAST_MS_EVENT` above is re-stamped by
    // `install_hooks()` and by the watchdog's idle early-return, so it cannot
    // answer "did the mouse hook actually fire?" — and that question is half of
    // `both_dead`, which raised every alarm in the owner's 1.0.96 session.
    // Two relaxed writes beside the one already here (PROBLEM 58 envelope).
    LAST_MS_CALLBACK.store(ms_now, Ordering::Relaxed);
    MS_EVENTS.fetch_add(1, Ordering::Relaxed);
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

    // PROBLEM 263 — THE MIDDLE BUTTON'S RELEASE, and it sits here for exactly
    // the reason the line above it does: WHOEVER EATS THE DOWN OWES THE UP.
    //
    // We suppressed the `WM_MBUTTONDOWN`, so this up has no matching down in
    // the app underneath. Every gate below can flip mid-hold — the user
    // alt-tabs into an excluded app, a game goes fullscreen, bypass is switched
    // on from the tray — and all of them `return CallNextHookEx`. Put this
    // branch under any of them and the release of a press we already ate leaks
    // to an app that never saw the press, AND the ring stays up with no
    // teardown coming, which is PROBLEM 218's stranded HUD on a third path.
    //
    // The cookie test is FIRST: our own replayed click (see
    // `replay_middle_click`) must never be read as a new press, or the replay
    // would raise a ring and owe another replay, forever. Keyboard law 1 — our
    // cookie, never the OS's INJECTED flag.
    if msg == WM_MBUTTONUP {
        let ms = &*(l_param.0 as *const MSLLHOOKSTRUCT);
        if ms.dwExtraInfo != MAGIC_INJECTED_MOUSE
            && MIDDLE_UP_OWED.swap(false, Ordering::Relaxed)
        {
            if let Some(ev) = on_middle_button_up(ms_now) {
                send_event(ev);
            }
            return LRESULT(1);
        }
    }

    // --- App exceptions: same gate as kb_hook_proc, before anything is eaten.
    // MODIFIER_ACTIVE can still be TRUE from a Space held just before the
    // switch, and without this the wheel would stay swallowed for the first
    // scroll inside an excluded app.
    //
    // PROBLEM 267 — the exception SCOPE splits this gate in two. `EXCLUDED_ACTIVE`
    // is the SPACE verdict (off entirely, or middle-only); the middle button has
    // its own verdict (`exclusions::MIDDLE_EXCLUDED_ACTIVE`, read inside the
    // WM_MBUTTONDOWN branch below). So a `WM_MBUTTONDOWN` must reach its branch
    // even where Space stands down, and a LIVE middle hold must keep its mouse
    // moves, clicks and wheel — the ring inside a "Middle only" app is useless
    // without them. Cost on the common path: nothing new — the two extra loads
    // are only evaluated when `EXCLUDED_ACTIVE` is already true (PROBLEM 58's
    // envelope; short-circuit `&&`).
    if EXCLUDED_ACTIVE.load(Ordering::Relaxed)
        && msg != WM_MBUTTONDOWN
        && !MIDDLE_HOLD_ACTIVE.load(Ordering::Relaxed)
    {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    // PROBLEM 263 — THE MIDDLE BUTTON'S PRESS: the ring's second trigger.
    //
    // ABOVE the `hold_latched` gate below, because the whole point of this
    // branch is to run when NO hold is latched — it is what CREATES one. Below
    // the App-exceptions gate, because the user's own exception list stands the
    // entire app down and this is part of the app.
    //
    // COST ON THE COMMON PATH: one `u32` comparison per mouse event. Every
    // atomic load, the cookie read and the pure gate are inside the branch, so
    // a mouse-move pays nothing at all (PROBLEM 58's envelope).
    if msg == WM_MBUTTONDOWN {
        let ms = &*(l_param.0 as *const MSLLHOOKSTRUCT);
        if ms.dwExtraInfo != MAGIC_INJECTED_MOUSE
            && middle_button_down_accepted(
                MIDDLE_BUTTON_RING.load(Ordering::Relaxed),
                orbit_apps::WATCHER_ALIVE.load(Ordering::Relaxed),
                orbit_apps::ORBIT_ACTIVE.load(Ordering::Relaxed),
                // PROBLEM 267 — the user's OWN row for this app, with a scope
                // that stands the middle button down (off entirely / Space
                // only). Published by the exclusion watcher beside
                // EXCLUDED_ACTIVE; one relaxed load, inside the branch. (Until
                // 1.0.109 this was a literal `false`, because EXCLUDED_ACTIVE
                // had already returned above for every excluded app.)
                exclusions::MIDDLE_EXCLUDED_ACTIVE.load(Ordering::Relaxed),
                BYPASS_MODE.load(Ordering::Relaxed),
                FULLSCREEN_ACTIVE.load(Ordering::Relaxed),
                // THE ARBITRATION, rule A.
                MODIFIER_ACTIVE.load(Ordering::Relaxed),
                OWN_HOLD_ACTIVE.load(Ordering::Relaxed),
                MIDDLE_HOLD_ACTIVE.load(Ordering::Relaxed),
            )
        {
            on_middle_button_down(ms_now.max(1));
            return LRESULT(1); // suppress: the app must not start autoscroll
        }
        // Declined — hand it to the app underneath byte-identically to a build
        // that never had this feature. Nothing was eaten, so nothing is owed.
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    // PROBLEM 261 — "is a hold latched?", asked of ALL THREE witnesses.
    //
    // This used to read `MODIFIER_ACTIVE` alone, and that single load is the
    // whole of the owner's 1.0.105 report: with the dashboard focused the
    // keyboard hook is never called (PROBLEM 257), so `MODIFIER_ACTIVE` is
    // false for the entire hold, so this gate returned before `note_cursor`
    // ("it's not seeing my cursor movement") and before the `WM_LBUTTONDOWN`
    // branch below ("it's not opening apps when clicked"). The ring was drawn
    // by the engine, which the fallback DOES reach; the pointer hangs off the
    // mouse callback, which it did not.
    //
    // COST: one extra relaxed load per mouse event when no hook hold is
    // latched, i.e. on the common path. Nothing else changes — no branch, no
    // call, no allocation. `hold_latched` is `#[inline(always)]` and short-
    // circuits, so a hook hold pays nothing at all (PROBLEM 58's envelope).
    if !hold_latched(
        MODIFIER_ACTIVE.load(Ordering::Relaxed),
        OWN_HOLD_ACTIVE.load(Ordering::Relaxed),
        MIDDLE_HOLD_ACTIVE.load(Ordering::Relaxed),
    ) {
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

// ═══════════════════════════════════════════════════════════════════════════
// PROBLEM 259 — THE OWN-WINDOW FALLBACK
//
// PROBLEM 257, measured live on 2026-09-07: while our own WebView2 window
// holds the foreground, NEITHER keyboard hook in this process is called —
// primary_real:0, reference:0, mouse:2705 in the same minute on the same
// thread — and re-hooking does not recover it. The hook is not deciding
// anything about those keys; it is not being asked.
//
// The dashboard PAGE, however, receives ordinary `keydown`/`keyup` for them:
// it is the window with focus, so the events reach it through the normal
// WebView2 input path, which is upstream of nothing we lost. So the page can
// be the witness the hook cannot be, and `src/own-window-keys.ts` feeds what
// it sees back in through the three `own_window_*` commands.
//
// THE ONE THING THIS MUST NEVER DO IS DOUBLE-FIRE. If the hook is healthy over
// our window (on another machine, or after the OS-side cause goes away) both
// paths see the same Space and the user gets two holds, two rings, and two
// launches. Three independent guards make that structurally impossible:
//
//   1. FOREGROUND. The fallback only ever acts while OUR window is the
//      foreground window — the exact and only situation PROBLEM 257 describes.
//   2. DEDUPE. If the hook stamped a Space-down inside the last
//      `OWN_WINDOW_DEDUPE_MS`, or has a hold latched right now
//      (`MODIFIER_ACTIVE`), the page's Space-down is dropped: the hook got
//      there first and owns this hold.
//   3. OWNERSHIP. `own_window_key` and `own_window_space_up` do nothing unless
//      the fallback itself accepted the matching Space-down. A hold belongs to
//      exactly one path for its whole life; there is no interleaving.
//
// Nothing here runs on the hook callback. These functions are called from
// Tauri command handlers, where logging, `GetForegroundWindow` and a mutex are
// all legal (the same reasoning as PROBLEM 243's shown-over line).
// ═══════════════════════════════════════════════════════════════════════════

/// How recently the HOOK must have stamped a Space-down for the page's
/// Space-down to be read as a duplicate of it rather than a new hold.
///
/// 100 ms: far longer than the microseconds between the hook's `send_event`
/// and the page's `invoke` for one physical press, and far shorter than the
/// fastest realistic gap between two deliberate Space presses (~120 ms even
/// for a fast double-tap).
pub(crate) const OWN_WINDOW_DEDUPE_MS: u64 = 100;

/// The fallback's own stuck-hold bound, mirroring `MAX_MODIFIER_HOLD_MS` in
/// the combo branch. A page that is torn down mid-hold (navigation, a crash,
/// a reload) never sends its `keyup`, and without this the fallback would
/// stay latched forever and hand the next stray `own_window_key` a combo.
pub(crate) const OWN_WINDOW_MAX_HOLD_MS: u64 = 30_000;

/// The engine channel, cloned out of `spawn_hook_thread`. `None` until the
/// hook thread is spawned — which is also the safe-mode answer (see there).
static INJECT_TX: std::sync::Mutex<Option<Sender<HookEvent>>> = std::sync::Mutex::new(None);

/// Tick of the Space-down the FALLBACK accepted, or 0 for "no fallback hold".
/// Written only by the `own_window_*` commands, never by the callback.
static OWN_HOLD_TS: AtomicU64 = AtomicU64::new(0);
/// Has the current fallback hold already fired a combo? Reported back on
/// `SpaceUp { modifier_fired }` so the engine sees the same shape the hook
/// would have sent.
static OWN_HOLD_COMBO: AtomicBool = AtomicBool::new(false);
/// How many holds the fallback has served. Purely for the log line, so one
/// episode reads as an episode rather than as scattered lines.
static OWN_WINDOW_HOLDS: AtomicU32 = AtomicU32::new(0);

// --- PROBLEM 261 — the fallback hold's OWN latch -------------------------
//
// THE FAILURE. 1.0.105 shipped PROBLEM 259's fallback and it drew the ring
// inside the dashboard exactly as asked. It armed NOTHING: the owner's report
// on 2026-09-07 is *"it's not seeing my cursor movement and it's not opening
// apps when clicked"*, with pointer aiming working normally over every other
// app. Pointer activation was never wired to the second witness.
//
// WHY. Every gate pointer activation passes through asks `MODIFIER_ACTIVE`,
// and `MODIFIER_ACTIVE` is written by the keyboard CALLBACK — the one thing
// that by definition never runs for a fallback hold (PROBLEM 257). Three
// gates, all of them shut:
//   1. `ms_hook_proc` returns `CallNextHookEx` before `note_cursor`, so no
//      cursor position is ever recorded — "not seeing my cursor movement".
//   2. The same early return sits above the `WM_LBUTTONDOWN` branch, so
//      gesture B never runs — "not opening apps when clicked".
//   3. `HoldTracker::tick`'s `live` term is
//      `enabled && modifier_active && hud_visible && !blocked`, so even with a
//      cursor the poller could not arm.
// And a fourth, quieter one: the poller identifies WHICH hold a tick belongs
// to by `SPACE_DOWN_TS`, also callback-written. A fallback hold reuses the
// last hook hold's stamp (or 0 at boot), so `CURSOR_STAMP >= hold_ts` would
// have accepted a cursor position from a previous hold.
//
// WHY NOT JUST SET `MODIFIER_ACTIVE` FROM HERE. It is the obvious fix and it
// is wrong, for four independent reasons — this comment is the record so it is
// not "simplified" back:
//   * **It is guard 2.** `own_window_space_down_accepted` refuses a fallback
//     hold while `MODIFIER_ACTIVE` is latched, because a latched hook hold is
//     the same physical press. If the fallback set it, a fallback hold whose
//     Space-UP was lost would refuse EVERY later fallback hold, permanently.
//     PROBLEM 259 states the invariant outright: `SPACE_DOWN_TS` and
//     `MODIFIER_ACTIVE` are *read, never written* by this path, *which is why
//     they can arbitrate*. An arbiter may not be a party.
//   * **It is the deafness verdict.** `proven_keyboard_deaf` takes
//     `MODIFIER_ACTIVE` as evidence a hold is in progress. A fallback hold
//     that set it would talk the PROBLEM 260 instrument out of the very
//     verdict the fallback exists because of.
//   * **It arms the keyboard callback.** `if MODIFIER_ACTIVE && is_down` is
//     the combo branch. Alt-Tab mid-hold into a window where the hook is NOT
//     deaf and the next letter typed there is eaten as a shortcut.
//   * **It breaks the hook's own Space-down branch**, which reads a latched
//     `MODIFIER_ACTIVE` as "this is an auto-repeat" and sends no `SpaceDown`.
//
// SO THE FALLBACK GETS ITS OWN LATCH, and every consumer that asked
// "is a hold latched?" is changed to ask BOTH. `MODIFIER_ACTIVE` keeps meaning
// exactly what it meant — *the callback saw a Space go down and has not seen
// it come up* — and stays the arbiter. Consequence stated so nobody looks for
// it later: **a fallback hold can never latch `MODIFIER_ACTIVE`, because
// nothing on this path writes it.** The PROBLEM 218 class is closed here by
// construction rather than by a reaper; the reaper below exists for the latch
// this path *does* own.
/// `true` while the PROBLEM 259 fallback owns a Space-hold. The exact
/// counterpart of `MODIFIER_ACTIVE` for the second witness, and read on the
/// mouse callback (one relaxed load) so the cursor and the click reach the
/// pointer the same way they do for a hook hold.
static OWN_HOLD_ACTIVE: AtomicBool = AtomicBool::new(false);

/// How many fallback holds the reaper below has torn down. Non-zero is the
/// fingerprint of a page that stopped talking mid-hold.
static OWN_HOLDS_REAPED: AtomicU32 = AtomicU32::new(0);

/// How often the fallback reaper re-asks the question guard 1 asked once.
/// 250 ms: fast enough that a stranded ring is a blink rather than a state,
/// slow enough that the foreground probe (three Win32 calls and a `String`)
/// runs four times a second during a hold and never otherwise.
pub(crate) const OWN_HOLD_FG_CHECK_MS: u64 = 250;

pub(crate) fn register_inject_sender(tx: Sender<HookEvent>) {
    *INJECT_TX.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
}

/// Push an event onto the engine channel from OUTSIDE the hook thread.
///
/// `send_event` cannot be used: its sender is a `thread_local!` that only the
/// hook thread ever sets, so a Tauri command calling it silently does nothing.
/// Returns whether the event was actually queued.
///
/// PRIVATE ON PURPOSE — `pub(crate)`, and every caller in the tree goes
/// through the guarded `own_window_*` functions below. This is a back door
/// into the engine; it may never grow a public or un-guarded entrance.
pub(crate) fn inject_hook_event(ev: HookEvent) -> bool {
    let guard = INJECT_TX.lock().unwrap_or_else(|p| p.into_inner());
    match guard.as_ref() {
        Some(tx) => tx.try_send(ev).is_ok(),
        None => false,
    }
}

/// Age in ms of the last Space-down the HOOK stamped, or `None` if it has
/// never stamped one in this process (`SPACE_DOWN_TS` starts at 0).
fn hook_space_down_age_ms() -> Option<u64> {
    let ts = SPACE_DOWN_TS.load(Ordering::Relaxed);
    if ts == 0 {
        return None;
    }
    Some(tick_count().saturating_sub(ts))
}

fn own_hold_age_ms() -> Option<u64> {
    let ts = OWN_HOLD_TS.load(Ordering::Relaxed);
    if ts == 0 {
        return None;
    }
    Some(tick_count().saturating_sub(ts))
}

/// Is the foreground window one of OURS? Compared by exe stem, the same
/// normalisation `exclusions` and PROBLEM 243's shown-over line use, so
/// "own window" means one thing across the whole log.
fn foreground_is_own_window() -> bool {
    #[cfg(windows)]
    {
        let own = exclusions::own_stem();
        let fg = unsafe { exclusions::foreground_stem() };
        !fg.is_empty() && fg == own
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// THE GUARD, as a pure function — every reason the fallback may decline, in
/// one place that a test can walk.
#[allow(clippy::too_many_arguments)]
pub(crate) fn own_window_space_down_accepted(
    foreground_is_own_window: bool,
    bypass_active: bool,
    hook_hold_latched: bool,
    hook_space_down_age_ms: Option<u64>,
    dedupe_ms: u64,
    own_hold_age_ms: Option<u64>,
    own_hold_max_ms: u64,
    middle_hold_active: bool,
) -> bool {
    // Guard 1 — the fallback exists for exactly one situation.
    if !foreground_is_own_window {
        return false;
    }
    // Bypass mode means "Space is an ordinary space" (the hook passes
    // everything through), and a fallback that ignored it would make the
    // dashboard the one window where bypass does not work. The escape hatch
    // is NOT Space+`.` here — that needs a hold the fallback just refused to
    // start — it is the pause control in Settings, which is on screen.
    if bypass_active {
        return false;
    }
    // Guard 2 — the hook got there first. `MODIFIER_ACTIVE` is a live latched
    // hold; the age is the press that latched it (or one whose latch was
    // dropped by a gate, which is still a press we must not duplicate).
    if hook_hold_latched {
        return false;
    }
    // Guard 2c — PROBLEM 263, THE ARBITRATION rule C. Guard 2 asks about the
    // hook; this asks the identical question about the third witness. A middle
    // button already holding a ring owns the gesture, so a Space pressed inside
    // the dashboard while it is held is an ordinary space — exactly what rule B
    // makes it everywhere else. Without this, the ONE window where the hook is
    // deaf would be the one window where the two triggers could both fire.
    if middle_hold_active {
        return false;
    }
    // Guard 2b — PROBLEM 262 item 5. THE FALLBACK-VS-FALLBACK DEDUPE, which
    // guard 2 never was: it asks about the HOOK, and the hook is the one
    // witness that cannot double-fire this path.
    //
    // THE HOLE, from the 1.0.106 ship notes: two `own-window fallback:` lines
    // were logged in the SAME MILLISECOND for one physical press. Nothing here
    // refused the second call, so it armed a second time and injected a second
    // `OwnWindowSpaceDown`. `arm_own_window_hold` is idempotent by construction
    // (every line is a store of a constant), so the state survived it — but the
    // engine got two holds, and the second one's `SpaceUp` is the one that
    // would have typed a phantom space or launched twice.
    //
    // The predicate is `own_window_hold_is_ours` — guard 3's, deliberately the
    // same one — so the two questions can never drift apart: *a Space-down
    // arriving while the fallback still owns a hold IS that hold.* Using the
    // bounded form rather than a bare `OWN_HOLD_ACTIVE` matters: a page torn
    // down mid-hold leaves the latch set with no `keyup` coming, and a bare
    // flag would then refuse every new ring for the full 30 s until the reaper
    // caught up. An EXPIRED hold is not a duplicate of anything, so it is let
    // through and the fresh arm overwrites it.
    if own_window_hold_is_ours(own_hold_age_ms, own_hold_max_ms) {
        return false;
    }
    !matches!(hook_space_down_age_ms, Some(age) if age < dedupe_ms)
}

/// Guard 3 — a key or a release only counts while the fallback owns the hold.
pub(crate) fn own_window_hold_is_ours(own_hold_age_ms: Option<u64>, max_hold_ms: u64) -> bool {
    matches!(own_hold_age_ms, Some(age) if age <= max_hold_ms)
}

// ---------------------------------------------------------------------------
// PROBLEM 261 — the two questions pointer activation asks, answered for BOTH
// witnesses. Pure, so the whole arming decision is walkable in a test without
// a mouse, a hook or a window.
// ---------------------------------------------------------------------------

/// Is SOME hold latched right now? The mouse callback's gate and the poller's
/// `live` term both used to read `MODIFIER_ACTIVE` alone; this is the one
/// place that knows there are now THREE witnesses.
///
/// `||`, not `^`: they are mutually exclusive for a single press (see
/// `THE ARBITRATION` below), but a hold being torn down while another is taken
/// is a legal transient, and during it a hold IS latched.
#[inline(always)]
pub(crate) fn hold_latched(
    hook_latched: bool,
    own_hold_active: bool,
    middle_hold_active: bool,
) -> bool {
    hook_latched || own_hold_active || middle_hold_active
}

/// WHICH hold a poller tick belongs to. The stamp is the identity of a hold
/// (`HoldTracker` resets everything when it changes) AND the floor
/// `CURSOR_STAMP` must clear before a cursor position counts as "moved during
/// THIS hold". Both clocks are `GetTickCount64`, so they compare directly.
///
/// The fallback's stamp WINS while it owns the hold. Falling back to
/// `SPACE_DOWN_TS` — a stamp from some previous hook hold, or 0 at boot —
/// would hand the tracker a cursor position the user parked minutes ago and
/// arm a chip the user never pointed at.
///
/// PROBLEM 263 adds the middle-button stamp on the same rule and at the same
/// precedence-by-recency: a middle hold can only START when neither of the
/// other two is latched (THE ARBITRATION, below), so at most one of the three
/// `*_active` flags is true for any real press, and the order below only ever
/// decides a teardown transient.
#[inline(always)]
pub(crate) fn hold_ts_for(
    hook_ts: u64,
    own_hold_active: bool,
    own_ts: u64,
    middle_hold_active: bool,
    middle_ts: u64,
) -> u64 {
    if middle_hold_active && middle_ts != 0 {
        middle_ts
    } else if own_hold_active && own_ts != 0 {
        own_ts
    } else {
        hook_ts
    }
}

/// Why the fallback reaper tore a hold down. Named rather than boolean so the
/// log line says which bound fired, and so the test asserts the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OwnHoldReap {
    /// Our window stopped being the foreground window. Guard 1 is the ONLY
    /// reason this hold was allowed to start; re-asked, it now says no.
    ForegroundLost,
    /// `OWN_WINDOW_MAX_HOLD_MS` elapsed. The page was torn down, navigated or
    /// crashed mid-hold and its `keyup` is never coming.
    Expired,
}

/// THE FALLBACK REAPER'S DECISION, pure.
///
/// `foreground_is_own` is `None` on the ticks between foreground probes —
/// "not checked", which can never be a reason to reap. Ordering is
/// deliberate: expiry is checked FIRST, so a hold that is both expired and
/// still foreground reports the bound that is actually unrecoverable.
pub(crate) fn own_hold_reap_reason(
    active: bool,
    own_hold_age_ms: Option<u64>,
    max_hold_ms: u64,
    foreground_is_own: Option<bool>,
) -> Option<OwnHoldReap> {
    if !active {
        return None;
    }
    match own_hold_age_ms {
        // Active with no stamp is a torn state, not a hold: treat it as
        // expired so the latch cannot survive it.
        None => Some(OwnHoldReap::Expired),
        Some(age) if age > max_hold_ms => Some(OwnHoldReap::Expired),
        _ => {
            if foreground_is_own == Some(false) {
                Some(OwnHoldReap::ForegroundLost)
            } else {
                None
            }
        }
    }
}

/// Everything a hook Space-down sets that pointer activation depends on,
/// applied for a FALLBACK hold — from the IPC thread, never a callback.
///
/// IDEMPOTENT BY CONSTRUCTION (brief item 4). Every line is a store of a
/// constant, so running it twice for one press leaves exactly the state
/// running it once leaves. That matters because PROBLEM 259's dedupe is a
/// 100 ms window and a clock, not a mutex: if it ever lets two calls through
/// for one physical press, the worst outcome must be a duplicate log line,
/// not two arms or a doubled hold.
///
/// WHAT IS DELIBERATELY ABSENT, and each omission is a decision:
/// * `MODIFIER_ACTIVE` — see the long comment on `OWN_HOLD_ACTIVE`. It is the
///   arbiter; it may not be a party.
/// * `SPACE_INTERCEPTED` — the hook did not swallow this Space-down, so it
///   owes no Space-up. Setting it would make the NEXT real Space-up the hook
///   does see inject a phantom space the user never typed.
/// * `SPACE_TICK_TS` / `SPACE_REPEATS` / `SPACE_COMBO_SEEN` — the auto-repeat
///   liveness signal `reap_stale_hold` measures. A fallback hold produces no
///   hook auto-repeats, so writing those would feed that reaper evidence it
///   did not observe. The fallback's own bound is `own_hold_reap_reason`.
fn arm_own_window_hold(now: u64) {
    // Stamp BEFORE the latch, so the poller can never read active-with-no-ts.
    OWN_HOLD_TS.store(now, Ordering::Relaxed);
    OWN_HOLD_COMBO.store(false, Ordering::Relaxed);
    // A fresh hold starts unaborted, or `apply_to`'s CAS refuses every arm for
    // the whole hold (it claims `SPACE_ABORTED` false→true to arm). The hook's
    // Space-down branch does exactly this store for exactly this reason.
    SPACE_ABORTED.store(false, Ordering::Relaxed);
    // PROBLEM 206 — a fresh hold must not inherit the last hold's armed chip
    // or its wheel-block.
    pointer::on_space_down();
    OWN_HOLD_ACTIVE.store(true, Ordering::SeqCst);
}

/// The exact inverse, and the ONLY way `OWN_HOLD_ACTIVE` becomes false.
///
/// Returns whether this call is the one that owned the teardown — a
/// `swap`-based claim, so the release path, the reaper and the watchdog can
/// all race and only one logs. Also idempotent: a second call is a no-op that
/// returns `false`.
fn disarm_own_window_hold() -> bool {
    let owned = OWN_HOLD_ACTIVE.swap(false, Ordering::SeqCst);
    OWN_HOLD_TS.store(0, Ordering::Relaxed);
    OWN_HOLD_COMBO.store(false, Ordering::Relaxed);
    // PROBLEM 206 — an armed chip or a half-eaten click must not outlive the
    // hold that created it. Without this a stranded arm is what "opened
    // whatever my cursor was towards" (PROBLEM 218's owner report).
    //
    // ONLY WHEN WE OWNED ONE. `own_window_space_up` calls this on its
    // not-ours branch too (an expired hold must not survive its own release),
    // and `ARMED_INDEX` / `CLICK_EATEN` are SHARED with the hook path — a
    // stray release from a page with torn state would otherwise disarm a chip
    // a perfectly healthy HOOK hold had armed, or strand the up-half of a
    // suppressed click. If this path did not own the hold it does not own the
    // pointer latches either.
    if owned {
        pointer::reset_on_eviction();
    }
    owned
}

/// Is the fallback holding Space right now? One relaxed load; called on the
/// MOUSE callback, so it may never be anything more than that.
#[inline(always)]
pub(crate) fn own_hold_active() -> bool {
    OWN_HOLD_ACTIVE.load(Ordering::Relaxed)
}

/// The poller's hold identity, resolved across all three witnesses.
pub(crate) fn current_hold_ts() -> u64 {
    hold_ts_for(
        SPACE_DOWN_TS.load(Ordering::Relaxed),
        OWN_HOLD_ACTIVE.load(Ordering::Relaxed),
        OWN_HOLD_TS.load(Ordering::Relaxed),
        MIDDLE_HOLD_ACTIVE.load(Ordering::Relaxed),
        MIDDLE_DOWN_TS.load(Ordering::Relaxed),
    )
}

/// Is a hold latched by ANY witness? The poller's `live` term.
pub(crate) fn any_hold_latched() -> bool {
    hold_latched(
        MODIFIER_ACTIVE.load(Ordering::Relaxed),
        OWN_HOLD_ACTIVE.load(Ordering::Relaxed),
        MIDDLE_HOLD_ACTIVE.load(Ordering::Relaxed),
    )
}

/// PROBLEM 261 — the fallback's stale-hold reaper, `reap_stale_hold`'s twin.
///
/// Called from `st-hud-pointer` (an independent thread — the whole point is to
/// still be running when the page that owns the hold has stopped talking) and
/// from the hook pump's WM_TIMER branch, exactly like `reap_stale_hold`. Off
/// every callback, so logging here is legal.
///
/// `check_foreground` is the caller's throttle: the probe allocates a `String`
/// and makes three Win32 calls, which is nothing four times a second and not
/// nothing at 62 Hz.
///
/// WHY A FOREGROUND PROBE IS THE RIGHT LIVENESS SIGNAL HERE. `reap_stale_hold`
/// uses auto-repeat because the callback's own evidence is all it has. This
/// path has something better: guard 1 admitted this hold **because our window
/// was foreground**, and that condition is continuously observable from
/// outside the page. Re-asking it is the same question, not a proxy — and it
/// catches the case the page's `blur` listener cannot (the page is gone, so
/// its listener is gone with it). The 30 s bound underneath covers the
/// remaining shape: a dead page inside a window that is still foreground.
pub fn reap_own_window_hold(check_foreground: bool) -> bool {
    let reason = own_hold_reap_reason(
        OWN_HOLD_ACTIVE.load(Ordering::Relaxed),
        own_hold_age_ms(),
        OWN_WINDOW_MAX_HOLD_MS,
        check_foreground.then(foreground_is_own_window),
    );
    let Some(reason) = reason else { return false };
    // Claim it before anything else: the poller, the pump and a late
    // `own_window_space_up` all race here, and a double teardown would emit
    // two `guide-hud-hide` events.
    if !disarm_own_window_hold() {
        return false;
    }
    let hud_was_up = crate::guide_hud::is_visible();
    log::warn!(
        "own-window fallback: reaping a fallback Space-hold ({reason:?}) — the dashboard page \
         took the hold (PROBLEM 259) and never sent its release. HUD was up: {hud_was_up}. \
         Left standing this is a ring on screen with the pointer still arming chips behind \
         it, which is PROBLEM 218's failure on the path PROBLEM 218's reaper cannot see: \
         that one measures the keyboard hook's auto-repeat, and a fallback hold produces \
         none. Nothing here touches MODIFIER_ACTIVE — the fallback never latches it."
    );
    OWN_HOLDS_REAPED.fetch_add(1, Ordering::Relaxed);
    SPACE_ABORTED.store(false, Ordering::Relaxed);
    crate::guide_hud::hide_guide_hud();
    true
}

/// Drained into the 60 s diagnostics line beside `STALE_HOLDS_REAPED`.
pub(crate) fn drain_own_holds_reaped() -> u32 {
    OWN_HOLDS_REAPED.swap(0, Ordering::Relaxed)
}

/// VK → combo, for the SUBSET of the hook's map that is safe to take away
/// from a focused web page.
///
/// Deliberately smaller than the callback's map, and every omission is a
/// decision, not an oversight:
///
/// * `Escape`, `Enter`, `Tab` — the brief's exclusion list. They are how a
///   user closes a popover, submits a name and moves between fields; a
///   fallback that ate them would break the dashboard to add a shortcut.
/// * Arrows, `Backspace`, Right Alt — the same reasoning. Space+⌫ is
///   Force Close (Alt+F4) and Space+↑/↓ scroll; none is worth swallowing a
///   caret key inside a text field for.
/// * F1–F12 specials — they are gated on `BOUND_SPECIALS`, which is hook-side
///   state the page has no business re-deriving.
/// * DIGITS — `vk_to_char` maps A–Z only, so the hook produces NO event for a
///   digit either. Intercepting one would swallow a keystroke to do nothing.
///
/// What is left is what the ring actually shows: the letters, plus the three
/// punctuation combos that have no meaning in a text field beyond the
/// character they type (which the rollover window already protects).
pub(crate) fn own_window_combo_for_vk(vk: u16) -> Option<KeyCombo> {
    match vk {
        VK_OEM_3 => Some(KeyCombo::Backtick),
        VK_OEM_COMMA => Some(KeyCombo::Comma),
        VK_OEM_PERIOD => Some(KeyCombo::Period),
        VK_OEM_1 => Some(KeyCombo::Semicolon),
        VK_OEM_2 => Some(KeyCombo::Slash),
        VK_OEM_7 => Some(KeyCombo::Quote),
        v if is_alpha_vk(v) => vk_to_char(v).map(KeyCombo::Alpha),
        _ => None,
    }
}

/// The page saw Space go down. Returns `true` if the fallback took the hold.
pub(crate) fn own_window_space_down() -> bool {
    if !own_window_space_down_accepted(
        foreground_is_own_window(),
        BYPASS_MODE.load(Ordering::Relaxed),
        MODIFIER_ACTIVE.load(Ordering::Relaxed),
        hook_space_down_age_ms(),
        OWN_WINDOW_DEDUPE_MS,
        own_hold_age_ms(),
        OWN_WINDOW_MAX_HOLD_MS,
        MIDDLE_HOLD_ACTIVE.load(Ordering::Relaxed),
    ) {
        return false;
    }
    // PROBLEM 261 — arm the hold BEFORE the event goes to the engine, not
    // after. The engine's `SpaceDown` arm shows the ring after
    // `guide_hud_delay_ms`, and the poller only arms against a ring it can
    // see; but the MOUSE callback starts recording cursor positions the
    // instant the latch is set, and guard 2's travel test measures from where
    // the cursor was when the hold began. Setting the latch after the inject
    // would drop every mousemove in that window and start the travel
    // measurement late.
    arm_own_window_hold(tick_count().max(1));
    OWN_WINDOW_HOLDS.fetch_add(1, Ordering::Relaxed);
    if !inject_hook_event(HookEvent::OwnWindowSpaceDown) {
        // The channel is full or absent; the hold never reached the engine, so
        // do not leave the fallback thinking it owns one — and above all do
        // not leave `OWN_HOLD_ACTIVE` latched with no ring and no release
        // coming. Full teardown, not just the stamp.
        disarm_own_window_hold();
        return false;
    }
    true
}

/// The page saw a key go down while Space was held. Returns `true` if it was
/// dispatched as a combo (which is also the page's cue that suppressing the
/// keystroke was correct).
pub(crate) fn own_window_key(vk: u16) -> bool {
    if !own_window_hold_is_ours(own_hold_age_ms(), OWN_WINDOW_MAX_HOLD_MS) {
        return false;
    }
    let Some(combo) = own_window_combo_for_vk(vk) else {
        return false;
    };
    OWN_HOLD_COMBO.store(true, Ordering::Relaxed);
    inject_hook_event(HookEvent::KeyCombo(combo))
}

/// The page saw Space come up. Returns `true` if the fallback ended a hold it
/// owned.
pub(crate) fn own_window_space_up(had_combo: bool) -> bool {
    if !own_window_hold_is_ours(own_hold_age_ms(), OWN_WINDOW_MAX_HOLD_MS) {
        // Clear anyway: an expired hold must not survive its own release.
        disarm_own_window_hold();
        return false;
    }
    let modifier_fired = had_combo || OWN_HOLD_COMBO.load(Ordering::Relaxed);
    // PROBLEM 206 gesture A, mirrored from the hook's Space-UP branch and in
    // the same order: consume the arm FIRST, then tear the hold down.
    //
    // `take_armed_key` is the same function the callback calls, and it is the
    // ONLY consumer of `ARMED_INDEX` — so a chip armed under a fallback hold
    // launches on release exactly as it does under a hook hold. It also sets
    // `HOLD_BLOCKED`, which is what stops a click-then-release activating
    // twice; `disarm_own_window_hold` clearing that a moment later is fine,
    // because the hold is over by then.
    //
    // NO SPACE IS INJECTED HERE, and that is not an omission: the hook types
    // the space on release because it swallowed the down-stroke, and this path
    // never did — the browser already inserted (or the page already took back)
    // the character. See PROBLEM 259, "KNOWN DIVERGENCE FROM THE HOOK".
    let ev = match pointer::take_armed_key() {
        Some(ch) => HookEvent::PointerActivate(ch),
        None => HookEvent::SpaceUp { modifier_fired },
    };
    disarm_own_window_hold();
    inject_hook_event(ev)
}

/// The hold number, for the one log line the fallback prints per hold.
pub(crate) fn own_window_hold_count() -> u32 {
    OWN_WINDOW_HOLDS.load(Ordering::Relaxed)
}

// ═══════════════════════════════════════════════════════════════════════════
// PROBLEM 263 — THE MIDDLE MOUSE BUTTON AS A SECOND RING TRIGGER
//
// Hold the middle mouse button and the Guide HUD ring comes up, in exactly the
// place and shape holding Space raises it. Release over a chip and it launches;
// tap a bound letter while holding and that launches; left-click a chip and
// that launches. None of that is new code — the ring and the engine do not care
// which gesture raised them, and this section's whole job is to be a THIRD
// witness that speaks the same language the other two already speak.
//
// ───────────────────────────────────────────────────────────────────────────
// THE ARBITRATION — ONE PLACE, AND THIS IS IT.
//
// Three witnesses can now say "a hold is live": the keyboard hook
// (`MODIFIER_ACTIVE`), the own-window fallback (`OWN_HOLD_ACTIVE`, PROBLEM
// 259/261) and the middle button (`MIDDLE_HOLD_ACTIVE`). Two of them firing for
// one gesture would mean two rings, two launches and two toasts. The rule is
// stated once, here, and enforced in exactly three places that each name this
// comment:
//
//   A. A MIDDLE-BUTTON HOLD MAY NOT START WHILE EITHER SPACE HOLD IS LIVE.
//      `middle_button_down_accepted` refuses when `MODIFIER_ACTIVE` or
//      `OWN_HOLD_ACTIVE` is set, so the `WM_MBUTTONDOWN` passes straight
//      through to the app underneath, untouched. Holding Space and clicking the
//      middle button is therefore byte-identical to what it was before this
//      feature existed.
//   B. A SPACE PRESS WHILE A MIDDLE HOLD IS LIVE IS AN ORDINARY SPACE.
//      `kb_hook_proc`'s SPACE-DOWN branch returns `CallNextHookEx` when
//      `MIDDLE_HOLD_ACTIVE` is set, beside the existing `other_modifier_down()`
//      pass-through and for the same reason: it never sets `MODIFIER_ACTIVE`,
//      never swallows the key, and never owes an up. Space types a space.
//   C. THE OWN-WINDOW FALLBACK REFUSES A HOLD WHILE A MIDDLE HOLD IS LIVE.
//      A new guard in `own_window_space_down_accepted`, alongside guard 2's
//      identical question about the hook.
//
// The three are mutually exclusive by construction, not by timing: each asks
// about a latch that is already set before the competing path can be entered.
// `hold_latched` is the one function that knows there are three of them, and
// every consumer (the mouse gate, the pointer poller's `live` term, the hold
// identity) goes through it.
//
// PROBLEM 267 — THE RING KIND CHANGES NONE OF THIS. `middle_ring_style`
// decides, on the ENGINE thread, whether a `MiddleButtonDown` becomes the
// centred Guide HUD (phase 1, `SpaceDown` normalisation untouched) or the
// cursor-anchored icon ring (`guide_hud::show_middle_ring`). The witness is
// the same `MIDDLE_HOLD_ACTIVE` either way, rules A/B/C read it unchanged,
// and the release path is the same `on_middle_button_up` — a tile armed on
// the icon ring comes back through the same `take_armed_key` as a chip armed
// on the Space ring. Two things this feature DID add on the mouse path, both
// atomics-only: the App-exceptions gate above the `WM_MBUTTONDOWN` branch now
// lets that branch (and a live middle hold) through where only Space stands
// down (`exclusions::MIDDLE_EXCLUDED_ACTIVE` is the middle button's own
// verdict), and `pointer::RING_ACTIVE` tells the poller which hit test to
// run. No new witness, no new latch.
//
// ───────────────────────────────────────────────────────────────────────────
// TAP vs HOLD, AND WHY THE CLICK IS REPLAYED RATHER THAN PASSED
//
// The `WM_MBUTTONDOWN` is SUPPRESSED. It has to be: letting it through starts
// the browser's autoscroll (or the CAD program's orbit) at the same instant we
// start a ring, and the two cannot share the gesture. So this code owes the
// world a middle click whenever the press turns out to have been one — the
// exact contract `SPACE_INTERCEPTED` carries for the spacebar, and the exact
// reason `MIDDLE_UP_OWED` exists: **whoever eats the down owes the up.**
//
// The decision is made at RELEASE, not by a timer, which is what makes it a
// true mirror of Space: Space-down starts the HUD timer, and a Space released
// before the ring appears simply types a space and cancels the pending show.
// So a middle press injects `MiddleButtonDown` immediately (the ring timer
// starts, `guide_hud_delay_ms` and all), and the release decides:
//
//   * released inside `MIDDLE_TAP_MS` with nothing else claiming the press →
//     `MiddleButtonTap`: the engine replays a real middle click, down and up in
//     ONE `SendInput` batch (keyboard law 2), tagged with the `0x7A7A7A7A`
//     cookie so our own hook passes it through, and cancels the pending ring.
//     Browsers still open links in a new tab and still close tabs.
//   * anything else → the ordinary Space-release path: `PointerActivate` if a
//     chip is armed, `SpaceUp` otherwise.
//
// "Nothing else claiming the press" is `SPACE_ABORTED`, and reusing that flag
// rather than inventing one is the point: it is already set by a combo, by the
// wheel and by arming a chip, which is precisely the set of things that must
// stop a click being replayed. It is the same flag, asking the same question,
// that decides whether a Space release types a space.
//
// THE TRADE, ACCEPTED BY THE OWNER: holding the middle button past
// `MIDDLE_TAP_MS` raises the ring instead of starting browser autoscroll. The
// mitigation is `orbit_apps.rs` (3D, CAD and design programs never see this at
// all) plus the switch in Settings.
//
// ───────────────────────────────────────────────────────────────────────────
// NOTHING HERE MAY BE READ FROM THE CALLBACK EXCEPT AN ATOMIC
//
// Every decision function below is pure. The callback loads atomics, calls one
// of them, and stores atomics. The replay `SendInput`, the exclusion lookup and
// every log line happen on the engine thread or on a poller — never inside
// `ms_hook_proc`, which is the one hook in this process that makes no win32k
// call at all and is therefore the one that survives when the keyboard hooks
// are evicted (keyboard law 7b). Do not spend its budget.
// ═══════════════════════════════════════════════════════════════════════════

/// How long the middle button may be held before the press stops being a
/// click.
///
/// 250 ms. A deliberate click is 60–150 ms; Windows' own double-click window
/// is 500 ms, so 250 sits clear of one and inside the other. It is also below
/// the 300 ms default `guide_hud_delay_ms`, which means a click that is going
/// to be replayed has normally not drawn a ring at all — the two thresholds
/// were chosen to compose, and a user who shortens the HUD delay below this
/// simply sees the ring flash before their click lands, exactly as a slow
/// Space tap flashes the ring before typing its space.
///
/// The two ways to be wrong are not symmetric. Too LOW and a slow clicker's
/// click is swallowed and replaced by a ring — visible, annoying, recoverable.
/// Too HIGH and a genuine hold fires a click into the app underneath —
/// visible, and possibly destructive. 250 errs toward the first.
pub(crate) const MIDDLE_TAP_MS: u64 = 250;

/// The middle hold's own stuck-latch bound, mirroring `MAX_MODIFIER_HOLD_MS`
/// and `OWN_WINDOW_MAX_HOLD_MS`. Same value as both, and for the same reason:
/// people hold the trigger and READ the ring.
///
/// A middle hold whose `WM_MBUTTONUP` never arrives — alt-tab into an elevated
/// window (UIPI stops delivering to a non-elevated hook), the app under the
/// cursor crashing, an RDP disconnect, the hook being evicted — is
/// unfalsifiable from inside the process the moment the mouse callback stops
/// being called. This bound is the last thing standing between that and a ring
/// nothing can hide. See `middle_hold_reap_reason`.
pub(crate) const MIDDLE_MAX_HOLD_MS: u64 = 30_000;

/// How long the MOUSE callback must have been silent, while the OS was
/// accepting input it cannot account for, before a latched middle hold is torn
/// down on deafness evidence alone.
///
/// 3000 ms — the same figure as `HOLD_DEAF_SILENCE_MS`, and chosen the same
/// way: a hold is the one state where a false positive costs a live press, so
/// this path is asked for twice the silence the no-hold forced repair is.
pub(crate) const MIDDLE_DEAF_SILENCE_MS: u64 = 3_000;

/// PROBLEM 263 — is the middle-button trigger switched on?
///
/// The config mirror, read on the mouse callback as one relaxed load. Starts
/// FALSE even though the SETTING defaults to ON, for the identical reason
/// spelled out on `POINTER_HUD_ACTIVATION`: this atomic is the runtime mirror,
/// not the default. Seeding it `true` would mean a config that says OFF is
/// briefly honoured as ON — a window in which a middle click could be
/// swallowed by a feature the user turned off.
pub static MIDDLE_BUTTON_RING: AtomicBool = AtomicBool::new(false);

/// Publish the middle-button setting for the hook.
///
/// PROBLEM 180's rule, sixth instance: MUST be called from BOTH the startup
/// config load (lib.rs) AND `config::save` — the one funnel every mutation
/// path goes through, `reset_config` and friends included. The atomic starts
/// false, so skipping the startup call is the silent failure where the feature
/// works all session and then reads OFF for the entire next launch.
pub fn publish_middle_button_ring(cfg: &crate::config::AppConfig) {
    let on = cfg.middle_button_ring;
    let prev = MIDDLE_BUTTON_RING.swap(on, Ordering::Relaxed);
    if prev != on {
        log::info!(
            "hook: the middle-button ring trigger is now {} (PROBLEM 263)",
            if on { "ON" } else { "OFF" }
        );
    }
}

/// Tick of the `WM_MBUTTONDOWN` this process swallowed, or 0 for "no middle
/// hold". Written by the mouse callback and by the teardown paths.
static MIDDLE_DOWN_TS: AtomicU64 = AtomicU64::new(0);

/// The latch. `true` from the swallowed `WM_MBUTTONDOWN` until the release,
/// the reaper or a repair ends it.
///
/// **IT IS NOT `MODIFIER_ACTIVE`, AND IT MUST NEVER BE.** `MODIFIER_ACTIVE`
/// means "the keyboard hook is holding a Space it swallowed": it is what the
/// hook's Space-UP branch clears, what `reap_stale_hold`'s auto-repeat evidence
/// is about, what `proven_keyboard_deaf` suppresses itself on, and what THE
/// ARBITRATION reads to decide who owns a press. A middle hold that set it
/// would owe a Space-up nobody is going to send (PROBLEM 262's exact wedge),
/// and would make itself invisible to arbitration rule B. Same decision, same
/// reasoning, as `OWN_HOLD_ACTIVE` — see the comment there.
static MIDDLE_HOLD_ACTIVE: AtomicBool = AtomicBool::new(false);

/// We swallowed a `WM_MBUTTONDOWN`, so we owe its `WM_MBUTTONUP`.
///
/// The middle-button twin of `SPACE_INTERCEPTED` and of `pointer::CLICK_EATEN`,
/// and it is checked ABOVE every gate in `ms_hook_proc` for the reason PROBLEM
/// 218 wrote down for the keyboard: a gate that flips mid-hold must not be able
/// to strand the up-half of a press whose down-half we already ate.
static MIDDLE_UP_OWED: AtomicBool = AtomicBool::new(false);

/// How many middle-button holds have begun this session. The number in the log
/// line, so "which gesture raised this ring?" is answerable by grep.
static MIDDLE_HOLDS: AtomicU32 = AtomicU32::new(0);

/// Middle presses replayed as ordinary clicks. Drained into the 60 s
/// diagnostics line.
static MIDDLE_TAPS_REPLAYED: AtomicU32 = AtomicU32::new(0);

/// Middle holds the reaper had to tear down. Non-zero is the fingerprint of a
/// lost `WM_MBUTTONUP`; read it as a bug report, not as health.
static MIDDLE_HOLDS_REAPED: AtomicU32 = AtomicU32::new(0);

/// THE MIDDLE-BUTTON DOWN GATE, as a pure function — every reason the trigger
/// may decline, in ONE place a test can walk. There is no second gate and there
/// must never be one.
///
/// **There WAS a second one, briefly, and deleting it is a decision worth
/// recording (2026-09-10).** A helper `middle_trigger_armed()` folded
/// `feature_on && watcher_alive` into a single bool, and `orbit_apps.rs`'s
/// header documented it as *the* place the watcher gate lives — while nothing
/// in the process ever called it. The crate carries `#![allow(dead_code)]`
/// (lib.rs line 1), so neither rustc nor clippy said a word. Two things were
/// wrong with it and only one was the deadness: collapsing two independent
/// reasons into one bool means a caller that declines can no longer say WHICH
/// reason declined it, and "the feature is switched off" and "no 3D/CAD verdict
/// has ever been measured" are the two reasons a user is most likely to have to
/// tell apart. They are separate parameters here for exactly that reason.
/// Generalise: **a documented entry point that nothing calls is worse than no
/// entry point — a reader who greps it finds a function and cannot tell whether
/// the gate runs.**
///
/// Ordered cheapest-first, and each argument is a single relaxed atomic load at
/// the call site. Nothing here queries a window, allocates or locks.
#[allow(clippy::too_many_arguments)]
pub(crate) fn middle_button_down_accepted(
    feature_on: bool,
    watcher_alive: bool,
    orbit_app_active: bool,
    user_excluded: bool,
    bypass_active: bool,
    fullscreen_active: bool,
    hook_hold_latched: bool,
    own_hold_active: bool,
    middle_hold_active: bool,
) -> bool {
    // The switch in Settings. Off means the middle button is a middle button.
    if !feature_on {
        return false;
    }
    // No 3D/CAD verdict has ever been measured — see `orbit_apps::WATCHER_ALIVE`.
    // Fail toward stock behaviour, never toward eating the orbit gesture.
    if !watcher_alive {
        return false;
    }
    // The BUILT-IN list: SolidWorks, Blender, AutoCAD and the rest keep their
    // middle-drag orbit. This is NOT the user's App exceptions and it gates the
    // middle button ONLY — Space still works in there.
    if orbit_app_active {
        return false;
    }
    // The user's OWN App exceptions gate this exactly as they gate everything
    // else. Inside an app the user listed, Spaceadom decides nothing at all.
    if user_excluded {
        return false;
    }
    // Bypass mode means "Spaceadom is paused". A trigger that ignored it would
    // make the pause control a lie. The escape hatch out of bypass is Space+`.`
    // and the control in Settings, neither of which needs this.
    if bypass_active {
        return false;
    }
    // The fullscreen/game gate, same as everywhere else.
    if fullscreen_active {
        return false;
    }
    // THE ARBITRATION, rule A — a Space hold from EITHER witness owns the
    // gesture, and the middle button passes straight through untouched.
    if hook_hold_latched || own_hold_active {
        return false;
    }
    // And a middle hold cannot start twice. `WM_MBUTTONDOWN` can repeat if a
    // release was lost; the second one is not a new press.
    !middle_hold_active
}

/// Should the release replay a real middle click?
///
/// `aborted` is `SPACE_ABORTED`: set by a combo, by the wheel and by arming a
/// chip — the exact set of things that mean "something else claimed this
/// press". Identical in shape to the `!SPACE_ABORTED` test that decides whether
/// a Space release types a space, and deliberately so.
#[inline(always)]
pub(crate) fn middle_press_was_a_click(held_ms: u64, tap_ms: u64, aborted: bool) -> bool {
    held_ms < tap_ms && !aborted
}

/// Why the middle-hold reaper tore a hold down. Named rather than boolean so
/// the log says which instrument spoke and a test can assert the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MiddleHoldReap {
    /// The mouse callback is not being called at all, so the `WM_MBUTTONUP`
    /// that would end this hold can never arrive. PROBLEM 262's lesson applied
    /// to the other hook: *an instrument that can only be read by the thing
    /// that has failed is not an instrument.*
    MouseDeaf,
    /// `MIDDLE_MAX_HOLD_MS` elapsed. Nothing proved anything; the hold is
    /// simply older than any real one.
    Expired,
}

/// THE MIDDLE-HOLD REAPER'S DECISION, pure.
///
/// **`GetAsyncKeyState(VK_MBUTTON)` IS NOT AN OPTION HERE, AND THAT IS
/// KEYBOARD LAW 3, NOT AN OVERSIGHT.** We SUPPRESS the `WM_MBUTTONDOWN`, so
/// Windows never records the button as pressed and `GetAsyncKeyState` reports
/// it UP for the whole of a perfectly live hold. Building the liveness test on
/// it would tear down every hold on its first tick — the identical mistake that
/// once broke every shortcut in this app (see the FAILSAFE comment in
/// `kb_hook_proc`). The evidence below is our own bookkeeping and the OS's own
/// input clock, never the key state of a key we are hiding.
///
/// `MouseDeaf` is checked first because it fires at 3 s and `Expired` at 30 s:
/// in practice the bound only ever speaks when deafness could not be proven
/// (the user genuinely stopped touching the machine mid-hold, so there is no
/// unaccounted-for input to prove anything with).
pub(crate) fn middle_hold_reap_reason(
    active: bool,
    hold_age_ms: u64,
    max_hold_ms: u64,
    ms_callback_silence_ms: Option<u64>,
    os_input_age_ms: u64,
    deaf_silence_ms: u64,
    os_input_max_age_ms: u64,
) -> Option<MiddleHoldReap> {
    if !active {
        return None;
    }
    // PROOF: the OS accepted input recently AND our mouse callback has been
    // silent far longer than that. The callback stamps `LAST_MS_CALLBACK` on
    // EVERY mouse event above every gate, so silence past the threshold means
    // it was not entered at all — the hook is installed and is not being
    // called (keyboard law 7a, seen from the mouse hook's side).
    //
    // `None` is UNKNOWN, never proof (PROBLEM 228): a callback that has not
    // fired since the last install cannot be measured, and that case belongs to
    // the bound below.
    if os_input_age_ms <= os_input_max_age_ms
        && matches!(ms_callback_silence_ms, Some(ms) if ms >= deaf_silence_ms)
    {
        return Some(MiddleHoldReap::MouseDeaf);
    }
    if hold_age_ms > max_hold_ms {
        return Some(MiddleHoldReap::Expired);
    }
    None
}

/// Everything a hook Space-down sets that the ring and pointer activation
/// depend on, applied for a MIDDLE-BUTTON hold.
///
/// IDEMPOTENT BY CONSTRUCTION, like `arm_own_window_hold`: every line is a
/// store of a constant, so running it twice for one press leaves exactly what
/// running it once leaves.
///
/// WHAT IS DELIBERATELY ABSENT, and each omission is a decision:
/// * `MODIFIER_ACTIVE` — see `MIDDLE_HOLD_ACTIVE`. It is the arbiter; it may
///   not be a party.
/// * `SPACE_INTERCEPTED` — no Space was swallowed, so none is owed. Setting it
///   would make the next real Space-up inject a phantom space.
/// * `SPACE_TICK_TS` / `SPACE_REPEATS` / `SPACE_COMBO_SEEN` — the keyboard
///   auto-repeat evidence `reap_stale_hold` measures. A middle hold produces
///   none of it; writing those would feed that reaper evidence it never saw.
#[inline(always)]
fn arm_middle_hold(now: u64) {
    // Stamp BEFORE the latch, so the poller can never read active-with-no-ts.
    MIDDLE_DOWN_TS.store(now, Ordering::Relaxed);
    MIDDLE_UP_OWED.store(true, Ordering::Relaxed);
    // A fresh hold starts unaborted, or `pointer::apply_to`'s CAS refuses every
    // arm for the whole hold. The hook's Space-down branch does exactly this
    // store for exactly this reason.
    SPACE_ABORTED.store(false, Ordering::Relaxed);
    // A fresh hold must not inherit the last hold's armed chip or wheel-block.
    pointer::on_space_down();
    MIDDLE_HOLD_ACTIVE.store(true, Ordering::SeqCst);
}

/// The exact inverse, and the ONLY way `MIDDLE_HOLD_ACTIVE` becomes false.
///
/// Returns whether this call owned the teardown — a `swap`-based claim, so the
/// release path, the reaper, the repair and the watchdog can all race and only
/// one of them logs or hides a ring.
///
/// `MIDDLE_UP_OWED` is cleared here, and the trade is deliberate. Leaving it
/// set would mean the NEXT genuine middle click — one whose down we passed
/// through — has its UP eaten, which breaks a click the user is entitled to.
/// Clearing it means a physical up that arrives after a teardown reaches the
/// app with no matching down, which for the middle button is inert (nothing
/// starts a drag on an up). Same choice, same reasoning, as
/// `pointer::reset_on_eviction` makes for `CLICK_EATEN`.
fn disarm_middle_hold() -> bool {
    let owned = MIDDLE_HOLD_ACTIVE.swap(false, Ordering::SeqCst);
    MIDDLE_DOWN_TS.store(0, Ordering::Relaxed);
    MIDDLE_UP_OWED.store(false, Ordering::Relaxed);
    if owned {
        // An armed chip or a half-eaten click must not outlive the hold that
        // created it. ONLY when we owned one: `ARMED_INDEX` and `CLICK_EATEN`
        // are SHARED with the Space paths, and a stray teardown must not disarm
        // a chip a healthy Space hold armed.
        pointer::reset_on_eviction();
    }
    owned
}

/// Is the middle button holding a ring right now? One relaxed load; called on
/// the KEYBOARD callback (arbitration rule B), so it may never be more.
#[inline(always)]
pub(crate) fn middle_hold_active() -> bool {
    MIDDLE_HOLD_ACTIVE.load(Ordering::Relaxed)
}

fn middle_hold_age_ms() -> Option<u64> {
    let ts = MIDDLE_DOWN_TS.load(Ordering::Relaxed);
    if ts == 0 {
        return None;
    }
    Some(tick_count().saturating_sub(ts))
}

/// PROBLEM 263 — the middle hold's stale-hold reaper, twin of
/// `reap_stale_hold` and `reap_own_window_hold`.
///
/// Called from `st-hud-pointer` (an independent thread — the whole point is to
/// still be running when the hook thread's own hooks have been evicted) and
/// from the hook pump's `WM_TIMER` branch (so it still runs when that watcher
/// failed to spawn, PROBLEM 124). Two homes, two different failure modes; both
/// are off every callback, so logging here is legal.
///
/// `probe` is the caller's throttle: the deafness evidence costs two Win32
/// calls and is only useful at the resolution of a 3 s bound.
pub fn reap_middle_hold(probe: bool) -> bool {
    if !MIDDLE_HOLD_ACTIVE.load(Ordering::Relaxed) {
        // Cheapest possible exit, and it must stay first: with no hold latched
        // there is nothing to prove and no Win32 call worth making.
        return false;
    }
    let (ms_silence, os_input_age) = if probe {
        middle_deaf_evidence()
    } else {
        // Not measured. `None` + a huge input age can never satisfy the
        // deafness arm, which leaves only the 30 s bound — exactly the
        // behaviour an unthrottled tick would have had before the probe.
        (None, u64::MAX)
    };
    let Some(reason) = middle_hold_reap_reason(
        true,
        middle_hold_age_ms().unwrap_or(u64::MAX),
        MIDDLE_MAX_HOLD_MS,
        ms_silence,
        os_input_age,
        MIDDLE_DEAF_SILENCE_MS,
        FORCED_INPUT_MAX_AGE_MS,
    ) else {
        return false;
    };
    let age = middle_hold_age_ms().unwrap_or(0);
    // Claim it before anything else: the poller, the pump and a late
    // `WM_MBUTTONUP` all race here, and a double teardown would emit two
    // `guide-hud-hide` events.
    if !disarm_middle_hold() {
        return false;
    }
    let hud_was_up = crate::guide_hud::is_visible();
    MIDDLE_HOLDS_REAPED.fetch_add(1, Ordering::Relaxed);
    log::warn!(
        "middle-button ring: reaping-a-latched-middle-button-hold-spaceadom ({reason:?}) — the \
         middle button raised a ring {age}ms ago and its WM_MBUTTONUP never arrived (HUD was up: \
         {hud_was_up}). That happens when the button comes up over a window this non-elevated \
         hook is not delivered to (UIPI), when the app under the cursor dies mid-press, on an RDP \
         disconnect, or when the mouse hook itself stops being called. Left standing it is a ring \
         nothing can hide, with the pointer still arming chips behind it — PROBLEM 218's failure \
         on a path PROBLEM 218's reaper cannot see, because that one measures the KEYBOARD hook's \
         auto-repeat and a middle hold produces none. Note what is NOT used to decide this: \
         GetAsyncKeyState(VK_MBUTTON) reports a button we SUPPRESS as UP (keyboard law 3), so it \
         would tear down every live hold on its first tick. PROBLEM 263."
    );
    SPACE_ABORTED.store(false, Ordering::Relaxed);
    crate::guide_hud::hide_guide_hud();
    true
}

/// The deafness evidence a middle hold needs, gathered OFF every callback.
///
/// Returns `(ms_callback_silence_ms, os_input_age_ms)`. Scoped to the current
/// install exactly as `deaf_evidence_for_reap` scopes its clocks — a callback
/// stamp from before the last `install_hooks()` says nothing about this hook.
#[cfg(windows)]
fn middle_deaf_evidence() -> (Option<u64>, u64) {
    let t = tick_count();
    let installed_at = HOOKS_INSTALLED_AT.load(Ordering::Relaxed);
    if t.saturating_sub(installed_at) < INSTALL_GRACE_MS {
        // A hook that has not had a fair chance to be called cannot be proven
        // deaf — the same first rule `proven_keyboard_deaf` opens with.
        return (None, u64::MAX);
    }
    let cb = LAST_MS_CALLBACK.load(Ordering::Relaxed);
    let silence = (cb != 0 && cb >= installed_at).then(|| t.saturating_sub(cb));
    (silence, millis_since_last_input())
}

#[cfg(not(windows))]
fn middle_deaf_evidence() -> (Option<u64>, u64) {
    (None, u64::MAX)
}

/// The middle button went down and we swallowed it. Called from the mouse
/// callback: atomics and one `send_event`, nothing else.
///
/// Split out of `ms_hook_proc` so the whole arming step is one named thing the
/// reader can check against `arm_own_window_hold`, and so the callback body
/// stays a list of guards.
#[inline(always)]
fn on_middle_button_down(now: u64) {
    arm_middle_hold(now);
    MIDDLE_HOLDS.fetch_add(1, Ordering::Relaxed);
    send_event(HookEvent::MiddleButtonDown);
}

/// The middle button came up and we owe its release. Called from the mouse
/// callback; returns the event to send, or `None` when there is nothing to say.
#[inline(always)]
fn on_middle_button_up(now: u64) -> Option<HookEvent> {
    let ts = MIDDLE_DOWN_TS.load(Ordering::Relaxed);
    let held = if ts == 0 { u64::MAX } else { now.saturating_sub(ts) };
    let aborted = SPACE_ABORTED.load(Ordering::Relaxed);
    let click = middle_press_was_a_click(held, MIDDLE_TAP_MS, aborted);
    // PROBLEM 206 gesture A, mirrored from the hook's Space-UP branch and in
    // the same order: consume the arm FIRST, then tear the hold down. A chip
    // armed under a middle hold launches on release exactly as it does under a
    // Space hold, through the SAME `take_armed_key`.
    //
    // A click can never have armed anything — arming sets `SPACE_ABORTED`, and
    // `click` is false whenever that is set — so the two branches cannot both
    // be true. Asked in this order anyway, because "cannot happen" is how a
    // launch and a replayed click end up both firing.
    let ev = if click {
        MIDDLE_TAPS_REPLAYED.fetch_add(1, Ordering::Relaxed);
        HookEvent::MiddleButtonTap
    } else {
        match pointer::take_armed_key() {
            Some(ch) => HookEvent::PointerActivate(ch),
            None => HookEvent::SpaceUp { modifier_fired: aborted },
        }
    };
    if !disarm_middle_hold() {
        // Something else (the reaper, a repair) already ended this hold. We
        // still owed the up and have now eaten it, but there is no ring left to
        // take down and no click to replay for a press that is no longer ours.
        return None;
    }
    Some(ev)
}

/// PROBLEM 263 — replay the middle click this process swallowed.
///
/// ONE `SendInput` BATCH, DOWN AND UP TOGETHER — keyboard law 2. `SendInput`
/// followed by anything else does not preserve order (the `hte`-for-`the` bug),
/// and a down that lands without its up leaves the middle button latched in
/// whatever has focus, which for a browser is autoscroll running with no way to
/// stop it. `send_keys_raw`'s corrective-KEYUP repair only knows about keyboard
/// events, so this batch's atomicity is the only protection there is: it is one
/// call, and a short insert is reported rather than half-repaired.
///
/// Tagged with the `0x7A7A7A7A` cookie, so `ms_hook_proc` recognises it as ours
/// and passes it through instead of treating it as a fresh press.
///
/// NO CURSOR MOVE IS SENT. Without `MOUSEEVENTF_ABSOLUTE`/`MOVE` the click
/// lands wherever the cursor is NOW, which is where the user let go. If they
/// moved the mouse during the press the click lands at the release point rather
/// than the press point — for a click that is the right answer (the app sees a
/// clean down+up at one place) and for a drag it does not matter, because a
/// drag is not a click and never reaches here.
///
/// ENGINE THREAD ONLY. This is a win32k call and `ms_hook_proc` is the one hook
/// in this process that makes none — see THE ARBITRATION's closing paragraph.
#[cfg(windows)]
pub(crate) fn replay_middle_click() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
        MOUSEINPUT,
    };
    let mk = |flags| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: MAGIC_INJECTED_MOUSE,
            },
        },
    };
    let batch = [mk(MOUSEEVENTF_MIDDLEDOWN), mk(MOUSEEVENTF_MIDDLEUP)];
    let sent = unsafe { SendInput(&batch, std::mem::size_of::<INPUT>() as i32) } as usize;
    if sent == batch.len() {
        return true;
    }
    // PROBLEM 227's discipline, applied to the one mouse batch this app sends.
    // `SendInput` inserts events ONE AT A TIME and stops at the first one
    // another thread blocks, returning a SHORT COUNT — and a short count of
    // exactly 1 here means the DOWN went in and the UP did not, which leaves
    // the middle button physically latched in whatever has focus. For a browser
    // that is autoscroll running with no way to stop it: the worst outcome this
    // feature can produce. `unreleased_keys_into` cannot help — it decodes
    // keyboard events — so the repair is written out, and it is one event
    // because the batch is two.
    if sent == 1 {
        let up = [mk(MOUSEEVENTF_MIDDLEUP)];
        let _ = unsafe { SendInput(&up, std::mem::size_of::<INPUT>() as i32) };
    }
    false
}

#[cfg(not(windows))]
pub(crate) fn replay_middle_click() -> bool {
    true
}

/// The hold number, for the one line the middle-button trigger prints per hold.
pub(crate) fn middle_hold_count() -> u32 {
    MIDDLE_HOLDS.load(Ordering::Relaxed)
}

/// Drained into the 60 s diagnostics line beside `STALE_HOLDS_REAPED`.
pub(crate) fn drain_middle_counters() -> (u32, u32) {
    (
        MIDDLE_TAPS_REPLAYED.swap(0, Ordering::Relaxed),
        MIDDLE_HOLDS_REAPED.swap(0, Ordering::Relaxed),
    )
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

/// PROBLEM 230 — the two remaining ways this subtraction lied, and the shape of
/// both fixes.
///
/// PROBLEM 228 made the reference counter unforgeable by the repair. It did not
/// make the two sides of `classify_hook_window` count the same events, and it
/// did not stop Windows from evicting the reference BEFORE the primary. These
/// tests pin both properties. They are arithmetic, not behaviour: the fixes
/// live where the counters are written (`kb_hook_proc`'s add, moved above the
/// injected-cookie test; `install_hooks()`, which now installs the reference
/// FIRST so it lands at the tail of the chain).
#[cfg(test)]
mod deaf_instrument_population_tests {
    use super::{classify_hook_window, HookWindow};

    /// A 60-second window whose ONLY keyboard traffic was this app injecting to
    /// itself — `inject_space`, or `force_foreground`'s synthetic tap, both
    /// reachable from a tray click with nobody's hands on the keyboard.
    ///
    /// The reference hook counts on `n_code >= 0` and has never had a cookie
    /// test, so it counted those. The primary's add used to sit BELOW its
    /// `dwExtraInfo == MAGIC_INJECTED` early return, so it did not. The window
    /// arrived here as `(0, n)` — DEAF, at WARN, with a Sentry event, describing
    /// a hook that was working perfectly.
    #[test]
    fn a_window_of_only_our_own_injections_must_not_read_as_deafness() {
        let injected = 4u32;
        // What the old code produced.
        assert_eq!(classify_hook_window(0, injected), HookWindow::Deaf);
        // What it produces now that the add counts every callback entry.
        assert_eq!(classify_hook_window(injected, injected), HookWindow::Working);
    }

    /// The general property, stated as arithmetic so it cannot be argued with:
    /// while both counters count the same events, a live primary can never be
    /// called deaf — for ANY amount of traffic.
    #[test]
    fn equal_populations_can_never_produce_a_deaf_verdict() {
        for events in 1..64u32 {
            assert_eq!(classify_hook_window(events, events), HookWindow::Working);
        }
    }

    /// And the converse: every event the primary is not credited with is an
    /// event that pushes the verdict toward DEAF. One dropped from the
    /// numerator is enough, which is why the fix had to be at the counter.
    #[test]
    fn any_shortfall_in_the_primary_population_alone_manufactures_deafness() {
        let real_traffic = 12u32;
        assert_eq!(classify_hook_window(real_traffic, real_traffic), HookWindow::Working);
        assert_eq!(classify_hook_window(0, real_traffic), HookWindow::Deaf);
    }

    /// THE ORDERING FIX, as arithmetic.
    ///
    /// `CallNextHookEx` is synchronous, so a hook's measured duration includes
    /// every hook below it. Installed LAST, the reference sat at the head of the
    /// chain carrying the primary's whole callback on its own `LowLevelHooks-
    /// Timeout` clock, and Windows evicted it first. Measured on 1.0.95: the
    /// reference counter frozen at 2237 for 6¼ minutes while the primary logged
    /// `saw 49 key event(s)`.
    ///
    /// An evicted reference makes `genuine_ref_events` zero, so the verdict
    /// collapses to Quiet — the instrument goes BLIND rather than loud. That is
    /// why 22 hours of 1.0.95 produced zero DEAF lines while the watchdog raised
    /// 514 alarms: absence of the alarm was never evidence of health.
    #[test]
    fn an_evicted_reference_hook_reports_quiet_not_deaf_so_silence_proves_nothing() {
        assert_eq!(classify_hook_window(0, 0), HookWindow::Quiet);
        assert_ne!(classify_hook_window(0, 0), HookWindow::Deaf);
    }

    /// WHY THE TAIL POSITION COSTS NOTHING. At the tail the reference cannot see
    /// a key the primary SUPPRESSED — and that is free, because of when its
    /// count is read. `classify_hook_window` consults it only on the
    /// `primary_seen == 0` branch, and a primary that saw nothing suppressed
    /// nothing, so the whole stream reached the tail. Deafness is still detected
    /// at full strength.
    #[test]
    fn a_silent_primary_suppressed_nothing_so_the_tail_reference_sees_everything() {
        for arrived in 1..32u32 {
            // primary_seen == 0 ⟹ suppressed == 0 ⟹ the tail saw all of them.
            let tail_ref_events = arrived;
            assert_eq!(classify_hook_window(0, tail_ref_events), HookWindow::Deaf);
        }
    }
}

/// PROBLEM 236 — the instrument panel, and the two questions the 1.0.96 log
/// could not answer.
///
/// Baseline, measured on the owner's machine over 38 minutes of 1.0.96
/// (2026-09-04 16:28:47 → 17:06:17, the build that shipped PROBLEM 230's
/// install-order fix): **16 watchdog alarms, every one of them `both_dead`,
/// ZERO `kb_only_dead`, ZERO DEAF lines.** PROBLEM 230 held — the reference
/// counter climbed 6 → 395 → 484 → 588 → 701 → 730 and its silence tracked the
/// primary's exactly whenever a key flowed (`kb 7922ms` / `ref 7922ms` at
/// 16:59:39.641; `kb 3235ms` / `ref 3235ms` at 16:59:01.636). What did NOT hold
/// is the sentence `both_dead` prints: "NEITHER hook saw anything", asserted
/// from two clocks that `install_hooks()` and the watchdog's own idle
/// early-return both write.
///
/// These tests are arithmetic, not behaviour. The fixes live where the counters
/// are written (`ms_hook_proc` now stamps a callback-only clock and counter;
/// `kb_hook_proc` counts injections separately on the branch it already takes).
#[cfg(test)]
mod liveness_split_tests {
    use super::{alarm_is_evidenced, format_liveness_split, BLIND_MS};

    /// The reading that cost a session. `saw 60 key event(s)` is ONE number
    /// covering real keys and this app's own injections, so "the primary is
    /// seeing keys" and "the keyboard is dead and Spaceadom is typing to
    /// itself" print identically. Split, they cannot.
    #[test]
    fn a_window_of_pure_injection_is_visibly_different_from_a_window_of_typing() {
        let only_injection = format_liveness_split(0, 12, 12, 40);
        let real_typing = format_liveness_split(12, 0, 12, 40);
        assert_ne!(only_injection, real_typing);
        assert!(only_injection.contains("primary_real:0"));
        assert!(real_typing.contains("primary_real:12"));
    }

    /// PROBLEM 230's inversion, as a line a reader can grep. A live primary
    /// beside a frozen reference is the witness being evicted again — and in
    /// 1.0.95 that shape was only visible by correlating three lines minutes
    /// apart.
    #[test]
    fn a_live_primary_beside_a_frozen_reference_is_visible_in_one_line() {
        let evicted_witness = format_liveness_split(49, 0, 0, 210);
        assert!(evicted_witness.contains("primary_real:49"));
        assert!(evicted_witness.contains("reference:0"));
    }

    /// The 6-of-16 case: `kb 4000ms / mouse 4000ms`, two identical round
    /// numbers written by `install_hooks()` or the idle re-stamp, printed under
    /// the words "NEITHER hook saw anything". The callback clocks say the mouse
    /// hook fired 120 ms ago, so nothing was evicted.
    #[test]
    fn a_live_mouse_callback_makes_a_both_dead_alarm_unevidenced() {
        assert!(!alarm_is_evidenced(Some(9_000), Some(120), Some(9_000), BLIND_MS));
    }

    /// The ten measured non-seeded mouse clocks from that session
    /// (3172…5250 ms). Every one is a mouse hook that fired seconds ago and
    /// then crossed a 3-second line — an evicted hook's clock keeps growing,
    /// these did not. Above the threshold they DO count as evidence; the point
    /// of the function is that the verdict is now stated rather than assumed.
    #[test]
    fn silence_past_the_threshold_on_every_unforgeable_clock_is_evidence() {
        for ms in [3_172u64, 3_218, 3_234, 3_297, 3_313, 3_328, 3_437, 3_875, 5_250] {
            assert!(alarm_is_evidenced(Some(20_000), Some(ms), Some(20_000), BLIND_MS));
        }
        // …and one hair under it is not.
        assert!(!alarm_is_evidenced(Some(20_000), Some(2_999), Some(20_000), BLIND_MS));
    }

    /// NEVER-FIRED IS NOT STOPPED. The 16:28:53.412 alarm fired 6 s after
    /// launch, before any hook had been called once, and printed "NEITHER hook
    /// saw anything" — true, and about nothing. PROBLEM 228 is the record of
    /// what conflating "no evidence" with "evidence of death" costs, so `None`
    /// can never make an alarm evidenced no matter what the other two say.
    #[test]
    fn a_hook_that_has_never_fired_can_never_evidence_an_alarm() {
        assert!(!alarm_is_evidenced(None, Some(60_000), Some(60_000), BLIND_MS));
        assert!(!alarm_is_evidenced(Some(60_000), None, Some(60_000), BLIND_MS));
        assert!(!alarm_is_evidenced(Some(60_000), Some(60_000), None, BLIND_MS));
        assert!(!alarm_is_evidenced(None, None, None, BLIND_MS));
    }

    /// The idle window. All four zero prints nothing at all, so a log full of
    /// this line still means the machine was being used.
    #[test]
    fn the_line_reports_a_dead_window_as_four_zeroes_not_as_a_fault() {
        assert_eq!(
            format_liveness_split(0, 0, 0, 0),
            "primary_real:0 primary_injected:0 reference:0 mouse:0"
        );
    }
}

/// PROBLEM 236, DECISION CHANGED — the rule that now GATES the repair.
///
/// The tests above pin the WORDS the 1.0.97 pass added. These pin the
/// BEHAVIOUR: which alarms are still allowed to tear a hook down, and which
/// live hold is allowed to stop one that is.
///
/// Every case here is one of the shapes measured in the owner's 1.0.96 session
/// (2026-09-04, 16:28:47 → 17:06:17, 16 alarms in 38 minutes, all `both_dead`):
/// the six `4000/4000` seeded pairs, the ten mouse clocks clustered within
/// 437 ms of the 3000 ms trip line, and the alarm that fired 6 s after launch
/// before any callback had run.
#[cfg(test)]
mod alarm_decision_tests {
    use super::{
        classify_callback_liveness, hold_defers_rehook, CallbackLiveness, DeadKind, BLIND_MS,
        INSTALL_GRACE_MS, MAX_HOLD_DEFER_MS, UNKNOWN_MAX_MS,
    };

    /// Shorthand: the production constants, so a test can never pass against
    /// numbers the app does not actually use.
    fn verdict(
        kb: Option<u64>,
        ms: Option<u64>,
        rf: Option<u64>,
        since_install: u64,
    ) -> CallbackLiveness {
        classify_callback_liveness(
            kb,
            ms,
            rf,
            since_install,
            BLIND_MS,
            INSTALL_GRACE_MS,
            UNKNOWN_MAX_MS,
        )
    }

    /// THE SIX SEEDED ALARMS. `kb 4000ms / mouse 4000ms` — two identical round
    /// numbers written by `install_hooks()` or the idle re-stamp, printed under
    /// the words "NEITHER hook saw anything". No callback has run since the
    /// install, so there is nothing here to be evidence, and 1.0.96 re-hooked
    /// on it anyway.
    #[test]
    fn a_seeded_pair_with_no_callback_behind_it_never_alarms() {
        assert_eq!(verdict(None, None, None, 20_000), CallbackLiveness::Unknown);
    }

    /// THE 16:28:53.412 ALARM. Six seconds after launch, before any hook had
    /// been called once. Inside the grace, so the answer is "ask me later",
    /// not "the hooks are dead".
    #[test]
    fn within_the_post_install_grace_nothing_can_alarm() {
        // Even the shape that WOULD alarm a second later.
        assert_eq!(
            verdict(Some(60_000), Some(60_000), Some(60_000), INSTALL_GRACE_MS - 1),
            CallbackLiveness::Unknown
        );
        assert_eq!(verdict(None, None, None, 6_000), CallbackLiveness::Unknown);
    }

    /// The failure the watchdog exists for, stated from instruments the repair
    /// cannot write: all three hooks have fired since the install, and all
    /// three have now been silent past the threshold while the user is active
    /// (the caller has already proved the user is active before reaching here).
    #[test]
    fn all_three_callbacks_silent_past_the_threshold_is_still_an_alarm() {
        assert_eq!(
            verdict(Some(9_000), Some(9_000), Some(9_000), 120_000),
            CallbackLiveness::Dead(DeadKind::Both)
        );
        // And one hair under the threshold on any single one of them is not.
        assert_eq!(
            verdict(Some(BLIND_MS - 1), Some(9_000), Some(9_000), 120_000),
            CallbackLiveness::Alive
        );
    }

    /// THE TEN MEASURED MOUSE CLOCKS (3172…5250 ms). Above the line they are
    /// evidence — but only while the keyboard and the reference agree. The
    /// owner's cluster sat within 437 ms of the trip line, which is what an
    /// ordinary pause in mouse movement looks like; with a live keyboard
    /// callback beside it, no alarm.
    #[test]
    fn a_mouse_pause_beside_a_live_keyboard_callback_is_not_death() {
        for ms in [3_172u64, 3_218, 3_234, 3_297, 3_313, 3_328, 3_437, 3_875, 5_250] {
            assert_eq!(
                verdict(Some(140), Some(ms), Some(140), 120_000),
                CallbackLiveness::Alive,
                "mouse silent {ms}ms while the keyboard callback fired 140ms ago"
            );
        }
    }

    /// REVIEW FIX 2026-09-04 — THE CORRECTED SEMANTICS, and the test this
    /// replaces (`a_live_reference_hook_alone_prevents_the_alarm`) is the
    /// clearest record of how the rule went wrong: it asserted `Alive`, so the
    /// bug had a passing test defending it.
    ///
    /// The reference hook IS the third vote, but it votes on WHICH failure this
    /// is, never on whether there is one. A keyboard and mouse that have both
    /// gone quiet while the witness is still being called means keys are
    /// reaching the chain and ours is not being called for them — that is
    /// `kb_only_dead`'s premise, not `both_dead`'s. So the reference narrows
    /// the verdict to `KbOnly` (which the re-hook repairs) instead of
    /// cancelling it.
    #[test]
    fn a_live_reference_hook_narrows_the_verdict_instead_of_cancelling_it() {
        assert_eq!(
            verdict(Some(20_000), Some(20_000), Some(200), 120_000),
            CallbackLiveness::Dead(DeadKind::KbOnly)
        );
        // …and the same clocks WITHOUT a live reference are the other failure.
        assert_eq!(
            verdict(Some(20_000), Some(20_000), Some(20_000), 120_000),
            CallbackLiveness::Dead(DeadKind::Both)
        );
    }

    /// THE FAILURE THE OWNER LIVES WITH, stated in one assertion: the reference
    /// fires, the PRIMARY keyboard callback does not. Before this fix the rule
    /// answered `Alive` here and the `kb_only_dead` re-hook branch was
    /// unreachable dead code — the watchdog vetoed itself for exactly the shape
    /// it exists to catch (PROBLEM 181: 24 of 46 alarms had this fingerprint).
    ///
    /// The mouse is deliberately silent-but-present in the first case (the user
    /// is typing, not moving the mouse) and alive in the second, which must
    /// still be `Alive`: a live PRIMARY hook is proof, and the mouse is one.
    #[test]
    fn live_reference_with_silent_primary_is_kb_only_dead() {
        assert_eq!(
            verdict(Some(BLIND_MS + 1), Some(9_000), Some(120), 120_000),
            CallbackLiveness::Dead(DeadKind::KbOnly)
        );
        // A live MOUSE callback is a primary and does cancel the alarm.
        assert_eq!(
            verdict(Some(9_000), Some(120), Some(120), 120_000),
            CallbackLiveness::Alive
        );
        // The install grace still outranks it — nothing alarms inside it.
        assert_eq!(
            verdict(Some(9_000), Some(9_000), Some(120), INSTALL_GRACE_MS - 1),
            CallbackLiveness::Unknown
        );
        // And PROBLEM 228's law survives: a primary that has NEVER fired since
        // the install is UNKNOWN, however loudly the witness is shouting —
        // until the unknown bound expires, which is what stops a hook that
        // never fires from becoming permanent deafness.
        assert_eq!(
            verdict(None, Some(9_000), Some(120), UNKNOWN_MAX_MS - 1),
            CallbackLiveness::Unknown
        );
        assert_eq!(
            verdict(None, Some(9_000), Some(120), UNKNOWN_MAX_MS),
            CallbackLiveness::Dead(DeadKind::KbOnly)
        );
    }

    /// NEVER-FIRED IS NOT STOPPED — PROBLEM 228's law, applied to the decision
    /// rather than to the sentence. Any single hook that has not fired since
    /// the install holds the verdict at UNKNOWN, no matter how loudly the other
    /// two are shouting.
    #[test]
    fn one_hook_that_never_fired_since_the_install_holds_the_verdict_at_unknown() {
        let long = UNKNOWN_MAX_MS - 1;
        assert_eq!(verdict(None, Some(20_000), Some(20_000), long), CallbackLiveness::Unknown);
        assert_eq!(verdict(Some(20_000), None, Some(20_000), long), CallbackLiveness::Unknown);
        assert_eq!(verdict(Some(20_000), Some(20_000), None, long), CallbackLiveness::Unknown);
    }

    /// AND THE HOLE THAT WOULD OTHERWISE OPEN. Read literally, "never fired ⇒
    /// unknown ⇒ no alarm" means a `SetWindowsHookExW` that returns a handle
    /// and never fires can never be retried, because no callback can ever move
    /// to contradict it — intermittent deafness converted into permanent
    /// deafness, which this file's own law calls the fix being worse than the
    /// bug. The unknown state is therefore BOUNDED: past `UNKNOWN_MAX_MS` of a
    /// demonstrably-present user with not one callback on any hook, silence
    /// stops being an absence of evidence.
    #[test]
    fn an_install_that_has_produced_no_callback_at_all_is_eventually_evidence() {
        assert_eq!(
            verdict(None, None, None, UNKNOWN_MAX_MS - 1),
            CallbackLiveness::Unknown
        );
        assert_eq!(
            verdict(None, None, None, UNKNOWN_MAX_MS),
            CallbackLiveness::Dead(DeadKind::Both)
        );
    }

    /// The bound must not fire while something is demonstrably alive, or a
    /// working app would be re-hooked every 30 seconds forever.
    #[test]
    fn the_unknown_bound_never_overrides_a_hook_that_is_actually_firing() {
        assert_eq!(
            verdict(None, Some(50), None, 10 * UNKNOWN_MAX_MS),
            CallbackLiveness::Alive
        );
    }

    /// LIVE-HOLD PROTECTION. A hold the keyboard callback was entered inside is
    /// a hold the teardown would damage rather than repair.
    #[test]
    fn a_live_hold_defers_the_repair() {
        assert!(hold_defers_rehook(true, true, 0, MAX_HOLD_DEFER_MS));
        assert!(hold_defers_rehook(true, true, MAX_HOLD_DEFER_MS - 1, MAX_HOLD_DEFER_MS));
    }

    /// …and it is BOUNDED. A hold that outlives the alarm past the bound stops
    /// protecting anything: if the hook really is dead, the repair is ten
    /// seconds late and no later.
    #[test]
    fn the_deferral_expires_so_a_dead_hook_is_always_repaired() {
        assert!(!hold_defers_rehook(true, true, MAX_HOLD_DEFER_MS, MAX_HOLD_DEFER_MS));
        assert!(!hold_defers_rehook(true, true, 60_000, MAX_HOLD_DEFER_MS));
    }

    /// No hold, or a latch with no callback behind it, defers nothing. The
    /// second case matters: `MODIFIER_ACTIVE` can be left latched by an
    /// eviction that ate the Space-UP (PROBLEM 218), and that stale latch must
    /// never be able to block the repair for the eviction that created it.
    #[test]
    fn a_stale_latch_with_no_callback_inside_it_defers_nothing() {
        assert!(!hold_defers_rehook(false, true, 0, MAX_HOLD_DEFER_MS));
        assert!(!hold_defers_rehook(true, false, 0, MAX_HOLD_DEFER_MS));
        assert!(!hold_defers_rehook(false, false, 0, MAX_HOLD_DEFER_MS));
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

/// PROBLEM 257 — the proven-deaf test. Every row is a state the watchdog can
/// actually be in; the only one that may re-hook is "Space is down, nobody
/// intercepted it, nobody deliberately passed it through, and the callback
/// has been silent past the threshold".
#[cfg(test)]
mod keyboard_deaf_tests {
    use super::keyboard_deaf_with_space_down as deaf;
    const T: u64 = 1_500;

    /// The 2026-09-06 shape: dashboard focused, Space physically held, no hold
    /// latched, every gate off, callback silent for 20 s.
    #[test]
    fn space_down_with_a_silent_callback_is_deaf() {
        assert!(deaf(true, false, false, false, Some(20_000), T));
        assert!(deaf(true, false, false, false, Some(T), T));
    }

    /// A working hold: our hook swallowed the down, so the OS reports Space UP
    /// and `MODIFIER_ACTIVE` is latched. Neither half may trip.
    #[test]
    fn a_working_intercepted_hold_is_not_deaf() {
        assert!(!deaf(false, true, false, false, Some(20_000), T));
        assert!(!deaf(true, true, false, false, Some(20_000), T));
    }

    /// Space really is down and the callback saw it a moment ago (its
    /// auto-repeats keep the clock fresh): the hook is working.
    #[test]
    fn a_recent_callback_is_not_deaf() {
        assert!(!deaf(true, false, false, false, Some(0), T));
        assert!(!deaf(true, false, false, false, Some(T - 1), T));
    }

    /// The three stand-down gates and law 4's modifier pass-through are cases
    /// where the callback fired and CHOSE to let Space reach the OS. Excluded,
    /// never measured.
    #[test]
    fn a_deliberate_pass_through_is_not_deaf() {
        assert!(!deaf(true, false, true, false, Some(20_000), T));
        assert!(!deaf(true, false, false, true, Some(20_000), T));
    }

    /// Nobody is holding Space: there is nothing to be deaf to, however long
    /// the callback has been quiet (PROBLEM 101's whole lesson).
    #[test]
    fn space_up_is_never_deaf() {
        assert!(!deaf(false, false, false, false, Some(600_000), T));
    }

    /// A callback that has not fired since the last install is UNKNOWN, not
    /// dead (PROBLEM 236): this test may not declare a fresh install deaf.
    #[test]
    fn never_fired_since_install_is_unknown_not_deaf() {
        assert!(!deaf(true, false, false, false, None, T));
    }
}

// ---------------------------------------------------------------------------
// TESTS — PROBLEM 260. The proven-deaf verdict, the throttle it bypasses, and
// the one throttle it does not.
//
// Every case below is a shape MEASURED in the owner's 2026-09-07 log
// (1.0.103, with an independent WH_KEYBOARD_LL probe running beside the app):
//
//   10:10:24      the last keyboard callback of the episode
//   10:11:49      liveness split — primary_real:0 reference:0 mouse:217 / 60s
//   10:12:34.246  the first alarm — 130 s late, and only because the mouse
//                 fell quiet for 3032 ms at that instant
//   10:10:59      "WATCHDOG would re-hook … but the cooldown test says the
//                 last repair delivered events … Holding off for the rest of
//                 the 60s cooldown"
//
// These are arithmetic, not behaviour: the decision is a pure function and the
// watchdog does none of its own.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod proven_deaf_tests {
    use super::{
        forced_repair_allowed, forced_repair_backoff_ms, proven_keyboard_deaf, ProvenDeaf,
        FORCED_INPUT_MAX_AGE_MS, FORCED_MOUSE_ATTRIBUTION_MS, FORCED_REPAIR_BASE_MS,
        FORCED_REPAIR_MAX_MS, INSTALL_GRACE_MS, OWN_DEAF_SILENCE_MS,
    };

    /// The production constants, so no test can pass against numbers the app
    /// does not actually use. Arguments in the order a reader of the log needs
    /// them: is Space down, is a hold latched, keyboard-callback silence,
    /// the RAW keyboard clock's age (PROBLEM 268 — was the mouse callback's
    /// silence), how old the OS input clock is, install age.
    #[allow(clippy::too_many_arguments)]
    fn verdict(
        space_down: bool,
        modifier_active: bool,
        kb: Option<u64>,
        raw: Option<u64>,
        os_input_age: u64,
        since_install: u64,
    ) -> Option<ProvenDeaf> {
        proven_keyboard_deaf(
            space_down,
            modifier_active,
            false,
            false,
            kb,
            raw,
            os_input_age,
            since_install,
            OWN_DEAF_SILENCE_MS,
            INSTALL_GRACE_MS,
            FORCED_INPUT_MAX_AGE_MS,
            FORCED_MOUSE_ATTRIBUTION_MS,
        )
    }

    /// THE 2026-09-07 EPISODE, AT THE TICK THE OLD CODE SAID NOTHING.
    ///
    /// 10:11:20-ish: the keyboard callback 56 s silent, the mouse hook firing
    /// three times a second (mouse:217 in the window), the user typing into
    /// WindowsTerminal. `both_dead` was false because the mouse was alive;
    /// `kb_only_dead` was false because the reference was dead too. No
    /// candidate, no alarm, no line. This verdict is the one that fires.
    #[test]
    fn a_live_mouse_hook_does_not_hide_a_dead_keyboard_hook() {
        // The raw sink saw a keystroke 391 ms ago (the user typing into the
        // terminal); our callback is 56 s silent. A keystroke reached the OS
        // and never reached us — whatever the mouse hook is doing.
        assert_eq!(
            verdict(false, false, Some(56_000), Some(391), 391, 135_000),
            Some(ProvenDeaf::InputUnaccountedFor)
        );
    }

    /// THE MEASURED ALARM ITSELF (10:12:34.246): `kb 129672ms`, `mouse 3032ms`,
    /// `user active 391ms ago`. The old path needed the mouse to cross 3000 ms
    /// before it could even raise a candidate; this verdict does not care where
    /// the mouse is, only that it cannot account for the input.
    #[test]
    fn the_measured_alarm_is_proven_deaf_by_the_new_rule_too() {
        assert_eq!(
            verdict(false, false, Some(129_672), Some(391), 391, 135_016),
            Some(ProvenDeaf::InputUnaccountedFor)
        );
    }

    /// PROBLEM 101, THE ONE THIS MUST NOT REBECOME. A person reading a page
    /// moves the mouse and does not type: the OS input clock is fresh BECAUSE
    /// of the mouse, and our mouse callback fired for it. Nothing here says
    /// anything about the keyboard hook, so nothing may be repaired — that
    /// branch cost 95 of 255 false alarms when it was decided the other way up.
    #[test]
    fn a_moving_mouse_accounts_for_the_input_and_is_never_deafness() {
        // PROBLEM 268 — THE TOUCHPAD. The OS input clock is fresh (a two-finger
        // scroll), nobody has typed for ten minutes, and the raw keyboard sink
        // has seen nothing in that time. No keystroke was lost, so nothing may
        // be repaired — this exact shape produced 449 forced repairs in two
        // days when the mouse callback's clock was the discriminator.
        assert_eq!(verdict(false, false, Some(600_000), Some(600_000), 60, 900_000), None);
        assert_eq!(verdict(false, false, Some(600_000), None, 0, 900_000), None);
        // A keystroke the callback DID see: the raw stamp is the later of the
        // two, so the callback's silence is at most the raw age plus the pump
        // latency the margin allows. (Raw age 1600 so the callback's silence
        // clears the 1500 ms threshold and the MARGIN is what decides.)
        let raw = 1_600;
        assert_eq!(
            verdict(false, false, Some(raw + FORCED_MOUSE_ATTRIBUTION_MS), Some(raw), raw, 900_000),
            None
        );
        // One millisecond past the margin is the other verdict. This is the
        // whole width of the discriminator, asserted so a future widening of
        // the margin cannot happen silently.
        assert_eq!(
            verdict(false, false, Some(raw + FORCED_MOUSE_ATTRIBUTION_MS + 1), Some(raw), raw, 900_000),
            Some(ProvenDeaf::InputUnaccountedFor)
        );
    }

    /// A user who has genuinely stopped touching anything proves nothing. The
    /// watchdog returns early above 2000 ms anyway, but the pure function must
    /// hold the line on its own or it is not testable.
    #[test]
    fn a_stale_os_input_clock_proves_nothing() {
        assert_eq!(
            verdict(false, false, Some(600_000), Some(600_000), FORCED_INPUT_MAX_AGE_MS + 1, 900_000),
            None
        );
    }

    /// THE GRACE STILL SUPPRESSES. PROBLEM 236's 16:28:53.412 alarm fired six
    /// seconds after launch, before any hook had been called once. Inside the
    /// grace the answer is "ask me later", and that outranks every proof below
    /// it — including a physically-held Space.
    #[test]
    fn the_install_grace_suppresses_even_a_proven_shape() {
        assert_eq!(
            verdict(false, false, Some(56_000), Some(391), 391, INSTALL_GRACE_MS - 1),
            None
        );
        assert_eq!(verdict(true, false, Some(56_000), None, 100, INSTALL_GRACE_MS - 1), None);
        // One millisecond past the grace, the same shape is a verdict.
        assert_eq!(
            verdict(false, false, Some(56_000), Some(391), 391, INSTALL_GRACE_MS),
            Some(ProvenDeaf::InputUnaccountedFor)
        );
    }

    /// A LIVE HOLD IS NEVER REPAIRED HERE. The repair re-installs, which loses
    /// the Space-UP, clears the latches and hides the ring — the owner's "it
    /// dies mid-press" (PROBLEM 236). `reap_stale_hold()` runs above every
    /// early return in `watchdog_check`, so a hold the hook has stopped feeding
    /// is already cleared before this is asked; the latch cannot wedge it shut.
    #[test]
    fn a_latched_hold_suppresses_the_forced_repair() {
        assert_eq!(verdict(false, true, Some(56_000), Some(3_200), 391, 135_000), None);
        assert_eq!(verdict(true, true, Some(56_000), Some(3_200), 391, 135_000), None);
    }

    /// PROBLEM 228'S LAW, UNCHANGED. A keyboard callback that has never fired
    /// since the install is UNKNOWN, not dead — that is the failed-install case
    /// and it belongs to `classify_callback_liveness`'s unknown bound, where the
    /// evidence is thirty seconds of a present user rather than one keystroke.
    #[test]
    fn a_keyboard_callback_that_never_fired_is_unknown_not_proven() {
        assert_eq!(verdict(false, false, None, Some(3_200), 391, 900_000), None);
        assert_eq!(verdict(true, false, None, None, 100, 900_000), None);
    }

    /// A keyboard callback inside the threshold is a working hook, whatever the
    /// mouse is doing. This is the ordinary case and it must cost nothing.
    #[test]
    fn a_recent_keyboard_callback_is_never_deaf() {
        assert_eq!(
            verdict(false, false, Some(OWN_DEAF_SILENCE_MS - 1), Some(600_000), 10, 900_000),
            None
        );
        assert_eq!(
            verdict(true, false, Some(OWN_DEAF_SILENCE_MS - 1), Some(600_000), 10, 900_000),
            None
        );
    }

    /// PROBLEM 257'S ORIGINAL PROOF still fires, and still outranks the new one
    /// when both hold — the log reads better when it names the strongest
    /// instrument available.
    #[test]
    fn space_physically_down_is_still_its_own_proof() {
        assert_eq!(
            verdict(true, false, Some(21_875), Some(1_203), 16, 900_000),
            Some(ProvenDeaf::SpaceHeld)
        );
    }

    /// THE BACKOFF ENGAGES AFTER N INEFFECTIVE REPAIRS, and the first repair of
    /// an episode never waits at all.
    #[test]
    fn the_backoff_doubles_and_then_caps() {
        let b = |n| forced_repair_backoff_ms(n, FORCED_REPAIR_BASE_MS, FORCED_REPAIR_MAX_MS);
        assert_eq!(b(0), FORCED_REPAIR_BASE_MS);
        assert_eq!(b(1), FORCED_REPAIR_BASE_MS * 2);
        assert_eq!(b(2), FORCED_REPAIR_BASE_MS * 4);
        assert_eq!(b(3), FORCED_REPAIR_BASE_MS * 8);
        // Capped, and it stays capped however long the streak runs — a hostile
        // environment must not be able to drive this off the end of a u64.
        assert_eq!(b(4), FORCED_REPAIR_MAX_MS);
        assert_eq!(b(40), FORCED_REPAIR_MAX_MS);
        assert_eq!(b(u32::MAX), FORCED_REPAIR_MAX_MS);
    }

    /// THE RATE LIMIT ITSELF. Never repaired yet → repair now. Repaired
    /// recently → wait. Waited long enough → repair.
    #[test]
    fn the_first_forced_repair_never_waits_and_the_second_does() {
        assert!(forced_repair_allowed(None, 0, FORCED_REPAIR_BASE_MS, FORCED_REPAIR_MAX_MS));
        assert!(!forced_repair_allowed(
            Some(FORCED_REPAIR_BASE_MS - 1),
            0,
            FORCED_REPAIR_BASE_MS,
            FORCED_REPAIR_MAX_MS
        ));
        assert!(forced_repair_allowed(
            Some(FORCED_REPAIR_BASE_MS),
            0,
            FORCED_REPAIR_BASE_MS,
            FORCED_REPAIR_MAX_MS
        ));
        // Three ineffective repairs and the same 5 s wait is no longer enough.
        assert!(!forced_repair_allowed(
            Some(FORCED_REPAIR_BASE_MS),
            3,
            FORCED_REPAIR_BASE_MS,
            FORCED_REPAIR_MAX_MS
        ));
        assert!(forced_repair_allowed(
            Some(FORCED_REPAIR_BASE_MS * 8),
            3,
            FORCED_REPAIR_BASE_MS,
            FORCED_REPAIR_MAX_MS
        ));
    }

    /// A REPAIR THAT RESTORES EVENTS RESETS THE BACKOFF. The watchdog does this
    /// by storing 0 into the streak when a keyboard callback lands after the
    /// repair stamp; stated here as the arithmetic that follows from it, so the
    /// property is asserted rather than only commented.
    #[test]
    fn a_repair_that_delivers_events_puts_the_fast_cadence_back() {
        let slow = forced_repair_backoff_ms(4, FORCED_REPAIR_BASE_MS, FORCED_REPAIR_MAX_MS);
        assert_eq!(slow, FORCED_REPAIR_MAX_MS);
        // The streak reset the watchdog performs, in one line.
        let after_reset = forced_repair_backoff_ms(0, FORCED_REPAIR_BASE_MS, FORCED_REPAIR_MAX_MS);
        assert_eq!(after_reset, FORCED_REPAIR_BASE_MS);
        // A wait that was NOT enough while the streak stood is enough after it.
        assert!(!forced_repair_allowed(
            Some(FORCED_REPAIR_BASE_MS),
            4,
            FORCED_REPAIR_BASE_MS,
            FORCED_REPAIR_MAX_MS
        ));
        assert!(forced_repair_allowed(
            Some(FORCED_REPAIR_BASE_MS),
            0,
            FORCED_REPAIR_BASE_MS,
            FORCED_REPAIR_MAX_MS
        ));
    }

    /// THE POINT OF THE WHOLE CHANGE, as one assertion pair: an UNEVIDENCED
    /// alarm still respects the cooldown (it never reaches this function at
    /// all — it has no proof to offer), while the PROVEN verdict is reached
    /// from callback-only clocks and is repaired regardless of it.
    ///
    /// The 10:10:59 line — "the cooldown test says the last repair delivered
    /// events … Holding off for the rest of the 60s cooldown" — was printed
    /// 5344 ms after a keyboard callback genuinely arrived, so it was correct
    /// on its own terms. What it could not know is that the keyboard died again
    /// 35 seconds later. This function is what knows that.
    #[test]
    fn no_proof_means_no_forced_repair_and_the_cooldown_keeps_its_job() {
        // Nothing proven (PROBLEM 268): keyboard callback silent 4 s, the OS
        // input clock fresh (the touchpad), but the raw keyboard sink's last
        // keystroke is as old as the callback's — no keystroke went missing.
        // The `both_dead`/`kb_only_dead` path and its 60 s cooldown handle
        // this case exactly as before.
        assert_eq!(verdict(false, false, Some(4_000), Some(4_000), 100, 900_000), None);
        // Proven: the same keyboard silence, with a keystroke the sink saw
        // 100 ms ago that the callback never did.
        assert_eq!(
            verdict(false, false, Some(4_000), Some(100), 100, 900_000),
            Some(ProvenDeaf::InputUnaccountedFor)
        );
    }
}

// ---------------------------------------------------------------------------
// TESTS — PROBLEM 259. The own-window fallback's three guards.
//
// House rule (CLAUDE.md): test the pure logic a user only reaches after
// something else has already gone wrong — and this whole path exists because
// something already has (PROBLEM 257). The branch that MUST be exercised is
// the one nobody will ever see fail safely: the day the hook starts working
// over our own window again, these guards are the only thing standing between
// the owner and two rings, two launches and two spaces per press.
// ---------------------------------------------------------------------------

/// PROBLEM 262 — THE 2026-09-07 WEDGE, BRANCH BY BRANCH.
///
/// The measured episode, from installed 1.0.106's `debug.log`:
///
/// ```text
/// 11:37:33.240  hold start (hold #11)
/// 11:37:34.340  engine: combo Space+RightAlt received   <- last keyboard callback
/// 11:38:33.351  WATCHDOG alarm confirmed ... Holds protected this session: 1
/// 11:38:41.352  ...                                                        2
/// 11:39:08.352  ...                                                        3
/// 11:39:30.352  ...                                                        4
/// ```
///
/// Four "protected" lines for ONE hold is the fingerprint of an episode clock
/// that was being reset, and the numbers in these tests are that log's numbers.
#[cfg(test)]
mod deaf_hold_reaper_tests {
    use super::{
        defer_episode_ends_on_quiet_tick, hold_defers_rehook, hold_is_deaf_stale, hold_reap_reason,
        modifier_latched_past_bound, HoldReap, HOLD_DEAF_SILENCE_MS, MAX_HOLD_DEFER_MS,
        MAX_MODIFIER_HOLD_MS, MIN_OBSERVED_REPEATS, STALE_HOLD_GRACE_MS,
    };

    /// Every argument spelled once, so each test below changes exactly the term
    /// it is about. Defaults describe a HEALTHY combo hold: latched, a combo
    /// seen (so PROBLEM 219 stands the auto-repeat test down), the keyboard
    /// callback running, and nothing proven deaf.
    fn reason(
        combo_seen: bool,
        repeats: u32,
        since_last_tick_ms: u64,
        proven_deaf: bool,
        kb_silence: Option<u64>,
        hold_age_ms: u64,
    ) -> Option<HoldReap> {
        hold_reap_reason(
            true,
            repeats,
            since_last_tick_ms,
            STALE_HOLD_GRACE_MS,
            combo_seen,
            proven_deaf,
            kb_silence,
            HOLD_DEAF_SILENCE_MS,
            hold_age_ms,
            MAX_MODIFIER_HOLD_MS,
        )
    }

    /// PROBLEM 219 IS KEPT, AND THIS IS THE CASE IT WAS WRITTEN FOR. The user
    /// is holding Space and tapping a combo key; Windows moved auto-repeat to
    /// that key, so Space stops repeating — but the CALLBACK is alive and says
    /// so. No verdict, small silence: hands off.
    #[test]
    fn a_combo_seen_hold_is_still_protected_while_the_callback_is_alive() {
        // The literal PROBLEM 219 numbers: 5 repeats, then 2016ms of quiet
        // across four Space+Tab taps.
        assert_eq!(reason(true, 5, 2_016, false, Some(40), 2_500), None);
        // And a long, quiet, perfectly healthy read of the ring after a combo.
        for since in [3_000u64, 10_000, 25_000] {
            assert_eq!(
                reason(true, 40, since, false, Some(120), since + 500),
                None,
                "a combo hold with the callback still running must never be reaped"
            );
        }
    }

    /// THE SAME HOLD, ONCE THE KEYBOARD IS PROVEN DEAF. Nothing about the hold
    /// changed; what changed is that an instrument OFF the callback now says
    /// the callback is not being entered at all, so PROBLEM 219's exemption can
    /// never be lifted by any future event.
    #[test]
    fn the_same_combo_hold_is_reaped_once_the_keyboard_is_proven_deaf() {
        assert_eq!(
            reason(true, 5, 2_016, true, Some(58_922), 60_125),
            Some(HoldReap::KeyboardDeaf),
            "the 11:38:33 numbers from the 1.0.106 log"
        );
        // Even with zero repeats ever observed — a deaf hook cannot produce
        // one, so `MIN_OBSERVED_REPEATS` can never be satisfied on this path.
        assert_eq!(
            reason(true, 0, 0, true, Some(HOLD_DEAF_SILENCE_MS), 4_000),
            Some(HoldReap::KeyboardDeaf)
        );
    }

    /// The verdict is not enough on its own: the callback-only clock has to
    /// agree, and it has to have RUN once since this install (PROBLEM 228 —
    /// `None` is UNKNOWN, never proof).
    #[test]
    fn the_deaf_path_needs_both_the_verdict_and_the_callback_clock() {
        assert!(!hold_is_deaf_stale(true, true, None, HOLD_DEAF_SILENCE_MS));
        assert!(!hold_is_deaf_stale(
            true,
            true,
            Some(HOLD_DEAF_SILENCE_MS - 1),
            HOLD_DEAF_SILENCE_MS
        ));
        assert!(hold_is_deaf_stale(
            true,
            true,
            Some(HOLD_DEAF_SILENCE_MS),
            HOLD_DEAF_SILENCE_MS
        ));
        // No verdict, and no hold, are each fatal on their own.
        assert!(!hold_is_deaf_stale(true, false, Some(60_000), HOLD_DEAF_SILENCE_MS));
        assert!(!hold_is_deaf_stale(false, true, Some(60_000), HOLD_DEAF_SILENCE_MS));
    }

    /// PROBLEM 218's own shape still reports PROBLEM 218's reason, and still
    /// wins over the newer bounds — it is the most specific of the three.
    #[test]
    fn a_silent_hold_with_no_combo_is_still_reaped_as_auto_repeat_stopped() {
        assert_eq!(
            reason(false, MIN_OBSERVED_REPEATS, STALE_HOLD_GRACE_MS + 1, false, Some(40), 3_000),
            Some(HoldReap::AutoRepeatStopped)
        );
        // Deaf as well: 218's reason is still the one printed, because it is
        // the one that describes the hold rather than the process.
        assert_eq!(
            reason(false, 40, 60_000, true, Some(60_000), 61_000),
            Some(HoldReap::AutoRepeatStopped)
        );
    }

    /// ITEM 4, THE LAST RESORT. No verdict at all — the user simply stopped
    /// touching anything, so `proven_keyboard_deaf` has no input to reason
    /// from — but the latch has outlived the 30 s bound with NO keyboard
    /// callbacks in that whole span.
    #[test]
    fn a_latch_past_the_bound_with_no_callbacks_in_that_span_is_cleared() {
        assert_eq!(
            reason(true, 0, 0, false, Some(58_922), 60_125),
            Some(HoldReap::LatchedPastBound),
            "the 11:38:33 hold, with the deafness verdict withheld"
        );
        assert!(modifier_latched_past_bound(
            true,
            MAX_MODIFIER_HOLD_MS + 1,
            Some(MAX_MODIFIER_HOLD_MS + 1),
            MAX_MODIFIER_HOLD_MS
        ));
    }

    /// The bound may not fire on a hold the callback is still feeding — that is
    /// a person reading the ring, and tearing it down is PROBLEM 236's
    /// "it dies mid-press".
    #[test]
    fn a_long_hold_the_callback_is_still_feeding_is_never_cleared_by_the_bound() {
        assert!(!modifier_latched_past_bound(
            true,
            10 * 60_000,
            Some(120),
            MAX_MODIFIER_HOLD_MS
        ));
        // Exactly at the bound is not past it, on either term.
        assert!(!modifier_latched_past_bound(
            true,
            MAX_MODIFIER_HOLD_MS,
            Some(MAX_MODIFIER_HOLD_MS + 1),
            MAX_MODIFIER_HOLD_MS
        ));
        assert!(!modifier_latched_past_bound(
            true,
            MAX_MODIFIER_HOLD_MS + 1,
            Some(MAX_MODIFIER_HOLD_MS),
            MAX_MODIFIER_HOLD_MS
        ));
        // Never fired since the install is UNKNOWN, not proof (PROBLEM 228).
        assert!(!modifier_latched_past_bound(true, 10 * 60_000, None, MAX_MODIFIER_HOLD_MS));
        // And no hold means nothing to bound.
        assert!(!modifier_latched_past_bound(
            false,
            10 * 60_000,
            Some(10 * 60_000),
            MAX_MODIFIER_HOLD_MS
        ));
    }

    /// A hold with no latch is not a hold, whatever the other evidence says.
    #[test]
    fn nothing_is_reaped_when_no_hold_is_latched() {
        assert_eq!(
            hold_reap_reason(
                false,
                40,
                60_000,
                STALE_HOLD_GRACE_MS,
                false,
                true,
                Some(60_000),
                HOLD_DEAF_SILENCE_MS,
                60_000,
                MAX_MODIFIER_HOLD_MS
            ),
            None
        );
    }

    // ── ITEM 2 — the deferral bound, replayed against the real predicates ──

    /// One second-by-second run of `watchdog_check`'s episode bookkeeping,
    /// using the REAL predicates and nothing else. Returns the tick (in
    /// seconds from the first alarm) on which the repair finally proceeds.
    ///
    /// `reset_on_quiet_tick` is the ONLY difference between 1.0.106 and the
    /// fix: it models the `!both_dead && !kb_only_dead` early return, which
    /// used to zero `ALARM_DEFERRED_AT` unconditionally.
    fn first_repair_second(alarm_seconds: &[u64], run_for: u64, reset_on_quiet_tick: bool) -> Option<u64> {
        let mut started: Option<u64> = None;
        for now in 0..=run_for {
            if !alarm_seconds.contains(&now) {
                // The quiet tick. A hold IS latched throughout this episode.
                if reset_on_quiet_tick || defer_episode_ends_on_quiet_tick(true) {
                    started = None;
                }
                continue;
            }
            let start = *started.get_or_insert(now);
            let deferred_for = (now - start) * 1_000;
            if !hold_defers_rehook(true, true, deferred_for, MAX_HOLD_DEFER_MS) {
                return Some(now);
            }
        }
        None
    }

    /// THE MEASURED FAILURE. The alarm can only fire on a tick where the mouse
    /// callback has ALSO been silent past `BLIND_MS`, so with a hand on the
    /// mouse the alarms are sparse: 11:38:33, :41, 11:39:08, :30 — 0s, 8s, 35s
    /// and 57s apart. Under 1.0.106's unconditional reset the 10 s bound needs
    /// ten CONSECUTIVE alarm seconds, which never happen, so the repair never
    /// comes and each alarm re-opens the episode from zero (which is why the
    /// log printed "Holds protected this session: 1, 2, 3, 4" for one hold).
    #[test]
    fn the_deferral_bound_cannot_be_extended_by_new_alarms() {
        let measured = [0u64, 8, 35, 57];
        assert_eq!(
            first_repair_second(&measured, 600, true),
            None,
            "1.0.106: a bound reset by every quiet tick is not a bound — this is the wedge"
        );
        assert_eq!(
            first_repair_second(&measured, 600, false),
            Some(35),
            "the clock runs from the FIRST deferred alarm, so the third one is past the bound"
        );
    }

    /// A CONTIGUOUS run still behaves exactly as PROBLEM 236 designed it: ten
    /// seconds of protection, then the repair. The fix must not shorten the
    /// deferral it was written to provide.
    #[test]
    fn a_contiguous_alarm_run_still_gets_its_full_ten_seconds() {
        let every_second: Vec<u64> = (0..30).collect();
        assert_eq!(first_repair_second(&every_second, 600, false), Some(10));
        assert_eq!(first_repair_second(&every_second, 600, true), Some(10));
    }

    /// The predicate itself, stated: only the end of the hold ends the episode.
    #[test]
    fn only_the_end_of_the_hold_ends_a_deferral_episode() {
        assert!(!defer_episode_ends_on_quiet_tick(true));
        assert!(defer_episode_ends_on_quiet_tick(false));
    }
}

/// PROBLEM 262 item 3 + item 5 — the two branches that touch process state.
/// Kept in one module and one test each, because they mutate module statics.
#[cfg(test)]
mod repair_teardown_tests {
    use super::{
        arm_own_window_hold, own_hold_active, own_window_space_down_accepted,
        tear_down_hold_across_repair, tick_count, Ordering, MODIFIER_ACTIVE, OWN_HOLD_TS,
        OWN_WINDOW_DEDUPE_MS, OWN_WINDOW_MAX_HOLD_MS, REPAIR_HOLD_TEARDOWNS, SPACE_ABORTED,
        SPACE_COMBO_SEEN, SPACE_DOWN_TS, SPACE_INTERCEPTED,
    };

    /// These two tests write the module's real statics (`MODIFIER_ACTIVE`,
    /// `OWN_HOLD_ACTIVE`), and `cargo test` runs tests in parallel threads, so
    /// they must not run at the same time as each other. Every other test in
    /// this file is pure and needs no such thing.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// ITEM 3. A hold latched by a hook that is about to be replaced is
    /// unfalsifiable: its Space-UP belongs to a hook that will not exist. The
    /// repair must take it with it — and must be idempotent, because the
    /// ordinary re-hook path performs the same teardown inline.
    #[test]
    fn a_repair_clears_a_hold_that_predates_it() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let before = REPAIR_HOLD_TEARDOWNS.load(Ordering::Relaxed);
        // The 11:37:33 hold: latched, a combo seen, Space swallowed.
        MODIFIER_ACTIVE.store(true, Ordering::SeqCst);
        SPACE_COMBO_SEEN.store(true, Ordering::Relaxed);
        SPACE_INTERCEPTED.store(true, Ordering::Relaxed);
        SPACE_DOWN_TS.store(tick_count(), Ordering::Relaxed);

        tear_down_hold_across_repair("a unit test");

        assert!(!MODIFIER_ACTIVE.load(Ordering::SeqCst), "the latch must not survive a repair");
        assert!(!SPACE_COMBO_SEEN.load(Ordering::Relaxed));
        assert!(!SPACE_INTERCEPTED.load(Ordering::Relaxed));
        assert!(!SPACE_ABORTED.load(Ordering::Relaxed));
        assert_eq!(
            REPAIR_HOLD_TEARDOWNS.load(Ordering::Relaxed),
            before + 1,
            "the teardown is counted, so the 60s diagnostics line can report it"
        );

        // Idempotent: a second repair with nothing latched counts nothing and
        // logs nothing. Without this the ordinary path (which already tears the
        // hold down inline) would double-count every eviction.
        tear_down_hold_across_repair("a unit test, again");
        assert_eq!(REPAIR_HOLD_TEARDOWNS.load(Ordering::Relaxed), before + 1);
    }

    /// ITEM 5. The 1.0.106 ship noted two `own-window fallback:` lines logged
    /// in the SAME MILLISECOND for one press: guard 2 asks about the HOOK, and
    /// the hook is the one witness that cannot double-fire this path. A second
    /// fallback Space-down while one is live is now a no-op.
    #[test]
    fn a_second_fallback_space_down_while_one_is_live_is_a_no_op() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        // No fallback hold: the ordinary accept.
        assert!(own_window_space_down_accepted(
            true,
            false,
            false,
            None,
            OWN_WINDOW_DEDUPE_MS,
            None,
            OWN_WINDOW_MAX_HOLD_MS, false));
        // One live, of any age inside the bound — including the same
        // millisecond, which is the shape that was actually logged.
        for age in [0u64, 1, 200, OWN_WINDOW_MAX_HOLD_MS] {
            assert!(
                !own_window_space_down_accepted(
                    true,
                    false,
                    false,
                    None,
                    OWN_WINDOW_DEDUPE_MS,
                    Some(age),
                    OWN_WINDOW_MAX_HOLD_MS, false),
                "a fallback hold {age}ms old already owns this press"
            );
        }
        // An EXPIRED fallback hold is not a duplicate of anything: the page was
        // torn down mid-hold and its keyup is never coming. Refusing here would
        // wedge the ring shut for the rest of the 30s bound — the very failure
        // this whole entry is about.
        assert!(own_window_space_down_accepted(
            true,
            false,
            false,
            None,
            OWN_WINDOW_DEDUPE_MS,
            Some(OWN_WINDOW_MAX_HOLD_MS + 1),
            OWN_WINDOW_MAX_HOLD_MS, false));
    }

    /// And the same thing through the real latch, so the wiring is exercised
    /// and not just the predicate.
    #[test]
    fn the_live_fallback_latch_is_what_refuses_the_duplicate() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        MODIFIER_ACTIVE.store(false, Ordering::SeqCst);
        arm_own_window_hold(tick_count().max(1));
        assert!(own_hold_active());
        assert!(!own_window_space_down_accepted(
            true,
            false,
            false,
            None,
            OWN_WINDOW_DEDUPE_MS,
            super::own_hold_age_ms(),
            OWN_WINDOW_MAX_HOLD_MS, false));
        // Leave the statics as they were found.
        super::disarm_own_window_hold();
        OWN_HOLD_TS.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod own_window_fallback_tests {
    use super::{
        own_window_combo_for_vk, own_window_hold_is_ours, own_window_space_down_accepted,
        KeyCombo, OWN_WINDOW_DEDUPE_MS, OWN_WINDOW_MAX_HOLD_MS,
    };

    /// The 2026-09-07 shape, and the only one that may be accepted: our window
    /// is in front, bypass is off, and the hook has said nothing about Space —
    /// either ever, or for far longer than the dedupe window.
    #[test]
    fn a_deaf_hook_over_our_own_window_hands_the_hold_to_the_page() {
        assert!(own_window_space_down_accepted(
            true, false, false, None, OWN_WINDOW_DEDUPE_MS, None, OWN_WINDOW_MAX_HOLD_MS, false));
        assert!(own_window_space_down_accepted(
            true, false, false, Some(60_000), OWN_WINDOW_DEDUPE_MS, None, OWN_WINDOW_MAX_HOLD_MS, false));
        assert!(own_window_space_down_accepted(
            true, false, false, Some(OWN_WINDOW_DEDUPE_MS), OWN_WINDOW_DEDUPE_MS, None, OWN_WINDOW_MAX_HOLD_MS, false));
    }

    /// GUARD 2, THE ONE THAT MATTERS. A healthy hook stamped this same physical
    /// press microseconds ago. Accepting the page's copy is the double-fire.
    #[test]
    fn a_space_the_hook_just_saw_is_never_taken_twice() {
        for age in [0u64, 1, 50, OWN_WINDOW_DEDUPE_MS - 1] {
            assert!(
                !own_window_space_down_accepted(
                    true, false, false, Some(age), OWN_WINDOW_DEDUPE_MS, None, OWN_WINDOW_MAX_HOLD_MS, false),
                "the hook stamped a Space-down {age}ms ago — the page's is the same press"
            );
        }
    }

    /// A latched hold is the hook mid-press. Its Space-down may be older than
    /// the dedupe window (the owner reads the ring), so the age alone is not
    /// enough — `MODIFIER_ACTIVE` is the half that covers a long hook hold.
    #[test]
    fn a_hook_hold_already_in_flight_blocks_the_page_however_old_it_is() {
        assert!(!own_window_space_down_accepted(
            true, false, true, Some(9_000), OWN_WINDOW_DEDUPE_MS, None, OWN_WINDOW_MAX_HOLD_MS, false));
        assert!(!own_window_space_down_accepted(
            true, false, true, None, OWN_WINDOW_DEDUPE_MS, None, OWN_WINDOW_MAX_HOLD_MS, false));
    }

    /// GUARD 1. Nothing else in the process may reach the engine through this
    /// door: if our window is not the foreground window there is no PROBLEM
    /// 257 to work around, and an accepted event would be an unexplained hold.
    #[test]
    fn nothing_is_accepted_while_another_window_has_the_foreground() {
        assert!(!own_window_space_down_accepted(
            false, false, false, None, OWN_WINDOW_DEDUPE_MS, None, OWN_WINDOW_MAX_HOLD_MS, false));
    }

    /// Bypass means Space is an ordinary space everywhere, and "everywhere"
    /// has to include the one window that has a second way in.
    #[test]
    fn bypass_mode_switches_the_fallback_off_too() {
        assert!(!own_window_space_down_accepted(
            true, true, false, None, OWN_WINDOW_DEDUPE_MS, None, OWN_WINDOW_MAX_HOLD_MS, false));
    }

    /// GUARD 3. A key or a release with no fallback hold behind it is a stray:
    /// either the hook owns this hold, or a page reloaded mid-press.
    #[test]
    fn a_key_without_a_fallback_hold_is_ignored() {
        assert!(!own_window_hold_is_ours(None, OWN_WINDOW_MAX_HOLD_MS));
        assert!(own_window_hold_is_ours(Some(0), OWN_WINDOW_MAX_HOLD_MS));
        assert!(own_window_hold_is_ours(Some(4_000), OWN_WINDOW_MAX_HOLD_MS));
    }

    /// The page that never sent its `keyup` (navigation, reload, crash). The
    /// hold must expire on its own or the next stray key becomes a launch.
    #[test]
    fn a_fallback_hold_that_outlives_its_page_expires() {
        assert!(own_window_hold_is_ours(
            Some(OWN_WINDOW_MAX_HOLD_MS),
            OWN_WINDOW_MAX_HOLD_MS
        ));
        assert!(!own_window_hold_is_ours(
            Some(OWN_WINDOW_MAX_HOLD_MS + 1),
            OWN_WINDOW_MAX_HOLD_MS
        ));
    }

    /// The map is the hook's, narrowed. A–Z must survive it — that is the
    /// owner's requirement ("Space+letter must launch") in one assertion.
    #[test]
    fn every_letter_maps_to_the_same_alpha_the_hook_would_send() {
        for vk in 0x41u16..=0x5A {
            let ch = char::from_u32(vk as u32 + 32).unwrap();
            assert!(
                matches!(own_window_combo_for_vk(vk), Some(KeyCombo::Alpha(c)) if c == ch),
                "VK {vk:#04X} must map to Alpha('{ch}')"
            );
        }
    }

    /// The three punctuation combos the ring offers, and nothing else.
    #[test]
    fn the_three_punctuation_combos_survive_and_the_excluded_keys_do_not() {
        assert!(matches!(own_window_combo_for_vk(0xC0), Some(KeyCombo::Backtick)));
        assert!(matches!(own_window_combo_for_vk(0xBC), Some(KeyCombo::Comma)));
        assert!(matches!(own_window_combo_for_vk(0xBE), Some(KeyCombo::Period)));
        assert!(matches!(own_window_combo_for_vk(0xBA), Some(KeyCombo::Semicolon)));
        assert!(matches!(own_window_combo_for_vk(0xBF), Some(KeyCombo::Slash)));
        assert!(matches!(own_window_combo_for_vk(0xDE), Some(KeyCombo::Quote)));
        // Escape, Enter, Tab, Backspace, arrows, Right Alt — the brief's
        // exclusion list. A page needs these to be a page.
        for vk in [0x1Bu16, 0x0D, 0x09, 0x08, 0x25, 0x26, 0x27, 0x28, 0xA5] {
            assert!(
                own_window_combo_for_vk(vk).is_none(),
                "VK {vk:#04X} must stay with the page"
            );
        }
        // Digits: the hook maps none of them either (`vk_to_char` is A–Z), so
        // suppressing one here would cost a keystroke and buy nothing.
        for vk in 0x30u16..=0x39 {
            assert!(own_window_combo_for_vk(vk).is_none());
        }
        // F1–F12 are gated on hook-side `BOUND_SPECIALS` the page cannot read.
        for vk in 0x70u16..=0x7B {
            assert!(own_window_combo_for_vk(vk).is_none());
        }
    }
}

// ---------------------------------------------------------------------------
// PROBLEM 261 — pointer activation under a FALLBACK hold.
//
// The owner's 1.0.105 report: with the dashboard focused, holding Space drew
// the ring (PROBLEM 259's fallback working) but "it's not seeing my cursor
// movement and it's not opening apps when clicked". Every gate below is one of
// the reasons why, turned into a decision a test can walk.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod own_window_pointer_tests {
    use super::{
        arm_own_window_hold, disarm_own_window_hold, hold_latched, hold_ts_for,
        own_hold_active, own_hold_reap_reason, OwnHoldReap, OWN_HOLD_ACTIVE, OWN_HOLD_COMBO,
        OWN_HOLD_TS, OWN_WINDOW_MAX_HOLD_MS, SPACE_ABORTED,
    };
    use crate::hook::pointer;
    use std::sync::atomic::Ordering;

    /// The whole bug in one assertion. `MODIFIER_ACTIVE` is false for the
    /// entire life of a fallback hold — nothing on that path writes it, and
    /// deliberately so (it is guard 2's arbiter) — so a gate that reads it
    /// alone is a gate the fallback can never open.
    #[test]
    fn a_fallback_hold_is_a_latched_hold_even_though_the_callback_never_ran() {
        assert!(!hold_latched(false, false, false), "no hold at all");
        assert!(hold_latched(true, false, false), "an ordinary hook hold");
        assert!(
            hold_latched(false, true, false),
            "A FALLBACK HOLD. This is the assertion that was false in 1.0.105: the mouse \
             callback returned before note_cursor and before WM_LBUTTONDOWN, and the \
             poller's `live` term was false, so nothing could ever arm inside the dashboard."
        );
        // Both, briefly, while one hold is torn down as another begins. A hold
        // IS latched during that transient — `||`, never `^`.
        assert!(hold_latched(true, true, false));
        // PROBLEM 263 — THE THIRD WITNESS. Without this term the mouse
        // callback returns before `note_cursor` and before the WM_LBUTTONDOWN
        // branch for the whole of a middle-button hold, which is 1.0.105's
        // failure exactly: a ring on screen the cursor cannot aim at and a
        // click that cannot launch anything.
        assert!(hold_latched(false, false, true), "a middle-button hold");
        assert!(hold_latched(false, true, true));
        assert!(hold_latched(true, false, true));
    }

    /// The quiet fourth gate. `SPACE_DOWN_TS` identifies WHICH hold a poller
    /// tick belongs to and is the floor `CURSOR_STAMP` must clear before a
    /// cursor position counts as "moved during THIS hold". A fallback hold
    /// never stamps it, so without this the tracker would pair a fallback hold
    /// with a stamp from some previous hook hold — or with 0 at boot, which
    /// every stamp clears.
    #[test]
    fn a_fallback_hold_is_identified_by_its_own_stamp_not_the_last_hook_holds() {
        // No fallback hold: the hook's stamp, unchanged.
        assert_eq!(hold_ts_for(9_000, false, 0, false, 0), 9_000);
        assert_eq!(hold_ts_for(9_000, false, 4_242, false, 0), 9_000);
        // A fallback hold owns the identity.
        assert_eq!(hold_ts_for(9_000, true, 12_345, false, 0), 12_345);
        // Boot: the hook has never stamped one. Without the fallback's stamp
        // the floor would be 0 and EVERY stale cursor position would qualify.
        assert_eq!(hold_ts_for(0, true, 12_345, false, 0), 12_345);
        // Latched with no stamp is a torn state, not a hold — fall back to the
        // hook's stamp rather than publish 0 as a floor.
        assert_eq!(hold_ts_for(9_000, true, 0, false, 0), 9_000);
        // PROBLEM 263 — a MIDDLE-BUTTON hold owns the identity on exactly the
        // same rule, and its stamp must win over a stale hook stamp for exactly
        // the same reason: the tracker would otherwise pair it with a cursor
        // position the user parked minutes ago and arm a chip nobody aimed at.
        assert_eq!(hold_ts_for(9_000, false, 0, true, 777), 777);
        assert_eq!(hold_ts_for(0, false, 0, true, 777), 777);
        // Latched with no stamp is a torn state here too.
        assert_eq!(hold_ts_for(9_000, false, 0, true, 0), 9_000);
    }

    /// Brief item 3, the PROBLEM 218 class on the fallback's own latch. The
    /// reaper `reap_stale_hold` cannot see this shape: it measures the
    /// keyboard hook's auto-repeat, and a fallback hold produces none.
    #[test]
    fn the_reaper_covers_a_fallback_hold_whose_release_never_came() {
        // Nothing latched: never reap, however bad the other evidence looks.
        assert_eq!(
            own_hold_reap_reason(false, Some(999_999), OWN_WINDOW_MAX_HOLD_MS, Some(false)),
            None
        );
        // A healthy hold, foreground confirmed: leave it alone.
        assert_eq!(
            own_hold_reap_reason(true, Some(1_200), OWN_WINDOW_MAX_HOLD_MS, Some(true)),
            None
        );
        // A healthy hold on a tick that did NOT probe the foreground. "Not
        // checked" may never be a reason to reap.
        assert_eq!(
            own_hold_reap_reason(true, Some(1_200), OWN_WINDOW_MAX_HOLD_MS, None),
            None
        );
        // THE ALT-TAB SHAPE. Guard 1 admitted this hold because our window was
        // foreground; re-asked, it says no. This is also the case the page's
        // own `blur` listener cannot cover once the page itself is gone.
        assert_eq!(
            own_hold_reap_reason(true, Some(1_200), OWN_WINDOW_MAX_HOLD_MS, Some(false)),
            Some(OwnHoldReap::ForegroundLost)
        );
        // THE DEAD-PAGE SHAPE: still foreground, still latched, past the bound.
        assert_eq!(
            own_hold_reap_reason(
                true,
                Some(OWN_WINDOW_MAX_HOLD_MS + 1),
                OWN_WINDOW_MAX_HOLD_MS,
                Some(true)
            ),
            Some(OwnHoldReap::Expired)
        );
        // Exactly at the bound is still ours — `own_window_hold_is_ours` uses
        // `<=`, and the two must not disagree about the same millisecond.
        assert_eq!(
            own_hold_reap_reason(
                true,
                Some(OWN_WINDOW_MAX_HOLD_MS),
                OWN_WINDOW_MAX_HOLD_MS,
                Some(true)
            ),
            None
        );
        // Latched with no stamp: a torn state the latch must not survive.
        assert_eq!(
            own_hold_reap_reason(true, None, OWN_WINDOW_MAX_HOLD_MS, Some(true)),
            Some(OwnHoldReap::Expired)
        );
        // Expiry is reported ahead of a lost foreground: when both are true the
        // log should name the bound that is actually unrecoverable.
        assert_eq!(
            own_hold_reap_reason(true, Some(999_999), OWN_WINDOW_MAX_HOLD_MS, Some(false)),
            Some(OwnHoldReap::Expired)
        );
    }

    /// Brief item 4. PROBLEM 259's dedupe is a 100 ms window and a clock, not
    /// a mutex. If it ever lets two calls through for one physical press, the
    /// worst outcome must be a duplicate log line — never two arms, never a
    /// hold that survives its own release.
    ///
    /// Serial by construction rather than by attribute: it is the only test in
    /// the tree that writes `OWN_HOLD_*`, and it leaves them as it found them.
    #[test]
    fn arming_twice_for_one_press_is_the_same_state_as_arming_once() {
        // Poison every latch first, so a passing assertion below can only mean
        // the arm cleared it rather than that it happened to be clear already.
        SPACE_ABORTED.store(true, Ordering::SeqCst);
        pointer::ARMED_INDEX.store(7, Ordering::SeqCst);
        OWN_HOLD_COMBO.store(true, Ordering::SeqCst);

        arm_own_window_hold(1_000);
        let after_one = (
            OWN_HOLD_ACTIVE.load(Ordering::SeqCst),
            OWN_HOLD_TS.load(Ordering::SeqCst),
            OWN_HOLD_COMBO.load(Ordering::SeqCst),
            SPACE_ABORTED.load(Ordering::SeqCst),
            pointer::ARMED_INDEX.load(Ordering::SeqCst),
        );
        assert_eq!(
            after_one,
            (true, 1_000, false, false, -1),
            "one arm must latch the hold, stamp it, and clear every per-hold latch \
             pointer activation depends on"
        );

        arm_own_window_hold(1_000);
        assert_eq!(
            (
                OWN_HOLD_ACTIVE.load(Ordering::SeqCst),
                OWN_HOLD_TS.load(Ordering::SeqCst),
                OWN_HOLD_COMBO.load(Ordering::SeqCst),
                SPACE_ABORTED.load(Ordering::SeqCst),
                pointer::ARMED_INDEX.load(Ordering::SeqCst),
            ),
            after_one,
            "IDEMPOTENCE: a second arm for the same press must change nothing"
        );
        assert!(own_hold_active());

        // Teardown is a claim: exactly one caller owns it, and a second call
        // is a no-op. The release path, the reaper and the watchdog all race
        // here, and a double teardown would emit two `guide-hud-hide` events.
        assert!(disarm_own_window_hold(), "the first teardown owns the hold");
        assert!(!disarm_own_window_hold(), "a second teardown owns nothing");
        assert!(!own_hold_active());
        assert_eq!(OWN_HOLD_TS.load(Ordering::SeqCst), 0);
        assert_eq!(
            pointer::ARMED_INDEX.load(Ordering::SeqCst),
            -1,
            "PROBLEM 206/218 — an armed chip may not outlive the hold that armed it"
        );

        // Leave the world as it was found.
        SPACE_ABORTED.store(false, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// PROBLEM 263 — THE MIDDLE-BUTTON RING TRIGGER.
//
// Everything below is a PURE state-machine test. The gesture itself cannot be
// tested here — `SendInput` returns success and the hook sees nothing from a
// containerised agent shell (CLAUDE.md testing laws), and this suite has no
// mouse, no hook and no window. So the decisions were written as pure
// functions precisely so this file could walk every branch of them, and what
// these tests prove is the DECISION, never the gesture.
//
// NOTHING IN THIS FEATURE HAS RUN ON REAL HARDWARE.
// ---------------------------------------------------------------------------

/// THE DOWN GATE — every reason the middle button may decline to raise a ring.
///
/// One test per gate, plus one that walks all of them, because the failure this
/// guards against is not "a gate is wrong", it is "a gate was quietly dropped
/// during a refactor and nothing noticed". A gate with no test of its own is a
/// gate whose deletion is invisible.
#[cfg(test)]
mod middle_button_gate_tests {
    use super::middle_button_down_accepted;

    /// The one accepting shape, named so the other tests can be read as deltas
    /// from it: feature on, watcher alive, no exclusion of either kind, not
    /// bypassed, not fullscreen, and no hold latched anywhere.
    const OPEN: [bool; 9] = [true, true, false, false, false, false, false, false, false];

    fn call(a: [bool; 9]) -> bool {
        middle_button_down_accepted(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8])
    }

    /// Every gate open is the only combination that may ever return true.
    #[test]
    fn a_plain_middle_press_with_every_gate_open_raises_the_ring() {
        assert!(call(OPEN));
    }

    /// THE SETTINGS SWITCH. Off means the middle button is a middle button —
    /// `ms_hook_proc` returns `CallNextHookEx` and the app underneath receives
    /// a press byte-identical to a build that never had this feature.
    #[test]
    fn the_settings_switch_off_hands_the_button_straight_back() {
        let mut a = OPEN;
        a[0] = false;
        assert!(!call(a));
    }

    /// THE FAIL-CLOSED GATE, AND THE MOST IMPORTANT TEST IN THIS FILE.
    ///
    /// `st-exclusion-watcher` is explicitly ALLOWED to fail to spawn (PROBLEM
    /// 124). If it never runs, `ORBIT_ACTIVE` sits `false` for the whole
    /// session — not because no CAD program is in front, but because nothing
    /// ever looked. Without this gate the middle button would then be swallowed
    /// inside SolidWorks, Blender and AutoCAD, silently, for as long as the app
    /// is running.
    ///
    /// The rule it encodes: **when a guard and the feature it guards can fail
    /// independently, the feature must be the one that fails.**
    #[test]
    fn no_exclusion_watcher_means_no_trigger_at_all() {
        let mut a = OPEN;
        a[1] = false;
        assert!(
            !call(a),
            "with no 3D/CAD verdict ever measured the trigger must stand down — a false \
             ORBIT_ACTIVE is 'nobody looked', not 'no CAD program'"
        );
        // And it is not rescued by the feature being switched on, which is the
        // shape the bug would actually take.
        a[0] = true;
        assert!(!call(a));
    }

    /// THE BUILT-IN LIST. Middle-drag orbits the model in SolidWorks, Fusion
    /// 360, Blender and their neighbours; swallowing it there would delete a
    /// workflow rather than add a feature. This gate is `orbit_apps.rs` and it
    /// is SEPARATE from the user's own exceptions — see the next test.
    #[test]
    fn a_3d_or_cad_program_keeps_its_orbit_gesture() {
        let mut a = OPEN;
        a[2] = true;
        assert!(!call(a));
    }

    /// THE USER'S OWN APP EXCEPTIONS, which stand the WHOLE app down. Passed as
    /// a separate parameter from the built-in list even though `ms_hook_proc`
    /// has already returned on `EXCLUDED_ACTIVE` before this function is
    /// reached, so that the gate lists every reason in one readable place — and
    /// so this assertion exists to be broken if that early return is ever moved.
    #[test]
    fn the_users_own_app_exceptions_gate_this_like_everything_else() {
        let mut a = OPEN;
        a[3] = true;
        assert!(!call(a));
    }

    /// Bypass mode is "Spaceadom is paused". A trigger that ignored it would
    /// make the pause control a lie.
    #[test]
    fn bypass_mode_makes_the_middle_button_a_middle_button() {
        let mut a = OPEN;
        a[4] = true;
        assert!(!call(a));
    }

    /// The fullscreen/game gate, reused rather than re-derived. A middle click
    /// in a shooter is a weapon, not a ring.
    #[test]
    fn the_fullscreen_game_gate_covers_this_trigger_too() {
        let mut a = OPEN;
        a[5] = true;
        assert!(!call(a));
    }

    /// A `WM_MBUTTONDOWN` can repeat when a release was lost. The second one is
    /// not a new press, and accepting it would arm a second hold whose teardown
    /// nothing owes.
    #[test]
    fn a_repeated_down_cannot_start_a_second_hold() {
        let mut a = OPEN;
        a[8] = true;
        assert!(!call(a));
    }

    /// EVERY GATE IS INDEPENDENTLY SUFFICIENT. Walked as a loop so that adding a
    /// tenth reason without adding its test still fails here — the arity changes
    /// and this file stops compiling, which is the point.
    #[test]
    fn every_gate_alone_is_enough_to_decline() {
        // Indices 0 and 1 decline when FALSE; 2..=8 decline when TRUE.
        for i in 0..2 {
            let mut a = OPEN;
            a[i] = false;
            assert!(!call(a), "parameter {i} must decline on its own when false");
        }
        for i in 2..9 {
            let mut a = OPEN;
            a[i] = true;
            assert!(!call(a), "parameter {i} must decline on its own when true");
        }
    }
}

/// THE ARBITRATION — three witnesses, one gesture, tested IN BOTH ORDERS.
///
/// The rule is stated in full beside `MIDDLE_TAP_MS`; these tests are the half
/// of it that is pure. Rule B lives inside `kb_hook_proc`'s Space-down branch
/// and cannot be called from here, so what is tested is the LATCH that branch
/// reads — see `rule_b_is_a_latch_and_this_is_the_latch_it_reads`.
#[cfg(test)]
mod middle_button_arbitration_tests {
    use super::{
        middle_button_down_accepted, own_window_space_down_accepted, OWN_WINDOW_DEDUPE_MS,
        OWN_WINDOW_MAX_HOLD_MS,
    };

    /// ORDER ONE: A SPACE HOLD IS LIVE, THEN THE MIDDLE BUTTON GOES DOWN.
    ///
    /// Rule A. From EITHER Space witness — the keyboard hook's `MODIFIER_ACTIVE`
    /// or the own-window fallback's `OWN_HOLD_ACTIVE` — and from both at once,
    /// which is a legal teardown transient rather than a contradiction.
    /// Declining means the press is never eaten, so nothing is owed and the app
    /// underneath sees a completely ordinary middle click.
    #[test]
    fn rule_a_a_space_hold_from_either_witness_refuses_the_middle_button() {
        let open = [true, true, false, false, false, false, false, false, false];
        for (hook, own) in [(true, false), (false, true), (true, true)] {
            let mut a = open;
            a[6] = hook;
            a[7] = own;
            assert!(
                !middle_button_down_accepted(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8]),
                "a live Space hold (hook: {hook}, fallback: {own}) owns the gesture — the \
                 middle button must pass straight through"
            );
        }
    }

    /// ORDER TWO: A MIDDLE HOLD IS LIVE, THEN SPACE GOES DOWN.
    ///
    /// Rule C, the fallback's half. Guard 2c asks about the middle button the
    /// identical question guard 2 asks about the hook. Without it, the ONE
    /// window where the keyboard hook is deaf (PROBLEM 257, the dashboard) would
    /// be the one window where two triggers could both serve one gesture — two
    /// rings, two launches, two toasts.
    #[test]
    fn rule_c_a_live_middle_hold_refuses_the_own_window_fallback() {
        // The shape that WOULD be accepted, so the delta is only the middle
        // hold: our window in front, bypass off, hook silent, no fallback hold.
        assert!(own_window_space_down_accepted(
            true, false, false, None, OWN_WINDOW_DEDUPE_MS, None, OWN_WINDOW_MAX_HOLD_MS, false
        ));
        assert!(
            !own_window_space_down_accepted(
                true, false, false, None, OWN_WINDOW_DEDUPE_MS, None, OWN_WINDOW_MAX_HOLD_MS, true
            ),
            "a middle-button hold already owns a ring — this Space is an ordinary space"
        );
    }

    /// AND THE TWO ORDERS AS A SEQUENCE, which is the property that actually
    /// matters and is the one thing neither test above states on its own.
    ///
    /// **A NOTE ON A TEST THAT WAS WRITTEN WRONG FIRST, because the mistake is
    /// more instructive than the test (2026-09-10).** The first version of this
    /// walked all eight latch combinations and asserted
    /// `!(middle_taken && fallback_taken)` on each. It failed — correctly — on
    /// the all-clear row, and the code was right and the assertion was wrong.
    /// From a clean state BOTH witnesses are legitimately willing; that is not
    /// a double-fire, it is what "either trigger may start a ring" means. What
    /// makes them exclusive is not that they disagree from the same snapshot,
    /// it is that **whoever goes first LATCHES, and the latch is what the other
    /// one reads.** Exclusivity is a property of the sequence, so the test has
    /// to be a sequence. Generalise: *a mutual-exclusion test that never
    /// advances the state is testing a coincidence, not an invariant.*
    #[test]
    fn whichever_witness_goes_first_locks_the_other_out() {
        // Nothing latched: both are willing, and that is correct.
        let middle_from_clean =
            middle_button_down_accepted(true, true, false, false, false, false, false, false, false);
        let fallback_from_clean = own_window_space_down_accepted(
            true, false, false, None, OWN_WINDOW_DEDUPE_MS, None, OWN_WINDOW_MAX_HOLD_MS, false,
        );
        assert!(middle_from_clean && fallback_from_clean, "from rest, either may start a ring");

        // ORDER ONE — the middle button got there first, so its latch is set.
        // Rule C: the fallback must now refuse. (Rule B, the hook's half of the
        // same instant, is the branch `rule_b_is_a_latch_…` covers.)
        assert!(
            !own_window_space_down_accepted(
                true, false, false, None, OWN_WINDOW_DEDUPE_MS, None, OWN_WINDOW_MAX_HOLD_MS, true,
            ),
            "middle first: the fallback must be locked out (rule C)"
        );

        // ORDER TWO — a Space hold got there first, from EITHER witness, so its
        // latch is set. Rule A: the middle button must now refuse, and its
        // WM_MBUTTONDOWN passes through untouched.
        for (hook, own) in [(true, false), (false, true)] {
            assert!(
                !middle_button_down_accepted(
                    true, true, false, false, false, false, hook, own, false,
                ),
                "space first (hook:{hook} fallback:{own}): the middle button must be locked out \
                 (rule A)"
            );
        }

        // AND THE LATCH THAT DOES THE LOCKING IS NOT SELF-CLEARING: a second
        // WM_MBUTTONDOWN arriving under a live middle hold (a lost release, an
        // auto-repeating driver) is not a new press either.
        assert!(!middle_button_down_accepted(
            true, true, false, false, false, false, false, false, true
        ));
    }

    /// RULE B is enforced by an early `CallNextHookEx` inside `kb_hook_proc`,
    /// which needs a real hook to call and cannot be reached from a test. What
    /// CAN be pinned down is the thing that branch reads, and this is it: the
    /// accessor `middle_hold_active()` and the atomic behind it are the entire
    /// mechanism, so if a refactor ever changed what rule B consults, this
    /// assertion is the one that would have to be edited to keep passing.
    ///
    /// Kept in the same module as rules A and C on purpose: the arbitration is
    /// three rules or it is nothing, and a reader who found only two here would
    /// reasonably conclude Space-during-a-middle-hold was never considered.
    #[test]
    fn rule_b_is_a_latch_and_this_is_the_latch_it_reads() {
        use super::{middle_hold_active, MIDDLE_HOLD_ACTIVE};
        use std::sync::atomic::Ordering;
        let restore = MIDDLE_HOLD_ACTIVE.load(Ordering::SeqCst);
        MIDDLE_HOLD_ACTIVE.store(true, Ordering::SeqCst);
        assert!(middle_hold_active(), "kb_hook_proc's rule-B branch reads exactly this");
        MIDDLE_HOLD_ACTIVE.store(false, Ordering::SeqCst);
        assert!(!middle_hold_active());
        MIDDLE_HOLD_ACTIVE.store(restore, Ordering::SeqCst);
    }
}

/// TAP vs HOLD, and the replay decision — the same function, because they are
/// the same decision seen from two sides.
///
/// The contract this protects is the one the owner named: *a quick middle click
/// must still behave normally everywhere.* We SUPPRESS the `WM_MBUTTONDOWN`, so
/// a wrong answer here does not degrade a feature, it eats a click the user is
/// entitled to.
#[cfg(test)]
mod middle_tap_vs_hold_tests {
    use super::{middle_press_was_a_click, MIDDLE_TAP_MS};

    /// A deliberate click is 60-150 ms. All of it must be replayed.
    #[test]
    fn a_quick_click_is_replayed() {
        for held in [0u64, 1, 60, 100, 150, MIDDLE_TAP_MS - 1] {
            assert!(
                middle_press_was_a_click(held, MIDDLE_TAP_MS, false),
                "{held}ms is a click and this process owes the world one"
            );
        }
    }

    /// Past the threshold the press was a HOLD: it raised a ring, and firing a
    /// middle click into the app underneath as well would be the double-fire in
    /// its most damaging form (a click the user never made, at a place they were
    /// only pointing).
    #[test]
    fn a_hold_past_the_threshold_is_never_replayed() {
        for held in [MIDDLE_TAP_MS, MIDDLE_TAP_MS + 1, 1_000, 30_000, u64::MAX] {
            assert!(
                !middle_press_was_a_click(held, MIDDLE_TAP_MS, false),
                "{held}ms is a hold — no click may be replayed"
            );
        }
    }

    /// THE BOUNDARY IS EXCLUSIVE, and it is written down because `<` versus `<=`
    /// here is a one-character change no other test would catch: exactly
    /// `MIDDLE_TAP_MS` is a HOLD.
    #[test]
    fn the_boundary_belongs_to_the_hold() {
        assert!(middle_press_was_a_click(MIDDLE_TAP_MS - 1, MIDDLE_TAP_MS, false));
        assert!(!middle_press_was_a_click(MIDDLE_TAP_MS, MIDDLE_TAP_MS, false));
    }

    /// `SPACE_ABORTED` means SOMETHING ELSE CLAIMED THIS PRESS — a combo was
    /// typed, the wheel was turned, or a chip was armed. Every one of those is a
    /// reason a click must not also fire, and reusing the flag rather than
    /// inventing one is what keeps this identical to the `!SPACE_ABORTED` test
    /// that decides whether a Space release types a space.
    ///
    /// Note it beats the duration on BOTH sides of the boundary: a chip armed
    /// 40 ms in still cancels the replay.
    #[test]
    fn anything_else_claiming_the_press_cancels_the_replay() {
        for held in [0u64, 40, MIDDLE_TAP_MS - 1, MIDDLE_TAP_MS, 5_000] {
            assert!(
                !middle_press_was_a_click(held, MIDDLE_TAP_MS, true),
                "{held}ms with something else claiming the press must never replay a click"
            );
        }
    }

    /// A SPEC TEST ON THE TWO THRESHOLDS, not on a function.
    ///
    /// `MIDDLE_TAP_MS` (250) sits below the default `guide_hud_delay_ms` (300),
    /// which is what makes a replayed click one the user never saw a ring for.
    /// They were chosen to compose, and nothing else in the codebase records
    /// that relationship — if someone raises the tap threshold to 400 ms
    /// "because clicks are slow", every ordinary middle click starts flashing a
    /// ring before it lands, and no other test would say a word.
    #[test]
    fn the_tap_threshold_stays_below_the_default_ring_delay() {
        let default_delay = crate::config::AppConfig::default().guide_hud_delay_ms;
        assert_eq!(default_delay, 300, "the default this constant was chosen against");
        assert!(
            MIDDLE_TAP_MS < default_delay,
            "a click that is going to be replayed must normally not have drawn a ring: \
             MIDDLE_TAP_MS ({MIDDLE_TAP_MS}) must stay under guide_hud_delay_ms ({default_delay})"
        );
        // And clear of Windows' own 500 ms double-click window, so a double
        // middle-click is two replayed clicks rather than one click and a ring.
        assert!(MIDDLE_TAP_MS < 500);
    }
}

/// THE STALE-HOLD REAPER for the middle button — the hard requirement that a
/// middle-button hold can never latch forever.
///
/// Why a THIRD reaper rather than a branch in one of the two that exist:
/// `reap_stale_hold` measures KEYBOARD auto-repeat (PROBLEM 218) and a middle
/// hold produces none; `reap_own_window_hold` measures the foreground window.
/// This one measures whether the MOUSE callback is being called at all, because
/// the `WM_MBUTTONUP` that ends the hold is delivered by that hook and by
/// nothing else. Merging any two of them means one shape losing its evidence.
#[cfg(test)]
mod middle_hold_reaper_tests {
    use super::{
        middle_hold_reap_reason, MiddleHoldReap, FORCED_INPUT_MAX_AGE_MS, MIDDLE_DEAF_SILENCE_MS,
        MIDDLE_MAX_HOLD_MS,
    };

    fn reap(active: bool, age: u64, silence: Option<u64>, os_input_age: u64) -> Option<MiddleHoldReap> {
        middle_hold_reap_reason(
            active,
            age,
            MIDDLE_MAX_HOLD_MS,
            silence,
            os_input_age,
            MIDDLE_DEAF_SILENCE_MS,
            FORCED_INPUT_MAX_AGE_MS,
        )
    }

    /// PROBLEM 262's lesson applied to the other hook: the OS is accepting input
    /// right now, and our mouse callback — which stamps its clock above EVERY
    /// gate, so silence means it was not entered at all — has said nothing for
    /// far longer. The hook is installed and is not being called, so the release
    /// that would end this hold can never arrive.
    #[test]
    fn a_hold_whose_release_can_never_arrive_is_torn_down_on_deafness() {
        assert_eq!(
            reap(true, 500, Some(MIDDLE_DEAF_SILENCE_MS), 0),
            Some(MiddleHoldReap::MouseDeaf),
            "half a second into a hold, with proof the hook is deaf, is enough"
        );
        assert_eq!(
            reap(true, 500, Some(MIDDLE_DEAF_SILENCE_MS + 5_000), FORCED_INPUT_MAX_AGE_MS),
            Some(MiddleHoldReap::MouseDeaf)
        );
    }

    /// DEAFNESS NEEDS BOTH HALVES, and neither alone is evidence of anything. A
    /// silent callback on an idle machine is a user who walked away; recent OS
    /// input with a callback that is keeping up is a healthy hold.
    #[test]
    fn deafness_needs_both_halves_and_neither_alone_proves_it() {
        // Recent input, but the callback is keeping up — healthy.
        assert_eq!(reap(true, 500, Some(0), 0), None);
        assert_eq!(reap(true, 500, Some(MIDDLE_DEAF_SILENCE_MS - 1), 0), None);
        // Callback silent, but the OS has had no input either — nobody is
        // touching the machine, which proves nothing about the hook.
        assert_eq!(
            reap(true, 500, Some(60_000), FORCED_INPUT_MAX_AGE_MS + 1),
            None,
            "an idle machine mid-hold is not a deaf hook"
        );
    }

    /// UNKNOWN IS NEVER PROOF (PROBLEM 228). A callback that has not fired since
    /// the last `install_hooks()` cannot be measured, so its silence is `None` —
    /// and `None` must fall through to the bound, never to a verdict.
    #[test]
    fn an_unmeasurable_callback_is_unknown_not_deaf() {
        assert_eq!(reap(true, 500, None, 0), None);
        assert_eq!(reap(true, 500, None, FORCED_INPUT_MAX_AGE_MS), None);
        // ...and the bound still reaches it, which is the whole point of having
        // a bound as well as a verdict.
        assert_eq!(
            reap(true, MIDDLE_MAX_HOLD_MS + 1, None, 0),
            Some(MiddleHoldReap::Expired)
        );
    }

    /// THE LAST RESORT, and the reason a middle hold can never latch forever
    /// even when nothing can be proven about anything: 30 s, exclusive.
    #[test]
    fn the_thirty_second_bound_is_the_backstop() {
        assert_eq!(reap(true, MIDDLE_MAX_HOLD_MS, None, u64::MAX), None);
        assert_eq!(
            reap(true, MIDDLE_MAX_HOLD_MS + 1, None, u64::MAX),
            Some(MiddleHoldReap::Expired)
        );
        assert_eq!(
            reap(true, u64::MAX, None, u64::MAX),
            Some(MiddleHoldReap::Expired),
            "the unmeasurable-age case (MIDDLE_DOWN_TS == 0 under a live latch) must reap, not sit"
        );
    }

    /// Deafness is checked FIRST because it fires at 3 s and the bound at 30 s.
    /// When both are true the log must say which instrument spoke, and the
    /// answer must be the one that actually detected something.
    #[test]
    fn deafness_outranks_mere_expiry_when_both_are_true() {
        assert_eq!(
            reap(true, MIDDLE_MAX_HOLD_MS + 10_000, Some(MIDDLE_DEAF_SILENCE_MS), 0),
            Some(MiddleHoldReap::MouseDeaf)
        );
    }

    /// NO HOLD, NOTHING TO REAP — the first line of the function and the one
    /// that keeps this off the poller's hot path. Asserted for every evidence
    /// shape, because "active" being ignored is exactly the bug that would tear
    /// down a hold that does not exist and hide a ring somebody else raised.
    #[test]
    fn nothing_is_reaped_when_no_middle_hold_is_latched() {
        for silence in [None, Some(0), Some(60_000)] {
            for age in [0u64, MIDDLE_MAX_HOLD_MS + 1, u64::MAX] {
                assert_eq!(reap(false, age, silence, 0), None);
            }
        }
    }

    /// The two bounds are the same figures as their Space-side twins, and that
    /// is a decision (people hold the trigger and READ the ring), not a
    /// coincidence. Written down so that changing one alone is a visible act.
    #[test]
    fn the_bounds_match_their_space_side_twins() {
        assert_eq!(MIDDLE_MAX_HOLD_MS, super::OWN_WINDOW_MAX_HOLD_MS);
        assert_eq!(MIDDLE_MAX_HOLD_MS, super::MAX_MODIFIER_HOLD_MS);
        assert_eq!(MIDDLE_DEAF_SILENCE_MS, super::HOLD_DEAF_SILENCE_MS);
    }
}

/// THE LATCH ITSELF — arm, disarm, and the debt in between.
///
/// These are the only middle-button tests that touch process-wide state. They
/// are confined to the `MIDDLE_*` atomics, which nothing else in this suite
/// reads or writes, so they cannot flake against a test running in parallel —
/// and they deliberately do NOT assert on `SPACE_ABORTED`, `MODIFIER_ACTIVE` or
/// `pointer::ARMED_INDEX`, which are shared with the Space paths and with the
/// own-window tests. What those omissions cost is one reading of
/// `arm_middle_hold`; what asserting them would cost is a suite that fails once
/// a week for no reason.
#[cfg(test)]
mod middle_hold_latch_tests {
    use super::{
        arm_middle_hold, disarm_middle_hold, middle_hold_active, MIDDLE_DOWN_TS,
        MIDDLE_HOLD_ACTIVE, MIDDLE_UP_OWED,
    };
    use std::sync::atomic::Ordering;

    /// One test, not five, because these are sequential states of one machine
    /// and splitting them would let two of the pieces run concurrently against
    /// the same atomics.
    #[test]
    fn a_middle_hold_arms_owes_an_up_and_can_only_be_torn_down_once() {
        // Arm.
        arm_middle_hold(4_242);
        assert!(middle_hold_active(), "the ring's latch is set");
        assert_eq!(MIDDLE_DOWN_TS.load(Ordering::SeqCst), 4_242, "stamped");
        assert!(
            MIDDLE_UP_OWED.load(Ordering::SeqCst),
            "WHOEVER EATS THE DOWN OWES THE UP — the WM_MBUTTONDOWN was suppressed"
        );

        // Arming again for the same press must leave exactly what one arm
        // leaves: `arm_middle_hold` is idempotent by construction (every line is
        // a store of a constant), which is what lets the release path, the
        // reaper and a repair race without corrupting the state.
        arm_middle_hold(4_242);
        assert!(middle_hold_active());
        assert_eq!(MIDDLE_DOWN_TS.load(Ordering::SeqCst), 4_242);

        // Tear down. The FIRST caller owns it — that `swap`-based claim is what
        // stops the release path, the reaper, the watchdog and a repair from
        // each hiding the ring and emitting a `guide-hud-hide`.
        assert!(disarm_middle_hold(), "the first teardown owns the hold");
        assert!(!middle_hold_active());
        assert_eq!(MIDDLE_DOWN_TS.load(Ordering::SeqCst), 0);
        assert!(
            !MIDDLE_UP_OWED.load(Ordering::SeqCst),
            "the debt is cleared with the hold: leaving it set would eat the UP of the NEXT \
             genuine middle click, one whose down we passed through"
        );

        // And a second teardown owns nothing.
        assert!(!disarm_middle_hold(), "a second teardown owns nothing");
        assert!(!middle_hold_active());

        // Leave the world as it was found.
        MIDDLE_HOLD_ACTIVE.store(false, Ordering::SeqCst);
        MIDDLE_DOWN_TS.store(0, Ordering::SeqCst);
        MIDDLE_UP_OWED.store(false, Ordering::SeqCst);
    }
}
