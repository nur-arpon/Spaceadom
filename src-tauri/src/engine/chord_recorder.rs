//! PHASE A — the chord recorder behind the key editor's "Press the keys…".
//!
//! The hook forwards every keystroke as `HookEvent::RawKey(vk, down)` while
//! `hook::keys::RECORDING` is on (and passes it through untouched); the
//! engine thread calls `note` for each; the page polls `snapshot` every
//! 100 ms through the `chord_record_poll` command. The chord is WHAT WAS
//! HELD WHEN THE LAST KEY WENT DOWN, in press order — so Win, Shift, S gives
//! `[Win, Shift, S]` however the keys come back up, and releasing everything
//! does not erase it. `start` arms the hook for `RECORD_MAX_MS`; the hook
//! disarms itself past that whether or not `stop` is ever called.
//!
//! State is a pair of mutexes, not atomics: `note` runs on the engine thread
//! and `snapshot` on a command thread, and the vectors are tiny.

use std::sync::atomic::Ordering;
use std::sync::Mutex;

/// Keys currently down, in press order.
static HELD: Mutex<Vec<u16>> = Mutex::new(Vec::new());
/// The held set as it was when the LAST key went down — the answer.
static LAST_CHORD: Mutex<Vec<u16>> = Mutex::new(Vec::new());

/// Arm the hook and clear the last answer.
pub fn start() {
    HELD.lock().unwrap_or_else(|p| p.into_inner()).clear();
    LAST_CHORD.lock().unwrap_or_else(|p| p.into_inner()).clear();
    let deadline = crate::hook::tick_count_pub() + crate::hook::keys::RECORD_MAX_MS;
    crate::hook::keys::RECORD_DEADLINE.store(deadline, Ordering::Relaxed);
    crate::hook::keys::RECORDING.store(true, Ordering::Relaxed);
    log::info!("chord recorder: recording for up to {} s (Phase A)", crate::hook::keys::RECORD_MAX_MS / 1000);
}

/// Disarm the hook. The last answer stays readable.
pub fn stop() {
    crate::hook::keys::RECORDING.store(false, Ordering::Relaxed);
    HELD.lock().unwrap_or_else(|p| p.into_inner()).clear();
    log::info!("chord recorder: stopped");
}

/// One keystroke from the hook. Engine thread.
pub fn note(vk: u16, down: bool) {
    let mut held = HELD.lock().unwrap_or_else(|p| p.into_inner());
    apply(&mut held, vk, down);
    if down {
        *LAST_CHORD.lock().unwrap_or_else(|p| p.into_inner()) = held.clone();
    }
}

/// The pure step: a down appends the key once (auto-repeat is one press), an
/// up removes it.
pub fn apply(held: &mut Vec<u16>, vk: u16, down: bool) {
    if down {
        if !held.contains(&vk) {
            held.push(vk);
        }
    } else {
        held.retain(|k| *k != vk);
    }
}

/// The chord as of the last key-down, in press order.
pub fn snapshot() -> Vec<u16> {
    LAST_CHORD.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Win, Shift, S pressed and released in any order → `[Win, Shift, S]`,
    /// and auto-repeat of S does not duplicate it.
    struct Rec {
        held: Vec<u16>,
        last: Vec<u16>,
    }
    impl Rec {
        fn step(&mut self, vk: u16, down: bool) {
            apply(&mut self.held, vk, down);
            if down {
                self.last = self.held.clone();
            }
        }
    }

    #[test]
    fn the_chord_is_the_held_set_at_the_last_key_down() {
        let mut r = Rec { held: Vec::new(), last: Vec::new() };
        r.step(0x5B, true);
        r.step(0x10, true);
        r.step(0x53, true);
        r.step(0x53, true); // auto-repeat
        assert_eq!(r.last, vec![0x5B, 0x10, 0x53]);
        r.step(0x5B, false); // Win released first
        r.step(0x53, false);
        r.step(0x10, false);
        assert_eq!(r.last, vec![0x5B, 0x10, 0x53], "releases never change the answer");
        assert!(r.held.is_empty());
        // A new press starts a new chord.
        r.step(0x11, true);
        assert_eq!(r.last, vec![0x11]);
    }
}
