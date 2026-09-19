//! PHASE A — `Action::Chord`: press a key combination on the user's behalf.
//!
//! `keys` is the chord in PRESS order (`[Win, Shift, S]`); the batch is every
//! key down in that order, then every key up in REVERSE, all in ONE
//! `send_keys_checked` call with the app's injected signature — so the hook
//! ignores its own keys and a partial insert can never leave a modifier
//! latched (PROBLEM 227). Same shape as `osk.rs`: `toast_text` is pure and
//! tested; `send` is never called from a test, because it really presses the
//! keys on whoever's desktop the test runs on.

/// The toast line. Pure.
pub fn toast_text(keys: &[u16]) -> String {
    format!("⌨ {}", crate::engine::specials::chord_name(keys))
}

/// The longest chord this will send. Sixteen is four times anything a
/// person presses, and `send_keys_raw`'s stack repair buffer is sixteen
/// KEYUPs — a batch that could latch more than that could not be repaired.
pub const MAX_KEYS: usize = 8;

/// Press the chord. Returns the toast. An empty or over-long chord sends
/// nothing and says so.
pub fn send(keys: &[u16]) -> String {
    if keys.is_empty() {
        return "⌨ Nothing to press — this key has an empty chord".into();
    }
    if keys.len() > MAX_KEYS {
        log::warn!("chord: refusing a {}-key chord (max {MAX_KEYS})", keys.len());
        return format!("⌨ Chord too long ({} keys, max {MAX_KEYS})", keys.len());
    }
    #[cfg(windows)]
    unsafe {
        send_batch(keys);
    }
    log::info!("chord: sent {} (Phase A action)", crate::engine::specials::chord_name(keys));
    toast_text(keys)
}

/// The batch, as `(vk, is_up)` pairs: downs in order, ups in reverse. Pure,
/// so the ordering rule has a test without touching `SendInput`.
pub fn batch_plan(keys: &[u16]) -> Vec<(u16, bool)> {
    let mut plan: Vec<(u16, bool)> = keys.iter().map(|k| (*k, false)).collect();
    plan.extend(keys.iter().rev().map(|k| (*k, true)));
    plan
}

/// The one batch every chord in the app goes through — the key editor's
/// `Action::Chord` and (1.0.122) the touchpad's "Any shortcut" steps and its
/// ←/→ scrub taps (`touchpad::actions::send_chord`). Not the hook callback.
#[cfg(windows)]
pub(crate) unsafe fn send_batch(keys: &[u16]) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    let key = |vk: u16, up: bool| {
        // Arrows, Home/End/PgUp/PgDn, Insert/Delete and the right-hand
        // modifiers are EXTENDED keys; without the flag Windows reads the
        // numpad twin (Home becomes numpad 7).
        let extended = matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0xA3 | 0xA5 | 0x5B | 0x5C);
        let mut flags = if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
        if extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0x7A7A7A7A,
                },
            },
        }
    };
    let inputs: Vec<INPUT> = batch_plan(keys).into_iter().map(|(vk, up)| key(vk, up)).collect();
    let what = format!("chord: {}", crate::engine::specials::chord_name(keys));
    let _ = crate::hook::send_keys_checked(&inputs, &what);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toast_names_the_chord() {
        assert_eq!(toast_text(&[0x5B, 0x10, 0x53]), "⌨ Win+Shift+S");
    }

    /// Downs in press order, ups in reverse — the rule that keeps a modifier
    /// from being released under the key it modifies.
    #[test]
    fn the_batch_is_downs_in_order_then_ups_in_reverse() {
        assert_eq!(
            batch_plan(&[0x5B, 0x10, 0x53]),
            vec![(0x5B, false), (0x10, false), (0x53, false), (0x53, true), (0x10, true), (0x5B, true)]
        );
        assert!(batch_plan(&[]).is_empty());
    }
}
