/// engine/actions/smart_cascade.rs — Port of V11 SmartCascade() + ResolvePath()
// Trivial update to refresh IDE diagnostics.

use crate::config::KeyBinding;

#[cfg(windows)]
use windows::Win32::{
    Foundation::HWND,
    System::Threading::AttachThreadInput,
    UI::WindowsAndMessaging::{
        EnumWindows, GetWindowLongW,
        IsWindowVisible, ShowWindow, SetForegroundWindow, GetForegroundWindow,
        IsWindow, GetWindowThreadProcessId, IsIconic,
        BringWindowToTop, SwitchToThisWindow,
        GWL_STYLE, SW_MINIMIZE, SW_RESTORE, WS_VISIBLE,
    },
    UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_TYPE, KEYBDINPUT, KEYEVENTF_KEYUP, KEYBD_EVENT_FLAGS,
        SendInput, VK_MENU, VIRTUAL_KEY,
    },
};
use windows::Win32::System::Threading::GetCurrentThreadId;

use std::sync::{Arc, Mutex, OnceLock};
use std::collections::HashMap;

#[cfg(windows)]
fn get_app_cache() -> Arc<Mutex<HashMap<String, isize>>> {
    static CACHE: OnceLock<Arc<Mutex<HashMap<String, isize>>>> = OnceLock::new();
    CACHE.get_or_init(|| Arc::new(Mutex::new(HashMap::new()))).clone()
}

#[cfg(windows)]
use winreg::{enums::HKEY_LOCAL_MACHINE, enums::HKEY_CURRENT_USER, RegKey};

/// What the cascade actually did — the toast must tell the truth about a
/// fallback, or the user cannot tell why "the wrong app" opened (2026-08-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CascadeOutcome {
    /// The active profile's own binding acted.
    Primary,
    /// The active binding failed; the FOUNDERS binding for this key acted.
    Fallback,
    /// Nothing could be focused or launched at all.
    Failed,
}

/// Main cascade logic: focus → minimize toggle → launch.
/// Mirrors V11 SmartCascade(TargetApp, TargetWeb, FoundersApp, FoundersWeb).
pub fn smart_cascade(
    binding: &KeyBinding,
    fallback: Option<&KeyBinding>,
    app_handle: Option<tauri::AppHandle>,
) -> CascadeOutcome {
    if let Some(app) = &binding.app {
        // Store/UWP targets — try AUMID-based focus/minimize first, then
        // fall through to shell_launch if no window is found.
        if is_shell_target(app) {
            if aumid_focus_or_minimize(app) {
                return CascadeOutcome::Primary;
            }
            if launch_app(app, app_handle.clone()) { return CascadeOutcome::Primary; }
        } else if try_focus_or_minimize(app) {
            return CascadeOutcome::Primary;
        } else if launch_app(app, app_handle.clone()) {
            return CascadeOutcome::Primary;
        }
    }

    if let Some(url) = &binding.web_url {
        // Toggle an already-open browser window showing this site BEFORE
        // launching — otherwise every press opens another duplicate tab.
        if url_focus_or_minimize(url) { return CascadeOutcome::Primary; }
        if run_browser(url, app_handle.clone()) { return CascadeOutcome::Primary; }
    }

    // Fallback to the Founders binding for this key. This is a DELIBERATE
    // feature (owner's design): if the active profile's binding can't launch —
    // empty key, or the app isn't installed on this machine — try the
    // Founders entry so a sensible default still fires instead of nothing.
    if let Some(fb) = fallback {
        log::warn!(
            "cascade: active profile's binding failed (app={:?}, url={:?}) — falling back to the FOUNDERS binding for this key",
            binding.app, binding.web_url
        );
        if let Some(app) = &fb.app {
            if is_shell_target(app) {
                if aumid_focus_or_minimize(app) { return CascadeOutcome::Fallback; }
                if launch_app(app, app_handle.clone()) { return CascadeOutcome::Fallback; }
            } else {
                if try_focus_or_minimize(app) { return CascadeOutcome::Fallback; }
                if launch_app(app, app_handle.clone()) { return CascadeOutcome::Fallback; }
            }
        }
        if let Some(url) = &fb.web_url {
            if url_focus_or_minimize(url) { return CascadeOutcome::Fallback; }
            if run_browser(url, app_handle.clone()) { return CascadeOutcome::Fallback; }
        }
    }
    CascadeOutcome::Failed
}

use tauri::Emitter;

/// Launch anything the Windows shell can open — .exe, .lnk (arguments in the
/// shortcut survive), protocol URIs, documents — via ShellExecuteExW.
///
/// This replaced `std::process::Command::spawn`, which CANNOT execute .lnk
/// files (os error 193 "%1 is not a valid Win32 application", seen live with
/// a Swoosh.lnk binding), cannot open URIs at all, and errors with 740 on
/// exes whose manifest demands elevation. ShellExecute is what v11's AHK
/// `Run` used — this is the parity path.
#[cfg(windows)]
/// Launch via EXPLORER (medium integrity) instead of our elevated process.
///
/// PROBLEM 56 — the reason a tester's Space+letter "launched" apps that never
/// appeared. Spaceadom runs ELEVATED (the keyboard hook needs it). Chromium
/// browsers launched by an elevated parent de-elevate themselves by
/// re-launching through the shell, and on some machines that handoff dies
/// silently: ShellExecuteEx returns success, the process exits, no window is
/// ever created. The tester's log proved it — five `ShellExecute launched
/// brave.exe` lines, then `url_focus … Titles seen: []` showing ZERO Brave
/// windows existed seconds later.
///
/// The Microsoft-documented fix (Raymond Chen, "How can I launch an
/// unelevated process from my elevated process", + the ExecInExplorer SDK
/// sample) is to hand the launch to the DESKTOP's explorer via
/// IShellDispatch2::ShellExecute. Explorer runs unelevated at medium
/// integrity, so the app starts exactly as if the user double-clicked it —
/// browsers, .lnk shortcuts, URLs and shell: AUMIDs all behave normally.
#[cfg(windows)]
fn shell_launch_unelevated(file: &str, params: Option<&str>) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // `explorer.exe <target>` hands the launch to the ALREADY-RUNNING desktop
    // shell, which is unelevated at medium integrity, so the target starts
    // exactly as if the user had double-clicked it. explorer accepts exe
    // paths, .lnk shortcuts, URLs and `shell:AppsFolder\<AUMID>` alike.
    //
    // Chosen over the IShellDispatch2/ExecInExplorer COM chain deliberately:
    // same de-elevation, a fraction of the surface area, and no extra
    // `windows` crate features to keep in sync.
    //
    // NOTE: explorer.exe always exits ~immediately after handing off, so its
    // exit status says nothing about whether the app started — never treat it
    // as proof (that mistake is why a log once read "launched" for apps that
    // never appeared).
    let mut cmd = std::process::Command::new("explorer.exe");
    if let Some(p) = params.filter(|p| !p.is_empty()) {
        // explorer takes ONE argument; fold params into the target when the
        // caller supplied them (URLs passed to a browser exe).
        cmd.arg(format!("{file} {p}"));
    } else {
        cmd.arg(file);
    }
    match cmd.creation_flags(CREATE_NO_WINDOW).spawn() {
        Ok(_) => {
            log::info!("cascade: launched {file} UNELEVATED via explorer (as a double-click would)");
            true
        }
        Err(e) => {
            log::warn!(
                "cascade: unelevated launch failed for {file}: {e} — falling back to direct ShellExecute"
            );
            false
        }
    }
}

#[cfg(not(windows))]
fn shell_launch_unelevated(_f: &str, _p: Option<&str>) -> bool { false }

