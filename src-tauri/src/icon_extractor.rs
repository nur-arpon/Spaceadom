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
///  - It needs COM on the calling thread; `CoInitializeEx` is called here
///    and `RPC_E_CHANGED_MODE` is deliberately ignored (the thread already
///    having a different apartment is fine for this).

use base64::{engine::general_purpose::STANDARD, Engine};

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
    #[cfg(windows)]
    {
        let rgba = shell_icon_rgba(target, ICON_PX)?;
        let png = encode_rgba_as_png(&rgba, ICON_PX as u32, ICON_PX as u32)?;
        Some(STANDARD.encode(&png))
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        None
    }
}

/// Ask the shell for `target`'s icon and return straight RGBA8 pixels.
#[cfg(windows)]
fn shell_icon_rgba(target: &str, size: i32) -> Option<Vec<u8>> {
    use windows::core::{Interface, HSTRING, PCWSTR};
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDIBits, GetDC, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS,
    };
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
        SIIGBF_ICONONLY,
    };

    unsafe {
        // Ignore the result: RPC_E_CHANGED_MODE just means this thread is
        // already in a different apartment, which works fine here.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let wide = HSTRING::from(target);
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;

        // BIGGERSIZEOK: prefer a larger source over an upscaled small one.
        // ICONONLY: never substitute a document thumbnail for the app icon.
        let hbitmap = factory
            .GetImage(
                SIZE { cx: size, cy: size },
                SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
            )
            .ok()?;

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
            return None;
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

        Some(pixels)
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
