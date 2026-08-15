# Windows Platform Behavior

What a Tauri app has to get right to feel native rather than like a web page in a frame.

## Window chrome

Custom titlebar with Mica:

```json
{
  "app": {
    "windows": [{
      "label": "main",
      "decorations": false,
      "transparent": true,
      "windowEffects": { "effects": ["mica"] },
      "width": 1100, "height": 720,
      "minWidth": 900, "minHeight": 600,
      "visible": false,
      "center": true
    }]
  }
}
```

```html
<div class="titlebar" data-tauri-drag-region>
  <span class="titlebar-title">App</span>
  <div class="titlebar-controls">
    <button data-win="min" aria-label="Minimize">&#xE921;</button>
    <button data-win="max" aria-label="Maximize">&#xE922;</button>
    <button data-win="close" aria-label="Close">&#xE8BB;</button>
  </div>
</div>
```

```css
.titlebar { height: 32px; display: flex; align-items: center; user-select: none; }
[data-tauri-drag-region] { app-region: drag; }
[data-tauri-drag-region] button { app-region: no-drag; }
.titlebar-controls button {
  width: 46px; height: 32px;      /* Windows standard caption button size */
  border: 0; background: transparent;
  font-family: "Segoe Fluent Icons", "Segoe MDL2 Assets";
  font-size: 10px;
}
.titlebar-controls button:hover { background: oklch(1 0 0 / 0.06); }
[data-win="close"]:hover { background: #c42b1c; color: #fff; }
```

The glyphs above are Segoe Fluent Icons codepoints — Windows 11 ships that font, so caption buttons match the system exactly with no SVG.

Caption buttons are **46×32 px** and sit flush to the top-right corner with no margin. Close hover is `#c42b1c`. These are the values Windows itself uses; deviating is immediately noticeable.

```ts
import { getCurrentWindow } from '@tauri-apps/api/window';
const w = getCurrentWindow();
document.querySelector('[data-win="min"]')!.addEventListener('click', () => w.minimize());
document.querySelector('[data-win="max"]')!.addEventListener('click', () => w.toggleMaximize());
document.querySelector('[data-win="close"]')!.addEventListener('click', () => w.close());
```

**Maximized state.** A borderless maximized window on Windows extends slightly past the work area, clipping ~8 px of content on each edge. Detect maximize and add compensating padding, or content near the edges gets cut.

## Window effects

| Effect | Support | Verdict |
|---|---|---|
| **Mica** | Windows 11 | Use this. No jank during drag or resize. Tints from the desktop wallpaper. |
| **Mica Alt** | Windows 11 | Stronger tint, for tabbed shells. |
| **Acrylic** | Windows 10 1809+ | Causes visible jank while dragging or resizing on Win10 1903+ and Win11. Avoid on the main window; acceptable on a transient flyout. |
| **Tabbed** | Windows 11 | For tab-bar shells. |
| **Blur** | Windows 7+ | Janky on Win11 22621. Legacy fallback only. |

Mica is meant for the **base layer of a long-lived window**. Applying it to a transient popup or a small overlay is against its design intent and it will look wrong — those want acrylic or a solid surface.

Mica requires `"transparent": true` and needs the app background to be actually transparent — a `background: #1a1a1a` on `body` will hide it completely. Let the effect show through and tint with low-alpha layers instead.

Runtime control from Rust:

```rust
use window_vibrancy::apply_mica;

#[cfg(target_os = "windows")]
apply_mica(&window, Some(true))?;   // Some(true) = dark
```

## DPI and multi-monitor

The most common real-world Windows bug. Users run mixed-DPI setups constantly — a 4K laptop panel at 150% next to a 1080p external at 100%.

- **Test at 100%, 125%, 150%, 175%, and 200%.** 125% and 150% are the most common and break layouts that assume integer pixels.
- **Never hardcode pixel positions from screen coordinates.** Tauri's `PhysicalPosition` and `LogicalPosition` differ by the scale factor; mixing them is what puts a window half off-screen.
- **Dragging between monitors changes the scale factor live.** Listen for `ScaleFactorChanged` and re-measure anything cached.
- **Positioning an overlay** requires the target monitor's work area, not the primary monitor's:

