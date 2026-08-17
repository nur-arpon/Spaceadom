//! rival_install.rs — detect and remove a SECOND copy of Spaceadom.
//!
//! PROBLEM 141. The app already offers a one-click elevated repair for a stale
//! logon task (PROBLEM 75), and the owner remembered it correctly: *"there
//! would be something on the top of the dashboard saying there is a conflict,
//! there is an old version installed, just press this and a prompt will come up
//! and it will delete the old version."* That banner is real — but it detects a
//! stale TASK, and says nothing about a second INSTALL.
//!
//! A second install is a different fault with the same shape. The `.msi`
//! installed per-machine into `C:\Program Files\Spaceadom`; the `setup.exe`
//! installs per-user into `%LOCALAPPDATA%\Spaceadom`. Windows treats them as
//! two unrelated programs: two uninstall entries, two autostart registrations,
//! and at logon two processes each installing a `WH_KEYBOARD_LL` hook and
//! fighting over the spacebar. Measured on the owner's own machine on
//! 2026-08-17 (PROBLEM 129):
//!
//! ```text
//! HKLM: v1.0.37 -> C:\Program Files\Spaceadom\        (from the .msi)
//! HKCU: v1.0.40 -> %LOCALAPPDATA%\Spaceadom\          (from the setup.exe)
//! ```
//!
//! What the user actually sees is not "two apps are running". It is Space+D
//! opening Discord twice, or settings that keep reverting because two processes
//! write one config.json. Nothing about that points at an installer, which is
//! why this has to be detected and named rather than left to be diagnosed.
//!
//! Dropping the `.msi` in 1.0.41 stopped this happening to NEW installs. It did
//! nothing for machines that already have both — including anyone the owner
//! shared an old build with. That is the gap this closes.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set at startup when a second install is found. Read by `get_rival_install`.
pub static RIVAL_FOUND: AtomicBool = AtomicBool::new(false);

/// Where the other copy lives, for the banner text. Written once at startup.
static RIVAL_PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static RIVAL_VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The per-machine install directory the `.msi` used, and the only place a
/// rival can be: the per-user path is where WE live.
#[cfg(windows)]
fn per_machine_exe() -> PathBuf {
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    PathBuf::from(pf).join("Spaceadom").join("spaceadom.exe")
}

/// A description of the OTHER install, or None.
#[cfg(windows)]
pub fn detect() -> Option<(String, String)> {
    let me = std::env::current_exe().ok()?;
    let rival = per_machine_exe();

    // If we ARE the per-machine copy, there is nothing to warn about: this
    // module only ever offers to remove the per-machine one, and an app must
    // never offer to delete itself.
    if me.starts_with(rival.parent()?) {
        return None;
    }
    if !rival.exists() {
        return None;
    }

    let version = file_version(&rival).unwrap_or_else(|| "unknown version".to_string());
    Some((rival.to_string_lossy().to_string(), version))
}

#[cfg(not(windows))]
pub fn detect() -> Option<(String, String)> {
    None
}

/// Read a PE file's version resource. Best-effort: the banner is still useful
/// without a version number, so a failure here must not suppress the warning.
#[cfg(windows)]
fn file_version(path: &std::path::Path) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
    };
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    use std::os::windows::ffi::OsStrExt;
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        // `dwHandle` is a documented-ignored u32 here, not an Option.
        GetFileVersionInfoW(PCWSTR(wide.as_ptr()), 0, size, buf.as_mut_ptr() as *mut _).ok()?;
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        let sub: Vec<u16> = "\\".encode_utf16().chain(std::iter::once(0)).collect();
        if !VerQueryValueW(
            buf.as_ptr() as *const _,
            PCWSTR(sub.as_ptr()),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            || ptr.is_null()
        {
            return None;
        }
        let info = &*(ptr as *const VS_FIXEDFILEINFO);
        Some(format!(
            "{}.{}.{}",
            (info.dwFileVersionMS >> 16) & 0xffff,
            info.dwFileVersionMS & 0xffff,
            (info.dwFileVersionLS >> 16) & 0xffff,
        ))
    }
}