/// Re-resolve a saved path whose app has updated itself into a new folder.
///
/// PROBLEM 116, found live 2026-08-16. Space+D stopped opening Discord. The
/// binding held `...\Discord\app-1.0.9251\Discord.exe`; on disk was
/// `...\Discord\app-1.0.9253\Discord.exe`. Discord had updated, and the saved
/// path died with the folder it named. The user sees "the shortcut broke",
/// which is the wrong story — the shortcut is fine, the target moved.
///
/// This is not a Discord quirk. It is the Squirrel installer's layout, used by
/// Slack, Teams (classic), GitHub Desktop and Signal among others: the exe
/// lives in `<App>\app-<version>\` and EVERY self-update creates a new one.
/// Any absolute path saved into such a folder is guaranteed to break, on every
/// machine, at an unpredictable future date. Re-resolving is the only fix that
/// stays fixed.
///
/// Returns `(target, params)` ready for `shell_launch`, or `None` when the app
/// really is gone.
///
/// GENERALISE THIS: an absolute path stored today is a guess about tomorrow's
/// filesystem. Anything that stores one needs a recovery path, not just an
/// error message.
#[cfg(windows)]
fn repair_versioned_path(dead: &std::path::Path) -> Option<(String, Option<String>)> {
    use std::path::PathBuf;

    // Find the `app-<version>` ancestor. Only this exact shape is accepted —
    // matching looser patterns risks launching an unrelated executable.
    let comps: Vec<_> = dead.components().collect();
    let idx = comps.iter().position(|c| {
        c.as_os_str()
            .to_string_lossy()
            .to_ascii_lowercase()
            .starts_with("app-")
    })?;
    let base: PathBuf = comps[..idx].iter().collect();
    let tail: PathBuf = comps[idx + 1..].iter().collect();

    // Newest sibling `app-*`, chosen by modification time rather than by name:
    // version strings stop sorting lexicographically the moment a component
    // reaches double digits (app-1.0.9 vs app-1.0.10).
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(&base).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if !name.starts_with("app-") || !entry.path().is_dir() {
            continue;
        }
        let t = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        if newest.as_ref().is_none_or(|(best, _)| t > *best) {
            newest = Some((t, entry.path()));
        }
    }
    if let Some((_, dir)) = newest {
        let candidate = dir.join(&tail);
        if candidate.exists() {
            return Some((candidate.to_string_lossy().into_owned(), None));
        }
    }

    // Squirrel's own stable entry point. This is what the Start Menu shortcut
    // runs, it has never moved, and it will survive every future update — so
    // it is the better answer even though it is the fallback.
    let updater = base.join("Update.exe");
    if updater.exists() {
        let exe = dead.file_name()?.to_string_lossy().into_owned();
        return Some((
            updater.to_string_lossy().into_owned(),
            Some(format!("--processStart {exe}")),
        ));
    }
    None
}

fn shell_launch(file: &str, params: Option<&str>, app_handle: Option<tauri::AppHandle>) -> bool {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::System::Threading::{WaitForSingleObject, INFINITE};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    unsafe {
        // ShellExecuteEx wants COM (.lnk resolution goes through the shell).
        // Ignore the result: RPC_E_CHANGED_MODE just means the thread already
        // has a concurrency model, which is fine.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        // When elevated, prefer explorer's unelevated launch (PROBLEM 56).
        // The direct path below stays as the fallback for machines where the
        // desktop shell COM chain is unavailable.
        if crate::startup::is_elevated() && shell_launch_unelevated(file, params) {
            if let Some(app) = &app_handle {
                let _ = app.emit("app-launched", &file.to_string());
            }
            return true;
        }

        let file_h = HSTRING::from(file);
        let verb_h = HSTRING::from("open");
        let params_h = params.map(HSTRING::from);

        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(verb_h.as_ptr()),
            lpFile: PCWSTR(file_h.as_ptr()),
            lpParameters: params_h
                .as_ref()
                .map(|h| PCWSTR(h.as_ptr()))
                .unwrap_or(PCWSTR::null()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };

        if let Err(e) = ShellExecuteExW(&mut sei) {
            log::warn!("cascade: ShellExecute failed for {file}: {e}");
            return false;
        }

        // PROBLEM 54 — "ShellExecute launched" DOES NOT MEAN A WINDOW APPEARED.
        // ShellExecuteEx returns success for activations that create no
        // process at all, and hInstApp still carries the legacy error code.
        // This line previously claimed success unconditionally, and I read a
        // tester's log and told the user apps "were opening" when nothing had
        // appeared on his screen. Never report a launch without reporting
        // whether a PROCESS was actually created.
        //
        // hInstApp <= 32 is the classic ShellExecute error range:
        //   2 = file not found, 3 = path not found, 5 = ACCESS DENIED,
        //   26/27/31/32 = sharing/assoc/no-app failures.
        // ACCESS DENIED (5) is the one to watch: we run ELEVATED, and an
        // elevated process launching a per-user app can be refused outright.
        let inst = sei.hInstApp.0 as usize;
        let pid_created = !sei.hProcess.is_invalid() && !sei.hProcess.0.is_null();
        if inst <= 32 {
            log::warn!(
                "cascade: ShellExecute REFUSED {file} — hInstApp={inst} \
                 (2=not found, 3=bad path, 5=ACCESS DENIED, 31=no association). \
                 Nothing was opened."
            );
            return false;
        }
        log::info!(
            "cascade: ShellExecute accepted {file} (hInstApp={inst}, process_created={pid_created})"
        );

        let name = file.to_string();
        if let Some(app) = &app_handle {
            let _ = app.emit("app-launched", &name);
        }

        // SEE_MASK_NOCLOSEPROCESS gives us the child handle when the shell
        // actually created a process (not for DDE/Store activations) — wait on
        // it off-thread so the frontend still gets its app-closed event.
        let hproc = sei.hProcess;
        if !hproc.is_invalid() && !hproc.0.is_null() {
            let raw = hproc.0 as isize;
            std::thread::spawn(move || {
                let h = windows::Win32::Foundation::HANDLE(raw as *mut _);
                let _ = WaitForSingleObject(h, INFINITE);
                let _ = CloseHandle(h);
                if let Some(app) = &app_handle {
                    let _ = app.emit("app-closed", &name);
                }
            });
        }
        true
    }
}

#[cfg(not(windows))]
fn shell_launch(_file: &str, _params: Option<&str>, _app_handle: Option<tauri::AppHandle>) -> bool {
    false
}

/// Four-step foreground ladder — the minimum set that reliably beats
/// Windows 10/11's focus-theft prevention (adapted from MSDN community
/// notes, AutoHotkey source, and the implementation plan §Step 11).
///
/// Steps:
///  1. AttachThreadInput to the foreground thread so SetForegroundWindow is
///     not blocked by UIPI / focus-lock.
///  2. BringWindowToTop + SetForegroundWindow — first actual raise.
///  3. Synthetic Alt keydown+up (VK_MENU, cookie 0x7A7A7A7A) so Windows
///     treats the next SetForegroundWindow as user-initiated.
///  4. SwitchToThisWindow — final reliable hand-off.
///
/// After every step: verify with GetForegroundWindow() and log which step
/// actually brought the window to front, so we have data if a step regresses.
#[cfg(windows)]
unsafe fn force_foreground(hwnd: windows::Win32::Foundation::HWND) {
    let fg_before = GetForegroundWindow();
    if fg_before == hwnd {
        return; // Already foreground — nothing to do
    }
    let fg_thread = GetWindowThreadProcessId(fg_before, None);
    let my_thread = GetCurrentThreadId();

    // Step 1 — attach to the current foreground thread so SetForegroundWindow
    // is not blocked by Windows' focus-lock. Detach immediately after.
    if fg_thread != my_thread && fg_thread != 0 {
        let _ = AttachThreadInput(my_thread, fg_thread, true);
    }

    // Step 2 — bring to top + set foreground (may succeed on its own for
    // windows that allow being foregrounded).
    let _ = BringWindowToTop(hwnd);
    let _ = SetForegroundWindow(hwnd);

    if fg_thread != my_thread && fg_thread != 0 {
        let _ = AttachThreadInput(my_thread, fg_thread, false);
    }

    if GetForegroundWindow() == hwnd {
        log::debug!("force_foreground: step-2 SetForegroundWindow succeeded");
        return;
    }

    // Step 3 — synthetic Alt tap. Windows internally requires the last input
    // to be a keyboard event before it honours SetForegroundWindow from a
    // thread that didn't initiate user input. Tag with the SpaceToggle hook
    // cookie (0x7A7A7A7A) so the low-level keyboard hook ignores it.
    let vk_menu = VK_MENU.0 as u16;
    let inputs: [INPUT; 2] = [
        INPUT {
            r#type: INPUT_TYPE(1), // INPUT_KEYBOARD
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(vk_menu),
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0x7A7A7A7A,
                },
            },
        },
        INPUT {
            r#type: INPUT_TYPE(1),
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(vk_menu),
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(KEYEVENTF_KEYUP.0),
                    time: 0,
                    dwExtraInfo: 0x7A7A7A7A,
                },
            },
        },
    ];
    let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    log::debug!("force_foreground: SendInput sent {sent} events");

    let _ = SetForegroundWindow(hwnd);
    if GetForegroundWindow() == hwnd {
        log::debug!("force_foreground: step-3 (Alt tap + SetForegroundWindow) succeeded");
        return;
    }

    // Step 4 — SwitchToThisWindow: last-resort, always works but may flash
    // the taskbar button once.
    SwitchToThisWindow(hwnd, true);
    if GetForegroundWindow() == hwnd {
        log::debug!("force_foreground: step-4 SwitchToThisWindow succeeded");
    } else {
        log::warn!("force_foreground: all 4 steps failed for HWND {:?}", hwnd);
    }
}