```rust
let monitor = window.current_monitor()?.ok_or(AppError::NoMonitor)?;
let scale = monitor.scale_factor();
let size = monitor.size();          // physical pixels
let pos = monitor.position();       // physical, and negative on left-of-primary displays
```

Monitor coordinates on Windows can be **negative** when a display sits left of or above the primary. Code that assumes `(0,0)` is the top-left of the desktop places overlays off-screen for those users.

CSS side: use `rem` and let the browser scale, avoid `px` in layout structure, and check `window.devicePixelRatio` only for canvas backing-store sizing.

```ts
const dpr = window.devicePixelRatio;
canvas.width = cssWidth * dpr;
canvas.height = cssHeight * dpr;
ctx.scale(dpr, dpr);
```

Skipping that makes every canvas and chart blurry on a scaled display.

## Theme and accent color

```ts
import { getCurrentWindow } from '@tauri-apps/api/window';

const theme = await getCurrentWindow().theme();      // 'light' | 'dark'
document.documentElement.dataset.theme = theme ?? 'dark';

await getCurrentWindow().onThemeChanged(({ payload }) => {
  document.documentElement.dataset.theme = payload;
});
```

CSS should also respect the media query, so the very first paint is correct before JS runs:

```css
:root { color-scheme: light dark; }
```

`color-scheme` makes native scrollbars, form controls, and the WebView's own background follow the theme. Omitting it is why an otherwise-dark app flashes white on load and has light scrollbars.

**Accent color** comes from the registry:

```rust
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

let key = RegKey::predef(HKEY_CURRENT_USER)
    .open_subkey(r"Software\Microsoft\Windows\DWM")?;
let argb: u32 = key.get_value("AccentColor")?;   // stored ABGR, not ARGB
let (r, g, b) = (argb & 0xFF, (argb >> 8) & 0xFF, (argb >> 16) & 0xFF);
```

The value is stored **ABGR**, which trips people up — read it in that order.

Whether to use the system accent is a product decision. System integration utilities should; branded apps should not.

## Tray and background apps

For a utility that lives in the tray:

- **Closing the window hides it; it does not exit.** Intercept the close request and hide instead. Provide an explicit Exit in the tray menu — an app with no way out is a support ticket.
- **Left click** opens or focuses the main window. **Right click** opens the menu. Windows users expect exactly this.
- Provide a **16×16 and 32×32** icon. Monochrome reads better in the tray and adapts to light and dark taskbars.
- `tauri-plugin-single-instance` — a second launch should focus the existing window.
- `tauri-plugin-autostart` for launch-at-login, and it must be **user-toggleable in settings**, defaulting off.

```rust
tauri::Builder::default()
    .on_window_event(|window, event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            if window.label() == "main" {
                api.prevent_close();
                let _ = window.hide();
            }
        }
    })
```

## Overlay and HUD windows

For a transient always-on-top overlay:

```json
{
  "label": "overlay",
  "decorations": false,
  "transparent": true,
  "alwaysOnTop": true,
  "skipTaskbar": true,
  "focus": false,
  "resizable": false,
  "shadow": false,
  "visible": false
}
```

`"focus": false` is the critical one. An overlay that steals focus breaks whatever the user was typing into — for a keyboard utility that is fatal.

Further Win32 hardening usually needed:

- `WS_EX_NOACTIVATE` so clicking the overlay never activates it
- `WS_EX_TRANSPARENT` for click-through when the overlay is purely informational
- `WS_EX_TOOLWINDOW` to keep it out of Alt+Tab

```rust
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

unsafe {
    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    SetWindowLongPtrW(hwnd, GWL_EXSTYLE,
        ex | WS_EX_NOACTIVATE.0 as isize | WS_EX_TOOLWINDOW.0 as isize);
}
```

