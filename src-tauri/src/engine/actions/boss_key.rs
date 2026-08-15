/// engine/actions/boss_key.rs — Workspace hide/restore + system mute.
///
/// V11 Hybrid: uses native SendInput(Win+M) / SendInput(Win+Shift+M) instead of
/// manually enumerating and hiding windows one-by-one. This is faster, smoother,
/// and uses the OS compositor directly — exactly like pressing Win+M on a keyboard.
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct BossKeyState {
    /// Whether the boss key is currently engaged.
    pub engaged: bool,
}

impl BossKeyState {
    pub fn is_engaged(&self) -> bool {
        self.engaged
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Toggle the boss key. Call from the engine actor (not the hook thread).
///
/// ENGAGE:  SendInput(Win+M + Volume_Mute)
/// RESTORE: SendInput(Win+Shift+M + Volume_Mute)
pub fn toggle_boss_key(state: &Arc<Mutex<BossKeyState>>, _own_hwnd: Option<isize>) -> &'static str {
    let mut s = state.lock().unwrap_or_else(|p| p.into_inner());

    if s.is_engaged() {
        // --- RESTORE: Win + Shift + M + Mute ---
        s.engaged = false;

        #[cfg(windows)]
        unsafe {
            send_win_shift_m();
        }

        "🔓 Workspace Restored"
    } else {
        // --- ENGAGE: Win + M + Mute ---
        s.engaged = true;

        #[cfg(windows)]
        unsafe {
            send_win_m();
        }

        "🔒 Boss Key Engaged"
    }
}

// ---------------------------------------------------------------------------
// Native SendInput helpers  (Win+M  /  Win+Shift+M)
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn set_system_mute(mute: bool) {
    use windows::Win32::Media::Audio::{eRender, eConsole, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
    
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) {
            if let Ok(device) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) {
                if let Ok(volume) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
                    let _ = volume.SetMute(mute, std::ptr::null());
                }
            }
        }
    }
}


#[cfg(windows)]
unsafe fn send_win_m() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    const VK_LWIN: u16 = 0x5B;
    const VK_M: u16 = 0x4D;
    const VK_VOLUME_MUTE: u16 = 0xAD;

    let dn = |vk: u16| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0),
                time: 0,
                dwExtraInfo: 0x7A7A7A7A,
            },
        },
    };
    let up = |vk: u16| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0x7A7A7A7A,
            },
        },
    };

    // Win↓  M↓  M↑  Win↑
    let inputs = [dn(VK_LWIN), dn(VK_M), up(VK_M), up(VK_LWIN)];
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    
    // Explicitly MUTE the system
    set_system_mute(true);

    log::info!("boss_key: sent Win+M and explicitly MUTED audio via COM");
}

#[cfg(windows)]
unsafe fn send_win_shift_m() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    const VK_LWIN: u16 = 0x5B;
    const VK_LSHIFT: u16 = 0xA0;
    const VK_M: u16 = 0x4D;

    let dn = |vk: u16| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0),
                time: 0,
                dwExtraInfo: 0x7A7A7A7A,
            },
        },
    };
    let up = |vk: u16| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0x7A7A7A7A,
            },
        },
    };

    // Win↓  Shift↓  M↓  M↑  Shift↑  Win↑
    let inputs = [
        dn(VK_LWIN),
        dn(VK_LSHIFT),
        dn(VK_M),
        up(VK_M),
        up(VK_LSHIFT),
        up(VK_LWIN),
    ];
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);

    // Explicitly UNMUTE the system
    set_system_mute(false);

    log::info!("boss_key: sent Win+Shift+M and explicitly UNMUTED audio via COM");
}