/// Step 12 — AUMID-based focus/minimize for Microsoft Store apps.
///
/// Store apps' windows belong to `ApplicationFrameHost.exe`, not the app's own
/// process. `QueryFullProcessImageNameW` returns the HOST process, so the
/// normal exe-stem matching never finds them. Instead we enumerate all
/// top-level windows and call `SHGetPropertyStoreForWindow` to read
/// `PKEY_AppUserModel_ID`, which IS the user's `shell:AppsFolder\<AUMID>`
/// string (without the `shell:AppsFolder\` prefix).
///
/// If a window with a matching AUMID is found: toggle focus/minimize.
/// Returns `true` if acted, `false` if no matching window found (caller
/// should then launch the app via `shell_launch`).
#[cfg(windows)]
/// Site keyword for matching a browser window title, derived from a URL.
///
/// `https://www.youtube.com/watch?v=…` → `Some(("youtube", "youtube.com"))`
///
/// Returns `None` when the keyword would be too weak to match on safely — a
/// 1-2 character first label (`x.com`, `t.co`) would match almost any title
/// and could minimise an unrelated browser window. Callers treat `None` as
/// "just launch the URL", which is always safe.
fn url_match_keys(url: &str) -> Option<(String, String)> {
    // Strip scheme, then path/query/fragment, then userinfo and port.
    let rest = url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(url);
    let hostport = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(rest);
    let host = hostport
        .rsplit('@')
        .next()
        .unwrap_or(hostport)
        .split(':')
        .next()
        .unwrap_or(hostport)
        .trim_start_matches("www.")
        .to_lowercase();
    if host.is_empty() {
        return None;
    }
    let keyword = host.split('.').next().unwrap_or(&host).to_string();
    if keyword.len() < 3 {
        return None;
    }
    Some((keyword, host))
}

/// The package family name — everything before the `!` in an AUMID.
/// `SamsungElectronicsCo.Ltd.PCGallery_3c1yjt4zspk6g!App` → the part before
/// `!`. Stable per package; the app-id after `!` is not.
fn aumid_family(s: &str) -> &str {
    s.split('!').next().unwrap_or(s)
}

/// PROBLEM 79 — is this window DWM-cloaked? Suspended packaged apps and
/// windows parked on another virtual desktop stay IsWindowVisible()==true but
/// composite nothing; restoring one moves keyboard FOCUS to a window the user
/// cannot see (Enter could send a WhatsApp message into the void). Every
/// window-matching enum must skip cloaked windows.
#[cfg(windows)]
unsafe fn is_cloaked(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    let mut cloaked: u32 = 0;
    let _ = DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut _ as *mut core::ffi::c_void,
        4,
    );
    cloaked != 0
}

/// PROBLEM 79, fallback 2 — package family name of the process that owns
/// `hwnd`, lowercased. None for unpackaged processes (kernel32 returns
/// APPMODEL_ERROR_NO_PACKAGE) and for processes we cannot open (protected —
/// skip, never abort the enumeration).
#[cfg(windows)]
unsafe fn window_package_family(hwnd: windows::Win32::Foundation::HWND) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS};
    use windows::Win32::Storage::Packaging::Appx::GetPackageFamilyName;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 || pid == std::process::id() {
        return None;
    }
    let hproc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    // PACKAGE_FAMILY_NAME_MAX_LENGTH is 64; +1 for the NUL.
    let mut buf = [0u16; 65];
    let mut len = buf.len() as u32;
    let err = GetPackageFamilyName(hproc, &mut len, PWSTR(buf.as_mut_ptr()));
    let _ = CloseHandle(hproc);
    if err != ERROR_SUCCESS {
        return None; // unpackaged (15700) or query failure — either way, skip
    }
    // On success `len` INCLUDES the NUL terminator.
    Some(String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)]).to_lowercase())
}

/// PROBLEM 79, fallback 3 — the target exe path of an Apps-folder entry
/// (e.g. Arc: "C:\...\Arc\Arc.exe"). Packaged apps have no
/// System.Link.TargetParsingPath — GetString errs cleanly and we return None.
/// MUST run while COM is initialised on this thread. Takes the ORIGINAL-case
/// shell target: registered AUMIDs are arbitrary case-sensitive strings, and
/// parsing a lowercased copy can fail with 0x80070002.
#[cfg(windows)]
unsafe fn apps_folder_target_path(shell_target: &str) -> Option<String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Com::{CoTaskMemFree, IBindCtx};
    use windows::Win32::UI::Shell::{IShellItem2, SHCreateItemFromParsingName};
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
    use windows::core::GUID;

    // System.Link.TargetParsingPath — hand-rolled like PKEY_AppUserModel_ID
    // below, so no new Cargo feature (PROBLEM 30 class) is needed.
    let pkey_target = PROPERTYKEY {
        fmtid: GUID::from_u128(0xB9B4B3FC_2B51_4A42_B5D8_324146AFCF25),
        pid: 2,
    };

    let name = HSTRING::from(shell_target);
    let item: IShellItem2 =
        SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None::<&IBindCtx>).ok()?;
    let pwstr = item.GetString(&pkey_target).ok()?;
    let path = pwstr.to_string().ok();
    CoTaskMemFree(Some(pwstr.0 as *const _));
    path
}

