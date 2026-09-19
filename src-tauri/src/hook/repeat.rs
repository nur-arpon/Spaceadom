//! PHASE A step 3 (2026-09-19, §6 of brief 3) — AUTO-REPEAT DETECTION for
//! the keyboard hook, so a held Space+key fires ONCE for everything but a
//! chord.
//!
//! Holding Space+U at 10:47:33 on 2026-09-19 fired `Vol+` thirty times in
//! 200 ms. Fine for a volume chord; wrong for a toggle, a URI, a command or
//! a special (Screen off woke the panel and turned it off again six times
//! in 37 s, which is how the owner came to restart his laptop).
//! `WH_KEYBOARD_LL` has no repeat flag (`KBDLLHOOKSTRUCT.flags` carries
//! extended / injected / alt-down / up — never "previous key state", which
//! only `WM_KEYDOWN`'s lParam bit 30 has, and the LL hook does not see that
//! lParam). So the hook tracks it itself: a 256-bit "down already seen since
//! the last up" bitmap, four `AtomicU64`s, set on the first down, cleared on
//! up. A down whose bit is already set is a repeat.
//!
//! Callback-safe by construction: four relaxed atomics, no heap, no lock, no
//! syscall (`mark_down` is one `fetch_or`, `mark_up` one `fetch_and`). The
//! bitmap is reset by `install_hooks` — an evicted hook (law 7) may have
//! missed a key-up, and a stale bit would drop the next real press of that
//! key as a repeat until its up cleared it.

use std::sync::atomic::{AtomicU64, Ordering};

/// A 256-bit set of virtual keys, indexed by VK (0..=255).
pub struct KeyBitmap([AtomicU64; 4]);

impl KeyBitmap {
    pub const fn new() -> Self {
        Self([AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)])
    }

    #[inline]
    fn slot(vk: u16) -> (usize, u64) {
        let vk = (vk & 0xFF) as usize;
        (vk / 64, 1u64 << (vk % 64))
    }

    /// Note a key-down. Returns `true` when the key was ALREADY down — this
    /// event is an auto-repeat.
    #[inline]
    pub fn mark_down(&self, vk: u16) -> bool {
        let (w, bit) = Self::slot(vk);
        self.0[w].fetch_or(bit, Ordering::Relaxed) & bit != 0
    }

    /// Note a key-up: the next down of this key is a first press again.
    #[inline]
    pub fn mark_up(&self, vk: u16) {
        let (w, bit) = Self::slot(vk);
        self.0[w].fetch_and(!bit, Ordering::Relaxed);
    }

    /// Is the key currently recorded as down?
    #[inline]
    pub fn is_down(&self, vk: u16) -> bool {
        let (w, bit) = Self::slot(vk);
        self.0[w].load(Ordering::Relaxed) & bit != 0
    }

    /// Forget everything (hook (re)install).
    pub fn reset(&self) {
        for w in &self.0 {
            w.store(0, Ordering::Relaxed);
        }
    }

    /// How many keys are recorded down (diagnostics).
    pub fn count(&self) -> u32 {
        self.0.iter().map(|w| w.load(Ordering::Relaxed).count_ones()).sum()
    }
}

impl Default for KeyBitmap {
    fn default() -> Self {
        Self::new()
    }
}

/// THE bitmap the keyboard callback maintains. Written only from
/// `kb_hook_proc` (downs and ups the hook sees, our own injected keys
/// excluded by the cookie test above it) and `install_hooks` (reset).
pub static KEYS_DOWN: KeyBitmap = KeyBitmap::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_down_is_a_press_and_every_further_down_is_a_repeat_until_up() {
        let m = KeyBitmap::new();
        assert!(!m.mark_down(0x55), "U: first down");
        assert!(m.is_down(0x55));
        assert!(m.mark_down(0x55), "U: second down = repeat");
        assert!(m.mark_down(0x55), "U: third down = repeat");
        m.mark_up(0x55);
        assert!(!m.is_down(0x55));
        assert!(!m.mark_down(0x55), "after an up it is a press again");
        m.mark_up(0x55);
    }

    /// Keys in every 64-bit word are independent — a held Space (0x20, word
    /// 0) never makes U (0x55, word 1) or VK_OEM_7 (0xDE, word 3) read as a
    /// repeat, and clearing one leaves the others alone.
    #[test]
    fn keys_do_not_interfere_across_or_within_words() {
        let m = KeyBitmap::new();
        for vk in [0x20u16, 0x55, 0x1B, 0xDE, 0xAF, 0x00, 0xFF] {
            assert!(!m.mark_down(vk), "{vk:#04X} first");
        }
        assert_eq!(m.count(), 7);
        for vk in [0x20u16, 0x55, 0x1B, 0xDE, 0xAF, 0x00, 0xFF] {
            assert!(m.mark_down(vk), "{vk:#04X} repeat");
        }
        m.mark_up(0x55);
        assert!(!m.is_down(0x55));
        assert!(m.is_down(0x20) && m.is_down(0xDE) && m.is_down(0xFF) && m.is_down(0x00));
        assert_eq!(m.count(), 6);
        m.reset();
        assert_eq!(m.count(), 0);
        assert!(!m.mark_down(0x20), "after reset everything is a first press");
    }

    /// An up for a key never seen down is harmless (the hook can be
    /// installed while a key is held).
    #[test]
    fn an_orphan_up_is_a_no_op() {
        let m = KeyBitmap::new();
        m.mark_up(0x41);
        assert_eq!(m.count(), 0);
        assert!(!m.mark_down(0x41));
    }

    /// VKs above 255 cannot occur (`KBDLLHOOKSTRUCT.vkCode` is 1..=254) but
    /// the slot arithmetic must not index out of bounds if one ever did.
    #[test]
    fn a_vk_over_255_masks_rather_than_panics() {
        let m = KeyBitmap::new();
        assert!(!m.mark_down(0x1FF));
        assert!(m.is_down(0xFF), "0x1FF masks to 0xFF");
    }
}
