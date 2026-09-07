//! safe_mode.rs — PROBLEM 253. If the app cannot get through its own startup,
//! stop trying to do the dangerous part.
//!
//! # The condition this exists for
//!
//! Spaceadom installs a `WH_KEYBOARD_LL` hook, creates a transparent overlay
//! webview and starts an engine thread, all within the first second of every
//! launch. Any one of those can be the thing that kills the process on a
//! machine nobody here has ever seen — a display driver that cannot compose the
//! overlay, a WebView2 runtime mid-update, a rival remapper that makes the hook
//! chain fatal. When that happens the user gets an app that vanishes at every
//! launch, with no window and no way to reach any setting that could turn the
//! broken part off. The log says what happened; the user cannot read it and
//! cannot get to the button that would help.
//!
//! **Safe mode is the launch that does none of it.** No keyboard hook, no
//! overlay window, no engine work — just the dashboard, a banner that says what
//! happened, and two buttons: turn the shortcuts back on, or send a report.
//!
//! # What counts as "crashed at startup", exactly
//!
//! One counter, `boot-attempts.json` in the data dir, holding
//! `failed_starts`. Three transitions, and the whole design is in which of
//! them writes what:
//!
//! | When | What is written |
//! | --- | --- |
//! | `setup()`, i.e. once this process is the SURVIVING instance | `failed_starts + 1` — a launch is a failure until proven otherwise |
//! | alive [`HEALTHY_AFTER`] with no early panic | `0` |
//! | a CLEAN exit before the healthy mark | the value from before this launch — the increment is undone |
//!
//! **REVIEW FIXES 2026-09-05 (C1) — "process start" in that first row used to
//! be literal, and it was a bug.** The increment ran from `run()`, before
//! `tauri-plugin-single-instance` had decided which process survives. A
//! duplicate launch — a second double-click on the tray icon or the desktop
//! shortcut — exits from inside that plugin with `cleanup_before_exit()` +
//! `process::exit(0)`, producing no `RunEvent`, so nothing ever undid the `+1`
//! it had just written. Three impatient double-clicks put a completely healthy
//! app into safe mode. **Only the surviving instance writes**; see
//! [`note_surviving_instance`].
//!
//! Anything else — the process dying, being killed, or panicking inside the
//! first [`HEALTHY_AFTER`] — simply leaves the incremented value on disk,
//! because nothing came along to lower it. That is the point: a crash cannot
//! forget to record itself, since recording it is the DEFAULT and the healthy
//! paths are what have to run.
//!
//! **The clean-exit row is not a nicety, it is PROBLEM 224.** `session_end.rs`
//! takes `WM_ENDSESSION` and calls `std::process::exit(0)` from inside the
//! handler — a sign-out, a shutdown, or an installer using the Restart Manager
//! to close us so it can replace the exe. Every one of those can land in the
//! first thirty seconds of a launch (a reboot autostarts the app and then an
//! update arrives; a user logs on and immediately logs off), and every one of
//! them would otherwise be counted as a startup crash. Three reboots in a row
//! shortly after logon would put a perfectly healthy app into safe mode and
//! tell the owner it had crashed three times. So `session_end::teardown` and
//! the tray's Exit both call [`note_clean_exit`], which undoes this launch's
//! increment.
//!
//! # Why a panic needs its own flag
//!
//! A panic on the hook or engine thread is survivable — PROBLEM 82's supervisor
//! restarts it — so the process can panic at 5 s and still be alive at 30 s.
//! Without [`note_panic`] the 30-second timer would then write `0` and call
//! that launch healthy, which is how a machine that panics on every launch
//! would never reach safe mode. `PANICKED_EARLY` is what makes the healthy
//! marker stand down.
//!
//! # Rules for anything added here
//!
//! 1. **Nothing in this module may be able to stop the app starting.** Every
//!    disk operation is best-effort; an unreadable or corrupt
//!    `boot-attempts.json` reads as "zero failures", never as an error. A
//!    crash-recovery mechanism that can itself crash the app is worse than
//!    none — that is PROBLEM 118's lesson, and this file is exactly the shape
//!    of code it was about (a branch a user only reaches after something has
//!    already gone wrong).
//! 2. **The decision is pure and the I/O is not.** [`decide`],
//!    [`counter_at_launch`] and [`run_launch`] take numbers and return numbers,
//!    so the three cases that matter can be asserted without a filesystem, a
//!    clock, or a crash.
//! 3. **Safe mode is never entered silently.** It writes [`MARKER`] at WARN and
//!    reports one `Degraded::SafeModeEntered` event. An app that quietly
//!    stopped installing its keyboard hook would be indistinguishable from the
//!    hook being broken, which is the fault it is recovering from.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// The file, in `%APPDATA%\Spaceadom`, beside `config.json` and `debug.log`.
pub const BOOT_FILE: &str = "boot-attempts.json";

