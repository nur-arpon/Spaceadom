/// engine/actions/focus_engine.rs — Context-aware input field focus.
/// Mirrors V11 FocusInputEngine().

#[cfg(windows)]
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW},
    System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION},
    UI::WindowsAndMessaging::GetWindowThreadProcessId,
    UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, KEYBDINPUT, INPUT_KEYBOARD,
        KEYEVENTF_KEYUP, KEYEVENTF_EXTENDEDKEY,
        VK_ESCAPE, VK_CONTROL, VK_F, VK_K, VK_E, VK_L,
        VIRTUAL_KEY,
    },
};

/// Detect the foreground application and send the appropriate focus shortcut.
/// Returns a toast notification string.
pub fn focus_input_engine() -> String {
    #[cfg(windows)]
    unsafe {
        focus_engine_win32()
    }
    #[cfg(not(windows))]
    String::from("Focus engine not supported on this platform")
}

#[cfg(windows)]
unsafe fn focus_engine_win32() -> String {
    let hwnd = GetForegroundWindow();
    if hwnd.0.is_null() {
        return String::new();
    }

    let proc_name = get_process_name(hwnd).to_lowercase();
    let win_title = get_window_title(hwnd).to_lowercase();

    // Check for browser contexts first
    let is_browser = proc_name.contains("chrome")
        || proc_name.contains("brave")
        || proc_name.contains("msedge")
        || proc_name.contains("firefox");

    if is_browser {
        if win_title.contains("gemini") {
            // Escape to clear focus, then the Gemini input shortcut
            send_key(VK_ESCAPE.0, false);
            std::thread::sleep(std::time::Duration::from_millis(50));
            send_char('i');
            return "✨ Gemini Input Focused".into();
        } else if win_title.contains("youtube") {
            send_char('/');
            return "📺 YouTube Search".into();
        } else if win_title.contains("spotify") {
            send_char('/');
            return "🎵 Spotify Search".into();
        } else if win_title == "new tab" || win_title == "home" {
            // Focus address bar
            send_ctrl(VK_L.0);
            return "🌍 Address Bar".into();
        }
        // Generic web page search
        send_char('/');
        return "🔍 Page Search".into();
    }

    if proc_name.contains("whatsapp") {
        send_ctrl(VK_F.0);
        return "💬 WhatsApp Search".into();
    }

    if proc_name.contains("discord") {
        send_ctrl(VK_K.0);
        return "💬 Discord Switcher".into();
    }

    if proc_name.contains("explorer") {
        send_ctrl(VK_E.0);
        return "📁 Explorer Search".into();
    }

    // Generic fallback
    send_ctrl(VK_F.0);
    "🎯 Find/Search Focused".into()
}

#[cfg(windows)]
unsafe fn send_key(vk: u16, extended: bool) {
    let flags = if extended { KEYEVENTF_EXTENDEDKEY } else {
        windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0)
    };
    let up_flags = flags | KEYEVENTF_KEYUP;

    let inputs = [
        make_key_input(vk, flags),
        make_key_input(vk, up_flags),
    ];
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
}

#[cfg(windows)]
unsafe fn send_ctrl(vk: u16) {
    let inputs = [
        make_key_input(VK_CONTROL.0, windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0)),
        make_key_input(vk, windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0)),
        make_key_input(vk, KEYEVENTF_KEYUP),
        make_key_input(VK_CONTROL.0, KEYEVENTF_KEYUP),
    ];
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
}

#[cfg(windows)]
unsafe fn send_char(c: char) {
    // Use KEYEVENTF_UNICODE for direct character injection
    use windows::Win32::UI::Input::KeyboardAndMouse::KEYEVENTF_UNICODE;
    let inputs = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: c as u16,
                    dwFlags: KEYEVENTF_UNICODE,
                    time: 0,
                    dwExtraInfo: 0x7A7A7A7A,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: c as u16,
                    dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0x7A7A7A7A,
                },
            },
        },
    ];
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
}

#[cfg(windows)]
fn make_key_input(
    vk: u16,
    flags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS,
) -> INPUT {
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
}

#[cfg(windows)]
unsafe fn get_process_name(hwnd: HWND) -> String {
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 { return String::new(); }

    let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
        Ok(h) => h,
        Err(_) => return String::new(),
    };

    let mut buf = [0u16; 260];
    let mut size = buf.len() as u32;
    let pwstr = windows::core::PWSTR(buf.as_mut_ptr());
    let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_FORMAT(0), pwstr, &mut size);
    let _ = windows::Win32::Foundation::CloseHandle(handle);

    if ok.is_err() { return String::new(); }

    let path = String::from_utf16_lossy(&buf[..size as usize]);
    std::path::Path::new(&path)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(windows)]
unsafe fn get_window_title(hwnd: HWND) -> String {
    let len = GetWindowTextLengthW(hwnd);
    if len == 0 { return String::new(); }
    let mut buf = vec![0u16; (len + 1) as usize];
    GetWindowTextW(hwnd, &mut buf);
    String::from_utf16_lossy(&buf[..len as usize])
}
