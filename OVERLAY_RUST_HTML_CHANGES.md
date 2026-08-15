# The exact non-TS edits that make the overlay work

Apply these to a V13 fork, alongside dropping in `toast.ts` and
`overlay-earthy.css`. Every one of these was necessary — each fixes a
specific defect named below.

---

## 1. `src-tauri/tauri.conf.json` — the overlay window must be TRANSPARENT

```json
{ "label": "overlay", "url": "overlay.html", "transparent": true,
  "decorations": false, "alwaysOnTop": true, "skipTaskbar": true,
  "resizable": false, "focus": false, "shadow": false, "visible": false,
  "width": 600, "height": 460 }
```

**Why:** V13 had `"transparent": false` because an earlier note claimed
transparency renders nothing here. That was true only for a **fullscreen**
transparent window. A **small, on-demand** one renders fine — verified by
sampling pixels (interior read R31,G31,B30, identical to the desktop behind
it). Keep the window small and on-demand; never size it to the full work area.

---

## 2. `src-tauri/src/lib.rs` — kill the 1px DWM border

In the overlay setup block, BEFORE `set_ignore_cursor_events`:

```rust
#[cfg(windows)]
if let Ok(hwnd) = overlay.hwnd() {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR,
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    };
    const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
    let pref = DWMWCP_DONOTROUND;
    let none = DWMWA_COLOR_NONE;
    let raw = windows::Win32::Foundation::HWND(hwnd.0 as *mut _);
    unsafe {
        let _ = DwmSetWindowAttribute(raw, DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const _, std::mem::size_of_val(&pref) as u32);
        let _ = DwmSetWindowAttribute(raw, DWMWA_BORDER_COLOR,
            &none as *const _ as *const _, std::mem::size_of_val(&none) as u32);
    }
}
```

**Why:** Windows 11 draws a border on undecorated windows regardless of
`decorations:false` / `shadow:false`. Measured at **R27 against an R32
desktop** — it was the faint rectangle around the toasts the user kept
reporting. `DONOTROUND` stops the compositor clipping the pills' own corners.

---

## 3. `src-tauri/src/commands.rs` — centre the HUD window on BOTH axes

Inside `overlay_fit_hud`, replace the bottom-anchored placement:

```rust
// BEFORE (V13): bottom-anchored panel
// let w = width.clamp(320.0, ms.width - 32.0);
// let h = height.clamp(120.0, ms.height - 120.0);
// ... y = mp.y + ms.height - h - 80.0

// AFTER: radial HUD is centred, and clamped to 94% — never fullscreen
let w = width.clamp(320.0, ms.width * 0.94);
let h = height.clamp(120.0, ms.height * 0.94);
let _ = win.set_size(tauri::LogicalSize::new(w, h));
let _ = win.set_position(tauri::LogicalPosition::new(
    mp.x + (ms.width - w) / 2.0,
    mp.y + (ms.height - h) / 2.0,
));
```

`overlay_fit` (used by toasts) stays bottom-centred — do not change it.

---

## 4. `src-tauri/src/guide_hud/mod_impl.rs` — centre before showing

Add, and call it instead of `place_overlay` in `show_guide_hud`:

```rust
fn place_overlay_centred(win: &tauri::WebviewWindow, w: f64, h: f64) {
    if let Ok(Some(mon)) = win.primary_monitor() {
        let sf = mon.scale_factor();
        let ms = mon.size().to_logical::<f64>(sf);
        let mp = mon.position().to_logical::<f64>(sf);
        let _ = win.set_size(tauri::LogicalSize::new(w, h));
        let _ = win.set_position(tauri::LogicalPosition::new(
            mp.x + (ms.width - w) / 2.0,
            mp.y + (ms.height - h) / 2.0,
        ));
    }
}
```

Then in `show_guide_hud`:
```rust
place_overlay_centred(&win, HUD_W, HUD_H);
let _ = win.set_always_on_top(true);   // re-assert on EVERY show
let _ = win.show();                    // show BEFORE content is emitted
```

**Why centred-before-show:** the frontend re-sizes via `overlay_fit_hud`
milliseconds later; without this the window first appears bottom-anchored and
visibly jumps.
**Why re-assert always-on-top:** other topmost windows that appeared since
the last show can otherwise sit above the HUD.

---

## 5. `overlay.html` — transparent page + link the stylesheet

```html
<link rel="stylesheet" href="/src/styles.css" />
<link rel="stylesheet" href="/src/styles/overlay-earthy.css" />
<style>
  html, body { margin:0; overflow:hidden; height:100%;
               background: transparent !important; }
  body { pointer-events: none; }
  /* backdrop-filter on this window is the white-box bug — keep it off */
  * { backdrop-filter: none !important; -webkit-backdrop-filter: none !important; }
</style>
```
Body must have **no background and no border**: each toast pill and the HUD
paint their own. A page-level `box-shadow: inset 0 0 0 1px …` (V13 had a
white "specular edge") traces the window rectangle and looks like a box.

---

## 6. `src-tauri/capabilities/default.json` — non-negotiable

```json
"windows": ["settings", "overlay"]
```
An unlisted window is DEAF: every `listen()` in it rejects silently and the
HUD renders as an empty box. This cost an earlier session hours.

---

## 7. Things NOT to do (each was tried and failed here)

- **Do not** shape the window with `SetWindowRgn` to get gaps between pills.
  It must be called on the window's own thread (`run_on_main_thread`) or it
  silently does nothing, and GDI regions have no antialiasing so rounded
  corners come out jagged. Transparency is the correct answer.
- **Do not** use `emit_to("overlay", …)`. Only global `emit` + a single
  listener (the overlay page) ever delivered.
- **Do not** emit content before `win.show()`; hidden webviews don't paint.
- **Do not** register the toast/HUD listeners in the dashboard as well, or
  every toast renders twice.

---

## 8. How to verify (do this, don't assume)

1. Build → install → run. Hold Space: chips must bloom outward from a
   centred terracotta SPACE pill, only for letters that have bindings.
2. Fire 3 toasts fast: newest biggest at the bottom, older ones shrinking
   upward, max 3, glow uncut on all sides.
3. If anything looks "cut" or "boxy", **sample the pixels** of a screenshot
   (`Bitmap.GetPixel` in PowerShell) rather than eyeballing a zoom — that is
   how the DWM border was identified as a 1px R27 line rather than a fill.