/// How many consecutive failed startups it takes. Three, not two: a single
/// unlucky pair (one crash plus one shutdown the clean-exit path failed to
/// record) must not disarm a working app's shortcuts.
pub const SAFE_MODE_THRESHOLD: u32 = 3;

/// How long the app has to stay alive before a launch counts as healthy.
pub const HEALTHY_AFTER: std::time::Duration = std::time::Duration::from_secs(30);

/// The long unique log marker, per CLAUDE.md's "use a long `log::` FORMAT
/// STRING as the marker, never a short identifier". `grep` this in `debug.log`
/// to find every safe-mode entry, and in a built exe to prove the feature
/// shipped.
pub const MARKER: &str =
    "safe-mode-entered-after-three-consecutive-startup-crashes-spaceadom";

/// The counterpart marker for the launch that leaves safe mode by the user's
/// own button. Same greppability, opposite event.
pub const MARKER_CLEARED: &str =
    "safe-mode-cleared-by-the-user-turn-back-on-button-spaceadom";

// ---------------------------------------------------------------------------
// The persisted record
// ---------------------------------------------------------------------------

/// `boot-attempts.json`. Deliberately tiny and deliberately its own file rather
/// than a field in `config.json`: it is written on a path where the app may be
/// about to die, and `config.json` is the one file in this project that has
/// already been lost once (PROBLEM 94/159). Nothing that crashes gets to touch
/// it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BootAttempts {
    /// Launches in a row that started and never reached the healthy mark.
    #[serde(default)]
    pub failed_starts: u32,
    /// When this file was last written, seconds since the Unix epoch. Purely
    /// diagnostic — nothing reads it back — but a counter with no timestamp is
    /// unreadable when someone opens the file to ask why they are in safe mode.
    #[serde(default)]
    pub written_unix: u64,
}

// ---------------------------------------------------------------------------
// Pure part — no clock, no disk, no Win32. This is the whole decision.
// ---------------------------------------------------------------------------

/// Does a launch that finds `stored` failed starts behind it run in safe mode?
pub fn decide(stored: u32) -> bool {
    stored >= SAFE_MODE_THRESHOLD
}

/// **REVIEW FIXES 2026-09-05.** The same decision, but told whether the counter
/// file can actually be WRITTEN.
///
/// `trusted == false` means a value read off disk that nothing running on this
/// machine can change: neither the 30-second healthy marker (`write_counter(0)`)
/// nor the user's own "Turn back on" button can lower it. Honouring a `>= 3` in
/// that state would disarm the keyboard hook on every launch for the rest of
/// the install's life, with no way out short of hand-editing a file the app has
/// just demonstrated it cannot open. So an untrusted counter reads as NOT safe
/// mode — the app comes up normally and the escape is logged at WARN.
///
/// The asymmetry is deliberate and matches rule 1 in the module header: safe
/// mode is a recovery, and a recovery that cannot be exited is worse than the
/// fault it recovers from.
pub fn decide_trusted(stored: u32, trusted: bool) -> bool {
    trusted && decide(stored)
}

/// The value a process writes the moment it starts: a launch is a failure until
/// something proves otherwise. `saturating_add` so a corrupt or hand-edited
/// file holding `u32::MAX` cannot wrap the counter back to zero and silently
/// disarm safe mode.
pub fn counter_at_launch(stored: u32) -> u32 {
    stored.saturating_add(1)
}

/// How one launch ended, from this module's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchOutcome {
    /// Alive past [`HEALTHY_AFTER`] with no panic in that window.
    Healthy,
    /// A deliberate, orderly exit BEFORE the healthy mark: a sign-out, a
    /// shutdown, a Restart Manager close, or the tray's Exit. PROBLEM 224's
    /// path. Not a crash, and must never be counted as one.
    CleanExitEarly,
    /// The process died, was killed, or panicked inside the healthy window.
    Lost,
}

/// Fold ONE launch onto the persisted counter.
///
/// Returns `(did that launch run in safe mode, what the counter is afterwards)`.
/// This is the function the tests drive over a sequence of launches, and it is
/// the same three primitives the runtime uses — `decide` at start,
/// `counter_at_launch` at start, and one of the three outcomes at the end.
pub fn run_launch(stored: u32, outcome: LaunchOutcome) -> (bool, u32) {
    let safe = decide(stored);
    let during = counter_at_launch(stored);
    let after = match outcome {
        LaunchOutcome::Healthy => 0,
        // Undo this launch's increment, and only this launch's. NOT "subtract
        // one from whatever is on disk": if the healthy marker already wrote 0
        // the disk value is 0, and decrementing from `during` would put a
        // phantom failure back.
        LaunchOutcome::CleanExitEarly => stored,
        LaunchOutcome::Lost => during,
    };
    (safe, after)
}