fn aumid_focus_or_minimize(shell_target: &str) -> bool {
    // Strip the "shell:AppsFolder\" prefix to get the bare AUMID.
    // NOTE: `shell_target` itself must stay ORIGINAL-case — fallback 3 parses
    // it through the shell namespace, which is case-sensitive for registered
    // AUMIDs. Only this comparison copy is lowercased.
    let aumid = shell_target
        .strip_prefix("shell:AppsFolder\\")
        .or_else(|| shell_target.strip_prefix("shell:appsFolder\\"))
        .unwrap_or(shell_target)
        .to_lowercase();

    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, IsWindowVisible};
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::{
        SHGetPropertyStoreForWindow, IPropertyStore, GPS_READWRITE,
    };
    use windows::core::PCWSTR;
    // PKEY_AppUserModel_ID: {9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}, 5
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
    use windows::core::GUID;

    let pkey_aumid = PROPERTYKEY {
        fmtid: GUID {
            data1: 0x9F4C2855,
            data2: 0x9F79,
            data3: 0x4B39,
            data4: [0xA8, 0xD0, 0xE1, 0xD4, 0x2D, 0xE1, 0xD5, 0xF3],
        },
        pid: 5,
    };

    struct SearchPayload {
        aumid: String,
        found: Option<HWND>,
        pkey: PROPERTYKEY,
        /// Every packaged window's AUMID we looked at, for diagnostics when
        /// nothing matches.
        seen: Vec<String>,
    }

    unsafe extern "system" fn aumid_enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
        use windows::core::Interface;

        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        // PROBLEM 79 — a cloaked ApplicationFrameWindow keeps its AUMID
        // property; matching one restores focus onto an invisible window.
        if is_cloaked(hwnd) {
            return BOOL(1);
        }
        let payload = &mut *(lparam.0 as *mut SearchPayload);

        // Try to get the property store for this window.
        let store_result: windows::core::Result<IPropertyStore> =
            SHGetPropertyStoreForWindow(hwnd);
        let Ok(store) = store_result else { return BOOL(1); };

        let pv_result = store.GetValue(&payload.pkey);
        let Ok(pv) = pv_result else { return BOOL(1); };

        // PropVariantToStringAlloc allocates; we must free the pointer.
        match PropVariantToStringAlloc(&pv) {
            Ok(pwstr) => {
                let window_aumid = pwstr.to_string().unwrap_or_default().to_lowercase();
                // Free the CoTaskMem allocation.
                windows::Win32::System::Com::CoTaskMemFree(Some(pwstr.0 as *mut _));

                // Exact match first, then PACKAGE FAMILY NAME (the part before
                // '!'). Windows does NOT guarantee a packaged app's window
                // reports the same AUMID that launched it — the app-id after
                // '!' is chosen by the app, and apps with several entry points
                // launch as "…!App" while their window reports "…!Gallery" or
                // similar. Exact-only matching is why Samsung Notes minimised
                // correctly on 2026-08-11 while Samsung Gallery relaunched
                // every time: same code, different app. The family name before
                // '!' identifies the package uniquely, so it is a safe
                // fallback — it cannot collide across different packages.
                let matched = window_aumid == payload.aumid
                    || (aumid_family(&window_aumid) == aumid_family(&payload.aumid)
                        && !aumid_family(&payload.aumid).is_empty());

                if matched {
                    payload.found = Some(hwnd);
                    return BOOL(0); // stop enumeration
                }
                // Record what packaged windows we DID see. Without this, "no
                // match" is indistinguishable from "no packaged windows at
                // all", and the only way to tell was to add logging and ask
                // the user to reproduce — a whole round trip.
                if !window_aumid.is_empty() {
                    payload.seen.push(window_aumid);
                }
            }
            Err(_) => {}
        }
        BOOL(1)
    }

    let mut payload = SearchPayload {
        aumid,
        found: None,
        pkey: pkey_aumid,
        seen: Vec::new(),
    };

    unsafe {
        // COM must be initialised on this thread for SHGetPropertyStoreForWindow
        // AND for fallback 3's SHCreateItemFromParsingName. The CoUninitialize
        // moved BELOW the whole ladder (PROBLEM 79) — uninitialising here made
        // the shell parse fail with CO_E_NOTINITIALIZED, an Err that reads
        // exactly like "property not found" and silently disables the fix.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let _ = EnumWindows(
            Some(aumid_enum_cb),
            LPARAM(&mut payload as *mut SearchPayload as isize),
        );

        // ------------------------------------------------------------------
        // PROBLEM 79, fallback 2 — match by the PROCESS's package family.
        // WinUI3 apps (modern WhatsApp: process WhatsApp.Root) own visible,
        // titled windows that carry NO AppUserModel_ID property, so the
        // property-store pass above never sees them — every press fell
        // through to ShellExecute re-activation, which no-ops on a running
        // instance ("it does nothing", tester + owner, 2026-08-12).
        // Only meaningful for PACKAGED bindings — a family exists only when
        // the AUMID has the "family!appid" shape.
        // ------------------------------------------------------------------
        if payload.found.is_none() && payload.aumid.contains('!') {
            use windows::Win32::UI::WindowsAndMessaging::{
                GetWindow, GetWindowTextLengthW, GW_OWNER,
            };

            struct FamilyScan {
                family: String,
                candidates: Vec<HWND>,
            }
            unsafe extern "system" fn family_enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
                let scan = &mut *(lparam.0 as *mut FamilyScan);
                // Cheap rejections first: visible → not cloaked → titled →
                // unowned. Titleless kills XAML flyout popups; GW_OWNER kills
                // owned dialogs so a modal's MAIN window is what we act on.
                if !IsWindowVisible(hwnd).as_bool() || is_cloaked(hwnd) {
                    return BOOL(1);
                }
                if GetWindowTextLengthW(hwnd) == 0 {
                    return BOOL(1);
                }
                if GetWindow(hwnd, GW_OWNER).map(|h| !h.is_invalid()).unwrap_or(false) {
                    return BOOL(1);
                }
                if window_package_family(hwnd).as_deref() == Some(scan.family.as_str()) {
                    // NO early exit — collect, then rank. A topmost helper
                    // (mini player, toast) enumerates before the main window;
                    // first-hit would toggle the helper forever.
                    scan.candidates.push(hwnd);
                }
                BOOL(1)
            }

            let mut scan = FamilyScan {
                family: aumid_family(&payload.aumid).to_string(),
                candidates: Vec::new(),
            };
            let _ = EnumWindows(
                Some(family_enum_cb),
                LPARAM(&mut scan as *mut FamilyScan as isize),
            );
            // Rank: the foreground window if it matched (so the minimize half
            // of the toggle acts on what the user is looking at), else the
            // first in enum order (≈ most recently active).
            let fg = GetForegroundWindow();
            payload.found = scan
                .candidates
                .iter()
                .copied()
                .find(|&h| h == fg)
                .or_else(|| scan.candidates.first().copied());
            if let Some(h) = payload.found {
                log::info!(
                    "aumid_focus: matched by PROCESS package family {:?} ({} candidate(s)) — {:?}",
                    scan.family,
                    scan.candidates.len(),
                    h
                );
            }
        }

        // ------------------------------------------------------------------
        // PROBLEM 79, fallback 3 — unpackaged Apps-folder entries (Arc et
        // al.): the entry is shortcut-backed, so resolve its target exe and
        // delegate to the Win32 stem matcher, which owns the same
        // minimize/restore cycle plus the HWND cache.
        // ------------------------------------------------------------------
        if payload.found.is_none() {
            if let Some(target) = apps_folder_target_path(shell_target) {
                log::info!(
                    "aumid_focus: Apps-folder entry resolves to {:?} — delegating to the stem matcher",
                    target
                );
                CoUninitialize();
                return try_focus_or_minimize(&target);
            }
        }

        CoUninitialize();
    }

    if let Some(hwnd) = payload.found {
        unsafe {
            let is_active = GetForegroundWindow() == hwnd;
            let is_minimized = IsIconic(hwnd).as_bool();
            if is_active && !is_minimized {
                log::info!("aumid_focus: AUMID match — minimizing HWND {:?}", hwnd);
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            } else {
                log::info!("aumid_focus: AUMID match — restoring HWND {:?}", hwnd);
                let _ = ShowWindow(hwnd, SW_RESTORE);
                force_foreground(hwnd);
            }
        }
        true
    } else {
        // log::info, not debug — debug lines are filtered out of the shipped
        // log, so the ONE line that explains why a Store app relaunched
        // instead of minimising was invisible exactly when it was needed.
        // Listing what we DID see turns "it doesn't work" into a one-look fix.
        log::info!(
            "aumid_focus: no window matched AUMID {:?} (family {:?}). Packaged windows seen: {:?}",
            shell_target,
            aumid_family(&payload.aumid),
            payload.seen,
        );
        false
    }
}

#[cfg(not(windows))]
fn aumid_focus_or_minimize(_shell_target: &str) -> bool { false }

/// Which browser `run_browser` would use, as a lowercase process stem.
/// Must stay in step with run_browser's preference order.
#[cfg(windows)]
/// Process stem of the user's DEFAULT browser, e.g. "firefox", "msedge".
///
/// PROBLEM 60. This used to guess brave → chrome → msedge, which meant the
/// URL toggle (`url_focus_or_minimize`) looked for windows belonging to a
/// browser the user may not even have — so Space+Y could never find the tab it
/// had just opened, and every press launched a duplicate.
///
/// Resolved properly from the shell association Windows itself uses:
///   HKCU\...\UrlAssociations\https\UserChoice → ProgId
///   HKCR\<ProgId>\shell\open\command          → "C:\...\firefox.exe" -- "%1"
fn browser_stem() -> Option<String> {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER};
        use winreg::RegKey;

        let prog_id: String = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(
                r"SOFTWARE\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
            )
            .ok()?
            .get_value("ProgId")
            .ok()?;

        let cmd: String = RegKey::predef(HKEY_CLASSES_ROOT)
            .open_subkey(format!(r"{prog_id}\shell\open\command"))
            .ok()?
            .get_value("")
            .ok()?;

        // cmd looks like: "C:\Program Files\Mozilla Firefox\firefox.exe" -osint -url "%1"
        // Take the quoted path if present, else everything up to the first space.
        let path = if cmd.starts_with('"') {
            cmd[1..].split('"').next()?.to_string()
        } else {
            cmd.split_whitespace().next()?.to_string()
        };

        let stem = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())?;
        log::info!("cascade: default browser resolved → {stem} (ProgId {prog_id})");
        return Some(stem);
    }
    #[cfg(not(windows))]
    None
}

