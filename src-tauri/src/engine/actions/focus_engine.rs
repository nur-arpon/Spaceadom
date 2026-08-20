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
        // A chat/prompt web app: its input is the PAGE's, not the browser's,
        // so no browser key can reach it. Ask the page's accessibility tree.
        // (Esc + 'i' — v11's guess, ported faithfully — never worked: the
        // owner tested it on 2026-08-20.)
        if is_prompt_site(&win_title) {
            if focus_text_input_uia(hwnd, InputSpot::Bottom) {
                decide("prompt site: UIA -> bottom-most input");
                return "✨ Chat Box".into();
            }
            decide("prompt site: UIA found nothing -> Ctrl+L");
            send_ctrl(VK_L.0);
            return "🌍 Address Bar".into();
        }
        // Google DOCUMENTS '/' as focus-the-search-box, on both the home page
        // and results pages — so it belongs with YouTube in the class that
        // works, not with the pages that get the address bar. Titles are
        // "Google" (home) and "<query> - Google Search" (results).
        if win_title == "google" || win_title.ends_with("google search") {
            decide("google: real '/'");
            send_slash_class_key('/');
            return "🔍 Google Search".into();
        }
        if win_title.contains("youtube") {
            decide("youtube: real '/'");
            send_slash_class_key('/');
            return "📺 YouTube Search".into();
        }
        if win_title.contains("spotify") {
            decide("spotify web: real '/'");
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

    // Discord: Esc genuinely lands in the compose box — CONFIRMED working by
    // the owner on 2026-08-20, so it keeps the fast path and never pays for a
    // tree walk.
    if proc_name.contains("discord") {
        decide("discord: Esc -> message box");
        send_key(VK_ESCAPE.0, false);
        return "💬 Message Box".into();
    }

    // WhatsApp: Esc does NOT reach the compose box (owner, same test) — it
    // backs out to the chat list. There is no published shortcut for it, so
    // find the box.
    if proc_name.contains("whatsapp") {
        if focus_text_input_uia(hwnd, InputSpot::Bottom) {
            decide("whatsapp: UIA -> bottom-most input");
            return "💬 Message Box".into();
        }
        decide("whatsapp: UIA found nothing -> Esc");
        send_key(VK_ESCAPE.0, false);
        return "💬 WhatsApp".into();
    }

    // Spotify's own app: Ctrl+F is not its search, and the '/' that works on
    // the WEB player does nothing here.
    if proc_name.contains("spotify") {
        if focus_text_input_uia(hwnd, InputSpot::Top) {
            decide("spotify app: UIA -> top-most input");
            return "🎵 Spotify Search".into();
        }
        decide("spotify app: UIA found nothing -> Ctrl+L");
        send_ctrl(VK_L.0);
        return "🎵 Spotify".into();
    }

    if proc_name.contains("explorer") {
        decide("explorer: Ctrl+E");
        send_ctrl(VK_E.0);
        return "📁 Explorer Search".into();
    }

    // Everything else: look for the box first, and only fall back to Ctrl+F —
    // which is a FIND bar in most apps, not their search field.
    if focus_text_input_uia(hwnd, InputSpot::Top) {
        decide("generic: UIA -> top-most input");
        return "🎯 Search Focused".into();
    }
    decide("generic: UIA found nothing -> Ctrl+F");
    send_ctrl(VK_F.0);
    "🎯 Find/Search Focused".into()
}

