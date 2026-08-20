/// engine/actions/focus_engine.rs — Smart Search: context-aware input focus.
///
/// Started as a mirror of V11's FocusInputEngine(); RETARGETED 2026-08-20 on
/// the owner's orders after he used it in anger:
///
///  - WhatsApp/Discord now aim for the MESSAGE BOX, not the app's search.
///    v11 (and the first port) sent Ctrl+F / Ctrl+K, which he reported as
///    landing in "start a new chat" and the quick switcher — technically as
///    designed, but never what he wanted. Neither app has a compose-box
///    shortcut, so we send ESC: both return focus to the message input when
///    every transient panel is closed. (Discord side effect: Esc also jumps
///    to the newest message, which is where you type anyway.)
///  - Ordinary websites now get the ADDRESS BAR (Ctrl+L), not '/'. There is
///    no universal "focus this site's search" key; '/' works on YouTube-class
///    sites and silently dies on most others — his "hundreds of sites".
///  - '/' is now a REAL key press. The first port injected it as a UNICODE
///    character (KEYEVENTF_UNICODE = text input, VK 0, scan-only), and sites
///    that bind their shortcut to a physical keydown ignore text input — the
///    likely reason ␣, did nothing on YouTube while v11's AHK Send("/"),
///    which presses the real key, worked. VkKeyScanW maps the char through
///    the CURRENT layout so a non-US keyboard still produces '/'.

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
        VK_ESCAPE, VK_CONTROL, VK_F, VK_E, VK_L,
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
    // One line per press: which window it saw and what it decided. Without
    // this, "␣, does nothing on YouTube" and "␣, never fired" read identical.
    let decide = |what: &str| {
        log::info!("smart_search: proc='{}' title='{}' -> {}", proc_name, win_title, what);
    };

    // Check for browser contexts first
    let is_browser = proc_name.contains("chrome")
        || proc_name.contains("brave")
        || proc_name.contains("msedge")
        || proc_name.contains("firefox");

    if is_browser {
        if win_title.contains("gemini") {
            // Escape to clear focus, then the Gemini input shortcut
            decide("gemini: Esc + 'i'");
            send_key(VK_ESCAPE.0, false);
            std::thread::sleep(std::time::Duration::from_millis(50));
            send_slash_class_key('i');
            return "✨ Gemini Input Focused".into();
        } else if win_title.contains("youtube") {
            decide("youtube: real '/'");
            send_slash_class_key('/');
            return "📺 YouTube Search".into();
        } else if win_title.contains("spotify") {
            decide("spotify: real '/'");
            send_slash_class_key('/');
            return "🎵 Spotify Search".into();
        }
        // Everything else — new tab, home, and every ordinary page — gets the
        // address bar. Typing there searches the web, and it works on all of
        // the owner's "hundreds of sites" where '/' silently died.
        decide("browser page: Ctrl+L address bar");
        send_ctrl(VK_L.0);
        return "🌍 Address Bar".into();
    }

    if proc_name.contains("whatsapp") || proc_name.contains("discord") {
        // The message box. Neither app has a focus-compose shortcut; Esc
        // closes whatever transient panel holds focus and both apps then
        // return it to the input you type in.
        decide("chat app: Esc -> message box");
        send_key(VK_ESCAPE.0, false);
        return "💬 Message Box".into();
    }

    if proc_name.contains("explorer") {
        decide("explorer: Ctrl+E");
        send_ctrl(VK_E.0);
        return "📁 Explorer Search".into();
    }

    // Generic fallback
    decide("generic: Ctrl+F");
    send_ctrl(VK_F.0);
    "🎯 Find/Search Focused".into()
}

/// Press a character as a REAL key (down+up of the virtual key the current
/// layout assigns it, with Shift wrapped around it when the layout needs
/// Shift) — the difference between "the page saw text" and "the page saw the
/// '/' KEY", which is what shortcut handlers listen for.
#[cfg(windows)]
unsafe fn send_slash_class_key(c: char) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VkKeyScanW, VK_SHIFT};
    let scan = VkKeyScanW(c as u16);
    if scan == -1 {
        // The layout cannot type this character at all — fall back to text
        // injection rather than doing nothing.
        send_char(c);
        return;
    }
    let vk = (scan & 0xFF) as u16;
    let needs_shift = (scan & 0x0100) != 0;
    let none = windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0);
    let mut inputs: Vec<INPUT> = Vec::with_capacity(4);
    if needs_shift { inputs.push(make_key_input(VK_SHIFT.0, none)); }
    inputs.push(make_key_input(vk, none));
    inputs.push(make_key_input(vk, KEYEVENTF_KEYUP));
    if needs_shift { inputs.push(make_key_input(VK_SHIFT.0, KEYEVENTF_KEYUP)); }
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
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