// ---------------------------------------------------------------------------
// Runtime state
// ---------------------------------------------------------------------------

/// True for the whole life of a process that started in safe mode, until the
/// user presses "Turn back on".
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// The counter as it was BEFORE this process incremented it. What
/// [`note_clean_exit`] restores.
static COUNTER_BEFORE: AtomicU32 = AtomicU32::new(0);

/// The counter this process wrote at start — reported to the UI so the banner
/// can be argued with.
static FAILED_STARTS: AtomicU32 = AtomicU32::new(0);

/// A panic reached the hook inside [`HEALTHY_AFTER`]. Blocks the healthy
/// marker; see the module header.
static PANICKED_EARLY: AtomicBool = AtomicBool::new(false);

/// **REVIEW FIXES 2026-09-05 (C1).** Has THIS process written its `+1`?
///
/// False for the whole life of a duplicate launch, because the increment now
/// happens in `setup()` and a duplicate exits before `setup()` runs. Read by
/// [`note_clean_exit`], which must not "undo" an increment that was never made
/// — writing `COUNTER_BEFORE` back over a file the surviving instance has
/// already raised would silently erase the real instance's record.
static INCREMENTED: AtomicBool = AtomicBool::new(false);

/// The 30-second mark has passed and the counter was reset.
static HEALTHY: AtomicBool = AtomicBool::new(false);

/// When this process started, for [`note_panic`]'s "within the first 30 s"
/// test. `Instant`, not the wall clock: a clock change mid-launch must not be
/// able to make a panic look late.
static STARTED_AT: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// The hook thread's channel and rollover, parked here when safe mode kept the
/// hook from being spawned, so "Turn back on" can spawn it without a restart.
///
/// `Mutex<Option<…>>` and TAKEN on use: spawning the hook twice would be two
/// `WH_KEYBOARD_LL` hooks in one process fighting over the spacebar, which is
/// PROBLEM 129/141's fault re-created from the inside.
static PENDING_HOOK: std::sync::Mutex<Option<(crossbeam_channel::Sender<crate::hook::HookEvent>, u64)>> =
    std::sync::Mutex::new(None);

/// What the dashboard is told. Serialised straight to the banner.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct SafeModeState {
    /// Is this process running in safe mode right now?
    pub active: bool,
    /// Consecutive failed startups recorded before this launch.
    pub failed_starts: u32,
    /// [`SAFE_MODE_THRESHOLD`], so the banner never has to hardcode "3".
    pub threshold: u32,
}

/// Is the hook off and the overlay absent because of safe mode?
pub fn active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

/// The snapshot behind the `get_safe_mode` command.
pub fn state() -> SafeModeState {
    SafeModeState {
        active: active(),
        failed_starts: FAILED_STARTS.load(Ordering::SeqCst),
        threshold: SAFE_MODE_THRESHOLD,
    }
}

/// One line for `system.txt` in a diagnostics bundle.
pub fn describe() -> String {
    let s = state();
    if s.active {
        format!(
            "SAFE MODE — the keyboard hook was NOT installed and no overlay was created. \
             {} consecutive failed startups were recorded before this launch (threshold {}).",
            s.failed_starts, s.threshold
        )
    } else {
        format!(
            "normal (not safe mode); {} consecutive failed startup(s) recorded before this \
             launch, threshold {}",
            s.failed_starts, s.threshold
        )
    }
}

// ---------------------------------------------------------------------------
// Disk
// ---------------------------------------------------------------------------

/// `%APPDATA%\Spaceadom\boot-attempts.json`.
pub fn boot_file() -> std::path::PathBuf {
    crate::startup::data_dir().join(BOOT_FILE)
}

