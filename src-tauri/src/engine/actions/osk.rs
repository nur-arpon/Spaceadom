//! Space + ' and the icon ring's "Keyboard" tile: Windows' own on-screen
//! keyboard, toggled with Win+Ctrl+O. Owner ask 2026-09-18: a shortcut to
//! the DEFAULT one, not a keyboard of our own — press it again and it goes
//! away (osk.exe treats the chord as a toggle).
//!
//! Why osk.exe and not the touch keyboard (TabTip): the documented COM
//! route to the touch keyboard — CLSID `UIHostNoLaunch` / `ITipInvocation`
//! `Toggle` — answers REGDB_E_CLASSNOTREG on Windows 11 26200 (checked
//! 2026-09-18 on the owner's machine), and `TabTip.exe` refuses to start
//! from an unelevated process. The accessibility keyboard has a public
//! chord that toggles it; the touch keyboard does not.
//!
//! Same rules as voice typing and the screenshot: one `send_keys_checked`
//! batch with the app's injected signature so the hook ignores its own keys
//! and no partial insert can latch Win or Ctrl (PROBLEM 227).

/// The toast line. Pure — tests read it without pressing anything.
pub fn toast_text() -> &'static str {
    "On-screen keyboard — press again to hide it"
}

/// Press Win+Ctrl+O. NEVER called from a test: it really opens osk.exe on
/// whoever's desktop the test runs on.
pub fn toggle_osk() -> &'static str {
    #[cfg(windows)]
    unsafe {
        send_win_ctrl_o();
    }
    toast_text()
}

#[cfg(windows)]
unsafe fn send_win_ctrl_o() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    const VK_LWIN: u16 = 0x5B;
    const VK_CONTROL: u16 = 0x11;
    const VK_O: u16 = 0x4F;
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
    // Win↓ Ctrl↓ O↓ O↑ Ctrl↑ Win↑ — one batch.
    let inputs = [
        key(VK_LWIN, false),
        key(VK_CONTROL, false),
        key(VK_O, false),
        key(VK_O, true),
        key(VK_CONTROL, true),
        key(VK_LWIN, true),
    ];
    let _ = crate::hook::send_keys_checked(&inputs, "on-screen keyboard: Win+Ctrl+O");
    log::info!("osk: sent Win+Ctrl+O (Windows on-screen keyboard) — Space + ' or the ring tile");
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_toast_names_the_feature() {
        assert!(super::toast_text().to_lowercase().contains("keyboard"));
    }
}
