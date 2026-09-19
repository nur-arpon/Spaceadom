//! TOUCHPAD T2 — the one atomic the LL mouse hook reads.
//!
//! While a band is live the app's existing `WH_MOUSE_LL` callback eats
//! `WM_MOUSEMOVE`, `WM_MOUSEWHEEL` and `WM_MOUSEHWHEEL` so the pointer holds
//! still and the page underneath does not scroll (the T1b hardware result:
//! one finger, eat the moves, cursor moved 0 px). Keyboard-hook laws: the
//! callback may touch atomics and nothing else, so this is the ENTIRE
//! interface between the touchpad engine and the hook — one `AtomicBool`,
//! written only by `touchpad::mod` on gesture enter/exit, read only by
//! `ms_hook_proc`. No second mouse hook (there is exactly one `WH_MOUSE_LL`
//! in this process); the callback gets one extra atomic test at its top.
#![cfg(windows)]

use std::sync::atomic::AtomicBool;

/// True while a single finger that landed inside an enabled edge band is down.
/// Set/cleared by `touchpad::mod::Engine`; read by `hook::ms_hook_proc`.
pub static BAND_LIVE: AtomicBool = AtomicBool::new(false);
