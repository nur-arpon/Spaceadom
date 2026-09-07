/// icon_extractor.rs — App icons as base64 PNG, via the Windows shell.
///
/// HISTORY (2026-08-10) — why this file was rewritten. The old version used
/// `ExtractIconExW` + `CreateCompatibleBitmap`, and produced the "weird
/// icons" the user reported in the app picker. TWO independent bugs:
///
///  1. `ExtractIconExW` only understands .exe/.dll/.ico. It cannot resolve a
///     `.lnk` shortcut (the picker returns .lnk paths for apps whose
///     shortcut carries arguments, e.g. Discord) and knows nothing about
///     `shell:AppsFolder\<AUMID>` Store apps — so those got a generic icon
///     or none at all.
///  2. `CreateCompatibleBitmap(screen_dc, ..)` returns a device-dependent
///     bitmap with NO alpha channel. Drawing an icon into it discards
///     transparency, so `GetDIBits` read back garbage/zero alpha and the
///     icons rendered with black boxes or vanished.
///
/// The fix is one API that covers every case: `IShellItemImageFactory`
/// (`SHCreateItemFromParsingName` → `GetImage`). The shell resolves .lnk
/// targets, packaged-app AUMIDs, folders and documents, and hands back a
/// 32-bit **premultiplied** BGRA bitmap at whatever size we ask for.
///
/// Two things to remember if you touch this:
///  - GetImage returns PARGB. Un-premultiply before writing PNG or every
///    semi-transparent edge pixel comes out too dark.
///  - It needs COM on the calling thread. `ComSta` below takes care of it,
///    BALANCED: every successful `CoInitializeEx` (S_OK *and* S_FALSE both
///    bump the apartment's refcount) is paired with a `CoUninitialize` on
///    drop, and `RPC_E_CHANGED_MODE` (the thread already owns a *different*
///    apartment) is left alone because the shell objects work from an MTA
///    too — they are just marshalled. Before 2026-09-04 the init was
///    unbalanced, which was harmless on the main thread (already an STA for
///    life) and would have leaked a permanent STA onto every pooled runtime
///    thread once the callers went `async` — see PROBLEM 237.
///
/// THREADING (PROBLEM 237, measured): this whole file runs correctly on a
/// dedicated non-main STA thread with NO message pump. `IShellItem` /
/// `IShellItemImageFactory` are in-proc shell objects created in whichever
/// apartment calls `SHCreateItemFromParsingName`, and `GetImage` is a
/// synchronous in-apartment call, so nothing needs to be pumped. Proof:
/// `picker_worker::tests::icons_extract_on_a_non_main_sta_thread`.

use base64::{engine::general_purpose::STANDARD, Engine};

/// One thread's membership of a single-threaded COM apartment, released on
/// drop — and released ONLY if this call is the one that succeeded.
///
/// `CoInitializeEx` returns S_OK (we created the apartment), S_FALSE (it was
/// already there; the refcount went up anyway) or `RPC_E_CHANGED_MODE` (the
/// thread already belongs to an MTA; nothing was counted). The first two are
/// `Ok` and must be balanced by a `CoUninitialize`; the third must not be,
/// or we would decrement somebody else's count. That is exactly what `.is_ok()`
/// tells apart, so the guard stores it.
///
/// Why STA and not MTA: `SHCreateItemFromParsingName` may instantiate
/// third-party icon handlers (`IExtractIcon` shell extensions), and those are
/// overwhelmingly registered `ThreadingModel=Apartment`. From an MTA thread
/// COM would still work, but by spinning up a hidden host STA and marshalling
/// every call across — slower and with more that can time out. A worker that
/// IS an STA gets them in-process and direct. This is the same choice
/// `smart_cascade::run_browser` made for its dedicated thread, and the one
/// Explorer's own helper threads make.
pub struct ComSta(bool);

impl ComSta {
    /// Join (or create) this thread's STA. Never fails; a `RPC_E_CHANGED_MODE`
    /// thread simply gets a guard that will not uninitialise anything.
    pub fn new() -> Self {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            ComSta(CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok())
        }
        #[cfg(not(windows))]
        {
            ComSta(false)
        }
    }

    /// True when this guard's own `CoInitializeEx` succeeded (S_OK or S_FALSE)
    /// and will therefore be balanced on drop. Exposed for the proof test,
    /// which asserts the worker thread genuinely joined an apartment.
    pub fn joined(&self) -> bool {
        self.0
    }
}