/// Read the counter. Every failure — missing file, unreadable file, malformed
/// JSON — reads as zero, per rule 1 in the module header. A corrupt file must
/// not be able to put a healthy app into safe mode OR to keep a broken one out
/// of it; zero is the answer that changes nothing.
///
/// **REVIEW FIXES 2026-09-05 — every failure that is not "the file does not
/// exist yet" is logged at WARN.** A first launch has no `boot-attempts.json`
/// and that is the ordinary case, so it stays at DEBUG. Anything else — a
/// permission error, a locked file, a directory where the file should be — is
/// a condition under which this whole mechanism is inert, and a mechanism that
/// goes inert without saying so is indistinguishable from one that decided the
/// app is healthy (CLAUDE.md: "document the CONDITION, not just the failure").
fn read_counter() -> u32 {
    let path = boot_file();
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log::debug!(
                "safe-mode: {} does not exist yet — this is a first launch (or the counter was \
                 cleared by hand); reading it as zero failed startups",
                path.display()
            );
            return 0;
        }
        Err(e) => {
            log::warn!(
                "safe-mode: {} could not be READ ({e}) — reading it as zero failed startups and \
                 carrying on. Note the condition: while this persists the boot counter records \
                 nothing, so safe mode can never arm, however many launches crash.",
                path.display()
            );
            return 0;
        }
    };
    match serde_json::from_str::<BootAttempts>(raw.trim_start_matches('\u{feff}')) {
        Ok(rec) => rec.failed_starts,
        Err(e) => {
            log::warn!(
                "safe-mode: {} could not be parsed ({e}) — reading it as zero failed startups \
                 and carrying on. A damaged counter must never be the reason an app does or \
                 does not start normally.",
                path.display()
            );
            0
        }
    }
}