/// Browser tabs whose input belongs to the PAGE and sits at the bottom.
/// Matched on the window title, which carries the tab's title in every
/// Chromium browser.
#[cfg(windows)]
fn is_prompt_site(title: &str) -> bool {
    const PROMPT_SITES: [&str; 8] = [
        "gemini", "chatgpt", "claude", "copilot", "perplexity",
        "web.whatsapp", "whatsapp", "messenger",
    ];
    PROMPT_SITES.iter().any(|s| title.contains(s))
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

// ---------------------------------------------------------------------------
// PROBLEM 153 — finding the text box instead of guessing its shortcut
//
// The owner's 2026-08-20 test of the shortcut approach, verbatim: Discord,
// YouTube-in-browser and a new browser tab worked; WhatsApp, Spotify and
// Gemini did not. The pattern is exact — every app where a REAL shortcut is
// documented worked, and every app where the key was a guess failed. There is
// no shortcut left to guess better: WhatsApp and Spotify simply do not publish
// one for their main input, and a web app's input is not the browser's to
// focus.
//
// So ask the accessibility tree where the box IS. UI Automation is how screen
// readers do it; Electron (WhatsApp, Discord, Spotify) and Chromium (every
// browser page, Gemini included) both expose their inputs through it.
//
// COST: FindAll over a Chromium descendant tree takes tens to a few hundred
// milliseconds. That is fine HERE and nowhere else — this runs on the engine
// actor, never on the hook thread, where anything over ~1s gets the hook
// evicted by Windows (PROBLEM 134's law).
// ---------------------------------------------------------------------------

/// Where the app's main input lives on screen, which is the only way to pick
/// between several text boxes without knowing the app.
#[cfg(windows)]
#[derive(Clone, Copy, PartialEq)]
enum InputSpot {
    /// Chat and prompt apps: the compose box sits at the BOTTOM.
    Bottom,
    /// Everything else: the search box sits at the TOP.
    Top,
}

/// Focus the foreground window's main text input via UI Automation.
/// Returns false if nothing usable was found — callers keep their old
/// shortcut as the fallback, so this can only ever add behaviour.
#[cfg(windows)]
unsafe fn focus_text_input_uia(hwnd: HWND, spot: InputSpot) -> bool {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationElement, TreeScope_Descendants,
        UIA_ControlTypePropertyId, UIA_DocumentControlTypeId, UIA_EditControlTypeId,
    };

    // The engine thread may already be initialised (boss_key uses MTA); an
    // RPC_E_CHANGED_MODE here is harmless — COM stays usable either way.
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

    let uia: IUIAutomation = match CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
        Ok(u) => u,
        Err(e) => { log::warn!("smart_search: UIA unavailable ({e})"); return false; }
    };
    let root = match uia.ElementFromHandle(hwnd) {
        Ok(r) => r,
        Err(e) => { log::warn!("smart_search: ElementFromHandle failed ({e})"); return false; }
    };

    // Edit first, Document second. A Chromium contenteditable (Gemini's prompt,
    // WhatsApp's compose box) reports as Document, not Edit — searching only
    // for Edit is what a first attempt would miss.
    let mut best: Option<(IUIAutomationElement, i32)> = None;
    for ctype in [UIA_EditControlTypeId, UIA_DocumentControlTypeId] {
        let cond = match uia.CreatePropertyCondition(UIA_ControlTypePropertyId, &windows::core::VARIANT::from(ctype.0)) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let found = match root.FindAll(TreeScope_Descendants, &cond) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let n = found.Length().unwrap_or(0);
        for i in 0..n {
            let el = match found.GetElement(i) { Ok(e) => e, Err(_) => continue };
            // Only somewhere the caret can actually go, and only somewhere the
            // user can actually see — an offscreen or zero-sized box is a
            // hidden template, and focusing it looks like nothing happened.
            if el.CurrentIsKeyboardFocusable().unwrap_or_default().as_bool() != true { continue; }
            if el.CurrentIsOffscreen().unwrap_or_default().as_bool() { continue; }
            let r = match el.CurrentBoundingRectangle() { Ok(r) => r, Err(_) => continue };
            if r.right - r.left < 40 || r.bottom - r.top < 12 { continue; }
            let score = match spot {
                InputSpot::Bottom => r.top,      // largest top  = lowest on screen
                InputSpot::Top => -r.top,        // smallest top = highest
            };
            if best.as_ref().map_or(true, |(_, b)| score > *b) {
                best = Some((el, score));
            }
        }
        if best.is_some() { break; }   // Edits win outright when any exist
    }

    match best {
        Some((el, _)) => match el.SetFocus() {
            Ok(()) => true,
            Err(e) => { log::warn!("smart_search: SetFocus failed ({e})"); false }
        },
        None => false,
    }
}