impl Default for ComSta {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ComSta {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.0 {
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}

/// Icon edge in pixels. 48 stays crisp on high-DPI displays while keeping
/// the base64 payload small (the picker sends one per app in a single IPC
/// response).
const ICON_PX: i32 = 48;

/// Extract an app icon as a base64-encoded PNG.
///
/// `target` may be an .exe, a .lnk shortcut, a `shell:AppsFolder\<AUMID>`
/// Store app, or any other shell-parsable path. Returns `None` if the shell
/// has no image for it.
pub fn extract_icon(target: &str) -> Option<String> {
    extract_icon_checked(target).ok()
}

/// `extract_icon`, but a failure says WHICH call failed and with what HRESULT
/// (`"SHCreateItemFromParsingName: 0x80070002"`). The picker's proof test
/// needs that: a `None` cannot distinguish "the shell has no image for this"
/// from "COM is broken on this thread", and the second is the one the test
/// exists to rule out.
pub fn extract_icon_checked(target: &str) -> Result<String, String> {
    #[cfg(windows)]
    {
        let rgba = shell_icon_rgba(target, ICON_PX)?;
        let png = encode_rgba_as_png(&rgba, ICON_PX as u32, ICON_PX as u32)
            .ok_or_else(|| "png encode failed".to_string())?;
        Ok(STANDARD.encode(&png))
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        Err("not windows".into())
    }
}

/// Ask the shell for `target`'s icon and return straight RGBA8 pixels.
#[cfg(windows)]
fn shell_icon_rgba(target: &str, size: i32) -> Result<Vec<u8>, String> {
    use windows::core::{Interface, HSTRING, PCWSTR};
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDIBits, GetDC, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS,
    };
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
        SIIGBF_ICONONLY,
    };

    unsafe {
        // Balanced on every exit path, including the `?`s below — see ComSta.
        let _com = ComSta::new();

        let wide = HSTRING::from(target);
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)
                .map_err(|e| format!("SHCreateItemFromParsingName: 0x{:08X}", e.code().0))?;

        // BIGGERSIZEOK: prefer a larger source over an upscaled small one.
        // ICONONLY: never substitute a document thumbnail for the app icon.
        let hbitmap = factory
            .GetImage(
                SIZE { cx: size, cy: size },
                SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
            )
            .map_err(|e| format!("IShellItemImageFactory::GetImage: 0x{:08X}", e.code().0))?;

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size, // negative = top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut pixels = vec![0u8; (size * size * 4) as usize];
        let screen_dc = GetDC(None);
        let scanlines = GetDIBits(
            screen_dc,
            hbitmap,
            0,
            size as u32,
            Some(pixels.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, screen_dc);
        let _ = DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(hbitmap.0));

        if scanlines == 0 {
            return Err("GetDIBits: 0 scanlines".into());
        }

        // Premultiplied BGRA → straight RGBA.
        let mut any_visible = false;
        for px in pixels.chunks_exact_mut(4) {
            let (b, g, r, a) = (px[0], px[1], px[2], px[3]);
            if a != 0 {
                any_visible = true;
                // Un-premultiply, saturating so rounding can't wrap.
                px[0] = ((r as u32 * 255) / a as u32).min(255) as u8;
                px[1] = ((g as u32 * 255) / a as u32).min(255) as u8;
                px[2] = ((b as u32 * 255) / a as u32).min(255) as u8;
            } else {
                // Fully transparent: zero the colour so PNG compresses well.
                px[0] = 0;
                px[1] = 0;
                px[2] = 0;
            }
            px[3] = a;
        }

        // Some shell sources hand back a 24-bit image with alpha all zero,
        // which would render as a completely invisible icon. Treat that as
        // opaque rather than showing nothing.
        if !any_visible {
            for px in pixels.chunks_exact_mut(4) {
                let (b, g, r) = (px[0], px[1], px[2]);
                px[0] = r;
                px[1] = g;
                px[2] = b;
                px[3] = 255;
            }
        }

        Ok(pixels)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};

    /// Writes real PNGs so a human (or an agent with image tooling) can LOOK
    /// at them — the old extractor "succeeded" while producing black boxes,
    /// so a non-empty return value proves nothing on its own.
    /// Run: cargo test --release -- --nocapture icon_smoke
    #[test]
    fn icon_smoke() {
        let out = std::env::temp_dir().join("spacetoggle-icon-test");
        std::fs::create_dir_all(&out).unwrap();
        println!("writing icons to {}", out.display());

        let start = std::env::var("ProgramData").unwrap_or_default()
            + r"\Microsoft\Windows\Start Menu\Programs";
        let lnk = walk_first_lnk(std::path::Path::new(&start));

        let mut cases: Vec<(String, String)> = vec![
            ("exe_notepad".into(), r"C:\Windows\System32\notepad.exe".into()),
            (
                "store_calculator".into(),
                r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".into(),
            ),
            (
                "store_settings".into(),
                r"shell:AppsFolder\windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel".into(),
            ),
        ];
        if let Some(p) = lnk {
            cases.push(("lnk_shortcut".into(), p));
        }

        for (name, target) in cases {
            match extract_icon(&target) {
                Some(b64) => {
                    let bytes = STANDARD.decode(&b64).expect("valid base64");
                    let path = out.join(format!("{name}.png"));
                    std::fs::write(&path, &bytes).unwrap();
                    println!("OK   {name}: {} bytes PNG <- {target}", bytes.len());
                }
                None => println!("FAIL {name}: no icon <- {target}"),
            }
        }
    }

    fn walk_first_lnk(dir: &std::path::Path) -> Option<String> {
        let rd = std::fs::read_dir(dir).ok()?;
        let mut subdirs = Vec::new();
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                subdirs.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
                return Some(p.to_string_lossy().into_owned());
            }
        }
        subdirs.iter().find_map(|d| walk_first_lnk(d))
    }
}

/// Encode straight RGBA8 as a compressed PNG.
fn encode_rgba_as_png(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Best);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(out)
}