/// URL bindings get the same launch → focus → minimise cascade as apps.
///
/// WHY THIS EXISTS (2026-08-11): `run_browser` unconditionally launched the
/// URL, so pressing Space+Y a second time opened ANOTHER YouTube tab instead
/// of toggling. URL bindings were the only kind with no cascade at all.
///
/// Returns true if it handled the press (focused or minimised an existing
/// window); false means "no window is showing this site" and the caller
/// should launch the URL normally.
///
/// DELIBERATELY NOT SENDING Ctrl+W. The user proposed closing the tab on the
/// second press. Ctrl+W closes whatever tab is ACTIVE, which is not
/// necessarily the bound site — a half-written comment or form would be
/// destroyed, on a key pressed dozens of times a day, and the app cannot
/// check-then-send without racing the user. Minimising achieves the actual
/// goal ("get it out of my way") with no destruction. Do not "improve" this
/// into a tab-closing feature.
///
/// KNOWN LIMITATION: a window title only reveals its ACTIVE tab. If the site
/// sits in a background tab we return false and open a duplicate. Detecting
/// background tabs needs browser-extension access — out of scope. A duplicate
/// tab is a far smaller harm than closing the wrong one.
#[cfg(windows)]
fn url_focus_or_minimize(url: &str) -> bool {
    let Some((keyword, host)) = url_match_keys(url) else {
        log::info!("url_focus: {url:?} has no safe title keyword — launching normally");
        return false;
    };
    let Some(stem) = browser_stem() else {
        log::info!("url_focus: no known browser resolved — launching normally");
        return false;
    };

    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, IsWindowVisible};

    struct UrlSearch {
        stem: String,
        keyword: String,
        host: String,
        found: Option<HWND>,
        seen: Vec<String>,
    }

    unsafe extern "system" fn url_enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowTextW, GetWindowThreadProcessId,
        };

        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let p = &mut *(lparam.0 as *mut UrlSearch);

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == std::process::id() {
            return BOOL(1);
        }

        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return BOOL(1);
        };
        let mut buf = [0u16; 260];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        if ok.is_err() {
            return BOOL(1);
        }
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        let this_stem = std::path::Path::new(&path)
            .file_stem()
            .map(|f| f.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if this_stem != p.stem {
            return BOOL(1);
        }

        // Browser window title is "<active tab title> - Brave". A captionless
        // window is a helper, not a browsing window.
        let mut tbuf = [0u16; 512];
        let n = GetWindowTextW(hwnd, &mut tbuf);
        if n <= 0 {
            return BOOL(1);
        }
        let title = String::from_utf16_lossy(&tbuf[..n as usize]).to_lowercase();

        if title.contains(&p.keyword) || title.contains(&p.host) {
            p.found = Some(hwnd);
            return BOOL(0); // stop
        }
        p.seen.push(title);
        BOOL(1)
    }

    let mut payload = UrlSearch {
        stem: stem.clone(),
        keyword: keyword.clone(),
        host: host.clone(),
        found: None,
        seen: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(
            Some(url_enum_cb),
            LPARAM(&mut payload as *mut UrlSearch as isize),
        );
    }

    if let Some(hwnd) = payload.found {
        unsafe {
            let is_active = GetForegroundWindow() == hwnd;
            let is_minimized = IsIconic(hwnd).as_bool();
            if is_active && !is_minimized {
                log::info!(
                    "url_focus: {stem} window showing {keyword:?} is foreground — minimizing {hwnd:?}"
                );
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            } else {
                log::info!(
                    "url_focus: {stem} window showing {keyword:?} — restoring {hwnd:?}"
                );
                let _ = ShowWindow(hwnd, SW_RESTORE);
                force_foreground(hwnd);
            }
        }
        true
    } else {
        // info!, not debug! — a debug-level diagnostic does not exist in the
        // shipped log, which is exactly when it is needed (PROBLEM 38).
        log::info!(
            "url_focus: no {stem} window titled {keyword:?} — launching {url:?}. Titles seen: {:?}",
            payload.seen
        );
        false
    }
}

#[cfg(not(windows))]
fn url_focus_or_minimize(_url: &str) -> bool { false }

/// Returns `true` if an existing window was found and acted upon.\n
#[cfg(windows)]
fn try_focus_or_minimize(exe_name: &str) -> bool {
    // Match by file STEM (no extension): bindings may store a .lnk path (the
    // app picker keeps shortcuts whose arguments matter), and "discord.lnk"
    // must still match the running "discord.exe" process.
    let exe_lower = std::path::Path::new(exe_name)
        .file_stem()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| exe_name.to_lowercase());
    
    // Check Cache first
    let cache_lock = get_app_cache();
    let mut cache = cache_lock.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(&hwnd_raw) = cache.get(&exe_lower) {
        let hwnd = windows::Win32::Foundation::HWND(hwnd_raw as *mut _);
        unsafe {
            // Same NATIVE-SAFETY rule as the enum filter below: a cached
            // handle may predate the shell-window fix (or Explorer may have
            // recycled the HWND) — never act on shell infrastructure. Treat an
            // unsafe cached handle exactly like a dead one: drop it from the
            // cache and fall through to fresh enumeration.
            let cached_unsafe = exe_lower == "explorer" && IsWindow(hwnd).as_bool() && {
                let mut cls_buf = [0u16; 64];
                let n = windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut cls_buf);
                String::from_utf16_lossy(&cls_buf[..n.max(0) as usize]) != "CabinetWClass"
            };
            if cached_unsafe {
                cache.remove(&exe_lower);
            } else if IsWindow(hwnd).as_bool() {
                let is_active = GetForegroundWindow() == hwnd;
                let is_minimized = IsIconic(hwnd).as_bool();
                
                if is_active && !is_minimized {
                    log::info!("Event: Space+? | Target: {} | HWND: {:?} | Action: Minimize", exe_name, hwnd);
                    let _ = ShowWindow(hwnd, SW_MINIMIZE);
                } else {
                    log::info!("Event: Space+? | Target: {} | HWND: {:?} | Action: Restore", exe_name, hwnd);
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                    force_foreground(hwnd);
                }
                return true;
            } else {
                // Window dead, remove from cache
                cache.remove(&exe_lower);
            }
        }
    }
    drop(cache); // Release lock before EnumWindows

    let found: Arc<Mutex<Option<HWND>>> = Arc::new(Mutex::new(None));
    let found_clone = Arc::clone(&found);

    unsafe {
        // Enumerate all top-level windows looking for our exe.
        //
        // The payload must be reclaimed after the call. Previously this was
        // `Box::into_raw(...)` with no matching `from_raw`, so every uncached
        // Space+key press leaked the String AND an Arc clone — meaning the
        // Mutex allocation never dropped either. Small per press, but this is
        // a tray app that runs all day.
        //
        // Safe to free here: EnumWindows is synchronous, so the callback
        // cannot outlive this scope.
        let payload = Box::into_raw(Box::new((exe_lower.clone(), Arc::clone(&found_clone))));
        let result = EnumWindows(
            Some(enum_callback),
            windows::Win32::Foundation::LPARAM(payload as isize),
        );
        drop(Box::from_raw(payload));
        let _ = result; // EnumWindows returns FALSE when callback returned FALSE (found one)
    }

    let hwnd = found.lock().unwrap_or_else(|p| p.into_inner()).take();
    if let Some(hwnd) = hwnd {
        // Store in cache for next time
        get_app_cache().lock().unwrap_or_else(|p| p.into_inner()).insert(exe_lower.clone(), hwnd.0 as isize);
        
        unsafe {
            let is_active = GetForegroundWindow() == hwnd;
            let is_minimized = IsIconic(hwnd).as_bool();
            
            if is_active && !is_minimized {
                log::info!("Event: Space+? | Target: {} | HWND: {:?} | Action: Minimize (Enum)", exe_name, hwnd);
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            } else {
                log::info!("Event: Space+? | Target: {} | HWND: {:?} | Action: Restore (Enum)", exe_name, hwnd);
                let _ = ShowWindow(hwnd, SW_RESTORE);
                force_foreground(hwnd);
            }
        }
        return true;
    }
    false
}