/// Write the counter. Best-effort; returns whether the bytes actually reached
/// the disk.
///
/// **REVIEW FIXES 2026-09-05 — failures are WARN, not DEBUG, and the return
/// value is now read.** A counter that cannot be written is the condition
/// behind [`begin`]'s escape hatch: the healthy marker's `write_counter(0)` is
/// the ONLY thing that ever lowers this file, so a counter stuck at or above
/// [`SAFE_MODE_THRESHOLD`] in an unwritable file would disarm the keyboard hook
/// on **every launch, forever**, with the one button that clears it
/// (`turn_back_on`) equally unable to write. That is a worse failure than the
/// crash loop safe mode exists for.
fn write_counter(value: u32) -> bool {
    let path = boot_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let rec = BootAttempts {
        failed_starts: value,
        written_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };
    match serde_json::to_string_pretty(&rec) {
        Ok(json) => match std::fs::write(&path, json) {
            Ok(()) => true,
            Err(e) => {
                log::warn!(
                    "safe-mode: could not WRITE {} ({e}) — the counter is unchanged on disk. \
                     Note the condition: nothing this launch does can raise or lower it.",
                    path.display()
                );
                false
            }
        },
        Err(e) => {
            log::warn!("safe-mode: could not serialise the boot counter ({e})");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// The three transitions
// ---------------------------------------------------------------------------

/// Read the counter and decide. **Call once, from `run()`, after the logger is
/// initialised and before anything that could crash.**
///
/// Returns whether this launch is a safe-mode launch.
///
/// **REVIEW FIXES 2026-09-05 (C1) — THIS FUNCTION NO LONGER WRITES ANYTHING ON
/// THE ORDINARY PATH.** It used to increment the counter here, which made every
/// duplicate launch a recorded startup crash:
///
/// `tauri-plugin-single-instance`'s plugin `setup` runs during
/// `Builder::build()` — before the app's own `.setup()` closure (tauri 2.11
/// `app.rs`: `initialize_plugins` at :2440, `(setup)(app)` at :2531) — and a
/// second instance exits from inside it with `app.cleanup_before_exit()` +
/// `std::process::exit(0)`. No `RunEvent` is ever produced, so
/// [`note_clean_exit`] never runs, the healthy timer never starts, and the
/// increment this function had already written simply stayed. **Three
/// double-clicks on the tray icon of a perfectly healthy app put the fourth
/// launch into safe mode.**
///
/// So the write moved to [`note_surviving_instance`], called from `setup()` —
/// the first point at which this process is known to have won the
/// single-instance mutex. **Only the surviving instance writes.** The read-only
/// decision stays here because `setup()` needs the answer before it creates the
/// hook thread and the overlay window.
///
/// The window this trades away is small and named: a crash between the plugin
/// initialisation and the first line of `setup()` is no longer counted. Nothing
/// in that gap belongs to this app — it is plugin construction — and the
/// failures safe mode exists for (the hook, the overlay, the engine) all happen
/// inside `setup()`, after the increment.
pub fn begin() -> bool {
    let _ = STARTED_AT.set(std::time::Instant::now());
    let stored = read_counter();

    // The escape hatch (REVIEW FIXES 2026-09-05, MEDIUM). Only probed on the
    // branch that would actually disarm the hook, so the ordinary launch still
    // touches the disk exactly once — in `note_surviving_instance`.
    let mut safe = decide(stored);
    if safe {
        // Rewriting the value that is already there is the honest test of "can
        // anything on this machine ever lower this number again?", and it is
        // idempotent: only `written_unix` changes.
        let writable = write_counter(stored);
        safe = decide_trusted(stored, writable);
        if !safe {
            log::warn!(
                "safe-mode: {stored} failed startups are recorded (threshold \
                 {SAFE_MODE_THRESHOLD}), which would normally mean SAFE MODE — but {} could not \
                 be written, so nothing could ever clear it again and the keyboard hook would \
                 be off on every launch for good. Starting NORMALLY instead and logging the \
                 escape. Fix the file's permissions (or delete it) to let safe mode work at \
                 all on this machine.",
                boot_file().display()
            );
        }
    }

    FAILED_STARTS.store(stored, Ordering::SeqCst);
    COUNTER_BEFORE.store(stored, Ordering::SeqCst);
    ACTIVE.store(safe, Ordering::SeqCst);

    if safe {
        log::warn!(
            "{MARKER}: starting in SAFE MODE — {stored} consecutive launches (threshold \
             {SAFE_MODE_THRESHOLD}) started and never stayed alive {}s. This launch installs NO \
             keyboard hook and creates NO overlay window; the dashboard opens with a banner \
             offering 'Turn back on' and 'Report a problem'. To clear it by hand, delete {} or \
             set its failed_starts to 0.",
            HEALTHY_AFTER.as_secs(),
            boot_file().display()
        );
    } else {
        log::info!(
            "safe-mode: normal start — {stored} consecutive failed startup(s) recorded, \
             threshold {SAFE_MODE_THRESHOLD}. The counter becomes {} once this process is known \
             to be the surviving instance (setup), and returns to 0 once it has been alive {}s.",
            counter_at_launch(stored),
            HEALTHY_AFTER.as_secs()
        );
    }
    safe
}

/// **REVIEW FIXES 2026-09-05 (C1) — the increment. Call ONCE, from the FIRST
/// line of `setup()`.**
///
/// `setup()` is the earliest point at which this process is known to be the
/// SURVIVING instance: `tauri-plugin-single-instance` runs in plugin
/// initialisation, which is strictly earlier, and a duplicate launch leaves
/// from inside it via `process::exit(0)`. Anything that increments the boot
/// counter before that point is counting the user's double-clicks as crashes.
///
/// Idempotent, and the flag it sets is what stops [`note_clean_exit`] from
/// "undoing" an increment that never happened.
pub fn note_surviving_instance() {
    if INCREMENTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let before = COUNTER_BEFORE.load(Ordering::SeqCst);
    let next = counter_at_launch(before);
    if write_counter(next) {
        log::info!(
            "safe-mode: this process is the surviving single instance — boot counter written as \
             {next} (was {before}). A duplicate launch exits inside the single-instance plugin, \
             before this line, and therefore never counts as a startup crash. The counter \
             returns to 0 once this launch has been alive {}s.",
            HEALTHY_AFTER.as_secs()
        );
    }
}

/// Start the named thread that marks this launch healthy after
/// [`HEALTHY_AFTER`]. Call once, from `setup()`.
///
/// A thread rather than a Tauri timer on purpose: this has to keep counting
/// even if the event loop is the thing that is wedged, which is one of the
/// failures safe mode exists to recover from.
pub fn spawn_healthy_timer() {
    if std::thread::Builder::new()
        .name("st-safe-mode".into())
        .spawn(|| {
            std::thread::sleep(HEALTHY_AFTER);
            mark_healthy();
        })
        .is_err()
    {
        log::warn!(
            "safe-mode: could not spawn the health timer — this launch's boot counter will \
             never be reset, so a few more launches like it would enter safe mode. Not fatal, \
             and 'Turn back on' clears the counter."
        );
    }
}

/// The launch is healthy: reset the counter. Idempotent.
pub fn mark_healthy() {
    if PANICKED_EARLY.load(Ordering::SeqCst) {
        log::warn!(
            "safe-mode: {}s reached, but a panic was recorded inside that window — the boot \
             counter is deliberately NOT reset. A panic on the hook or engine thread is \
             survivable (PROBLEM 82 restarts it), so 'still running' is not the same as \
             'started correctly', and treating it as healthy is how a machine that panics on \
             every launch would never reach safe mode.",
            HEALTHY_AFTER.as_secs()
        );
        return;
    }
    if HEALTHY.swap(true, Ordering::SeqCst) {
        return;
    }
    write_counter(0);
    log::info!(
        "safe-mode: alive {}s — boot counter reset to 0",
        HEALTHY_AFTER.as_secs()
    );
}

/// A panic reached the app's ONE panic hook. Called from that hook in `lib.rs`.
///
/// **Must not panic and must be cheap** — it runs inside the panic hook, and a
/// panic in a panic hook aborts the process with no log line at all. One
/// `Instant::elapsed`, one atomic, and at most one small file write.
pub fn note_panic() {
    let early = STARTED_AT
        .get()
        .map(|t| t.elapsed() < HEALTHY_AFTER)
        .unwrap_or(true);
    if !early {
        return;
    }
    if PANICKED_EARLY.swap(true, Ordering::SeqCst) {
        return;
    }
    // If the healthy marker has already run (a panic at exactly the boundary),
    // the counter on disk is 0 and this launch has to be put back on the board
    // by hand — otherwise a panic that lands one millisecond late is free.
    if HEALTHY.load(Ordering::SeqCst) {
        write_counter(counter_at_launch(COUNTER_BEFORE.load(Ordering::SeqCst)));
    }
    log::warn!(
        "safe-mode: a panic arrived inside the first {}s — this launch is recorded as a failed \
         startup and the health timer will not clear it.",
        HEALTHY_AFTER.as_secs()
    );
}

/// A DELIBERATE exit. Undo this launch's increment.
///
/// Called from `session_end::teardown` (PROBLEM 224's `WM_ENDSESSION` path: a
/// sign-out, a shutdown, or an installer closing us to replace the exe) and
/// from the tray's Exit. See the module header for why this is not optional.
///
/// Runs inside a `WM_ENDSESSION` handler with a `HungAppTimeout` budget, so it
/// does one read of two atomics and at most one small write.
pub fn note_clean_exit() {
    // REVIEW FIXES 2026-09-05 (C1) — nothing to undo if this process never
    // incremented. A duplicate launch exits before `setup()`, so it must not
    // write anything at all: the file it would write to belongs to the
    // instance that is still running, and `COUNTER_BEFORE` is a snapshot from
    // before that instance raised it.
    if !INCREMENTED.load(Ordering::SeqCst) {
        return;
    }
    if HEALTHY.load(Ordering::SeqCst) {
        return; // the counter is already 0 on disk; nothing to undo
    }
    if PANICKED_EARLY.load(Ordering::SeqCst) {
        // It really did fault, and then the session ended. Let the failure
        // stand: an app that panics at startup and is then shut down has still
        // failed to start.
        return;
    }
    let before = COUNTER_BEFORE.load(Ordering::SeqCst);
    write_counter(before);
    log::info!(
        "safe-mode: clean exit before the {}s health mark — this launch's boot counter \
         increment has been undone (back to {before}). A sign-out, a shutdown or an installer \
         closing the app is not a startup crash (PROBLEM 224).",
        HEALTHY_AFTER.as_secs()
    );
}

// ---------------------------------------------------------------------------
// Leaving safe mode
// ---------------------------------------------------------------------------

/// Park the hook's channel so [`turn_back_on`] can spawn the hook thread later.
/// Called from `setup()` on the safe-mode branch INSTEAD of
/// `hook::spawn_hook_thread`.
pub fn arm_pending_hook(tx: crossbeam_channel::Sender<crate::hook::HookEvent>, rollover_ms: u64) {
    let mut slot = PENDING_HOOK.lock().unwrap_or_else(|p| p.into_inner());
    *slot = Some((tx, rollover_ms));
}

/// The "Turn back on" button. Installs the hook now and clears the counter.
///
/// Returns `Err` only when there is nothing to install — which means either
/// this process was not in safe mode, or the button was pressed twice. Both are
/// reported rather than silently succeeding: a button that claims to have armed
/// a keyboard hook it did not arm is the worst possible outcome here.
pub fn turn_back_on() -> Result<(), String> {
    let taken = {
        let mut slot = PENDING_HOOK.lock().unwrap_or_else(|p| p.into_inner());
        slot.take()
    };
    let Some((tx, rollover_ms)) = taken else {
        // Clear the counter anyway: a user pressing this has told us the app is
        // fine, and leaving a counter at 3 would send the NEXT launch back into
        // safe mode for a fault that is over.
        write_counter(0);
        ACTIVE.store(false, Ordering::SeqCst);
        return Err(
            "The shortcuts are already on — nothing was waiting to be started. The startup \
             counter has been cleared, so the next launch will be a normal one."
                .into(),
        );
    };

    crate::hook::spawn_hook_thread(tx, rollover_ms);
    ACTIVE.store(false, Ordering::SeqCst);
    write_counter(0);
    log::warn!(
        "{MARKER_CLEARED}: the user pressed 'Turn back on' — the keyboard hook thread has been \
         spawned and the boot counter cleared to 0. The overlay window is still absent for this \
         launch (it is created at startup only), so the Guide HUD and toasts return at the next \
         restart; Space+key launching, focusing and minimising work now."
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests — the pure decision only. Everything above the "Runtime state" line
// takes numbers and returns numbers precisely so that this is possible without
// a filesystem, a clock, or a crash.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive a sequence of launches through `run_launch` and report, for each
    /// one, whether it ran in safe mode. This is the whole feature in one
    /// helper: the tests below are sequences and their expected verdicts.
    fn simulate(outcomes: &[LaunchOutcome]) -> Vec<bool> {
        let mut counter = 0u32;
        let mut verdicts = Vec::new();
        for &o in outcomes {
            let (safe, next) = run_launch(counter, o);
            verdicts.push(safe);
            counter = next;
        }
        verdicts
    }

    #[test]
    fn three_lost_launches_in_a_row_arm_safe_mode_on_the_fourth() {
        use LaunchOutcome::Lost;
        let v = simulate(&[Lost, Lost, Lost, Lost]);
        assert_eq!(
            v,
            vec![false, false, false, true],
            "the FOURTH launch is the safe one — three crashes have to have happened first"
        );
    }

    #[test]
    fn two_crashes_then_a_healthy_run_resets_the_counter() {
        use LaunchOutcome::{Healthy, Lost};
        // The near miss, and the case a user actually lives through: something
        // was wrong twice, then it was not. Nothing after that may enter safe
        // mode until three fresh failures.
        let v = simulate(&[Lost, Lost, Healthy, Lost, Lost, Lost, Lost]);
        assert_eq!(v, vec![false, false, false, false, false, false, true]);
    }

    #[test]
    fn a_healthy_run_after_three_failures_clears_it_for_good() {
        use LaunchOutcome::{Healthy, Lost};
        let (safe, after) = run_launch(3, Healthy);
        assert!(safe, "a launch that finds 3 failures behind it runs in safe mode");
        assert_eq!(after, 0, "and a healthy safe-mode launch clears the counter");
        let (safe_next, _) = run_launch(after, Lost);
        assert!(!safe_next, "the launch after that is a normal one");
    }

    #[test]
    fn a_clean_shutdown_never_counts_as_a_startup_crash() {
        use LaunchOutcome::CleanExitEarly;
        // PROBLEM 224's path: WM_ENDSESSION inside the first 30 seconds. A user
        // who reboots three times shortly after logon — or whose app is closed
        // three times by an installer's Restart Manager — must never be told
        // his app crashed three times.
        let v = simulate(&[CleanExitEarly; 8]);
        assert!(
            v.iter().all(|&safe| !safe),
            "eight clean early exits in a row must leave safe mode disarmed: {v:?}"
        );
    }

    #[test]
    fn clean_exits_do_not_dilute_real_crashes_either_way() {
        use LaunchOutcome::{CleanExitEarly, Lost};
        // Two crashes, a shutdown, a third crash. The shutdown must neither
        // count towards the three nor reset them.
        let v = simulate(&[Lost, Lost, CleanExitEarly, Lost, Lost]);
        assert_eq!(
            v,
            vec![false, false, false, false, true],
            "the shutdown is a no-op: the third real crash is what arms it"
        );
    }

    #[test]
    fn a_clean_exit_undoes_only_its_own_launch() {
        use LaunchOutcome::CleanExitEarly;
        // The trap this is written against: "decrement whatever is on disk".
        // The counter written during the launch is 3; the value that must
        // survive is the 2 that was there BEFORE it.
        let (_, after) = run_launch(2, CleanExitEarly);
        assert_eq!(after, 2);
    }

    #[test]
    fn the_counter_cannot_wrap_on_a_corrupt_file() {
        // A hand-edited or damaged boot-attempts.json holding u32::MAX must not
        // wrap to 0 and silently disarm safe mode.
        assert_eq!(counter_at_launch(u32::MAX), u32::MAX);
        assert!(decide(u32::MAX));
    }

    /// REVIEW FIXES 2026-09-05 (C1). The duplicate-launch bug, expressed over
    /// the pure seam: a launch that never reaches `setup()` must not touch the
    /// counter at all, so the value on disk is exactly what it was.
    #[test]
    fn a_duplicate_launch_that_never_reaches_setup_leaves_the_counter_alone() {
        // The surviving instance is healthy; three duplicates arrive while it
        // runs. Each of them exits inside the single-instance plugin, i.e.
        // before `note_surviving_instance`, so none of them folds a launch onto
        // the counter — modelled here as simply not calling `run_launch`.
        let mut counter = 0u32;
        let (safe, after) = run_launch(counter, LaunchOutcome::Healthy);
        assert!(!safe);
        counter = after;
        // Three double-clicks arrive here. Each one exits inside the
        // single-instance plugin, so there is no `run_launch` to fold: the
        // absence of a call IS the fix.
        assert_eq!(counter, 0, "duplicate launches must leave the counter at 0");
        let (safe_next, _) = run_launch(counter, LaunchOutcome::Healthy);
        assert!(
            !safe_next,
            "the next real launch must be a NORMAL one — three icon double-clicks are not \
             three startup crashes"
        );
    }

    /// REVIEW FIXES 2026-09-05 (C1). The regression this replaces: with the
    /// increment at process start, each duplicate was a `Lost` launch.
    #[test]
    fn the_old_behaviour_is_what_armed_safe_mode_on_three_double_clicks() {
        use LaunchOutcome::Lost;
        // Documented, not shipped: three duplicates counted as `Lost` DO arm
        // safe mode on the fourth launch. This asserts the mechanism the C1 fix
        // disconnects, so a future refactor that moves the increment back to
        // `run()` fails a test instead of a user.
        let v = simulate(&[Lost, Lost, Lost, Lost]);
        assert_eq!(v, vec![false, false, false, true]);
    }

    /// REVIEW FIXES 2026-09-05 (H5). A successful self-update exits the
    /// process deliberately, inside the 30-second health window (the first
    /// update check fires at 15 s). It is `CleanExitEarly`, never `Lost`.
    #[test]
    fn a_self_update_that_exits_the_process_is_never_a_startup_crash() {
        use LaunchOutcome::{CleanExitEarly, Healthy};
        // Three days, three updates, each installed 15-20 seconds after launch
        // and each exiting the process to let the installer run. The fourth
        // launch must be a NORMAL one.
        let v = simulate(&[CleanExitEarly, CleanExitEarly, CleanExitEarly, Healthy]);
        assert_eq!(
            v,
            vec![false, false, false, false],
            "three self-updates in a row must not disarm the keyboard hook: {v:?}"
        );
        // And the counter genuinely returns to where it started, rather than
        // creeping: an update from a machine that already had 2 real failures
        // behind it leaves those 2 intact and adds nothing.
        let (safe, after) = run_launch(2, CleanExitEarly);
        assert!(!safe);
        assert_eq!(after, 2);
    }

    /// REVIEW FIXES 2026-09-05 (MEDIUM). An unwritable counter file must not
    /// pin safe mode on for the life of the install.
    #[test]
    fn an_unwritable_counter_can_never_pin_safe_mode_on() {
        // Trusted: the ordinary decision, unchanged.
        assert!(decide_trusted(SAFE_MODE_THRESHOLD, true));
        assert!(!decide_trusted(SAFE_MODE_THRESHOLD - 1, true));
        // Untrusted: the value is real but nothing can ever lower it — neither
        // the 30s healthy marker nor the user's "Turn back on" button. Safe
        // mode would be permanent, so it is refused and the escape is logged.
        assert!(!decide_trusted(SAFE_MODE_THRESHOLD, false));
        assert!(!decide_trusted(u32::MAX, false));
        // And an untrusted counter below the threshold changes nothing either.
        assert!(!decide_trusted(0, false));
    }

    #[test]
    fn decide_is_exactly_the_threshold() {
        assert!(!decide(0));
        assert!(!decide(SAFE_MODE_THRESHOLD - 1));
        assert!(decide(SAFE_MODE_THRESHOLD));
        assert!(decide(SAFE_MODE_THRESHOLD + 1));
    }

    #[test]
    fn the_record_round_trips_and_an_absent_field_reads_as_zero() {
        // `#[serde(default)]` on both fields is load-bearing for the same
        // reason it is in config/schema.rs (PROBLEM 159): a future field added
        // to this struct must not make every existing boot-attempts.json
        // unreadable — and an unreadable one is one that cannot record a crash.
        let rec = BootAttempts { failed_starts: 2, written_unix: 1_757_000_000 };
        let json = serde_json::to_string(&rec).expect("serialise");
        let back: BootAttempts = serde_json::from_str(&json).expect("round trip");
        assert_eq!(back, rec);

        let empty: BootAttempts = serde_json::from_str("{}").expect("an empty object is valid");
        assert_eq!(empty.failed_starts, 0);
    }
}
