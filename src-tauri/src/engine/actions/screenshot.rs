//! Space + / and the icon ring's "Screenshot" tile: Windows' own region
//! snip (Win+Shift+S — the Snipping Tool overlay: drag a region, and it goes
//! to the clipboard and the Screenshots folder per the user's Snipping Tool
//! settings). Owner decision 2026-09-17: the app draws nothing of its own
//! for this; the ring is the TRIGGER, Windows is the tool.
//!
//! Sent as ONE `send_keys_checked` batch with the app's injected signature
//! (the hook ignores its own keys; a partial insert can never latch Win or
//! Shift — PROBLEM 227).

/// The toast line. Pure — tests read it without pressing anything.
pub fn toast_text() -> &'static str {
    "Screenshot — drag a region; it lands on the clipboard"
}

/// Press Win+Shift+S. NEVER called from a test: it really opens the snip
/// overlay on whoever's desktop the test runs on.
pub fn start_snip() -> &'static str {
    #[cfg(windows)]
    unsafe {
        send_win_shift_s();
    }
    toast_text()
}

#[cfg(windows)]
unsafe fn send_win_shift_s() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    const VK_LWIN: u16 = 0x5B;
    const VK_SHIFT: u16 = 0x10;
    const VK_S: u16 = 0x53;
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
    // Win↓ Shift↓ S↓ S↑ Shift↑ Win↑ — one batch.
    let inputs = [
        key(VK_LWIN, false),
        key(VK_SHIFT, false),
        key(VK_S, false),
        key(VK_S, true),
        key(VK_SHIFT, true),
        key(VK_LWIN, true),
    ];
    let _ = crate::hook::send_keys_checked(&inputs, "screenshot: Win+Shift+S");
    log::info!("screenshot: sent Win+Shift+S (Windows snip) — Space + / or the ring tile");
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_toast_names_the_feature() {
        assert!(super::toast_text().to_lowercase().contains("screenshot"));
    }
}