#[cfg(not(windows))]
fn try_focus_or_minimize(_exe_name: &str) -> bool { false }

#[cfg(windows)]
unsafe extern "system" fn enum_callback(
    hwnd: HWND,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::BOOL {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    use std::sync::{Arc, Mutex};

    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1); // continue
    }

    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    if (style & WS_VISIBLE.0) == 0 {
        return BOOL(1);
    }

    let data = &*(lparam.0 as *mut (String, Arc<Mutex<Option<HWND>>>));
    let (exe_lower, found) = data;

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return BOOL(1);
    }

    let my_pid = std::process::id();
    if pid == my_pid {
        return BOOL(1);
    }

    let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
        Ok(h) => h,
        Err(_) => return BOOL(1),
    };

    let mut buf = [0u16; 260];
    let mut size = buf.len() as u32;
    let pwstr = windows::core::PWSTR(buf.as_mut_ptr());
    let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_FORMAT(0), pwstr, &mut size);
    let _ = windows::Win32::Foundation::CloseHandle(handle);

    if ok.is_err() {
        return BOOL(1);
    }

    let path = String::from_utf16_lossy(&buf[..size as usize]);
    let exe = std::path::Path::new(&path)
        .file_name()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    // Target is a STEM (see try_focus_or_minimize) — compare stems.
    let stem = std::path::Path::new(&path)
        .file_stem()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if stem == exe_lower.as_str() {
        // ==================================================================
        // NATIVE-SAFETY RULE (see NATIVE_SAFETY.md — incident 2026-08-10):
        // explorer.exe IS the Windows shell. Its visible top-level windows
        // include the taskbar, the desktop, the Task-View/gesture overlays
        // and assorted helpers (ThumbnailDeviceHelperWnd, ...). Minimizing or
        // force-foregrounding those broke the user's 3/4-finger touchpad
        // gestures and window management until Explorer was restarted.
        //
        // A class DENYLIST proved unreliable (each test found a new helper
        // class), so for explorer.exe we use a POSITIVE filter: the only
        // windows we may ever touch are real File Explorer file-manager
        // windows, class "CabinetWClass".
        // ==================================================================
        let mut cls_buf = [0u16; 64];
        let n = windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut cls_buf);
        let cls = String::from_utf16_lossy(&cls_buf[..n.max(0) as usize]);

        if exe == "explorer.exe" {
            if cls != "CabinetWClass" {
                return BOOL(1); // shell infrastructure — never touch, keep looking
            }
        } else {
            // For every other app: skip captionless tool/helper windows.
            // Real application main windows have a title; invisible helpers
            // (IME hosts, trays, splash leftovers) usually don't.
            use windows::Win32::UI::WindowsAndMessaging::GetWindowTextLengthW;
            if GetWindowTextLengthW(hwnd) == 0 {
                return BOOL(1);
            }
        }

        *found.lock().unwrap_or_else(|p| p.into_inner()) = Some(hwnd);
        return BOOL(0); // stop enumeration
    }

    BOOL(1)
}

/// Attempt to launch an app by exe name or absolute path.
///
/// Priority:
///   1. If `exe_name` is already an absolute path → use directly (dashboard-set paths)
///   2. Protocol URI shortcut (discord://, spotify://, etc.)
///   3. Known-app lookup table via resolve_path()
///   4. Registry HKLM/HKCU App Paths fallback (inside resolve_path)
/// `shell:AppsFolder\<AUMID>` (Microsoft Store app) or any other shell: verb.
fn is_shell_target(target: &str) -> bool {
    target.len() > 6 && target[..6].eq_ignore_ascii_case("shell:")
}

fn launch_app(exe_name: &str, app_handle: Option<tauri::AppHandle>) -> bool {
    // Case 0: Store/UWP app — hand the shell: path straight to ShellExecute,
    // which activates the package by AppUserModelID.
    if is_shell_target(exe_name) {
        log::info!("cascade: activating Store app: {exe_name}");
        return shell_launch(exe_name, None, app_handle);
    }

    let p = std::path::Path::new(exe_name);

    // Case 1: caller provided a full absolute path (e.g. from the settings
    // dashboard). ShellExecute handles .exe, .lnk (keeping the shortcut's
    // arguments — how Discord-style "Update.exe --processStart" launchers
    // work), and elevation-manifest exes.
    if p.is_absolute() {
        if p.exists() {
            log::info!("cascade: launching absolute path: {exe_name}");
            return shell_launch(exe_name, None, app_handle);
        } else {
            // PROBLEM 116 — a saved path that no longer exists is USUALLY not
            // an uninstalled app. It is an app that updated itself into a new
            // folder. Try to re-resolve before giving up.
            #[cfg(windows)]
            if let Some((target, params)) = repair_versioned_path(p) {
                log::warn!(
                    "cascade: '{exe_name}' is gone — the app updated itself into a new \
                     folder. Re-resolved to '{target}{}'",
                    params.as_deref().map(|a| format!(" {a}")).unwrap_or_default()
                );
                return shell_launch(&target, params.as_deref(), app_handle);
            }
            log::warn!("cascade: absolute path does not exist: {exe_name}");
            return false;
        }
    }

    // Case 2: Check for URI protocol shortcuts first
    if let Some(uri) = protocol_uri(exe_name) {
        log::info!("cascade: launching via URI protocol: {uri}");
        return shell_launch(&uri, None, app_handle);
    }

    // Case 3 & 4: resolve from known-app table + registry
    match resolve_path(exe_name) {
        Some(path) => {
            if !std::path::Path::new(&path).exists() {
                log::warn!("cascade: resolved path does not exist: {path}");
                return false;
            }
            log::info!("cascade: launching {path}");
            shell_launch(&path, None, app_handle)
        }
        None => {
            log::warn!("cascade: could not resolve path for {exe_name}");
            false
        }
    }
}


/// Run a URL in the preferred browser (Brave → Chrome → shell open).
/// Open a URL in the user's DEFAULT browser.
///
/// PROBLEM 60. This used to try brave.exe, then chrome.exe, and only then fall
/// back to a shell open — so on a tester's machine with neither installed, the
/// preferred paths missed and links reportedly opened the OneDrive Documents
/// FOLDER instead of a browser.
///
/// Two separate defects, both fixed here:
///
/// 1. HARDCODED BROWSERS. The user's choice of browser is Windows' business,
///    not ours. Deleted entirely.
///
/// 2. THE FOLDER-INSTEAD-OF-URL SYMPTOM. The old path went through
///    `shell_launch`, which uses `ShellExecuteExW` with `SEE_MASK_NOCLOSEPROCESS`
///    and calls `CoInitializeEx(COINIT_APARTMENTTHREADED)` on the ENGINE thread,
///    ignoring `RPC_E_CHANGED_MODE`. Ignoring that error means COM may already
///    be in a different apartment, and http protocol activation goes through
///    COM/DDE — when it fails, ShellExecute falls back to treating the argument
///    as a path relative to the process's CURRENT WORKING DIRECTORY. Launched by
///    the logon task, that cwd is the user profile, hence Documents/OneDrive.
///
///    Fixed by opening URLs on a DEDICATED thread that owns a clean STA, using
///    plain `ShellExecuteW` — the documented way to hand a URL to the default
///    browser — and by passing an explicit directory so a mis-parse can never
///    silently resolve against the cwd.
pub fn run_browser(url: &str, app_handle: Option<tauri::AppHandle>) -> bool {
    let url = url.trim().trim_matches('"').to_string();
    if url.is_empty() {
        log::warn!("cascade: refusing to open an empty URL");
        return false;
    }
    // Guard the exact failure above: anything without a scheme could be taken
    // as a file path. Give it one rather than letting the shell guess.
    let url = if url.contains("://") || url.starts_with("mailto:") {
        url
    } else {
        log::info!("cascade: URL {url:?} had no scheme — assuming https://");
        format!("https://{url}")
    };

    log::info!("cascade: opening {url} in the DEFAULT browser");

    #[cfg(windows)]
    {
        let u = url.clone();
        // Dedicated thread: a fresh STA, so protocol activation cannot be
        // poisoned by whatever apartment the engine thread happens to be in.
        let joiner = std::thread::spawn(move || unsafe {
            use windows::core::HSTRING;
            use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
            use windows::Win32::UI::Shell::ShellExecuteW;
            use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let file = HSTRING::from(u.as_str());
            let verb = HSTRING::from("open");
            // Explicit working directory — never let a mis-parse resolve the
            // URL against the process cwd (that is the Documents bug).
            let dir = HSTRING::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()));
            let inst = ShellExecuteW(
                None,
                windows::core::PCWSTR(verb.as_ptr()),
                windows::core::PCWSTR(file.as_ptr()),
                windows::core::PCWSTR::null(),
                windows::core::PCWSTR(dir.as_ptr()),
                SW_SHOWNORMAL,
            );
            if hr.is_ok() {
                CoUninitialize();
            }
            // ShellExecuteW returns >32 on success.
            inst.0 as usize > 32
        });

        return match joiner.join() {
            Ok(true) => {
                if let Some(app) = &app_handle {
                    use tauri::Emitter;
                    let _ = app.emit("app-launched", &url);
                }
                true
            }
            Ok(false) => {
                log::error!(
                    "cascade: ShellExecuteW refused {url} — no default browser is registered \
                     for http/https on this machine"
                );
                false
            }
            Err(_) => {
                log::error!("cascade: the URL-opening thread panicked for {url}");
                false
            }
        };
    }

    #[cfg(not(windows))]
    {
        let _ = app_handle;
        false
    }
}