/// Run at startup, off the critical path. Records what was found so the
/// dashboard can offer the repair.
pub fn scan() {
    if let Some((path, version)) = detect() {
        RIVAL_FOUND.store(true, Ordering::Relaxed);
        let _ = RIVAL_PATH.set(path.clone());
        let _ = RIVAL_VERSION.set(version.clone());
        log::warn!(
            "rival install: a SECOND copy of Spaceadom is installed at {path} (v{version}). \
             Both register autostart, so at the next logon two processes will each install a \
             keyboard hook and fight over the spacebar (PROBLEM 129/141). The dashboard is \
             offering a one-click elevated removal."
        );
    } else {
        log::info!("rival install: no second copy found — this machine has one Spaceadom");
    }
}

/// `(found, path, version)` for the banner.
pub fn status() -> (bool, String, String) {
    (
        RIVAL_FOUND.load(Ordering::Relaxed),
        RIVAL_PATH.get().cloned().unwrap_or_default(),
        RIVAL_VERSION.get().cloned().unwrap_or_default(),
    )
}

/// Remove the per-machine copy with ONE elevated step, the same shape as
/// PROBLEM 75's `repair_stale_task`: a single `runas` ShellExecute the user
/// consents to once, then verify by looking at the disk rather than trusting an
/// exit code (PROBLEM 127's lesson — an installer's exit code is a claim about
/// the installer, not about the machine).
#[cfg(windows)]
pub fn repair() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::System::Threading::{WaitForSingleObject, INFINITE};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let Some((path, _)) = detect() else {
        RIVAL_FOUND.store(false, Ordering::Relaxed);
        return true; // already gone
    };

    // One PowerShell pass, elevated: stop any process running FROM the rival
    // directory (never ours), uninstall every per-machine registration, and
    // clear the machine-wide autostart. Written as a single -Command so the
    // user sees exactly one UAC prompt.
    let dir = std::path::Path::new(&path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let script = format!(
        "Get-Process spaceadom -EA SilentlyContinue | \
           Where-Object {{ $_.Path -like '{dir}\\*' }} | Stop-Process -Force; \
         Start-Sleep -Milliseconds 800; \
         foreach ($r in @('HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall',\
                          'HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall')) {{ \
           Get-ChildItem $r -EA SilentlyContinue | ForEach-Object {{ \
             $p = Get-ItemProperty $_.PSPath -EA SilentlyContinue; \
             if ($p.DisplayName -like '*Spaceadom*') {{ \
               Start-Process msiexec.exe -ArgumentList \"/X$($_.PSChildName)\",'/qn','/norestart' -Wait }} }} }}; \
         Start-Sleep -Milliseconds 800; \
         Remove-Item '{dir}' -Recurse -Force -EA SilentlyContinue; \
         Remove-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run' \
           -Name 'Spaceadom' -EA SilentlyContinue"
    );

    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let verb = wide("runas");
    let file = wide("powershell.exe");
    let params = wide(&format!(
        "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command \"{}\"",
        script.replace('"', "\\\"")
    ));

    unsafe {
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(params.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        // Declining the prompt returns an error. That is a clean "no".
        if ShellExecuteExW(&mut sei).is_err() {
            log::info!("rival install: removal cancelled at the UAC prompt");
            return false;
        }
        if !sei.hProcess.is_invalid() {
            WaitForSingleObject(sei.hProcess, INFINITE);
            let _ = windows::Win32::Foundation::CloseHandle(sei.hProcess);
        }
    }

    // Verify against the DISK, not the exit code.
    let gone = detect().is_none();
    RIVAL_FOUND.store(!gone, Ordering::Relaxed);
    if gone {
        log::info!("rival install: the second copy at {path} is gone — one Spaceadom remains");
    } else {
        log::warn!("rival install: {path} is STILL present after the elevated removal");
    }
    gone
}

#[cfg(not(windows))]
pub fn repair() -> bool {
    true
}
