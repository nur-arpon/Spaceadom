//! What the app was DOING when it died (PROBLEM 131).
//!
//! The 14 crashes found on 2026-08-17 all reported the same tao line and a
//! backtrace of `<unknown>` frames. Symbols fix the second half of that; this
//! module fixes the first. A backtrace says which code was on the stack. It
//! does NOT say that the overlay had been rebuilt twice in the last minute, or
//! that a display was unplugged four seconds earlier — and for a crash that
//! only happens on someone else's machine, that history is usually the part
//! that identifies the trigger.
//!
//! DESIGN RULES, both learned from bugs in this project:
//!
//! 1. **Nothing here may block, and nothing here may panic.** It is read from
//!    inside a panic hook. If the thread that panicked was holding one of
//!    these locks, `lock()` would deadlock and the app would hang instead of
//!    dying — strictly worse, because a hang leaves no log line at all. Every
//!    read is `try_lock`, and a lock that is busy reports itself as busy.
//!
//! 2. **This is never called from the keyboard hook callback.** That callback
//!    may not touch the heap (see the hook iron laws), and every writer here
//!    allocates a String.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Last size/position operation performed on the overlay window, and when.
/// 6 of the 14 recorded crashes had an `overlay_fit` line immediately before
/// them, which is the single strongest lead we have.
static LAST_OVERLAY_OP: Mutex<String> = Mutex::new(String::new());
static LAST_OVERLAY_AT: AtomicU64 = AtomicU64::new(0);

/// Last thing the engine was asked to do — the user-visible action.
static LAST_ACTION: Mutex<String> = Mutex::new(String::new());
static LAST_ACTION_AT: AtomicU64 = AtomicU64::new(0);

/// Display topology changes and overlay rebuilds. The leading hypothesis is a
/// window message arriving after its host window was destroyed, so "how many
/// times has the overlay been destroyed and rebuilt this session" is a direct
/// test of it.
static LAST_DISPLAY_EVENT: Mutex<String> = Mutex::new(String::new());
static LAST_DISPLAY_AT: AtomicU64 = AtomicU64::new(0);
static OVERLAY_REBUILDS: AtomicU32 = AtomicU32::new(0);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Store `what` in `slot`, best-effort. A busy or poisoned lock is skipped
/// rather than waited on: losing one breadcrumb is nothing, stalling a UI
/// thread to record one is a real bug.
fn note(slot: &Mutex<String>, stamp: &AtomicU64, what: String) {
    if let Ok(mut g) = slot.try_lock() {
        *g = what;
        stamp.store(now_ms(), Ordering::Relaxed);
    }
}

pub fn note_overlay_op(what: impl Into<String>) {
    note(&LAST_OVERLAY_OP, &LAST_OVERLAY_AT, what.into());
}

pub fn note_action(what: impl Into<String>) {
    note(&LAST_ACTION, &LAST_ACTION_AT, what.into());
}

pub fn note_display_event(what: impl Into<String>) {
    note(&LAST_DISPLAY_EVENT, &LAST_DISPLAY_AT, what.into());
}

pub fn note_overlay_rebuild() {
    OVERLAY_REBUILDS.fetch_add(1, Ordering::Relaxed);
}

/// One line per breadcrumb, with an age. Ages matter more than timestamps
/// here: "overlay resized 40ms ago" and "overlay resized 40 minutes ago" are
/// completely different stories about the same crash.
fn line(name: &str, slot: &Mutex<String>, stamp: &AtomicU64, now: u64) -> String {
    match slot.try_lock() {
        Ok(g) if g.is_empty() => format!("    {name}: <none this session>"),
        Ok(g) => {
            let at = stamp.load(Ordering::Relaxed);
            let age = if at == 0 || now < at { "?".to_string() } else { format!("{}ms ago", now - at) };
            format!("    {name}: {} ({age})", *g)
        }
        // Not a failure to report — it is itself a clue. A busy lock means the
        // panicking thread was most likely inside that very code path.
        Err(_) => format!("    {name}: <lock busy — the panic may be INSIDE this path>"),
    }
}

/// Rendered into the panic log immediately after the message and before the
/// backtrace.
pub fn snapshot() -> String {
    let now = now_ms();
    format!(
        "app context at panic:\n{}\n{}\n{}\n    overlay rebuilds this session: {}",
        line("last overlay op ", &LAST_OVERLAY_OP, &LAST_OVERLAY_AT, now),
        line("last action     ", &LAST_ACTION, &LAST_ACTION_AT, now),
        line("last display evt", &LAST_DISPLAY_EVENT, &LAST_DISPLAY_AT, now),
        OVERLAY_REBUILDS.load(Ordering::Relaxed),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ONE test on purpose. Everything in this module is process-global state,
    /// and cargo runs tests in one process on parallel threads — splitting this
    /// into three readable little tests re-creates PROBLEM 130 exactly, where
    /// four tests sharing one static passed alone and failed in the suite. If
    /// you add a case, add it HERE, in sequence.
    #[test]
    fn snapshot_is_readable_and_never_blocks() {
        // 1. an untouched slot must announce itself, not render as blank data
        let empty = snapshot();
        assert!(
            empty.contains("last display evt: <none this session>"),
            "an empty slot must say so: {empty}"
        );

        // 2. a recorded breadcrumb comes back, with an age
        note_overlay_op("overlay_fit 520x282");
        let s = snapshot();
        assert!(s.contains("overlay_fit 520x282"), "{s}");
        assert!(s.contains("ms ago)"), "a breadcrumb needs its age: {s}");

        // 3. the property that actually matters: while a slot is HELD — which
        //    is the likely state if the panic happened inside that path — the
        //    snapshot must still return, and must say the lock was busy rather
        //    than waiting on it. A deadlock here would replace a logged crash
        //    with a silent hang, which is strictly worse.
        let held = LAST_ACTION.lock().unwrap();
        let busy = snapshot();
        drop(held);
        assert!(
            busy.contains("lock busy"),
            "a held lock must be reported, not waited on: {busy}"
        );

        // 4. the rebuild counter accumulates
        let before = OVERLAY_REBUILDS.load(Ordering::Relaxed);
        note_overlay_rebuild();
        note_overlay_rebuild();
        assert_eq!(OVERLAY_REBUILDS.load(Ordering::Relaxed), before + 2);
    }
}
