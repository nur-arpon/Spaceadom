//! Space + ; and the icon ring's "Voice Typing" tile: Windows' own
//! dictation panel (Win+H). Owner decision 2026-09-17 — a ring tile, not a
//! left+right-click chord, because the ring already has release-to-launch
//! and the chord collides with drag-selects, context menus and CAD.
//!
//! Nothing is drawn by this app: the panel is Microsoft's, it types into
//! whatever has focus, and it needs the microphone permission the user has
//! already granted (or not) to Windows. All this does is press the chord
//! with the app's injected-input signature so the app's own hook ignores
//! it, through `send_keys_checked` so a partial insert can never leave the
//! Windows key latched (PROBLEM 227).

/// The toast line. Pure — the test reads it without pressing anything.
pub fn toast_text() -> &'static str {
    "Voice typing — speak, and it types where the cursor is"
}

/// Press Win+H. Returns the toast line. NEVER called from a test: it
/// really opens the dictation panel on whoever's desktop the test runs on.
pub fn start_voice_typing() -> &'static str {
    #[cfg(windows)]
    unsafe {
        send_win_h();
    }
    toast_text()
}

#[cfg(windows)]
unsafe fn send_win_h() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    const VK_LWIN: u16 = 0x5B;
    const VK_H: u16 = 0x48;
    let key = |vk: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0x7A7A7A7A,
            },
        },
    };
    // Win↓ H↓ H↑ Win↑ — one batch, so nothing else can slip between them.
    let inputs = [key(VK_LWIN, false), key(VK_H, false), key(VK_H, true), key(VK_LWIN, true)];
    let _ = crate::hook::send_keys_checked(&inputs, "voice typing: Win+H");
    log::info!("voice_typing: sent Win+H (Windows dictation) — Space + ; or the ring tile");
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_toast_names_the_feature() {
        assert!(super::toast_text().to_lowercase().contains("voice typing"));
    }
}