fn open_with(exe: &str, url: &str, app_handle: Option<tauri::AppHandle>) -> bool {
    shell_launch(exe, Some(url), app_handle)
}

fn open_uri(uri: &str, app_handle: Option<tauri::AppHandle>) -> bool {
    let clean = uri.trim_matches('"');
    shell_launch(clean, None, app_handle)
}

/// Check for protocol-based URIs (discord://, spotify://, etc.)
fn protocol_uri(exe: &str) -> Option<String> {
    match exe.to_lowercase().as_str() {
        "discord.exe" => Some("discord://".into()),
        "spotify.exe" => Some("spotify://".into()),
        "whatsapp.exe" => Some("whatsapp://".into()),
        "steam.exe" => Some("steam://".into()),
        _ => None,
    }
}

/// Resolve an exe name to its full absolute path.
/// Mirrors V11 ResolvePath() exactly with the same priority ordering.
pub fn resolve_path(exe_name: &str) -> Option<String> {
    let p = std::env::var("ProgramFiles").unwrap_or_default();
    let p86 = std::env::var("ProgramFiles(x86)").unwrap_or_default();
    let l = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let a = std::env::var("APPDATA").unwrap_or_default();

    let candidates: Vec<String> = match exe_name.to_lowercase().as_str() {
        "brave.exe" => vec![
            format!("{p}\\BraveSoftware\\Brave-Browser\\Application\\brave.exe"),
            format!("{l}\\BraveSoftware\\Brave-Browser\\Application\\brave.exe"),
        ],
        "chrome.exe" => vec![
            format!("{p}\\Google\\Chrome\\Application\\chrome.exe"),
            format!("{l}\\Google\\Chrome\\Application\\chrome.exe"),
        ],
        "obs64.exe" => vec![
            format!("{p}\\obs-studio\\bin\\64bit\\obs64.exe"),
            format!("{p86}\\obs-studio\\bin\\64bit\\obs64.exe"),
        ],
        "excel.exe" => vec![
            format!("{p}\\Microsoft Office\\root\\Office16\\EXCEL.EXE"),
            format!("{p86}\\Microsoft Office\\root\\Office16\\EXCEL.EXE"),
        ],
        "powerpnt.exe" => vec![
            format!("{p}\\Microsoft Office\\root\\Office16\\POWERPNT.EXE"),
        ],
        "outlook.exe" => vec![
            format!("{p}\\Microsoft Office\\root\\Office16\\OUTLOOK.EXE"),
        ],
        "photoshop.exe" => {
            // Dynamic search for all Adobe Photoshop version folders
            let mut v = vec![
                format!("{p}\\Adobe\\Adobe Photoshop 2026\\Photoshop.exe"),
                format!("{p}\\Adobe\\Adobe Photoshop 2025\\Photoshop.exe"),
                format!("{p}\\Adobe\\Adobe Photoshop 2024\\Photoshop.exe"),
            ];
            // Try glob-style search via read_dir
            if let Ok(entries) = std::fs::read_dir(format!("{p}\\Adobe")) {
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    if name.starts_with("adobe photoshop") {
                        v.push(format!("{}\\Photoshop.exe", e.path().display()));
                    }
                }
            }
            v
        }
        "leagueclient.exe" => vec![
            "C:\\Riot Games\\League of Legends\\LeagueClient.exe".into(),
        ],
        "epicgameslauncher.exe" => vec![
            format!("{p86}\\Epic Games\\Launcher\\Portal\\Binaries\\Win64\\EpicGamesLauncher.exe"),
            format!("{p}\\Epic Games\\Launcher\\Portal\\Binaries\\Win64\\EpicGamesLauncher.exe"),
        ],
        "blender.exe" => vec![
            format!("{p}\\Blender Foundation\\Blender\\blender.exe"),
        ],
        "canva.exe" => vec![
            format!("{l}\\Programs\\Canva\\Canva.exe"),
        ],
        "resolve.exe" => vec![
            format!("{p}\\Blackmagic Design\\DaVinci Resolve\\Resolve.exe"),
        ],
        "slack.exe" => vec![
            format!("{l}\\Programs\\slack\\slack.exe"),
        ],
        "telegram.exe" => vec![
            format!("{a}\\Telegram Desktop\\Telegram.exe"),
        ],
        "utorrent.exe" => vec![
            format!("{a}\\uTorrent\\uTorrent.exe"),
        ],
        "vlc.exe" => vec![
            format!("{p}\\VideoLAN\\VLC\\vlc.exe"),
            format!("{p86}\\VideoLAN\\VLC\\vlc.exe"),
        ],
        "zoom.exe" => vec![
            format!("{a}\\Zoom\\bin\\Zoom.exe"),
        ],
        "notepad.exe" => vec![
            "C:\\Windows\\System32\\notepad.exe".into(),
        ],
        "notion.exe" => vec![
            format!("{l}\\Programs\\Notion\\Notion.exe"),
        ],
        "wt.exe" => vec![
            format!("{l}\\Microsoft\\WindowsApps\\wt.exe"),
        ],
        "radeon software.exe" | "radeonsoftware.exe" => vec![
            format!("{p}\\AMD\\CNext\\CNext\\RadeonSoftware.exe"),
        ],
        "msiafterburner.exe" => vec![
            format!("{p86}\\MSI Afterburner\\MSIAfterburner.exe"),
        ],
        "explorer.exe" => vec![
            "C:\\Windows\\explorer.exe".into(),
        ],
        _ => vec![],
    };

    // Check each candidate
    for c in &candidates {
        if std::path::Path::new(c).exists() {
            return Some(c.clone());
        }
    }

    // Registry fallback: HKLM then HKCU App Paths
    #[cfg(windows)]
    {
        let reg_key = format!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{exe_name}");
        for hive in [
            RegKey::predef(HKEY_LOCAL_MACHINE),
            RegKey::predef(HKEY_CURRENT_USER),
        ] {
            if let Ok(key) = hive.open_subkey(&reg_key) {
                if let Ok(path) = key.get_value::<String, _>("") {
                    if std::path::Path::new(&path).exists() {
                        return Some(path);
                    }
                }
            }
        }
    }

    // ---------------------------------------------------------------------
    // PATH search — catches anything on the system PATH (wt.exe, git, etc.)
    // ---------------------------------------------------------------------
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(';').filter(|d| !d.is_empty()) {
            let cand = std::path::Path::new(dir).join(exe_name);
            if cand.exists() {
                return Some(cand.to_string_lossy().into_owned());
            }
        }
    }

    // ---------------------------------------------------------------------
    // START MENU SHORTCUT SEARCH — the fallback that actually finds things.
    //
    // PROBLEM 53. Everything above only works for apps whose exact install
    // path is hardcoded in the table, or which register an App Paths key, or
    // which sit on PATH. A tester's log showed the consequence plainly:
    //   cascade: could not resolve path for Battle.net.exe
    //   cascade: could not resolve path for NVIDIA GeForce Experience.exe
    //   cascade: could not resolve path for HaloInfinite.exe
    // …so Space+key did nothing for most of his bindings, even though the
    // hook and engine were working perfectly. Nearly every installed Windows
    // app puts a .lnk in one of the two Start Menu trees, which is exactly
    // where the app picker already finds them — so resolve here from the same
    // source. ShellExecute launches a .lnk fine (the picker already stores
    // .lnk paths for apps whose shortcut carries arguments).
    // ---------------------------------------------------------------------
    let stem = std::path::Path::new(exe_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !stem.is_empty() {
        let roots = [
            std::env::var("APPDATA")
                .map(|v| format!("{v}\\Microsoft\\Windows\\Start Menu\\Programs"))
                .unwrap_or_default(),
            std::env::var("ProgramData")
                .map(|v| format!("{v}\\Microsoft\\Windows\\Start Menu\\Programs"))
                .unwrap_or_default(),
        ];
        for root in roots.iter().filter(|r| !r.is_empty()) {
            if let Some(hit) = find_shortcut(std::path::Path::new(root), &stem, 0) {
                log::info!("cascade: resolved {exe_name:?} via Start Menu → {hit}");
                return Some(hit);
            }
        }
    }

    log::warn!(
        "cascade: could not resolve {exe_name:?} — not in the known-paths table, not in \
         App Paths, not on PATH, and no matching Start Menu shortcut. It is probably not \
         installed, or was installed without a Start Menu entry."
    );
    None
}