**Create overlay windows once at startup and hide them**, rather than creating on demand. Window creation plus WebView2 initialization costs 100–300 ms, which is far too slow for a HUD that should appear after a 300 ms key hold. Pre-create, then `show()`/`hide()`.

Be aware this trades startup time and idle RAM for latency — see `performance-budget.md`.

## Focus and foreground

Windows deliberately restricts which processes may steal foreground. `SetForegroundWindow` silently fails when the calling process is not the foreground one, which is why "bring the app to front" works inconsistently.

The reliable sequence:

1. `ShowWindow(hwnd, SW_RESTORE)` if minimized
2. `AttachThreadInput` to attach to the current foreground thread
3. `SetForegroundWindow` / `BringWindowToTop`
4. `AttachThreadInput` to detach

Skipping the attach/detach is the usual cause of a window that flashes in the taskbar instead of coming forward. Always detach — leaving threads attached causes input queue problems that surface much later.

Cache `HWND` values, and **validate with `IsWindow(hwnd)` before every use**. Handles are recycled by the OS: a stale handle can point at a different window entirely, and acting on it makes an unrelated app misbehave.

## Keyboard hooks

For low-level input interception (`WH_KEYBOARD_LL`):

- The hook callback runs on the **thread that installed it**, and that thread must run a message pump. No pump means the hook silently stops firing.
- **Windows enforces a timeout** (`LowLevelHooksTimeout`, default 300 ms). Exceed it and the OS removes your hook with no notification — the app appears to stop working at random. Do nothing in the callback beyond reading the event and pushing it to a channel.
- Never allocate, lock a contended mutex, log to disk, or call into the webview from inside the callback.
- Hooks require the process to be at the same or higher integrity level than the target. An elevated app's input is invisible to a non-elevated hook.
- UIPI blocks synthetic input to elevated windows regardless.

```rust
// Callback: read and forward only.
unsafe extern "system" fn hook_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = &*(l.0 as *const KBDLLHOOKSTRUCT);
        let _ = TX.get().unwrap().try_send(KeyEvent::from(kb));  // non-blocking
        if should_swallow(kb) { return LRESULT(1); }
    }
    CallNextHookEx(None, code, w, l)
}
```

`try_send` rather than `send` — a full channel must drop an event, never block the hook thread.

## File paths

Never construct paths by hand.

```rust
let dir = app.path().app_config_dir()?;     // %APPDATA%\<identifier>
let data = app.path().app_data_dir()?;
let cache = app.path().app_cache_dir()?;
```

Windows-specific traps: `MAX_PATH` is 260 characters unless long paths are enabled; paths are case-insensitive but case-preserving; and `CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9` are reserved filenames that will fail to create.

## Installer and updates

- **MSI** (WiX) for enterprise deployment and Group Policy; **NSIS** for a lighter per-user install with more control over the UI. Shipping both is common.
- **Sign the binary.** Unsigned installers trigger SmartScreen, and users will not click through. An OV certificate builds reputation over time; EV bypasses the reputation period immediately.
- `webviewInstallMode: "downloadBootstrapper"` keeps the installer small but needs network at install time. `embedBootstrapper` or `offlineInstaller` for machines that may be offline — the last adds roughly 130 MB.
- `tauri-plugin-updater` requires a keypair; keep the private key out of the repo.
- Windows 11 ships WebView2; Windows 10 versions may not, which is what the bootstrapper handles.

## Accessibility

- Windows exposes reduced motion via Settings → Accessibility → Visual effects. `prefers-reduced-motion` reflects it.
- High contrast mode maps to `prefers-contrast: more` and `forced-colors: active`. Under forced colors the OS overrides your palette — test that the app stays usable rather than fighting it.
- Screen readers (Narrator, NVDA) read the webview through standard ARIA. Semantic HTML gets most of this; custom `div`-based controls get none of it without explicit roles.
- Every interactive element needs a visible focus state, and the tab order must follow visual order.