/// Recursively look for a `.lnk` whose file name matches `stem`.
/// Depth-capped: Start Menu trees are shallow, and an unbounded walk on a
/// keypress path is not acceptable.
#[cfg(windows)]
fn find_shortcut(dir: &std::path::Path, stem: &str, depth: u32) -> Option<String> {
    if depth > 3 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            subdirs.push(path);
            continue;
        }
        if path.extension().and_then(|x| x.to_str()).map(|x| x.eq_ignore_ascii_case("lnk"))
            != Some(true)
        {
            continue;
        }
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        // Exact first, then a contains-match so "NVIDIA app" finds
        // "NVIDIA App.lnk" and "Battle.net" finds "Battle.net Launcher.lnk".
        if name == stem || name.starts_with(stem) || stem.starts_with(&name) {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    for d in subdirs {
        if let Some(hit) = find_shortcut(&d, stem, depth + 1) {
            return Some(hit);
        }
    }
    None
}

#[cfg(not(windows))]
fn find_shortcut(_d: &std::path::Path, _s: &str, _depth: u32) -> Option<String> { None }

/* ===========================================================================
   TESTS — PROBLEM 116, the self-updating-app path repair.
   ===========================================================================
   The first automated tests in this project, added deliberately and only here.
   The reason is PROBLEM 118: 1.0.33 shipped a recovery branch that had never
   once been executed, and it was wrong in two ways that a single run would
   have caught. This repair is the same shape — a branch that only fires when
   something has already gone wrong, so it is exactly the code least likely to
   be exercised before a user hits it.

   It also could not be tested on the developer's machine by hand: Discord was
   open during every attempt, so `smart_cascade` matched the running window by
   executable name and the launch path never ran at all. These tests build the
   Squirrel folder layout in a temp directory and check the resolution directly,
   which needs no application installed and works on any machine.
   =========================================================================== */
#[cfg(all(test, windows))]
mod repair_tests {
    use super::repair_versioned_path;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// A scratch directory that removes itself, so a failed test cannot leave
    /// litter behind that makes the NEXT run pass for the wrong reason.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let mut p = std::env::temp_dir();
            p.push(format!("spaceadom-test-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).expect("scratch dir");
            Scratch(p)
        }
        fn path(&self) -> &Path { &self.0 }
    }
    impl Drop for Scratch {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    fn touch(p: &Path) {
        if let Some(d) = p.parent() { fs::create_dir_all(d).unwrap(); }
        fs::write(p, b"x").unwrap();
    }

    /// The real case: Discord updated 9251 -> 9253 and the saved path died.
    #[test]
    fn resolves_to_the_newer_version_folder() {
        let s = Scratch::new("newer");
        let app = s.path().join("Discord");
        let live = app.join("app-1.0.9253").join("Discord.exe");
        touch(&live);

        let dead = app.join("app-1.0.9251").join("Discord.exe");
        let (target, params) = repair_versioned_path(&dead).expect("should re-resolve");

        assert_eq!(Path::new(&target), live.as_path());
        assert!(params.is_none(), "a direct exe needs no arguments");
    }

    /// Version strings stop sorting lexicographically once a component reaches
    /// double digits: "app-1.0.10" sorts BEFORE "app-1.0.9" as text. Choosing
    /// by modification time is what makes this correct, and this test is here
    /// to stop anyone "simplifying" it back to a name sort.
    #[test]
    fn picks_by_modification_time_not_by_name() {
        let s = Scratch::new("sorting");
        let app = s.path().join("Slack");

        let older = app.join("app-1.0.9").join("Slack.exe");
        touch(&older);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let newer = app.join("app-1.0.10").join("Slack.exe");   // sorts EARLIER as text
        touch(&newer);

        let dead = app.join("app-1.0.8").join("Slack.exe");
        let (target, _) = repair_versioned_path(&dead).expect("should re-resolve");
        assert_eq!(
            Path::new(&target), newer.as_path(),
            "must choose the most recently written folder, not the alphabetically last"
        );
    }

    /// When no usable version folder remains, fall back to Squirrel's own
    /// launcher — the stable entry point that survives every future update.
    #[test]
    fn falls_back_to_the_squirrel_updater() {
        let s = Scratch::new("updater");
        let app = s.path().join("Teams");
        touch(&app.join("Update.exe"));

        let dead = app.join("app-1.0.1").join("Teams.exe");
        let (target, params) = repair_versioned_path(&dead).expect("should fall back");

        assert_eq!(Path::new(&target), app.join("Update.exe").as_path());
        assert_eq!(params.as_deref(), Some("--processStart Teams.exe"));
    }

    /// A version folder with the WRONG executable inside must not be accepted;
    /// the updater is the correct answer there.
    #[test]
    fn does_not_accept_a_version_folder_missing_the_exe() {
        let s = Scratch::new("wrongexe");
        let app = s.path().join("Signal");
        touch(&app.join("app-2.0.0").join("SomethingElse.exe"));
        touch(&app.join("Update.exe"));

        let dead = app.join("app-1.0.0").join("Signal.exe");
        let (target, params) = repair_versioned_path(&dead).expect("should fall back");
        assert_eq!(Path::new(&target), app.join("Update.exe").as_path());
        assert_eq!(params.as_deref(), Some("--processStart Signal.exe"));
    }

    /// Genuinely uninstalled: nothing to offer, and the caller must get None so
    /// it logs the real error instead of launching something arbitrary.
    #[test]
    fn returns_none_when_the_app_is_really_gone() {
        let s = Scratch::new("gone");
        let dead = s.path().join("Ghost").join("app-1.0.0").join("Ghost.exe");
        assert!(repair_versioned_path(&dead).is_none());
    }

    /// An ordinary program in Program Files has no `app-<version>` ancestor and
    /// must be left completely alone — this repair must never fire on a path it
    /// does not understand.
    #[test]
    fn ignores_paths_that_are_not_squirrel_shaped() {
        let s = Scratch::new("plain");
        let exe = s.path().join("Notepad++").join("notepad++.exe");
        touch(&exe);
        let dead = s.path().join("Notepad++").join("gone.exe");
        assert!(repair_versioned_path(&dead).is_none());
    }
}
