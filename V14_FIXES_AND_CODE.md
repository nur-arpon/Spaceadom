# V14 — every problem, and the exact code that solved it

**Written 2026-08-11 by Claude Opus 5 (Claude Code).**

Purpose: an AI picking this up should not have to re-diagnose anything below,
and should not have to hunt for where the fix lives. Each entry is
**symptom → root cause → the exact file, the exact code, how it was verified.**

Companion docs: `PROJECT_STATUS.md` (chronological log), `WHAT_HAPPENED.md`
(plain English), `V13_TO_V14_METHOD.md` (why two earlier attempts failed),
`OVERLAY_ACHIEVED.md` + `OVERLAY_RUST_HTML_CHANGES.md` (the overlay).

---

## FEATURE 185 — App exceptions: Spaceadom stands down inside chosen apps

**Symptom.** Hold-Space canvas panning does nothing in Photoshop / Figma /
Blender while Spaceadom runs. Measured 2026-08-25: the target app never receives
a Space keydown.

**Root cause.** By design. `kb_hook_proc` suppresses Space-down globally so it
can decide tap-vs-hold; there was no way to say "not in this app".

**Exact files.**

`src-tauri/src/config/schema.rs`
```rust
    /// Apps Spaceadom stands down inside — the owner’s "exception list".
    /// Stored as LOWERCASE EXE STEMS ("photoshop").
    /// `#[serde(default)]` is load-bearing: every config written before 1.0.79
    /// lacks the field, and without it they all fail to deserialise.
    #[serde(default)]
    pub excluded_apps: Vec<String>,
```

`src-tauri/src/hook/mod.rs` — the atomic, the counter and the gate:
```rust
pub static EXCLUDED_ACTIVE: AtomicBool = AtomicBool::new(false);
pub static SUPPRESS_EXCLUDED: AtomicU32 = AtomicU32::new(0);

    // --- App exceptions: pass everything through immediately ---
    // Note what is NOT here: the Space + . bypass escape hatch that the bypass
    // branch below keeps. Full stock behaviour means full stock behaviour.
    if EXCLUDED_ACTIVE.load(Ordering::Relaxed) {
        if is_down {
            SUPPRESS_EXCLUDED.fetch_add(1, Ordering::Relaxed);
        }
        return CallNextHookEx(None, n_code, w_param, l_param);
    }
```
The same two-line gate is in `ms_hook_proc`, placed AFTER the `LAST_MS_EVENT`
liveness stamp and BEFORE the `MODIFIER_ACTIVE` early-return — `MODIFIER_ACTIVE`
can still be true from a Space held just before the app switch, and without it
the first scroll inside an excluded app would still be swallowed.

`src-tauri/src/hook/exclusions.rs` (new) — the poller. Pure, tested core:
```rust
pub fn normalize_stem(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches('"');
    // Split on BOTH separators by hand rather than using `Path::file_name`:
    // on a non-Windows build `Path` does not treat `\` as a separator.
    let name = trimmed.rsplit(|c| c == '\\' || c == '/').next().unwrap_or(trimmed);
    let lower = name.to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

pub fn is_excluded(foreground: &str, list: &[String]) -> bool {
    let fg = normalize_stem(foreground);
    if fg.is_empty() { return false; }   // "could not read it", NOT "matches"
    list.iter().any(|e| normalize_stem(e) == fg)
}
```

Published from BOTH ends (PROBLEM 180): `config/mod.rs::save` and `lib.rs`
startup both call `hook::exclusions::publish_excluded_apps(cfg)`, and
`lib.rs` setup step 8b calls `hook::exclusions::start_exclusion_watcher()`.

**Generalise this.** *A per-app verdict belongs in a poller, never in the hook.*
Any question of the form "which window is in front" is a win32k call, and a
win32k call in a `WH_KEYBOARD_LL` callback is how the hook gets evicted
(PROBLEM 58/134/184). The pattern is now used twice — fullscreen and exclusions —
and both fail toward "the app keeps working" when the probe panics.

**Frontend.** `src/components/app-grid.ts` (new leaf, PROBLEM 148) holds the
tiles, the icon `onerror` fallback and PROBLEM 97’s truncation notice.
`key-detail-panel.ts` and `settings-panel.ts` both call `drawAppGrid()`. It was
extracted rather than copied on purpose: two copies of a list that has already
needed two separate fixes is how the next fix reaches only one of them.
`exeStem()` there mirrors `normalize_stem` exactly, so both sides of the config
agree without a translation step.

**How it was verified.** `npx tsc --noEmit` and `npm run build` clean;
`cargo check --lib --all-targets` 0 errors 0 warnings; `cargo test --lib`
25 passed including `hook::exclusions::tests::stem_normalisation_accepts_every_form`
and `matching_is_case_and_form_insensitive`. Behaviour in Photoshop is NOT yet
hand-verified on the real machine.

---

## PROBLEM 30 — inherited Store-app (AUMID) code had never compiled

**Symptom.** First `cargo check` after carrying `smart_cascade.rs` over from
attempt #2:

```
src\engine\actions\smart_cascade.rs:292:36: error[E0432]: unresolved import
  `windows::Win32::UI::Shell::PropertiesSystem`: could not find
  `PropertiesSystem` in `Shell`      (x2)
```

**Root cause.** The `windows` crate is feature-gated per API path. The code
uses `SHGetPropertyStoreForWindow` / `IPropertyStore` / `PROPERTYKEY` to read
`PKEY_AppUserModel_ID` off a window (the only way to match Store/UWP windows,
whose HWNDs belong to `ApplicationFrameHost.exe`, so exe-name matching can
never find them). Those live behind `Win32_UI_Shell_PropertiesSystem`, which
was **named in `OVERLAY_RUST_HTML_CHANGES.md` but never added to
`Cargo.toml`**. Its author labelled the code "written but never verified" — it
had in fact never built.

**Fix — `src-tauri/Cargo.toml`**, in `[target.'cfg(windows)'.dependencies.windows]`
`features`:

```toml
  # Shell execute (runas elevation)
  "Win32_UI_Shell_Common",
  # SHGetPropertyStoreForWindow + PKEY_AppUserModel_ID — how Store/UWP
  # windows are matched. Their windows belong to host processes
  # (ApplicationFrameHost), so exe-name matching can never find them.
  "Win32_UI_Shell_PropertiesSystem",
  "Win32_System_Variant",
```

**Verified.** `cargo check` and `cargo build --release` → 0 errors, 0 warnings.

**Generalise this.** A missing `windows` feature reads as `unresolved import`
on a path that plainly exists in the docs. Inherited code that names features
in prose has not necessarily had them added to `Cargo.toml`. And
"unverified at runtime" ≠ "does not compile" — check which one you inherited.

---

## PROBLEM 31 — the board's design width is 1048, not 1046

**Symptom.** None visible yet — caught before it shipped.

**Root cause.** 16 units at `U=56, G=10` is `16*56 + 15*10 = 1046`. But each
key's width is rounded individually:
`Math.round(units * U + (units - 1) * GAP)`. The fractional keys (1.5, 1.75,
2.25) each round up, adding **2px per row**. Measured in a live page: every
row renders at exactly **1048**. A 2px underestimate lets the board overflow
its container at the exact size where it only just fits.

**Fix — `src/components/keyboard-matrix.ts`:**

```ts
// Board geometry (mockup: U=56, G=10).
// DESIGN_W is 1048, not the 1046 that 16 clean units would give: the
// fractional-unit keys (1.5/1.75/2.25) are rounded to whole pixels
// individually, and those roundings add 2px per row. Measured, not assumed —
// every row renders at exactly 1048. The mockup uses 1048 for the same reason.
const U = 56;
const GAP = 10;
export const DESIGN_W = 1048;
export const DESIGN_H = 5 * U + 4 * GAP;     // 320
```

**Verified.** In-page: `matrix.offsetWidth === 1048`, all five
`.kb-row` widths equal 1048.

---

## PROBLEM 32a — a start-hidden popover rendered OPEN on launch

**Symptom.** On first run the profile popover was visible over the keyboard
before any click (see the first V14 screenshot).

**Root cause.** CSS specificity. The shared rule is a class + attribute:

```css
.popover[hidden] { display: none; }        /* (0,2,0) */
```

but the popover's own rule is an **id**:

```css
#profile-popover { display: flex; ... }    /* (1,0,0) — WINS */
```

`(1,0,0)` beats `(0,2,0)`, so `display:flex` won and the `hidden` attribute
did nothing.

**Fix — `src/styles.css`**, immediately after the `#profile-popover` block:

```css
/* MUST be an ID selector, not the shared `.popover[hidden]` rule: an id
   (1,0,0) outranks a class+attribute (0,2,0), so `display:flex` above wins
   and the popover renders open on launch. Every element that sets `display`
   in an ID rule needs its own `[hidden]` companion — see #specials-tray and
   #new-profile-row below. */
#profile-popover[hidden] { display: none; }
```

**Rule to apply going forward:** any element that sets `display` inside an
**ID** rule needs its own `#id[hidden] { display: none; }`. Currently that is
`#profile-popover`, `#specials-tray`, `#new-profile-row`, `#key-detail-panel`,
`#editor-backdrop`. Class-based ones are fine (`.dashed-btn[hidden]` is
`(0,2,0)` vs `.dashed-btn` `(0,1,0)`).

**Verified.** Audited every start-hidden element in a live page; all six
compute `display: none`. Then confirmed on the real app after a rebuild.

---

## PROBLEM 32b — I nearly reported a window-placement bug that did not exist

**Symptom.** `GetWindowRect` said the window was at x=1949 — off the primary
display — and 1236x919 in size.

**Root causes, both in the measuring tool, not the app:**
1. **The user had dragged the window** to the second monitor. I was measuring
   a window a human had moved.
2. **The PowerShell doing the measuring was DPI-unaware**, so Windows fed it
   virtualised coordinates. It reported the secondary display as 1707x1067
   when it is really **2560x1600 @150%**, and the window as 1236x919 when it
   was really 2582x1574.

**Fix — call this BEFORE any window/monitor query in a diagnostic script:**

```powershell
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public class Dpi {
 [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
}
'@
# PER_MONITOR_AWARE_V2 = -4
[void][Dpi]::SetProcessDpiAwarenessContext([IntPtr](-4))
```

**This machine's real display layout** (CLAUDE.md's "single 2560x1440
monitor" is out of date):

| Display | Primary | Bounds | Scale |
| --- | --- | --- | --- |
| `\\.\DISPLAY3` | **yes** | 1920x1080 at (0,0) | 100% |
| `\\.\DISPLAY1` | no | 2560x1600 at (1920,-97) | 150% (DPI 144) |

**Consequence for the app — `src-tauri/src/lib.rs`, setup step 9c.** Use
`current_monitor()`, not `primary_monitor()`, and **read back what actually
happened**:

```rust
// current_monitor, NOT primary_monitor: this machine has a 1920x1080
// primary and a 2560x1600 @150% secondary, and Windows may open the window
// on either. Fitting it to the monitor it is ACTUALLY on is the only
// version that is right in both cases. (The Guide HUD stays
// primary-monitor-only — a separate, explicit user decision.)
let mon = win.current_monitor().ok().flatten()
    .or_else(|| win.primary_monitor().ok().flatten());
if let Some(mon) = mon {
    let sf = mon.scale_factor();
    let ms = mon.size().to_logical::<f64>(sf);
    let mp = mon.position().to_logical::<f64>(sf);
    let w = 1220.0_f64.min(ms.width  * 0.92);
    let h =  880.0_f64.min(ms.height * 0.92);
    let _ = win.set_size(tauri::LogicalSize::new(w, h));
    let _ = win.set_position(tauri::LogicalPosition::new(
        mp.x + (ms.width - w) / 2.0,
        mp.y + (ms.height - h) / 2.0,
    ));

    // READ BACK what actually happened. A set_size/set_position that
    // silently does not stick looks identical in the log to one that
    // worked, and this window has already shipped once at the wrong size.
    let got_sz = win.outer_size().map(|s| s.to_logical::<f64>(sf));
    let got_ps = win.outer_position().map(|p| p.to_logical::<f64>(sf));
    log::info!("setup: dashboard asked for {w:.0}x{h:.0} @ ({:.0},{:.0}) on a \
                {:.0}x{:.0} monitor (scale {sf}); got size {:?} pos {:?}", ...);
}
```

**Verified.** Fresh launch logs
`asked for 1220x880 @ (350,100) … got size Ok((1236.0, 919.0)) pos Ok((350.0, 100.0))`
— 1236x919 is the OUTER size (client 1220x880 + title bar + resize borders),
which is correct. DPI-aware measurement independently confirmed 342,100 on the
primary.

---

## PROBLEM 33 — running a dev build silently hijacked the user's startup entry

**Symptom.** After testing from the repo, the HKCU Run entry `SpaceToggleV14`
pointed at
`D:\Claude-Projects\SpaceToggle-V14\src-tauri\target\release\space-toggle-v14.exe`
instead of the installed copy. A path inside a build directory that
`cargo clean` deletes — after which the app "stops starting on boot" with no
visible cause.

**Root cause — old `src-tauri/src/startup.rs`:** `register_startup()` wrote
`current_exe()` into the Run key on **every launch, unconditionally**, with no
check of what was already there.

**Fix — `src-tauri/src/startup.rs`.** Two rules: a dev build never overwrites
a valid existing entry, and a write only happens on a real change.

```rust
#[cfg(windows)]
const RUN_VALUE: &str = "SpaceToggleV14";
#[cfg(windows)]
const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";

#[cfg(windows)]
fn is_dev_build(exe: &std::path::Path) -> bool {
    let p = exe.to_string_lossy().to_ascii_lowercase();
    p.contains(r"\target\release\") || p.contains(r"\target\debug\")
}

pub fn register_startup() {
    #[cfg(windows)]
    {
        let exe_path = match std::env::current_exe() { Ok(p) => p, Err(e) => {
            log::error!("startup: cannot read current exe path: {e}"); return } };
        let exe_str = exe_path.to_string_lossy().to_string();

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = match hkcu.open_subkey_with_flags(
            RUN_KEY,
            winreg::enums::KEY_SET_VALUE | winreg::enums::KEY_QUERY_VALUE,
        ) { Ok(k) => k, Err(e) => {
            log::error!("startup: cannot open Run key: {e}"); return } };

        let existing: Option<String> = key.get_value(RUN_VALUE).ok();

        // Rule 1 — a dev build defers to any existing, still-valid entry.
        if is_dev_build(&exe_path) {
            if let Some(ref cur) = existing {
                if std::path::Path::new(cur.trim_matches('"')).exists() {
                    log::info!("startup: dev build — LEAVING the existing startup entry alone (→ {cur})");
                    return;
                }
            }
            log::warn!("startup: dev build and no valid existing entry — registering the build path ({exe_str}); install the MSI and launch it once to point this at Program Files");
        }

        // Rule 2 — write only on a real change.
        if existing.as_deref() == Some(exe_str.as_str()) {
            log::info!("startup: entry already correct (→ {exe_str})");
            return;
        }

        match key.set_value(RUN_VALUE, &exe_str) {
            Ok(()) => log::info!("startup: registered startup key → {exe_str} (was: {:?})",
                                 existing.as_deref().unwrap_or("<unset>")),
            Err(e) => log::error!("startup: failed to write Run key: {e}"),
        }
    }
}
```

**How it was verified (and how the FIRST verification was invalid).**
First attempt: ran the repo build, saw the Run key unchanged, nearly declared
success. But the log had **no new lines at all** — the app never initialised
(UAC prompt not approved). The key was unchanged because nothing ran.
Correct test — poll the log until it grows, proving the app actually started:

```powershell
$linesBefore = (Get-Content $log | Measure-Object -Line).Lines
Start-Process $repoBuildExe
for ($i=0; $i -lt 45; $i++) { Start-Sleep 1
  if ((Get-Content $log | Measure-Object -Line).Lines -gt $linesBefore) { $started=$true; break } }
```
Result at 03:17:03 —
`startup: dev build — LEAVING the existing startup entry alone (→ C:\Program Files\SpaceToggle V14\space-toggle-v14.exe)`
and the Run key unchanged. **Now** it is verified.

---

## PROBLEM 34 — app icons have NEVER rendered, in any version, because of CSP

**Symptom.** Every tile in the editor's "Apps on this device" grid showed the
browser's broken-image glyph. The user: *"it shows folder images but not the
app icons."*

**Root cause.** `src-tauri/tauri.conf.json`:

```json
"csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self'"
```

There is no `img-src`, so images fall back to `default-src 'self'` — and
`'self'` does **not** include the `data:` scheme. Every
`<img src="data:image/png;base64,…">` was blocked at the CSP layer, silently,
with no console visible in production.

**This is NOT a V14 regression.** V13 has a byte-identical CSP and V13's
`app-picker.ts` built the same `data:image/png;base64,` URLs. The icons were
broken there too.

**Why it was believed fixed.** PROJECT_STATUS 2026-08-10 records the icon
extractor as "FIXED, verified visually" — and that verification was real, but
it verified the *wrong layer*: a smoke test wrote real PNGs to
`%TEMP%\spacetoggle-icon-test\` and those files were opened and looked at.
The extractor was genuinely fixed. **The rendering path was never tested**,
and that is where the failure was. Proving a component correct is not proving
the feature works.

**Fix — `src-tauri/tauri.conf.json`:**

```json
"csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self'; img-src 'self' data:"
```

**Defence in depth — `src/components/key-detail-panel.ts`**, so a future
failure degrades to the letter disc instead of a torn-paper glyph:

```ts
const letterFallback = () => {
  disc.innerHTML = "";
  disc.style.background = DISC_COLORS[i % DISC_COLORS.length];
  disc.textContent = (app.name[0] || "?").toUpperCase();
};
if (app.icon_base64) {
  const img = document.createElement("img");
  img.onerror = letterFallback;          // <-- CSP block also fires this
  img.src = `data:image/png;base64,${app.icon_base64}`;
  img.alt = "";
  disc.appendChild(img);
} else { letterFallback(); }
```

**Generalise:** any new scheme in a `src` or `url()` — `data:`, `blob:`,
`asset:` — needs an explicit CSP directive here. The failure mode is a silent
non-render, not an error.

---

## PROBLEM 35 — the HUD's glow sat BENEATH the SPACE pill

**Symptom.** User, on seeing the working HUD: *"the background glowy type
thing … was supposed to be around SPACE the writing space, but it's beneath,
so it looks bad."* Present in both palettes.

**Root cause — two separate things.**

1. **A leaked toast glow.** `#st-toastglow` is created by `toastLayer()` and
   is deliberately **bottom-anchored** (`position:fixed; bottom:8px`) because
   it belongs behind the toast stack. It was only ever hidden here:

   ```ts
   if (_toasts.length === 0 && !_hudActive) {      // <-- the bug
     const g = document.getElementById("st-toastglow");
     if (g) g.style.opacity = "0";
     invoke("overlay_toasts_done").catch(() => {});
   }
   ```

   If the last toast expired **while Space was held**, `_hudActive` was true,
   so the branch was skipped and the glow stayed at `opacity:1` forever. Every
   subsequent HUD then showed a warm smear at the bottom of the overlay
   window — i.e. under the SPACE pill.

2. **The HUD had no glow of its own.** Only `.pulse` (a one-shot expanding
   ring) and `.space`. So there was nothing correct to see even once the
   stray one was gone.

**Fix 1 — `src/components/toast.ts`.** Separate the two concerns: hiding the
glow is about the toast stack, telling Rust the window is free is about the
HUD.

```ts
/** The toast glow is bottom-anchored and belongs to the toast stack ONLY.
 *  The HUD has its own centred glow (#st-hud .glow). Never let this one be
 *  visible while the HUD is up — it renders below the SPACE pill. */
function hideToastGlow(): void {
  const g = document.getElementById("st-toastglow");
  if (g) g.style.opacity = "0";
}

// …in the toast-removal timeout:
if (_toasts.length === 0) {
  hideToastGlow();                                     // ALWAYS
  if (!_hudActive) invoke("overlay_toasts_done").catch(() => {});
}

// …and defensively in showGuideHud():
if (_toasts.length === 0) hideToastGlow();
```

**Fix 2 — give the HUD its own centred glow.** `buildHud()`, `.glow` FIRST so
DOM order paints it behind everything:

```ts
_hudEl.innerHTML =
  '<div class="glow"></div><div class="pulse"></div><div class="space">SPACE</div>';
```

`src/styles/overlay-earthy.css`:

```css
#st-hud .glow { position: absolute; left: 50%; top: 50%;
  transform: translate(-50%, -50%);
  width: 560px; height: 320px; border-radius: 50%; pointer-events: none;
  /* No z-index: it is the FIRST child, and every sibling is auto-z, so DOM
     order alone puts it behind the ring, the pill and the chips. */
  filter: blur(34px);
  background: radial-gradient(ellipse,
              rgba(var(--st-glow-rgb), .34) 0%,
              rgba(var(--st-glow-rgb), .14) 45%, transparent 70%);
  animation: st-hud-glow 4s ease-in-out infinite; }

@keyframes st-hud-glow {
  0%, 100% { opacity: .55; transform: translate(-50%, -50%) scale(1); }
  50%      { opacity: .9;  transform: translate(-50%, -50%) scale(1.06); }
}
```

`--st-glow-rgb` is already palette-aware (terracotta in Earthy, indigo in
Nocturne), so this is correct in both themes with no extra rule.

**Window size check:** the glow is 560x320 plus a 34px blur ≈ 628x388 centred.
The HUD window is at least `360 + PAD(180) = 540` on each axis and normally
much larger, so it cannot clip. If the glow is ever enlarged, raise `PAD` in
`buildHud()` with it — the window box must always exceed the glow, the same
rule that fixed the toast glow being "cut" (OVERLAY_ACHIEVED §4.4).

---

## PROBLEM 37 — a blurred glow made the WHOLE overlay window compose zero pixels

**The single most expensive mistake of this session. Read this before touching
anything visual in the overlay window.**

**Symptom.** After the PROBLEM 35 "fix", **neither the Guide HUD nor the
toasts appeared at all**. Both had worked minutes earlier. No error anywhere.

**What made it hard.** Every check said the code was fine:

| Check | Result |
| --- | --- |
| Built overlay CSS | all rules present, braces balanced |
| Built overlay JS | all four listeners, all markup present |
| `overlay-js: listeners registered OK` | logged at every startup |
| `guide_hud: overlay window shown` | logged on every Space hold |
| Any JS exception | none — `window.onerror`/`unhandledrejection` bridge silent |

**What broke the deadlock: instrumentation.** `overlay_fit` and
`overlay_fit_hud` logged **nothing at all**, so a wrong size, a wrong
position, and a window that never moved were indistinguishable. Adding a
readback log to both produced the answer in one reproduction:

```
overlay_fit_hud: asked 1194x572 → clamped 1194x572 @ (256,247);
  monitor 1707x1067 at (0,0) scale 1.5;
  GOT size Ok((1194.0, 572.0)) pos Ok((257.0, 247.0)); visible Ok(true)
```

Correct size, correct centring, `visible = true`, and the frontend had run all
the way through `buildHud()` to that invoke. Everything worked **except that
the window painted nothing.**

**Root cause.** The PROBLEM 35 change added a HUD backlight:

```css
#st-hud .glow { width: 560px; height: 320px; filter: blur(34px); … }
```

A ~4x larger blurred surface than the existing toast glow (340x150, blur
22px). On this machine that tips WebView2 into composing **zero pixels for the
entire transparent window** — not just the glow. This is the same failure
already recorded in `OVERLAY_ACHIEVED.md` §2.1 and V13 PROBLEM 14, previously
only ever seen by making the window fullscreen. **A large blurred surface is a
second way to trigger it.**

**Fix.** Remove the element and the CSS rule entirely; keep the removal note
in `overlay-earthy.css` where the rule used to be, so the next person finds it
before re-adding one.

```ts
// toast.ts, buildHud() — back to the documented markup
_hudEl.innerHTML = '<div class="pulse"></div><div class="space">SPACE</div>';
```

**The glow was never needed.** The user's report was *"the glow is beneath
SPACE"* — that was the bottom-anchored **toast** glow leaking (PROBLEM 35's
real cause). Fixing the leak removes the misplaced glow. Adding a second glow
was an unrequested embellishment, and it is the part that broke everything.

**Three rules that come out of this:**

1. **Fix only what was reported.** The leak fix alone solved it. Everything
   after that was volunteered risk on a window documented as fragile.
2. **Deviating from a documented-working configuration needs a reason.** Two
   deviations shipped together here: this glow, and swapping
   `overlay.html`'s `/src/styles.css` link for `design-system.css` (against
   `OVERLAY_RUST_HTML_CHANGES.md` §5). Both reverted. When a config is
   recorded as working, match it — saving a few unused CSS rules is not a
   reason.
3. **On the transparent overlay: no `filter: blur()` beyond the size already
   proven** (340x150 / 22px), and never `backdrop-filter`. If a backlight is
   wanted, bake the softness into the gradient stops instead. And verify by
   holding Space **before** shipping — this surface cannot be checked in a
   browser harness, because the failure is in the OS compositor, not the page.

**Also fixed here — the silence that caused the round trip.** `overlay_fit`
and `overlay_fit_hud` now log the request, the monitor they computed against,
and what the window actually became (`outer_size`/`outer_position`/
`is_visible`). Both are marked "never remove" in the source. Without them this
was undiagnosable from the outside.

---

## PROBLEM 38 — Store apps: exact AUMID matching only worked for some apps

**Symptom.** Samsung **Notes** minimised correctly; Samsung **Gallery**
relaunched every press. Reported as "you broke Store apps", but the log shows
both behaviours from the same unchanged code — Notes was simply the app tested
first.

**Root cause.** `aumid_enum_cb` compared with exact string equality:

```rust
if window_aumid == payload.aumid { … }
```

Windows does not guarantee a packaged app's **window** reports the same AUMID
that **launched** it. The app-id after `!` is chosen by the app; apps with
several entry points launch as `…!App` while their window reports something
else. Confirmed from the diagnostic log — the launch target was
`…PCGallery_3c1yjt4zspk6g!App`, and the packaged windows actually on screen
were:

```
["samsungelectronicscoltd.samsungnotes_wyx1vj98g3asy!app",
 "microsoft.office.onenote.memorypreview", "brave.userdata.profile1",
 "brave", "msedge",
 "windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel"]
```

Note that several report **no `!` at all** (`brave`, `msedge`), which is why
the family-name comparison must tolerate a missing separator.

**Fix — `src-tauri/src/engine/actions/smart_cascade.rs`:**

```rust
/// The package family name — everything before the `!` in an AUMID.
/// Stable per package; the app-id after `!` is not.
fn aumid_family(s: &str) -> &str { s.split('!').next().unwrap_or(s) }

// …in aumid_enum_cb:
let matched = window_aumid == payload.aumid
    || (aumid_family(&window_aumid) == aumid_family(&payload.aumid)
        && !aumid_family(&payload.aumid).is_empty());
```

Family names are unique per package, so this cannot collide across apps.

**And the diagnostic that made it a one-look fix** — `SearchPayload` gained a
`seen: Vec<String>`, and the no-match branch was promoted from `log::debug!`
(filtered out of the shipped log, i.e. invisible exactly when needed) to
`log::info!`:

```rust
log::info!(
    "aumid_focus: no window matched AUMID {:?} (family {:?}). Packaged windows seen: {:?}",
    shell_target, aumid_family(&payload.aumid), payload.seen,
);
```

**Generalise:** a diagnostic at `debug!` level is a diagnostic that does not
exist in production. If a line explains *why* a feature silently did nothing,
it belongs at `info!`.

**Status: written, NOT verified.** Needs Space+W twice on Gallery.

---

## PROBLEM 39 — press feedback existed only on the bindable letters

**Symptom.** User: the keyboard used to be "very satisfying to tap" — hover
motion and a circular ripple on EVERY key, including unassignable ones — "but
the one you made, the click feels nothing."

**Root cause — a fidelity regression in my own port, in two halves:**

1. **CSS.** The hover-lift and press-shrink were written as
   `.key.bindable:hover` / `.key.bindable:active`, so only the 26 letters
   reacted. The mockup (`Dashboard Earthy v2.dc.html` line 72) puts
   `style-hover` / `style-active` and a press handler **on every key in the
   board** — Tab, Shift, arrows, SPACE, all of them.
2. **JS.** The ripple was spawned inside main.ts's *select* callback, which
   only letters trigger. The mockup's `pressKey(label, e)` fires the ripple
   and the 520Hz tick for ANY label, and only *additionally* opens the editor
   when the label is a letter (`/[a-z]/i`), after a 90ms delay so the ripple
   is seen first.

**Fix, part 1 — `src/styles.css`:** move the feedback to the base class.

```css
.key { …; cursor: pointer; }          /* was on .key.bindable only */
.key:hover {                          /* was .key.bindable:hover */
  transform: translateY(-4px);
  box-shadow: 0 14px 26px rgba(90, 60, 30, .22);
  border-color: var(--st-accent);
}
body.nocturne .key:hover { box-shadow: 0 14px 26px rgba(0, 0, 0, .45); }
.key:active { transform: translateY(0) scale(.94); }
```

**Fix, part 2 — `src/components/keyboard-matrix.ts`:** the ripple and tick
moved INTO the matrix, attached to every cell in `createKeyCell()`:

```ts
cell.addEventListener("click", () => { spawnRipple(cell); keyBeep(); });
```

`spawnRipple(cell)` (relocated verbatim from main.ts — 130px ring, 2px
terracotta border, centred on the key, `st-ripple` 520ms, reduced-motion
respected) and `keyBeep()` (520Hz sine, gain .05, 90ms decay — the mockup's
`beep(520)`) both live in keyboard-matrix.ts now. Sound is fed by main.ts:

```ts
// keyboard-matrix.ts
export function setKeyboardSound(on: boolean): void { _soundOn = on; }
// main.ts applySound()
setKeyboardSound(on);   // alongside the overlay's "sound-changed" event
```

main.ts's select callback no longer spawns ripples at all — it only opens the
editor on the mockup's 90ms delay. Letters therefore have two click listeners
(ripple first, select second — registration order), same sequencing as the
mockup.

**Verified** in the preview harness (this surface is a plain DOM, so the
browser check is valid — unlike the overlay): clicking Tab, Shift and SPACE
spawned one ripple each, a letter still spawns one, computed ripple is
130x130px with a 2px `rgb(198,113,57)` border, `cursor: pointer` on all keys,
and the old `.key.bindable:hover` rule is gone from the sheet.

**Generalise:** when a mockup attaches behaviour to every element in a
collection, restricting it to the "functional" subset is not an optimisation,
it is a fidelity bug. The dead keys' feedback IS the feature.

---

## PROBLEM 40 — the intro animation permanently killed ALL hover/press motion

**Symptom.** User, twice in one session: keys don't visually depress when
clicked ("the pressed key feel which makes the key go a bit down" is missing),
and then "I am moving my cursor around the keys but no response, no motion
graphics." The `:hover` and `:active` rules were present and correct in the
stylesheet — and did nothing.

**Root cause — CSS animation fill-mode precedence.** The keyboard cascade was
applied as:

```ts
cell.style.animation = `st-key-in 560ms var(--ease-spring) both`;   // BUG
```

and **never removed**. Per the CSS cascade, a running OR filling animation
owns its animated properties at animation-level precedence, which beats
normal author declarations — including `:hover`, `:active`, and even inline
`style.transform`. With `fill-mode: both`, a *finished* animation keeps its
final keyframe applied **forever**. `st-key-in`'s final keyframe is
`translateY(0) scale(1)`, so every key's transform was pinned to identity for
the life of the page:

- `:hover  { transform: translateY(-4px) }` → overridden, dead
- `:active { transform: translateY(0) scale(.94) }` → overridden, dead
- hover box-shadow/border-color still worked (not keyframe properties),
  which made the board look "half alive" and the bug look like a feel issue
  rather than a mechanical one.

**The mockup does not have this bug because it removes the animation.**
`Dashboard Earthy v2.dc.html` line 264-266: once `introDone` fires (1700ms),
the re-render sets the animation string to `""`. That removal — not just the
animation itself — is part of the design, and porting the animation without
the removal is what broke it.

**Fix — two mechanisms, both in place:**

1. Structural (`keyboard-matrix.ts`, `createKeyCell`): fill `backwards`, not
   `both`. `backwards` hides the key during its stagger delay (required) and
   RELEASES the transform channel when the animation ends. The final keyframe
   equals the natural state, so the cascade looks identical.

```ts
cell.style.animation = `st-key-in 560ms var(--ease-spring) backwards`;
cell.style.animationDelay = `${ri * 55 + ci * 16}ms`;
```

2. The mockup's own mechanism (`initKeyboardMatrix`): strip the animation
   outright when the cascade ends.

```ts
window.setTimeout(() => {
  _cascadeDone = true;
  container.querySelectorAll<HTMLDivElement>(".key").forEach((c) => {
    c.style.animation = "";
    c.style.animationDelay = "";
  });
}, 1700);
```

**Same bug, second instance:** `.ed-tile` in `styles.css` had
`animation: st-pop-in 380ms var(--ease-spring) both;` at class level AND a
hover transform — its lift was dead the same way. Changed to `backwards`.

**Audit of every other animated element:** `.popover`, `.profile-row`,
`.dashed-btn`, `.ed-cap`, `#ed-search`, `.special-item`, `.set-row` keep
`both` safely — their hover states change background/border/shadow only,
none of which are keyframe properties. `.key.popping` has no fill mode and
the class is removed after 560ms — safe.

**How it was verified, including two probe traps worth recording:**

- Bug proof: 2s after load, `tab.getAnimations().length === 1` and an inline
  `translateY(-4px)` computed to the *keyframe's* matrix — the animation
  owned the channel.
- Fix proof: after 2.2s, inline animation cleared, `getAnimations() === 0`,
  and with the transition neutralised (`transition:'none'`) an inline
  `translateY(-4px)` computes to `matrix(1,0,0,1,0,-4)` and `scale(.94)` to
  `matrix(.94,0,0,.94,0,0)`.
- Probe trap 1: a hidden Browser pane does not composite frames, so CSS
  animation AND transition timelines are frozen — a probe that waits
  wall-clock time for a transition to settle reads the START value and looks
  like a failure. Neutralise the transition to test cascade precedence
  without needing a timeline.
- Probe trap 2: elements inside a `display:none` subtree report
  `transform: none` from `getComputedStyle` — probing the (hidden) editor's
  tiles this way proves nothing.

**Generalise:** never leave a forwards-filling animation (`both`/`forwards`)
attached to an element that also has hover/press transforms. Entrance
animations either use `backwards` fill, or get removed on completion — ports
must carry the REMOVAL logic, not just the animation. When "hover works but
nothing moves" — shadow reacts, transform doesn't — suspect a filling
animation pinning the transform channel before suspecting the hover rules.

---

## PROBLEM 41 — URL bindings had no cascade: every press opened a duplicate tab

**Symptom.** Space+Y opens YouTube in Brave. Press it again and you get
*another* YouTube tab, forever. App bindings toggle (launch → focus →
minimise); URL bindings were the only kind with no toggle at all.

**Root cause.** `smart_cascade`'s web branch was one unconditional line:

```rust
if let Some(url) = &binding.web_url {
    if run_browser(url, app_handle.clone()) { return CascadeOutcome::Primary; }
}
```

`run_browser` always shells out to the browser with the URL. Nothing ever
looked for an existing window.

**Fix — `url_focus_or_minimize(url)`, called BEFORE `run_browser`** in both
the primary and fallback branches:

```rust
if let Some(url) = &binding.web_url {
    // Toggle an already-open browser window showing this site BEFORE
    // launching — otherwise every press opens another duplicate tab.
    if url_focus_or_minimize(url) { return CascadeOutcome::Primary; }
    if run_browser(url, app_handle.clone()) { return CascadeOutcome::Primary; }
}
```

Behaviour, matching the app cascade:

| Press | State | Action |
| --- | --- | --- |
| 1st | no browser window showing the site | launch the URL (opens + focuses the tab) |
| 2nd | that window is foreground | **minimise it** |
| 3rd | that window is minimised | restore + `force_foreground` |
| any | browser open on a different tab | launch the URL (switches/opens there) |

**How a window is matched.** `EnumWindows`, filtered to the browser process
`run_browser` would pick (`browser_stem()` — brave → chrome → msedge, same
preference order, keep them in step), then the window's title is lowercased
and tested against a keyword derived from the URL. A browser window's title
is `<active tab title> - Brave`, so YouTube's tab matches "youtube".

`url_match_keys(url)` derives `(keyword, host)`, and **returns `None` when the
keyword would be unsafe** — a 1-2 character first label like `x.com` or
`t.co` would match nearly any title and could minimise an unrelated window.
`None` means "just launch", which is always safe. Verified against real URLs
with a standalone `rustc` harness:

```
https://www.youtube.com/watch?v=abc  -> Some(("youtube", "youtube.com"))
https://mail.google.com/mail/u/0     -> Some(("mail", "mail.google.com"))
https://docs.google.com/document/d/1 -> Some(("docs", "docs.google.com"))
https://user@reddit.com:443/r/rust   -> Some(("reddit", "reddit.com"))
https://x.com/home                   -> None   (too short — launch instead)
https://t.co/abc                     -> None
""                                   -> None
```
(userinfo, port, path, query and `www.` are all stripped; `mail.google.com`
still matches Gmail's title because it contains "mail".)

### DELIBERATELY REJECTED: sending Ctrl+W to close the tab

The user proposed that the second press close the tab *and* minimise the
browser. **Do not implement this**, and this is why:

- `Ctrl+W` closes **whatever tab is active**, not the bound site's tab. If the
  user switched tabs since launching, it destroys that instead — a
  half-written comment, a form, an unsaved doc.
- The app cannot check-then-send safely: reading the title and then sending
  the keystroke races the user, who can switch tabs in between.
- It is unnecessary. The goal is "get it out of my way", and minimising
  already achieves that with zero destruction.

The user accepted this reasoning. Recorded here so it is not "fixed" later by
someone reading the original feature request.

### KNOWN LIMITATION (stated, not hidden)

A window title only reveals its **active** tab. If the site is sitting in a
background tab, no match is found and a duplicate tab opens. Detecting
background tabs requires browser-extension-level access and is out of scope.
A duplicate tab is a far smaller harm than closing the wrong one.

Diagnostics: the no-match branch logs at `info!` (not `debug!` — see
PROBLEM 38) and lists every browser window title it saw, so "why did it open a
duplicate" is answerable from the log alone.

**Status: written and compiling (`cargo check` clean, 0 warnings). NOT yet
verified at runtime** — needs Space+Y pressed twice with a URL binding.

---

## PROBLEM 42 — the user was running a 5-hour-old build after a reboot

**Symptom.** After restarting, the user reported the hover/press motion was
missing again and suspected "the one that started with startup is not the
version which had the last fixes." Correct.

**Root cause — a PROCESS failure, not a code one.**

Three UAC prompts for the MSI reinstall were cancelled during the session, so
after 04:01 I stopped reinstalling and simply launched the repo build
(`src-tauri\target\release\space-toggle-v14.exe`) by hand for each test. Every
fix after 04:01 — whole-board press feedback (PROBLEM 39), the animation
fill-mode fix (PROBLEM 40), the URL toggle (PROBLEM 41) — existed ONLY in the
repo build.

`C:\Program Files\SpaceToggle V14\` silently stayed at the **04:00** build,
and the startup entry points there. So on reboot Windows launched a binary
that predated three fixes — **including the glow bug that kills the HUD and
toasts** (PROBLEM 37), which had been fixed at 04:25 in the repo only.

| | Built | State |
| --- | --- | --- |
| Program Files (what boots) | 04:00 | 3 fixes missing + the overlay-killing glow |
| repo `target\release\` | 05:35 | everything |

**Diagnosis, from the outside, without guessing:**

```powershell
# 1. what boots
(Get-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run").SpaceToggleV14
# 2. compare the two binaries by TIMESTAMP, not by faith
Get-Item "C:\Program Files\SpaceToggle V14\space-toggle-v14.exe",
         "D:\...\target\release\space-toggle-v14.exe" |
  Select-Object FullName, Length, LastWriteTime
# 3. is even the repo build stale? any source newer than the exe?
Get-ChildItem -Recurse -File src, src-tauri\src -Include *.ts,*.css,*.rs |
  Where-Object { $_.LastWriteTime -gt (Get-Item $repoExe).LastWriteTime }
# 4. which fixes are actually IN a binary — grep its log strings
$t=[Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($exe))
$t.Contains('url_focus:'); $t.Contains('aumid_focus:'); $t.Contains('st-hud-glow')
```

Step 4 is the useful trick: the `log::info!` format strings and bundled CSS
are ASCII-searchable inside the built exe, so you can prove which fixes a
binary contains without running it. `st-hud-glow` present = the
overlay-killing build.

**Fix.** Reinstalled the 05:35 MSI over the stale product code
(`{C03F782F-…}` → uninstall → install), verified installed size == built size
(13,738,496 both), confirmed `url_focus:` / `aumid_focus:` / `overlay_fit_hud:`
present and `st-hud-glow` ABSENT in the installed exe, then launched it —
`startup: entry already correct`.

**RULE, and it is not optional: a fix that is not installed does not exist.**
The user boots from Program Files, never from `target\release`. Testing from
the repo is fine, but the session is not finished until the MSI is
reinstalled and the installed exe is verified. If a UAC prompt is declined,
say so loudly and treat the work as UNDELIVERED — do not quietly keep
testing from the repo, which is exactly how five hours of fixes failed to
reach the user's actual startup.

Remember the same-version trap while doing it (CLAUDE.md): Tauri regenerates
the ProductCode per build at the same version, so `msiexec /i` over an
existing install exits 0 while Program Files keeps the OLD exe. Always
uninstall the currently registered code first, then install, then compare
sizes.

---

## PROBLEM 43 — toasts painted INSIDE the HUD window (one cause, two symptoms)

**Symptoms, reported as two separate complaints:**
1. "While holding Space, the back glowing thing … is currently beneath
   Contextual Search and Cycle OS Profiles, which looks odd."
2. "Especially when tapping Space+Y twice, the animation was really bad."

**One root cause.** Every shortcut emits a toast (`engine/mod.rs` →
`cascade_toast` → `⚡ {label}`). Pressing a shortcut **while still holding
Space** therefore fires a toast while the HUD owns the overlay window — and
the toast layer is anchored to the window's BOTTOM (`container bottom:74px`,
`#st-toastglow bottom:8px`). In a small toast-sized window that is correct.
In the big centred HUD window (1194x572) the bottom edge sits under the lower
chips, so the glow — and the pill — rendered beneath "Contextual Search".

The same fact explains symptom 2: `fitToStack()` is suppressed while
`_hudActive`, so on release the window snapped from HUD-sized to toast-sized
with a toast already visible in it. Two rapid presses made it worse — two
toasts, two `overlay_fit` calls within a few frames.

**Fix 1 — park the toast layer while the HUD owns the window.**

```ts
function setToastLayerHidden(hidden: boolean): void {
  const c = document.getElementById("toast-container");
  if (c) c.style.visibility = hidden ? "hidden" : "visible";
}
```
`showGuideHud()` parks it; the `hideGuideHud()` 240ms timeout unparks it and
runs ONE clean `relayout()`. Toasts still arrive and age normally while
parked — they are only not painted — so nothing is lost, and the window
resize happens once, after the HUD is gone.

Guard, easily missed: `toastLayer()` writes the container's entire `cssText`
on first use, which clears the parked visibility. The first toast of a
session is very often the one fired during a hold, so `showToast()` re-applies
it: `if (_hudActive) setToastLayerHidden(true);`

**Fix 2 — one glow, re-anchored per surface.** The user asked for the glow to
sit behind SPACE or be removed. Reusing the EXISTING element (not adding a
second one) keeps us inside the proven-safe compositing envelope:

```ts
function anchorGlow(mode: "toast" | "hud"): void {
  const g = document.getElementById("st-toastglow");
  if (!g) return;
  if (mode === "hud") {
    g.style.top = "50%"; g.style.bottom = "auto";
    g.style.transform = "translate(-50%, -50%)";
  } else {
    g.style.top = "auto"; g.style.bottom = "8px";
    g.style.transform = "translateX(-50%)";
  }
}
```

**DO NOT enlarge it or raise its blur.** This is the 340x150 / `blur(22px)`
element proven to composite here. A separate, larger HUD glow (560x320 /
`blur(34px)`) made the whole transparent window compose ZERO pixels —
PROBLEM 37. Re-anchoring costs nothing; resizing risks everything.

**Fix 3 — coalesce rapid window resizes.** Leading-edge-immediate,
trailing-edge-merged, so the first toast appears with no added latency while
a burst produces one resize instead of several:

```ts
const COALESCE_MS = 90;
function requestFit(): void {
  if (_hudActive || _hudBusy) return;
  const since = performance.now() - _lastFitAt;
  if (since >= COALESCE_MS) { fitToStack(); return; }
  window.clearTimeout(_fitTimer);
  _fitTimer = window.setTimeout(() => fitToStack(), COALESCE_MS - since);
}
```
`relayout()` calls `requestFit()` instead of `fitToStack()`. This honours the
motion reference's rule that an OS window's bounds want ONE jump, never a
per-frame animation.

**Generalise:** a fixed-position layer anchored to a window edge is only
correct for the window size it was designed against. When two surfaces share
one window at very different sizes, either re-anchor per surface or hide the
one that does not own it — never leave both painting at once.

---

## PROBLEM 44 — the Guide HUD had a click, not a transition sound

**Ask.** "A transition space sound while holding the space and this coming up
would feel cool too."

**Change.** `showGuideHud`/`hideGuideHud` called `beep(640)` / `beep(400)` —
single fixed-pitch ticks, which read as a click rather than as something
arriving. Added a pitch sweep and used it for the HUD transitions:

```ts
function sweep(from: number, to: number, ms: number): void {
  if (!_soundOn) return;
  try {
    _ac = _ac || new AudioContext();
    const t = _ac.currentTime, dur = ms / 1000;
    const o = _ac.createOscillator(), g = _ac.createGain();
    o.type = "sine";
    o.frequency.setValueAtTime(from, t);
    o.frequency.exponentialRampToValueAtTime(to, t + dur);
    g.gain.setValueAtTime(0.0001, t);                        // never step the
    g.gain.exponentialRampToValueAtTime(0.055, t + 0.025);   // gain — a step
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);    // is a click
    o.connect(g); g.connect(_ac.destination);
    o.start(t); o.stop(t + dur + 0.02);
  } catch { /* never break the overlay for a sound */ }
}
// show: sweep(300, 820, 190)  rising — arriving
// hide: sweep(760, 280, 150)  falling — leaving
```

Two details worth keeping: exponential gain ramps cannot reach exactly 0, so
0.0001 is the floor; and starting at full gain instead of ramping produces an
audible click at note onset.

**It follows the existing "Sound ticks" setting and is OFF by default** — the
user must enable it in the gear panel to hear anything.

---

## PROBLEM 45 — UAC prompt on every launch → Scheduled Task (the 1.0.0 release pass)

**Ask.** "Asking for permission to make changes on device for installation
first time is okay, but asking for permission after every restart feels odd."
Plus: make it shareable to ~15 testers, error logs they can send back, a
run-at-startup toggle (on by default), MSI + EXE, production identity.

### Why the app elevates at all — do not remove this

A `WH_KEYBOARD_LL` hook in a non-elevated process receives NOTHING while an
ELEVATED window has focus (Task Manager, regedit, admin PowerShell). Space
would silently die there, which reads as "randomly broken". So elevation
stays — the fix is HOW it is obtained.

### The fix — a Task Scheduler entry with highest privileges

A task created with `/RL HIGHEST` runs its target elevated WITHOUT a UAC
prompt, both at logon (`/SC ONLOGON`) and when poked via `schtasks /Run`.
Admin consent is needed once (creating the task), then never again.

New flow (`startup.rs`, complete rewrite — read the file, it is documented):

```
non-elevated start ── task exists? ──yes─▶ schtasks /Run → exit   (SILENT)
                          └────────no──▶ ShellExecuteW runas → exit (ONE UAC)
elevated start     ──▶ ensure_startup_task(cfg.run_at_startup)
                       + remove_legacy_run_entries()
```

Key pieces:

```rust
const TASK_NAME: &str = "Spaceadom";
// create/refresh — /F makes this idempotent and self-healing; must be elevated
schtasks(&["/Create","/F","/TN",TASK_NAME,"/TR",&format!("\"{exe}\""),
           "/SC","ONLOGON","/RL","HIGHEST"]);
// the run-at-startup toggle is just the task's enabled state
schtasks(&["/Change","/TN",TASK_NAME, if enabled {"/ENABLE"} else {"/DISABLE"}]);
// silent elevation for manual launches
schtasks(&["/Run","/TN",TASK_NAME]);
```

Details that will bite if lost:
- **All schtasks calls use `creation_flags(CREATE_NO_WINDOW = 0x0800_0000)`**
  or a console window flashes on every launch.
- **schtasks' status text is NEVER parsed** — it is localized. Config
  (`run_at_startup: bool`, serde default true) is the source of truth; the
  task is only ever WRITTEN to. One command (`set_startup_enabled`) persists
  config AND flips the task so they cannot drift.
- `/Run` fails when the task is disabled — the code then falls through to
  the classic runas prompt, so a manual launch still works with startup off.
- PROBLEM 33's dev-build guard carries over: a dev build leaves an existing
  task alone (`is_dev_build` + `task_exists`), so testing from the repo
  cannot repoint the user's logon task at `target\release`.
- The Run key is GONE. `remove_legacy_run_entries()` deletes the old
  `SpaceToggleV14` / `SpaceToggleOrganic` HKCU values on sight, so an old
  build cannot ALSO start at logon and put a second keyboard hook on the
  machine. `SpaceToggleOS` (V13) is deliberately left — separate product,
  kept as the dev machine's fallback.

### Identity: Spaceadom 1.0.0 ("space + freedom", the user's name)

| Where | Value |
| --- | --- |
| productName / window titles / brand text | `Spaceadom` |
| identifier | `com.spaceadom.app` |
| version | `1.0.0` |
| exe (from Cargo `name = "spaceadom"`) | `spaceadom.exe` |
| MSI upgradeCode (NEW — installs beside old V14) | `7A2C4E19-8B3D-4F6A-9C1E-5D8F2B7A4C63` |
| data dir | `%APPDATA%\Spaceadom` |
| Scheduled Task | `Spaceadom` |
| publisher / copyright | `Spaceadom` / `© 2026 Spaceadom` |

The Rust lib name stays `space_toggle_os_lib` (internal only; renaming it
touches main.rs for zero user value).

**Migration:** on first run, if `%APPDATA%\SpaceToggleV14\config.json` exists
it is copied wholesale (`config/mod.rs`, first-run branch), so the dev
machine keeps every binding. Migrated configs lack `run_at_startup` → serde
defaults it to true.

### Beta-tester support

- **Panic hook** (`lib.rs`, right after logger init): a release-build Rust
  panic previously vanished without a trace. Now `PANIC at file:line:col:
  msg` lands in debug.log — the line to grep for when a tester says "it just
  closed".
- **"Open log folder"** button in Settings → `open_log_folder` command →
  Explorer on `%APPDATA%\Spaceadom`. Log rotation already existed (5 MB × 3).
- **"Run at startup" toggle** in Settings, ON by default, wired to
  `set_startup_enabled`.
- READ-ME-FIRST.txt for testers in `share-spaceadom\` — covers SmartScreen
  ("More info → Run anyway", the binary is unsigned), the one-time UAC, where
  logs live, and the panic exit (quit from tray restores everything).

### VERIFIED, on the real machine (2026-08-11 ~21:12–21:17)

The entire flow ran end-to-end and the log proves each step:
- Install → first launch → ONE UAC → `task 'Spaceadom' → C:\Program
  Files\Spaceadom\spaceadom.exe (logon, highest)` + `enabled`.
- `config: migrated from legacy V14 config` once; the next launch says
  `loaded` — migration is genuinely one-time.
- Both legacy Run entries removed (`SpaceToggleV14`, `SpaceToggleOrganic`).
- **Silent relaunch works**: kill → start non-elevated → task fires, app up,
  no prompt (two consecutive runs).
- The user then live-tested unprompted: Space+Y toggled YouTube
  (`url_focus … restoring` / `is foreground — minimizing`, several cycles —
  PROBLEM 41 verified in production), created a profile, held the HUD
  repeatedly (`visible Ok(true)` every show).

### MEASUREMENT TRAP — log4rs writes are BUFFERED (nearly a false bug report)

The silent-relaunch test initially "failed": the app was running with a
window, but debug.log had not grown and its mtime was minutes old. Verdict
nearly shipped: "task launch broken". Reality: **log4rs buffers; the file on
disk can lag the app by minutes while idle**, and NTFS directory metadata
(size/mtime) is additionally stale while a handle is open. The lines all
appeared later, complete and correctly timestamped.

Rules:
- Never judge liveness by polling debug.log line-count right after launch —
  check the PROCESS (pid + window) first, log content second.
- A force-killed instance may lose its final buffered lines. Normal exit
  flushes. Tell testers "quit from the tray, then grab the log".

### Deliberately NOT done, and why

- **No code signing.** A certificate costs real money and needs a legal
  identity; without it SmartScreen shows "unknown publisher" once per
  machine. Acceptable for a friends beta; revisit before a true public launch.
  (The README pre-explains the SmartScreen click-through.)
- **The old "SpaceToggle V14" install is left on the dev machine.** Its
  logon entry is auto-removed by `remove_legacy_run_entries()`, so it will
  not double-hook; uninstall it manually whenever convenient.
- **V13's Run entry (`SpaceToggleOS`) is left untouched** on the dev machine
  by explicit policy (it is the user's fallback), even though two hooks at
  logon is the documented trap. If odd double-behaviour appears on THIS
  machine only, that entry is the first suspect.

---

## PROBLEM 45 — the dashboard had no toast container: 27 messages went nowhere

**Symptom.** A tester bound apps and websites and got **no confirmation for
anything** — not "Saved", not "Assigned", and critically not the ⚠️ failure
messages that would have told him what was wrong.

**Root cause.** `index.html` never contained `<div id="toast-container">`.
`toast.ts`'s `toastLayer()` opens with:

```ts
const c = document.getElementById("toast-container") as HTMLDivElement | null;
if (!c) return null;                  // ← every dashboard toast died here
```

and `showToast()` bails on null. The OVERLAY window has the element, so
engine toasts (⚡ app launched) worked; every toast raised by dashboard code
— 27 call sites — was silently discarded.

**Fix, three parts.**

1. Add the element to `index.html` (inside `#stage`).
2. `styles.css` must import the pill styles, which previously only the
   overlay loaded:
   ```css
   @import "./styles/overlay-earthy.css";
   ```
3. **Critical:** the dashboard must never drive the overlay WINDOW. `toast.ts`
   is shared by both documents, and `fitToStack()`/`overlay_toasts_done`
   resize and hide the separate always-on-top overlay — a "Settings saved"
   toast in the dashboard would have yanked the HUD's window around:
   ```ts
   let _isOverlay = false;
   export function markOverlayWindow(): void { _isOverlay = true; }   // overlay.ts calls this
   // fitToStack(): if (!_isOverlay) return;
   // both overlay_toasts_done sites gated on _isOverlay
   ```
   Default `false` so any new consumer is safe by default.

**Generalise:** when one module is shared by two windows, every call that
targets "the window" must be gated on which document it is running in.
Sharing rendering is fine; sharing window control is not.

---

## PROBLEM 46 — the window was fitted to the whole monitor, not the work area

**Symptom.** On a tester's 1280x720@150% laptop the log looked healthy —
`asked for 1178x662 … got size Ok((1192.0, 700.0))` — but the gear button and
the Special-keys pill sat behind the taskbar.

**Two root causes, both in `lib.rs` step 9c.**

1. It clamped against `mon.size()`, which is the **entire panel including the
   taskbar**. Centring in that rect pushes the bottom of the window under it.
2. `set_size` takes the **inner** (client) size while the clamp was compared
   against the outer budget, so decorations (~16 logical px wide, ~40 tall on
   Win11) pushed the real window past the limit — exactly the 662→700 gap.

**Fix.** Ask Win32 for the work area (Tauri only exposes the full monitor
rect) and subtract decorations before clamping:

```rust
let (wa_w, wa_h, wa_x, wa_y) = work_area_logical(&win, sf)
    .unwrap_or((ms.width, ms.height, mp.x, mp.y));
const DECOR_W: f64 = 16.0;
const DECOR_H: f64 = 40.0;
let w = 1220.0_f64.min((wa_w - DECOR_W).max(320.0));
let h =  880.0_f64.min((wa_h - DECOR_H).max(320.0));
let _ = win.set_size(tauri::LogicalSize::new(w, h));
let _ = win.set_position(tauri::LogicalPosition::new(
    wa_x + (wa_w - (w + DECOR_W)) / 2.0,
    wa_y + (wa_h - (h + DECOR_H)) / 2.0,
));
```

`work_area_logical()` = `MonitorFromWindow` + `GetMonitorInfoW().rcWork`,
divided by the scale factor.

**Also:** `minWidth`/`minHeight` were **900x660**, which on a 720p@150% work
area (~1280x680 logical) is physically unfittable — the user could not even
resize their way out. Lowered to **720x520**.

---

## PROBLEM 47 — reduced motion removed ALL motion, and the app looked broken

**The most damaging finding of the tester round.**

**Symptom.** Tester reported the cursor glow did not follow the mouse, key
presses produced no ripple, and motion was "not smooth" — while the HUD
rendered perfectly and app-binding worked.

**Root cause.** Windows *Settings > Accessibility > Visual effects > Animation
effects* being OFF (Battery Saver also does this) makes WebView2 report
`prefers-reduced-motion: reduce`. Three independent code paths then fired:

| Path | Effect |
| --- | --- |
| `main.ts` `wireCursorGlow()` early return | glow RAF never starts |
| `keyboard-matrix.ts` `spawnRipple()` early return | no press ripple, ever |
| `design-system.css` blanket rule | every animation AND transition → `.001ms` |

The blanket rule was mine:
```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after { animation-duration:.001ms !important;
    transition-duration:.001ms !important; }
}
```
Reduced motion means *don't fly large things around or loop forever*. It does
**not** mean *remove the feedback that tells me my click registered*. Killing
the hover lift and the press depress makes an app read as **frozen**, which is
precisely what the tester reported.

**Fix.** Replace the blanket rule with a targeted `.reduced-motion` class on
`<html>`:

* **Killed:** infinite ambient loops (auras, halo, toast glow, HUD ring) and
  long distance-travelling entrances (key cascade, chip bloom, editor bloom).
  The cursor glow is `display:none`.
* **Kept, shortened:** hover / press / open-close tweens —
  `--dur-micro:90ms; --dur-standard:120ms; --dur-hero:140ms`, spring easing
  swapped for a non-overshooting curve.
* Elements whose entrance keyframes carry their centring get it restored
  explicitly (`#halo`, `#st-hud .space`, `#key-detail-panel.open`) — the same
  bug class as PROBLEM 40: *a transform that lives only inside keyframes is
  lost the moment the animation is removed.*

**And it is now overridable.** `config.motion` is `"auto" | "full" |
"reduced"`, default `"auto"`:

```ts
export function applyMotion(pref): boolean {
  const osAsksForLess = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const reduced = pref === "reduced" || (pref !== "full" && osAsksForLess);
  document.documentElement.classList.toggle("reduced-motion", reduced);
  document.documentElement.dataset.motionResolved = "1";
  return reduced;
}
```

A "Visual effects" toggle in the gear writes an **explicit** `full`/`reduced`
(never `auto` — the user has just stated a preference). Accessibility is
honoured by default; a machine whose OS default makes the app look defective
can be corrected in-app.

**Every call site now reads the CLASS, not the media query** — `main.ts`
`motionReduced()`, `keyboard-matrix.ts`, and `toast.ts`'s `REDUCED()`.
Querying the OS directly behind the setting's back is what made the override
meaningless. The overlay is a separate document and resolves it independently
in `overlay.ts` from `get_config`.

**Diagnostic:** startup now logs
`dashboard-js: motion: setting=auto os-prefers-reduced=true → effective=REDUCED`
via a new `frontend_log` command, so a tester's "there are no animations" is
answered by line one of their log instead of a round trip.

---

## PROBLEM 48 — every suppression path in the hook was silent

**Symptom.** The tester's log contained **zero** `engine: combo Space+X
received` lines across a whole session, while the Guide HUD showed eight
times. Shortcuts did nothing and the log gave no reason whatsoever.

**Root cause.** The hook has five ways to decline to fire a shortcut and
**none of them logged at a level that survives a release build**:

| Path | Was | Now |
| --- | --- | --- |
| Exclusive-fullscreen detected | silent | `info` once per episode |
| Bypass / engine paused | silent | `info` once per episode |
| Rollover (key treated as typing) | silent | `info`, first 5 then every 20th, WITH the numbers |
| Combo dispatched | `debug` (filtered out of release) | `info` |
| Key mapped to no combo | silent | `info` |

Edge-latched with `AtomicBool`s so a held key cannot flood the file:

```rust
static FULLSCREEN_LOGGED: AtomicBool = AtomicBool::new(false);
static BYPASS_LOGGED: AtomicBool = AtomicBool::new(false);
static ROLLOVER_HITS: AtomicU32 = AtomicU32::new(0);
```

The rollover line names the actual cause and the fix, because this path
silently converts an intended shortcut into typed text:

```
hook: TYPED not command — key pressed 96ms after Space, inside the 130ms
rollover window (hit #1). Hold Space slightly longer, or lower 'Rollover
window' in Settings.
```

**Generalise (this is the third time this exact lesson has cost a round
trip):** `log::debug!` in a release build is not a log. Any line that explains
*why a feature silently did nothing* belongs at `info`. Also see PROBLEM 38.

**Still open:** these lines did not exist when the tester ran the build, so
*his* specific cause remains unproven. The next log he sends will state it
outright. Candidate causes it will now distinguish between: rollover eating
the key, another remapper capturing it (PROBLEM 49), the engine being paused,
or the user releasing Space before pressing the letter.

---

## PROBLEM 49 — the app was blind to other keyboard remappers

**Ask.** *"Make our app intelligent enough to inform the user if any
contradictions from other sources and ask to over power or delete that."*

**New:** `src-tauri/src/hook/conflicts.rs`. `CreateToolhelp32Snapshot` scans
running processes against a conservative list of known remappers — AutoHotkey
(all four exe names), PowerToys + its Keyboard Manager engine, SharpKeys,
KeyTweak, KbdEdit, HIDmacros, LuaMacros, spacedesk, and **older SpaceToggle
builds** (two spacebar hooks fight — a documented trap). Skips our own process
by PID *and* name.

Logged at startup, and exposed as `get_conflicts` for a dashboard banner.

**Deliberate scope limit — it OBSERVES AND REPORTS ONLY.** The request
mentioned "over power or delete that", and it deliberately does neither:
terminating or suspending someone else's running software is malware
behaviour, and a false positive would kill a program the user wanted. The app
names what it found, explains the risk in plain English, and leaves the
decision to the user.

The list is intentionally conservative: a false positive tells someone their
machine is misconfigured when it is fine, which is worse than silence.

---

## PROBLEM 50 — the misaligned ring in the tester's screenshot

**Symptom.** In the tester's photo, a thin circular arc sweeps through
"Multi-Corner PiP" and "Force Close App", clearly offset down-and-right of the
SPACE pill instead of ringing it.

**Root cause.** `#st-hud .pulse` was:

```css
#st-hud .pulse { position: absolute; left: 50%; top: 50%;
  width: 340px; height: 340px; border: 1.5px solid var(--st-ring);
  border-radius: 50%;
  animation: st-ring-pulse 900ms cubic-bezier(.2,.8,.2,1) both; }
```

with the centring living ONLY in the keyframes
(`transform: translate(-50%,-50%) scale(...)`) and no `opacity` on the element
(so it defaults to 1). On a machine where the animation does not run — the
tester had reduced motion — the ring therefore:

* lost its centring → a 340px circle whose TOP-LEFT sits at the centre point,
  i.e. displaced 170px right and 170px down; and
* never faded → `opacity: 1` forever instead of the keyframes' `0`.

**Fix — put BOTH defences on the element:**

```css
#st-hud .pulse { …
  transform: translate(-50%, -50%);   /* centred without the animation */
  opacity: 0;                         /* invisible unless the animation drives it */
  animation: st-ring-pulse 900ms cubic-bezier(.2,.8,.2,1) both; }

:root.reduced-motion #st-hud .pulse { display: none !important; }
```

`display:none` under reduced motion, not merely `animation:none` — it is a
one-shot entrance flourish with no meaning as a static circle.

**Verified** in a live page: the ring's centre now sits **0px** from the
viewport centre (previously ~240px diagonal off), and computes `display:none`
with `.reduced-motion` applied.

**THIRD INSTANCE OF THIS BUG CLASS** — see PROBLEM 40 (fill-mode pinning
transforms) and the halo. And a fourth was written *while fixing this one*:

### The same trap, caught during this fix

The new `#conflict-banner` was centred with `left:50%; transform:
translateX(-50%)` while ALSO running the `st-pop-in` entrance. An animation's
`transform` **replaces** the element's, so the banner rendered off-centre —
measured, not guessed. Fixed by centring without transform:

```css
left: 0; right: 0; margin-inline: auto; width: fit-content;
```

**THE RULE, now stated once for the whole codebase: never centre with
`transform` anything that also animates.** Use auto margins or a positioned
wrapper, and keep the transform channel free for the animation. Any element
that relies on `translate(-50%,-50%)` for placement must either never animate
its transform, or restate the translate in every keyframe AND in a
reduced-motion override.

---

## PROBLEM 51 — the conflicts UI: banner + Settings section, no startup toast

**Ask (verbatim):** *"the conflict warning should be dissemble banner in the
dashboard No need be a toast on start up like maybe in the settings part umm
make another section named Conflicts … No need a toast on startup because that
is very annoying."*

**Built exactly that.**

1. **Dismissible banner**, top-centre under the bar (`#conflict-banner` in
   `index.html`, rendered by `renderConflictBanner()` in `main.ts`). Hidden
   when nothing is detected. Dismissal is stored in `sessionStorage` keyed on
   the **sorted product list**, so dismissing a warning about AutoHotkey does
   not silence a *different* program that appears later:
   ```ts
   function conflictKey(list: Conflict[]): string {
     return list.map(c => c.product).sort().join("|");
   }
   ```
2. **Settings › Conflicts** — a permanent section listing each product, its
   process name, and why it matters, plus a "Re-check now" button.
3. **No startup toast.** Explicitly rejected by the user as annoying.

**There is deliberately no "kill it" button** in either surface, and the
Settings copy says so outright: *"Close one of them — either the program
above, or Spaceadom — so only one owns the spacebar. Spaceadom never closes
other programs for you."* Reasons: terminating another running program is not

> **Superseded 2026-08-20 (PROBLEM 155).** That sentence is no longer the
> app's behaviour or its text: the owner asked for a close button, and the
> Conflicts row now offers to end the program on request. The reasoning above
> is kept because it is still the right DEFAULT — what changed is that the
> consent was built rather than the capability withheld.
this app's business, a false positive would close something the user wanted,
and doing it silently is malware behaviour.

**Confirmed working on the developer's own machine at first run** — it
detected PowerToys and spacedesk, both genuinely capable of intercepting keys.

---

## PROBLEM 52 — the OS reduced-motion signal is now IGNORED entirely

**Owner decision, 2026-08-12, verbatim:** *"I don't want my app to respect
reduce animations even if something is on power saving mode."*

Also important: **the tester's laptop was NOT in power saving mode**, so
PROBLEM 47's reduced-motion theory is not confirmed as his cause. Removing the
OS signal takes an entire uncontrolled variable out of the picture — every
machine now renders the app identically, which makes the remaining
"Space+letter does nothing" report far easier to reason about.

**Change — three places, one rule: only an explicit in-app choice reduces
effects.**

```ts
// main.ts applyMotion()   — was: pref === "reduced" || (pref !== "full" && osAsksForLess)
const reduced = pref === "reduced";

// overlay.ts              — was: the same OS-consulting expression
const reduced = cfg?.motion === "reduced";

// toast.ts REDUCED()      — was: class || (unresolved && media query)
const REDUCED = () => document.documentElement.classList.contains("reduced-motion");
```

```rust
// config/schema.rs
fn default_motion() -> String { "full".into() }   // was "auto"
```

Anything that is not the literal string `"reduced"` — `undefined`, a legacy
`"auto"` from an already-saved config, `"full"` — means full effects, so no
config migration is needed.

**Verified in the shipped bundle:** the CSS contains **no**
`prefers-reduced-motion` media query at all, while the `.reduced-motion`
class rules survive so the Settings toggle still works as a manual opt-out.

**What is deliberately kept:** the "Visual effects" toggle. A user on a
genuinely weak machine can still switch effects off by choice — the app just
never makes that choice for them.

**Accessibility note, stated honestly:** ignoring `prefers-reduced-motion` is
a deliberate departure from the usual accessibility convention. It is the
owner's call for this app, and the manual toggle is the mitigation. Anyone
revisiting this should change the default rather than delete the toggle.

---

## PROBLEM 36 — the drag-and-drop affordance pointed users at installers

Not a code defect; a design decision, recorded so nobody "restores" it.

**User's reasoning (2026-08-11):** the `.exe` files people can actually find
in Explorer are usually **installers** (`something-setup.exe`), so an
invitation to drag one onto a key aims them at exactly the wrong file — it
would bind the installer, not the app. Searching the detected-apps list or
pasting a real path is both clearer and correct.

**Change.** The `.ed-drop-hint` element is removed from the editor
(`key-detail-panel.ts`) and its CSS deleted; `.ed-browse` now spans the row:

```css
/* Browse spans the row now that the drag-and-drop hint is gone. */
.ed-browse { flex: 1; justify-content: center; padding: 9px 16px; height: auto; }
```

The `dragover`/`dragleave`/`drop` handlers on the keys in
`keyboard-matrix.ts` are **kept and still work** — the feature is simply no
longer advertised. `V13_TO_V14_METHOD.md` §3.4 lists drag & drop as a gap to
fix; this supersedes it for the editor UI only.

---

## Trap: HTML comments inside a template literal

Writing the PROBLEM 36 note into the editor's `innerHTML` template broke the
build with a bare `error TS1127: Invalid character` — because the comment
contained **backticks** (around `something-setup.exe`), which terminate the
enclosing template literal. The error points at the backtick, not at the
cause. **No backticks inside a template literal, including in comments.**

---

## The theme wiring — ONE setting, two windows

**Problem.** The overlay is a separate webview. It cannot see
`document.body` on the dashboard, so a dark-mode toggle in the dashboard left
Nocturne toasts/HUD unthemed. And an event that only fires on *change* leaves
a freshly-opened overlay in the wrong palette after every restart.

**Fix, part 1 — `src-tauri/src/commands.rs`.** `save_config` gained an
`AppHandle` and re-emits on every save:

```rust
pub fn save_config(
    app: tauri::AppHandle,            // <-- added
    new_config: AppConfig,
    state: State<'_, ConfigState>,
) -> Result<(), String> {
    ...
    // THEME RULE: one setting drives the dashboard AND the overlay. Emitted
    // from Rust deliberately — a GLOBAL `emit` with a single listener (the
    // overlay page) is the only arrangement that has ever delivered in this
    // app; `emit_to` and webview-to-webview emits are not trusted here.
    {
        use tauri::Emitter;
        let _ = app.emit("theme-changed", new_config.dark_mode);
        let _ = app.emit("sound-changed", new_config.sound_enabled);
    }
    config::save(&new_config)
}
```

**Fix, part 2 — `src/components/toast.ts`**: `applyTheme` was module-local;
exported it, and added `applySound`:

```ts
export function applyTheme(dark: boolean): void {
  document.body.classList.toggle("nocturne", dark);
}
/** Sound ticks on/off. Exported so overlay.ts can seed it at startup. */
export function applySound(on: boolean): void { _soundOn = on; }
```

**Fix, part 3 — `src/overlay.ts`**, seed from persisted config on load:

```ts
// The "theme-changed" event only fires when the setting CHANGES, so without
// this the overlay would start in the light palette every launch and only
// correct itself the next time the user touched the toggle.
invoke<{ dark_mode?: boolean; sound_enabled?: boolean }>("get_config")
  .then((cfg) => { applyTheme(!!cfg?.dark_mode); applySound(!!cfg?.sound_enabled); })
  .catch(() => { /* default is the light palette, which is the default theme */ });
```

**Fix, part 4 — `src-tauri/src/config/schema.rs`** (carried from attempt #2)
adds `dark_mode: bool` and `sound_enabled: bool`, both `#[serde(default)]`,
both `false` in `Default`. Mirrored in `src/types.ts` as optional fields.

**Verified.** Dashboard side confirmed on the real machine: toggled via the
gear, persisted to `config.json`, applied before first paint on the next
launch. **Overlay side still unconfirmed** — needs a Nocturne toast or HUD to
be seen.

---

## The HUD had to be centred in TWO places

`overlay_fit_hud` alone is not enough: the frontend resizes milliseconds
*after* the window is shown, so the window first appears bottom-anchored and
visibly jumps.

**`src-tauri/src/commands.rs`, `overlay_fit_hud`** — was bottom-anchored:

```rust
// BEFORE
// let w = width.clamp(320.0, ms.width - 32.0);
// let h = height.clamp(120.0, ms.height - 120.0);
// ... mp.y + ms.height - h - 80.0
// AFTER
let w = width.clamp(320.0, ms.width * 0.94);
let h = height.clamp(120.0, ms.height * 0.94);
let _ = win.set_size(tauri::LogicalSize::new(w, h));
let _ = win.set_position(tauri::LogicalPosition::new(
    mp.x + (ms.width - w) / 2.0,
    mp.y + (ms.height - h) / 2.0,
));
```

**`src-tauri/src/guide_hud/mod_impl.rs`** — `place_overlay` (bottom-anchored)
became `place_overlay_centred`, and `show_guide_hud` calls it. This existed in
**neither V13 nor attempt #2** — it was only ever written down in
`OVERLAY_RUST_HTML_CHANGES.md` §4:

```rust
fn place_overlay_centred(win: &tauri::WebviewWindow, w: f64, h: f64) {
    if let Ok(Some(mon)) = win.primary_monitor() {
        let sf = mon.scale_factor();
        let ms = mon.size().to_logical::<f64>(sf);
        let mp = mon.position().to_logical::<f64>(sf);
        let x = mp.x + (ms.width - w) / 2.0;
        let y = mp.y + (ms.height - h) / 2.0;
        let _ = win.set_size(tauri::LogicalSize::new(w, h));
        let _ = win.set_position(tauri::LogicalPosition::new(x, y));
    }
}
```

`HUD_BOTTOM_MARGIN` was deleted with it (unused const = warning, and the build
must stay at 0 warnings). **`overlay_fit`, used by toasts, stays
bottom-centred — do not unify the two.**

---

## The board must fit twice over

Attempt #2 opened a window wider than the display and the keyboard ran off the
edge. Two independent guards, and **both** are required:

1. **The window fits the screen** — `lib.rs` step 9c, above.
2. **The board fits the window** — `src/main.ts`, scaling on **both** axes:

```ts
function wireKeyboardFit(): void {
  const outer = document.getElementById("keyboard-outer");
  const scale = document.getElementById("keyboard-scale");
  if (!outer || !scale) return;
  const fit = () => {
    const r = outer.getBoundingClientRect();
    if (!r.width || !r.height) return;
    const s = Math.min(1, (r.width - 12) / DESIGN_W, (r.height - 12) / DESIGN_H);
    scale.style.transform = `scale(${s.toFixed(4)})`;
  };
  fit();
  new ResizeObserver(fit).observe(outer);
  window.addEventListener("resize", fit);
}
```

The mockup scales on width alone (`(kbW - 12) / 1048`). That is the single-axis
version that failed. Do not "simplify" back to it.

---

## Files deleted, and where their behaviour went

| Deleted | Why | Where it went |
| --- | --- | --- |
| `src/components/hook-status-bar.ts` | The status bar is gone from the design | Its two useful listeners (`profile-changed`, `hook-status-update`) are now in `main.ts` |
| `src/components/app-picker.ts` | Replaced by the editor's inline "Apps on this device" grid | `AppInfo` moved to `src/types.ts`; `list_start_menu_apps` / `pick_file` / `extract_icon_cmd` are called from `key-detail-panel.ts` |

---

## Deliberate departures from the mockup (each has a reason)

1. **No `backdrop-filter` on the dashboard popovers.** The mockup blurs them;
   this machine has a documented WebView2 white-box bug with `backdrop-filter`
   (2026-07-10). High-alpha solids look the same over the cream stage and
   cannot resurrect that bug.
2. **No "Run at startup" toggle** in the gear panel. There is no backend
   command for it, and `startup.rs` writes the Run key on its own terms. A
   toggle that silently does nothing is the hollow-feature problem this
   rebuild exists to end. Add the command first, then the toggle.
3. **Special functions are labelled on their own keys** (` → PiP Cycle,
   ⌫ → Force Close, `,` → Search, `.` → Pause, ↑/↓ → Scroll Top/Btm,
   RAlt → Profile) as well as in the bottom tray — an explicit user
   requirement (`V13_TO_V14_METHOD.md` §3.5). Esc is not on this board, so it
   is tray-only. See `SPECIAL_ON_KEY` in `keyboard-matrix.ts`.
4. **Real extracted icons, never coloured letter discs**, wherever
   `icon_base64` / `icon_override` exists. The letter disc is the fallback
   only. V13's `IShellItemImageFactory` work is verified and must not be
   downgraded to match a mockup that had no real icons available.

---

## Machine state after this session

| Item | State |
| --- | --- |
| `SpaceToggle V14` 14.0.0 | Installed at `C:\Program Files\SpaceToggle V14\space-toggle-v14.exe`, product code regenerates per build |
| `SpaceToggle OS` 1.0.0 (V13) | Untouched at `C:\Program Files\SpaceToggle OS\` |
| Attempt #2's V14 (`{840E8917-…}`, v1.4.0, `space-toggle-os.exe`) | **Uninstalled and removed** |
| HKCU Run `SpaceToggleV14` | → `C:\Program Files\SpaceToggle V14\space-toggle-v14.exe` |
| HKCU Run `SpaceToggleOS` | Untouched (V13) |
| HKCU Run `SpaceToggleOrganic` | **Deleted** — dead entry from attempt #2 |
| `%APPDATA%\SpaceToggleV14\config.json` | Shared by the installed and repo builds. Backed up to `config.backup-before-install.json` next to it |
| `D:\Claude-Projects\_V14-attempt2-archive` | Safety copy of attempt #2's source. Delete once V14 is confirmed good |

**MSI same-version reinstall trap** (from CLAUDE.md, and it applies every
time you rebuild): Tauri regenerates the ProductCode per build at the same
version, so `msiexec /i` over an existing install exits 0 while Program Files
silently keeps the OLD exe. Correct sequence — look up the CURRENTLY
registered product code, uninstall it, install, then verify by size:

```powershell
$entry = Get-ItemProperty @(
  "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*",
  "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*") -EA SilentlyContinue |
  Where-Object { $_.DisplayName -eq "SpaceToggle V14" } | Select-Object -First 1
Start-Process msiexec.exe -ArgumentList "/X$($entry.PSChildName)","/qn","/norestart" -Wait -Verb RunAs
Start-Process msiexec.exe -ArgumentList "/i","`"$msi`"","/qn","/norestart" -Wait -Verb RunAs
# then ALWAYS compare (Get-Item $installedExe).Length against the built exe
```

---

## Verification status

**Confirmed by the user, on the real machine (2026-08-11):**
- **The radial Guide HUD renders correctly** in both palettes — "looks right",
  "looks very good". Chips bloom from a centred SPACE pill; only assigned
  letters appear; specials in terracotta, apps in sage.
- **Microsoft Store apps now open AND close** — the AUMID matching from
  PROBLEM 30 works. That code had never compiled before this session.
- **Nocturne (dark mode)** on the dashboard, persisted and applied before
  first paint.
- Space+F cycles Explorer restore/minimise (in the log, repeatedly).
- The dashboard, the key editor, and the detected-apps grid all render.

**Fixed this session, NOT yet re-confirmed on screen** — the build with these
in it was installed at 03:46; nobody has looked yet:
- PROBLEM 34, app icons (CSP `img-src data:`).
- PROBLEM 35, the HUD glow centred on the SPACE pill.
- PROBLEM 36, the drag-and-drop hint removed from the editor.

**Still never verified:**
- Whether dark mode reaches the OVERLAY window (a Nocturne toast or HUD).
- The island toasts' appearance and stack behaviour.
- The key editor's bloom animation.
- The foreground ladder for taskbar-flashing on ordinary (non-Store) apps.

Automated input cannot reach the Space-held paths: simulated keypresses do not
set the physical key state the hook checks, proved in the 2026-08-10 session.
Those need a human hand.

---

## Two measurement traps that wasted time in this session

Both are versions of the same law — **test the tool before trusting the test**
— and both nearly produced a false bug report.

1. **A DPI-unaware process gets lied to.** See PROBLEM 32b. Always call
   `SetProcessDpiAwarenessContext(-4)` first, and check its return value.
2. **The monitor layout changed mid-session.** Measurements taken at 03:00
   (1920x1080 primary + 2560x1600 secondary) and at 03:46 (a single
   2560x1600) disagreed, and the disagreement looked exactly like a window
   placement bug. It was not. **Re-read the display layout at the moment of
   measurement; never compare a position against a layout you sampled
   earlier.** The app's own readback log line (PROBLEM 32b) is the
   authoritative record because it reports the monitor it actually used.

---

# The 1.0.1 → 1.0.3 shipping block (PROBLEMS 58–63)

These were solved across two sessions on 2026-08-11/12 and logged in
`PROJECT_STATUS.md` at the time, but **never written up here** — so an AI
picking this up had to reconstruct them from a chronological log. That gap is
the exact failure this file exists to prevent. Backfilled 2026-08-12.

---

## PROBLEM 58 — I put logging inside the keyboard hook and killed the hook

**Symptom.** After adding diagnostics for the tester's "nothing launches"
report, Space+key stopped working entirely — on MY machine, where it had
always worked. No error, no log line, no crash. The hook simply stopped
receiving events partway through a session.

**Root cause.** I added eight `log::info!` calls inside the `WH_KEYBOARD_LL`
callback. `log4rs` writes **synchronously to a file**. That put disk I/O on
the hook callback path, and Windows enforces **`LowLevelHooksTimeout`**
(default 300 ms, `HKCU\Control Panel\Desktop`): a low-level hook callback that
overruns it is **silently evicted** — no notification, no error, the hook
handle stays non-null and looks perfectly valid.

**Exact file.** `src-tauri/src/hook/mod.rs`

**The code.** Every hook-path log call was replaced with a lock-free atomic
counter, drained from the engine thread where blocking is safe:

```rust
// NO log:: CALLS BEYOND THIS POINT — PROBLEM 58, and it broke the app.
// log4rs writes SYNCHRONOUSLY to a file. Doing that inside a
// WH_KEYBOARD_LL callback puts disk I/O on the hook path, and Windows
// enforces `LowLevelHooksTimeout` (300ms default): a callback that
// overruns it gets the hook SILENTLY EVICTED.
use std::sync::atomic::AtomicU32;
static SUPPRESS_FULLSCREEN: AtomicU32 = AtomicU32::new(0);
static SUPPRESS_BYPASS:     AtomicU32 = AtomicU32::new(0);
static ROLLOVER_HITS:       AtomicU32 = AtomicU32::new(0);
static STUCK_MODIFIER:      AtomicU32 = AtomicU32::new(0);
static UNMAPPED_KEYS:       AtomicU32 = AtomicU32::new(0);
static DROPPED_EVENTS:      AtomicU32 = AtomicU32::new(0);

pub fn drain_hook_diagnostics() { /* called from the ENGINE thread only */ }
```

**How it was verified.** Removed the logging, rebuilt, held Space+key — the
binding fired again. The counters still surface the same information, printed
from the engine thread where a blocking write is harmless.

**Generalise this.** *Nothing that can block may run inside a low-level hook
callback* — no file I/O, no COM, no Tauri call, no lock another thread holds.
The punishment is silent eviction, which presents identically to "the feature
was never wired up". `logger.rs:43` already carried this warning in writing and
I did it anyway; the warning is now duplicated at the site itself.

---

## PROBLEM 59 — WebView2 fails to attach on cold boot, and the app lied about it

**Symptom.** On the tester's laptop, and reproducibly after a real cold boot:
the app starts, the tray icon appears, the log says fully initialised — but the
dashboard is blank or never paints, and the first seconds show "not responding".
Found by an AI running **on the tester's machine**.

**Root cause.** A race at logon. The app launches from the Scheduled Task
before the WebView2 runtime has finished its own initialisation, so the webview
creation fails with `HRESULT(0x80070490)` — `ERROR_NOT_FOUND`. Tauri reported
the *window* as created; only the *webview inside it* was missing. Nothing in
the log distinguished those two states, so the app claimed success.

**Exact file.** `src-tauri/src/lib.rs` (setup), `src-tauri/src/startup.rs`,
`src-tauri/tauri.conf.json`.

**The code.** Three changes. The startup task stops racing the runtime:

```
schtasks /Create ... /RL LIMITED /DELAY 0000:30
```

The app checks the **webview** actually exists and rebuilds the window with
`WebviewWindowBuilder` if it does not, instead of trusting window creation.
And the installer stopped assuming WebView2 was present at all:

```json
"webviewInstallMode": { "type": "embedBootstrapper", "silent": true }
```

**How it was verified.** The 1.65 MB Microsoft-signed
`MicrosoftEdgeWebview2Setup.exe` is embedded in both installers — confirmed
2026-08-12 in the Tauri-generated NSIS script
(`target/release/nsis/x64/installer.nsi`):
`!define INSTALLWEBVIEW2MODE "embedBootstrapper"`, with
`WEBVIEW2BOOTSTRAPPERPATH` resolving to a real cached file. The MSI carries the
same payload, visible as a filename in the MSI file table.

**Generalise this.** *A window existing is not a webview existing.* Verify the
thing you actually need, not the container that holds it. And anything launched
at logon is racing the OS — delay it, and make the failure legible instead of
assuming the environment is ready.

Note on searching installers: **absence of an ASCII string in an NSIS `.exe` is
not proof of absence** — NSIS LZMA-compresses its payload. Read the generated
`installer.nsi` instead; it is plaintext and authoritative.

---

## PROBLEM 60 — URL bindings ignored the user's default browser

**Symptom.** The tester's letter-to-URL bindings opened nothing, or reportedly
opened the OneDrive **Documents folder**. I then claimed in chat that Brave had
opened when nothing had — I had inferred a visible window from a
`ShellExecute launched` log line. The user caught it:
*"you are saying brave opened up, but nothing ever opened up."*

**Root cause.** Two distinct defects.

1. `run_browser()` preferred hardcoded `brave.exe` then `chrome.exe`. The
   tester had neither installed.
2. The folder symptom: the old path called `ShellExecuteExW` after
   `CoInitializeEx(APARTMENTTHREADED)` **on the engine thread**, ignoring
   `RPC_E_CHANGED_MODE`. `http` activation goes through COM/DDE; from a thread
   whose apartment was already initialised differently, the shell falls back to
   opening the working directory.

**Exact file.** `src-tauri/src/engine/actions/smart_cascade.rs`

**The code.** Hardcoded browsers deleted. The launch runs on a **dedicated STA
thread** with an explicit working directory:

```rust
let joiner = std::thread::spawn(move || unsafe {
    let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    let file = HSTRING::from(u.as_str());
    let verb = HSTRING::from("open");
    let dir  = HSTRING::from(std::env::var("SystemRoot")
                   .unwrap_or_else(|_| "C:\\Windows".into()));
    let inst = ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(file.as_ptr()),
                             PCWSTR::null(), PCWSTR(dir.as_ptr()), SW_SHOWNORMAL);
    if hr.is_ok() { CoUninitialize(); }
    inst.0 as usize > 32
});
```

`browser_stem()` (used to decide focus-vs-launch) now asks the registry which
browser is actually default, instead of guessing:

```
HKCU\...\UrlAssociations\https\UserChoice  -> ProgId
HKCR\<ProgId>\shell\open\command           -> exe path -> file_stem
```

**How it was verified.** Live on this machine: `BraveHTML` resolved to
`brave.exe`, stem `brave`. Read back from the real registry, not assumed.

**Generalise this.** *Never hardcode which application the user prefers — ask
the OS.* And **a log line saying a call succeeded is not evidence the user saw
anything**: `ShellExecute` returning >32 means "handed off", not "a window
appeared". Do not report a UI outcome you did not observe.

---

## PROBLEM 61 — the app demanded admin it never needed

**Symptom.** A UAC prompt on **every launch**. Autostart silently failed on
standard accounts. The tester had to "Run as administrator" to get the app up
at all. User: *"asking for permission after every restart feels odd."*

**Root cause.** The app self-elevated at startup and relaunched itself.
`WH_KEYBOARD_LL` **does not require elevation** — this was cargo-culted.

**Exact file.** `src-tauri/src/lib.rs` (self-elevation removed),
`src-tauri/windows-app-manifest.xml`, `src-tauri/src/startup.rs`.

**The code.**

```xml
<requestedExecutionLevel level="asInvoker" uiAccess="false" />
```

The logon task registers at limited rights, so it never prompts:

```
schtasks /Create ... /RL LIMITED /DELAY 0000:30
```

`harden_task_settings()` then applies `-AllowStartIfOnBatteries
-DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero)
-StartWhenAvailable`, because Task Scheduler's defaults otherwise skip the task
on battery and stop it after 3 days.

**How it was verified.** Installed 1.0.3 from the MSI and launched
`C:\Program Files\Spaceadom\spaceadom.exe`: process alive, full initialisation
in the log, **no UAC prompt**.

**KNOWN, ACCEPTED LIMITATION.** A non-elevated hook receives no input while an
**elevated** window has focus (Task Manager, regedit, an admin terminal, the
UAC secure desktop). That is Windows UIPI and applies to every remapper. It is
documented, not worked around — elevating to dodge it costs a prompt on every
boot and makes a global keyboard hook look exactly like a keylogger to AV
heuristics.

---

## PROBLEM 62 — no application manifest, so the process was DPI-unaware

**Symptom.** Window and monitor maths silently wrong on scaled displays. The
tester runs 1280x720 @150%; the HUD ring was misaligned.

**Root cause.** With no manifest the process is **DPI-unaware**, so Windows
feeds it *virtualised* coordinates. Every `GetMonitorInfoW` result and every
window size is a lie — consistently enough to look like an arithmetic bug in
our own layout code.

**Exact file.** `src-tauri/windows-app-manifest.xml`, wired via
`src-tauri/build.rs`.

**The code.**

```xml
<dpiAware     xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
<dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
<longPathAware  xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
<activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
```

```rust
// build.rs
let windows = tauri_build::WindowsAttributes::new()
    .app_manifest(include_str!("windows-app-manifest.xml"));
tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
    .expect("failed to run tauri-build with the app manifest");
```

**How it was verified.** The manifest is present in the shipped binary —
`Common-Controls`, `PerMonitorV2` and `asInvoker` are all ASCII-searchable
inside `spaceadom.exe`.

---

## PROBLEM 63 — my own manifest bricked the binary (0xC0000139)

**Symptom.** The first 1.0.3 build **would not start at all**. Exit code
**`0xC0000139` STATUS_ENTRYPOINT_NOT_FOUND**, zero log output. Launching it by
hand produced the Windows dialog:

> spaceadom.exe - Entry Point Not Found
> The procedure entry point **TaskDialogIndirect** could not be located in the
> dynamic link library ...\spaceadom.exe

**Root cause.** `app_manifest()` **REPLACES Tauri's default manifest
wholesale**, and that default declares the `Microsoft.Windows.Common-Controls`
v6 dependent assembly. By supplying my own manifest for PROBLEM 61/62 and
omitting that block, the process bound to **comctl32 v5**, which does not
export `TaskDialogIndirect` — a v6 API the toolkit statically imports. The
loader fails before `main` runs, which is why there is no log line to diagnose
from.

**Exact file.** `src-tauri/windows-app-manifest.xml`

**The code.** The block that must never be omitted:

```xml
<dependency>
  <dependentAssembly>
    <assemblyIdentity
      type="win32"
      name="Microsoft.Windows.Common-Controls"
      version="6.0.0.0"
      processorArchitecture="*"
      publicKeyToken="6595b64144ccf1df"
      language="*"
    />
  </dependentAssembly>
</dependency>
```

**How it was caught** — this is the part worth copying. I launched the built
exe and read its **exit code**, then ran the **PREVIOUS installed build as a
control**: the old one exited 0, the new one did not. That isolated the
regression to my own change in a single step, without reading any code.

**How it was verified fixed (2026-08-12).**

1. The repo exe contains both the `TaskDialogIndirect` import and the
   `Common-Controls` / `6595b64144ccf1df` / `version=6.0.0.0` strings.
2. It launches: process alive, log grew, **no error dialog**.
3. The staged MSI's payload exe is **13,869,568 bytes — an exact size match**
   with the fixed build (extracted with `msiexec /a`, which needs no UAC).
4. The MSI was installed and `C:\Program Files\Spaceadom\spaceadom.exe` was
   launched: full initialisation, tray built, no dialog.

**Generalise this.** *Any custom Windows manifest for a Tauri app MUST include
the `Microsoft.Windows.Common-Controls` v6 dependent assembly, or the binary
will not launch.* More broadly: **supplying a manifest is a replacement, not a
merge** — as is true of most framework "override" hooks. Before you override a
default, find out everything the default was doing for you.

And: **a screenshot of an error is evidence about the build that produced it,
not about the build on disk now.** This exact error was re-reported after it
had already been fixed; the correct response was to check the current binary,
not to re-diagnose the symptom.


---

# The 1.0.4 ship-readiness block (PROBLEMS 64–66)

Found 2026-08-12 by auditing "will this work on any friend's non-ARM Windows
laptop", after 1.0.3 was installed and verified.

---

## PROBLEM 64 — "Run at startup" never worked on a non-admin machine

**Symptom.** After installing 1.0.3 and launching it, `debug.log` showed:

```
startup: task create FAILED: ERROR: Access is denied.
```

"Run at startup" is ON by default, the Settings toggle said so, and the app
never started with Windows. On every friend's laptop this would fail the same
way, invisibly.

**Root cause.** A NON-ELEVATED process cannot create a task in the Task
Scheduler **root folder** — verified directly on this machine with a fresh
task name and `/RL LIMITED`; `schtasks /Create` still returns
`ERROR: Access is denied.` PROBLEM 61 removed self-elevation, so the app is
*always* non-elevated now — meaning the very fix that removed the UAC prompt
also silently broke startup registration. The two fixes were tested
separately, never together.

**Exact file.** `src-tauri/src/startup.rs`, plus `src-tauri/src/lib.rs`.

**The code.** Task creation failure now falls back to the canonical per-user
autostart, which never needs elevation:

```rust
// startup.rs — on schtasks /Create failure:
Some(o) => {
    // PROBLEM 64 — the NORMAL path on a non-admin machine, not an edge case.
    log::warn!(
        "startup: task create failed ({}) — using HKCU Run autostart instead",
        String::from_utf8_lossy(&o.stderr).trim()
    );
    set_run_key(run_at_startup);
    return;
}
```

```rust
fn set_run_key(enabled: bool) {
    // HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run
    //   "Spaceadom" = "C:\Program Files\Spaceadom\spaceadom.exe" --autostart
}
```

Rules that keep the two mechanisms from double-starting the app:
- When the task IS created successfully, `set_run_key(false)` removes any
  Run value — the task is authoritative.
- `apply_task_enabled()` (the Settings toggle) routes to the Run key when no
  task exists.
- A dev build never writes a Run entry (PROBLEM 33's guard, Run-key edition).
- The single-instance plugin makes any residual double-start harmless.

`--autostart` replaces the task's `/DELAY 0000:30` (a Run value has no delay
flag) — `run()` sleeps 30s before building windows when the flag is present
(PROBLEM 59's cold-boot race). And the single-instance callback ignores
second instances carrying `--autostart`, so a waking autostart instance can
never pop the dashboard over a session the user already started manually.

**How it was verified.** Access-denied reproduced by hand (fresh task name,
`/RL LIMITED`, non-elevated → denied). Build clean. The Run-key write path
runs on a real machine at next install/launch — see PROJECT_STATUS for the
install verification of this build.

**Generalise this.** *When you remove a privilege, re-test every feature that
silently depended on it.* Privileged and unprivileged code paths must each be
tested end-to-end; "the task code is unchanged" proved nothing once the
process stopped being elevated. Also: **error lines in a log nobody reads are
not error handling** — a failed registration must fall back, not just log.

---

## PROBLEM 65 — hook eviction was permanent; a watchdog now reinstalls

**Symptom.** (Latent — found by audit, confirmed against the code.) After
Windows silently evicts the WH_KEYBOARD_LL hook (the PROBLEM 58 class:
callback overruns `LowLevelHooksTimeout`), Space+key dies for the rest of the
session. The pump keeps running, the log looks healthy, and NOTHING ever
reinstalls the hook — the handles were local variables no other code could
reach.

**Root cause.** No recovery path existed at all. Eviction is silent by
design: no error, no message, the hook handle still looks valid.

**Exact file.** `src-tauri/src/hook/mod.rs`

**The code.** Liveness stamps + a thread-queue timer in the pump:

```rust
// One lock-free store per callback — being called at all is the proof of life.
LAST_KB_EVENT.store(now, Ordering::Relaxed);   // kb_hook_proc
LAST_MS_EVENT.store(tick_count(), Ordering::Relaxed); // ms_hook_proc, BEFORE
                                               // the MODIFIER_ACTIVE early-return

// hook_thread_main — the pump:
let timer_id = SetTimer(None, 0, 3000, None);
while GetMessageW(&mut msg, None, 0, 0).as_bool() {
    if msg.message == WM_TIMER && msg.wParam.0 == timer_id {
        watchdog_check(&mut kb_hook, &mut ms_hook);
        continue;
    }
    ...
}
```

`watchdog_check` (runs every 3s, on the pump — NEVER inside a callback):
- user active <2s ago (GetLastInputInfo) AND both hooks silent >8s → both
  evicted → unhook + reinstall.
- user active AND keyboard silent >120s while the mouse hook is provably
  alive (<8s) → keyboard alone evicted (the realistic case: our keyboard
  callback is the heavy one, and the hooks are evicted independently) →
  reinstall. The long window is because "mouse active, no typing" is a normal
  way to read a page; a false positive costs one sub-ms unhook/rehook.
- On reinstall: clear MODIFIER_ACTIVE / SPACE_INTERCEPTED / SPACE_ABORTED so
  a mid-hold eviction cannot leave Space latched.

**Two traps baked into the fix — copy these, they cost real debugging time:**

1. **NULL-hwnd `SetTimer` IGNORES the id you pass** and returns a fresh
   system id; `WM_TIMER.wParam` carries THAT id. Compare against the RETURN
   VALUE or the watchdog compiles, runs, and never fires — a silent no-op.
2. **Unhook BEFORE logging.** `log::error!` writes to disk synchronously; do
   it while the old hook is still installed and the write itself can trip
   `LowLevelHooksTimeout` — the watchdog would cause the eviction it exists
   to repair.

Also: `GetLastInputInfo` reports in **32-bit GetTickCount space** — compare
there, never against `GetTickCount64`.

**How it was verified.** Build clean; watchdog code paths reviewed against the
two traps above. Eviction itself cannot be triggered on demand on a healthy
machine (the callback is deliberately fast), so the reinstall path is
verified by code inspection + the reinstall counter appearing in
`drain_hook_diagnostics` — labelled as such, not claimed live-tested.

**Generalise this.** *Any resource the OS can silently revoke needs a
watchdog that can prove liveness and re-acquire it.* And a watchdog that
cannot fire (wrong timer id) is worse than none — it reads as coverage.

---

## PROBLEM 66 — hook install failure was a silent panic and a dashboard lie

**Symptom.** (Latent — found by audit.) On a machine where security software
or policy blocks global hooks, `SetWindowsHookExW` fails; the code
`.expect()`ed it, so the hook THREAD panicked — but the app kept running.
Tray icon present, dashboard open, `get_hook_status` returning a hardcoded
`installed: true` — and no keystroke would ever do anything.

**Root cause.** Two halves: the `.expect()` on the hook thread (a panic
there kills only that thread), and `commands.rs`'s status command lying:

```rust
installed: true, // hook is always installed unless explicitly stopped
```

**Exact files.** `src-tauri/src/hook/mod.rs`, `src-tauri/src/commands.rs`.

**The code.** `install_hooks()` never panics — a failed install leaves an
invalid HHOOK, logs loudly once, and the PROBLEM 65 watchdog keeps retrying
every 3s. A new atomic carries the truth:

```rust
pub static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
// set by install_hooks(), cleared on thread exit

// commands.rs
installed: crate::hook::HOOK_INSTALLED.load(Ordering::Relaxed),
```

**How it was verified.** Build clean. The failure branch requires a machine
that blocks hook installs, which this one does not — verified by code
inspection; labelled as such.

**Generalise this.** *A status API that returns a constant is not a status
API.* Anything the UI reports as health must be read from the thing itself.
And `.expect()` on a worker thread is a silent kill switch: the process
survives, the feature dies, and nothing tells anyone.


---

## PROBLEM 67 — user-visible strings still said "SpaceToggle" after the rename

**Symptom.** Caught by SCREENSHOTTING the toast during 1.0.4 testing rather
than reading the log: the bypass toast rendered "⏸ SpaceToggle Paused", and
the Guide HUD pill read "Pause SpaceToggl…" (truncated). The product was
renamed to Spaceadom at 1.0.0; these strings were missed and would have gone
out to every friend.

**Root cause.** The rename (PROBLEM 45) covered identity — productName,
identifier, exe name, data dir, task name — but not literal display strings
scattered in the engine and commands layer. Nothing links the two, so nothing
failed.

**Exact files.**
- `src-tauri/src/commands.rs` — the bypass toast built for the tray/command path
- `src-tauri/src/engine/mod.rs` — the same toast from the hook path, plus the
  HUD pill label
- `src/main.ts` — the fatal-error message shown when the backend is unreachable

**The code.**

```rust
// commands.rs — before / after
if new_state { "⏸ SpaceToggle Paused" } else { "▶ SpaceToggle Active" }
if new_state { "⏸ Spaceadom Paused" }   else { "▶ Spaceadom Active" }

// engine/mod.rs — the bypass toasts (a SECOND copy of the same strings)
state.emit_toast("⏸ Spaceadom Paused");
state.emit_toast("▶ Spaceadom Active");

// engine/mod.rs — the Guide HUD pill (was "Pause SpaceToggle Engine",
// which the pill truncated to "Pause SpaceToggl…")
(".".to_string(), "Pause Spaceadom".to_string()),
```

```ts
// src/main.ts
showFatalError("Could not connect to the Spaceadom backend.");
```

**Deliberately NOT changed.** `hook/conflicts.rs` still names "SpaceToggle
v11 / V13 / V14" — those describe genuinely older builds of this app that may
be running on the machine, so the old name is CORRECT there.

**How it was verified.** Screenshot of the toast before the fix showed the
wrong name; strings replaced and rebuilt. Re-verify by screenshotting the
bypass toast after installing 1.0.5.

**Generalise this.** *A rename is not done when the identity fields change —
grep the codebase for the old name and triage every hit as either display text
(rename) or a genuine historical reference (keep).* And: **log lines cannot
catch a branding bug, because the logger legitimately keeps the old crate
name.** Only looking at the pixels found this — which is the argument for
screenshotting UI during verification instead of trusting structured output.

---

## PROBLEM 68 — the conflict banner nagged on every single launch

**Symptom.** The dashboard's "PowerToys / spacedesk are running and can
capture Space" banner reappeared at EVERY app start. The user's instruction
was explicit: *"no need to warn all the time, only on first install"* — and
earlier, about a startup toast, *"that is very annoying"*.

**Root cause.** Dismissal was stored in `sessionStorage`, which is cleared
when the webview process ends. Every launch is a new session, so every launch
re-armed the banner. The dismissal logic looked correct and was correct — it
just persisted to the wrong place.

**Exact file.** `src/main.ts` — `renderConflictBanner()`

**The code.**

```ts
// BEFORE — reset on every launch
if (sessionStorage.getItem("st-conflict-dismissed") === conflictKey(knownConflicts)) {

// AFTER — shown once per distinct conflict set, ever
const key = conflictKey(knownConflicts);
if (localStorage.getItem("st-conflict-seen") === key) {
  el.hidden = true;
  return;
}
// Marked seen at RENDER time, not on dismiss — closing the dashboard without
// clicking ✕ must not re-arm it for the next launch.
localStorage.setItem("st-conflict-seen", key);
```

The key is still the sorted product list, so a **different** remapper
appearing later earns exactly one new warning. The permanent home for this
information is Settings › Conflicts (`#set-conflicts`), which always lists
every detected program with its process name and why it matters.

**How it was verified.** Code change + rebuild. Behavioural check after
installing 1.0.5: launch twice, banner must appear at most once.

**Generalise this.** *`sessionStorage` is per-webview-session; for a desktop
app that means "until the user closes the window".* Anything that should be
remembered across launches needs `localStorage` or backend config. Also:
**"dismissed" and "seen" are different states** — marking on dismiss alone
means a user who ignores the banner gets it again forever.


---

## PROBLEM 69 — accidental launches for fast typists, and a setting nobody could answer

**Symptom.** User report: *"space + letter sometimes can give accidental
launches for fast typers."* An app opens in the middle of a sentence.

**The measurement that explains it.** Injected a Space-down, then the letter
`f` at increasing delays, with Space STILL HELD (the overlap a fast typist
produces constantly), against the shipped config (`rollover_ms: 120`):

```
letter lands  20ms after Space-down -> typed normally (safe)
letter lands  35ms after Space-down -> typed normally (safe)
letter lands  45ms after Space-down -> typed normally (safe)
letter lands  55ms after Space-down -> typed normally (safe)
letter lands  70ms after Space-down -> typed normally (safe)
letter lands  90ms after Space-down -> typed normally (safe)
letter lands 120ms after Space-down -> COMMAND FIRED  <-- accidental launch
```

The boundary is exactly `rollover_ms`. A ~100 wpm typist has ~120 ms between
keystrokes, so they sit ON the boundary and normal timing jitter tips
individual keystrokes over it.

**Root cause — two parts.**

1. **The direction of the knob is counter-intuitive, so the default was wrong
   for fast typists.** A FASTER typist needs a WIDER window. Fast typists
   overlap keys — they press the next letter before releasing Space — and any
   overlap LONGER than the window is read as a deliberate Space+key command.
   Narrowing the window (the instinctive "make it stricter") makes accidental
   launches MORE frequent, not fewer.
2. **The control asked an unanswerable question.** Settings exposed
   `Rollover window` as a raw 10–150 ms slider. No user knows their own
   key-overlap in milliseconds, so nobody could fix their own problem.

**Exact files.**
- `src-tauri/src/config/schema.rs` — new `typing_wpm` field + the mapping
- `src-tauri/src/config/mod.rs` — upgrade path for existing configs
- `src/components/settings-panel.ts` — the new control
- `src/styles.css` — tier labels
- `src/types.ts` — the optional field
- `src-tauri/src/hook/mod.rs` — the diagnostic hint now names the new control

**The code.** The mapping lives in Rust so there is ONE definition:

```rust
/// Faster typing means more key overlap, so the window GROWS with WPM.
/// Clamped 60..=220ms: below 60 even a light overlap misfires; above 220 a
/// deliberate command needs an awkwardly long hold.
pub fn rollover_ms_for_wpm(wpm: u32) -> u64 {
    let raw = (wpm as f64) * 1.4 + 20.0;
    raw.round().clamp(60.0, 220.0) as u64
}
```

mirrored in `settings-panel.ts` as `rolloverMsForWpm()`. Tiers: Slow (30–44),
Regular (45–74), Fast (75–104), Very fast (105–150). The UI is a 30–150 wpm
slider with the four tier names positioned ABOVE it at each band's midpoint,
the active one highlighted, and a live `Fast · 90 wpm` readout. Changing it
writes BOTH `typing_wpm` and the derived `rollover_ms`; the hook keeps reading
`rollover_ms`, so nothing downstream changed.

**The upgrade trap, and the fix.** `#[serde(default)]` alone would have given
every EXISTING user `typing_wpm: 65` while their real window stayed at
whatever `rollover_ms` they had — so the brand-new slider would confidently
display a speed that was not in force. The migration derives the wpm FROM the
real window instead, and only when the field was genuinely absent:

```rust
if !raw.contains("\"typing_wpm\"") {
    let wpm = (((cfg.rollover_ms as f64) - 20.0) / 1.4).round().clamp(30.0, 150.0) as u32;
    cfg.typing_wpm = wpm;   // e.g. 120ms -> 71 wpm ("Regular")
    dirty = true;
}
```

**How it was verified.** The threshold table above was measured on the
INSTALLED build. Build is 0 errors / 0 warnings. The slider's live behaviour
and the re-measured threshold at a new setting must be confirmed on the
installed 1.0.6 — see PROJECT_STATUS for the result.

**Generalise this.** *Measure the boundary before designing the control.* One
seven-row injection sweep turned "sometimes it launches by accident" into an
exact number and revealed the knob ran the opposite way to intuition. And:
**a setting expressed in implementation units is not a setting** — if the user
cannot answer the question it asks, it may as well not exist. Finally, when
adding a field that REPLACES the meaning of an old one, a serde default is not
a migration: derive the new field from the old value, or the UI lies to every
existing user on first launch.


---

## PROBLEM 70 — the dashboard threw itself in the user's face at every logon

**Symptom.** User: *"it doesn't have to fire up in front of the face every time
anyone restarts their laptop… It needs to work, but the app interface does not
have to fire up… A person can manually just open the app to get to the
dashboard."* Every boot, on every friend's laptop, a full 1220x880 window
appeared uninvited.

**Root cause.** The `settings` window was declared `"visible": true` in
`tauri.conf.json`, so Tauri showed it as soon as it was built — with no way to
distinguish "the user double-clicked the app" from "Windows started us at
logon". Nothing was wrong with the code; the app simply had no concept of a
background start until `--autostart` was introduced by PROBLEM 64.

**Exact files.** `src-tauri/tauri.conf.json`, `src-tauri/src/lib.rs`.

**The code.** The window is now born hidden and shown deliberately:

```json
// tauri.conf.json — settings window
"visible": false,
```

```rust
/// True when this process was started by the logon autostart entry rather
/// than by a person (PROBLEM 64 writes `--autostart` into the Run value).
fn autostart_launch() -> bool {
    std::env::args().any(|a| a == "--autostart")
}
```

```rust
// lib.rs, immediately after the work-area fit (step 9c)
if autostart_launch() {
    log::info!("setup: autostart launch — staying in the tray, dashboard not shown");
} else {
    let _ = win.show();
    let _ = win.set_focus();
}
```

**A second, easy-to-miss path.** PROBLEM 59's cold-boot recovery rebuilds a
failed webview with `.visible(false)`, and step 9c's `show()` already ran
against a window that no longer existed. Before this change `visible: true`
masked it; now it would strand a manual launch with a tray icon and no
dashboard. The rebuild therefore shows the window itself:

```rust
Ok(w) => {
    log::info!("setup: webview '{label}' rebuilt successfully");
    if label == "settings" && !autostart_launch() {
        let _ = w.show();
        let _ = w.set_focus();
    }
}
```

**Three ways back to the dashboard**, all pre-existing and verified in
`tray.rs`: left-click the tray icon, tray menu → "Open Settings", or launch
the app again (the single-instance plugin fronts the running window — and
correctly ignores a second instance carrying `--autostart`, PROBLEM 64).

**Bonus fix.** Showing after the fit also removes the flash of a
wrongly-sized, centred window that every manual launch used to produce before
the work-area clamp landed.

**How it was verified.** Build clean (0 errors, 0 warnings). Behaviour to
confirm on the installed build: launching with `--autostart` must produce a
tray icon and NO window; launching normally must show the dashboard.

**Generalise this.** *"Started by a human" and "started by the OS" are
different events and deserve different UI.* Any app with autostart needs to
tell them apart — and the moment you add a hidden-start path, audit every
`show()` and every window-rebuild branch, because a window that used to be
visible by default is now only visible if some code says so.

---

## PROBLEM 67b — the tray still carried the old product name

**Symptom.** User: *"I also noticed the name is still the old name in the
tray."* The notification-area tooltip read "SpaceToggle OS - Active" and the
context menu "Exit SpaceToggle OS".

**Root cause.** The PROBLEM 67 sweep covered toasts, the HUD pill and the
frontend fatal-error text, but I grepped only for *display strings in the UI
layer* and missed `tray.rs`. The tray tooltip is arguably the most visible
name of all — it is what a user reads when hunting for the app in the
notification area.

**Exact file.** `src-tauri/src/tray.rs`

```rust
.tooltip("Spaceadom — active")
let exit = MenuItem::with_id(app, "exit", "Exit Spaceadom", true, None::<&str>)?;
```

Also cleaned in `lib.rs`: the startup `println!` (said "SpaceToggle OS **V12**"
— two names out of date) and the fatal-error `.expect()` message, which is the
text a crash would surface.

**Deliberately still spelling the OLD name** — do not "fix" these:
- `hook/conflicts.rs` — detects genuinely older builds (v11 / V13 / V14) that
  might be running and fighting for the spacebar.
- `startup.rs` `LEGACY_RUN_VALUES` and `legacy_data_dir()` — these must match
  what the OLD versions actually wrote, or the cleanup and one-time config
  migration silently stop working.

**Generalise this.** *A rename sweep must cover every surface the OS renders
on the app's behalf* — tray tooltip and menu, window title, taskbar name,
installer strings, notification titles — not just strings inside your own UI.
And when a rename is reported incomplete, sweep the WHOLE tree once and
classify every hit (display text → rename, historical/compat → keep) instead
of fixing the one instance that was reported.


---

# The 1.0.8 regression round (PROBLEMS 71–75)

The user reported five faults after real use on two machines. Two were MY
regressions from this same day's work (71, 72), one was a latent bug my change
exposed (75), two were long-standing (73, 74). Written up together because the
lesson is shared: **each fix below was diagnosed from the live system before
any code was edited** — and two of my earlier "verified" claims were wrong
because I tested only half the paths.

---

## PROBLEM 71 — the Scheduled Task launched WITHOUT --autostart

**Symptom.** Tester: installed the update, restarted, and the dashboard
"fired up right in front" — the exact behaviour PROBLEM 70 claimed to fix.

**Root cause.** PROBLEM 70's silent-start keyed off a `--autostart` argument,
and PROBLEM 64's Run-key fallback passes it — but the Scheduled Task path
still registered `"C:\...\spaceadom.exe"` with NO flag:

```rust
let tr = format!("\"{exe_str}\"");            // BEFORE — no flag
let tr = format!("\"{exe_str}\" --autostart"); // AFTER
```

A task-based logon launch was therefore indistinguishable from a double-click:
dashboard shown, and no 30s cold-boot delay either. I verified PROBLEM 70 only
through the Run-key path on this machine (where task creation is denied), so
the task path was never exercised.

**Exact file.** `src-tauri/src/startup.rs`

**Generalise this.** *When a behaviour forks on a flag, EVERY launcher that
starts the program must agree on the flag.* Grep for every place the exe path
is written into a launcher (task, Run key, shortcut, installer) the moment such
a flag is introduced. And a fix verified through one launch path is not
verified for the others.

---

## PROBLEM 72 — the typing-speed mapping was BACKWARDS, and shipped

**Symptom.** Tester and owner both: MORE accidental launches after the
typing-speed slider landed, not fewer. Owner's config read
`typing_wpm: 30 → rollover_ms: 62`.

**Root cause.** 1.0.6's mapping was `wpm * 1.4 + 20` — the window GREW with
speed. I reasoned "fast typists overlap keys more, so give them a wider
window" and never checked the model against the one quantity the hook actually
measures: the delay from Space-DOWN to the next letter. That delay tracks the
typist's inter-key interval, `12000 / wpm` ms, which SHRINKS as speed rises:

```text
  40 wpm  ~300 ms between keys   needs a WIDE window
  70 wpm  ~170 ms
 120 wpm  ~100 ms                a narrow window is safe
```

So "Slow" produced a 62 ms window — nearly every letter typed while Space was
still settling became a command. The mapping made the slider actively harmful
at one end and useless at the other.

**The fix.** `rollover_ms_for_wpm(wpm) = clamp(8400 / wpm, 110, 300)`,
anchored so the DEFAULT (70 wpm) lands on **exactly 120 ms — the value the app
shipped with for months before the slider existed** (explicit user
requirement: a fresh install must behave like the pre-slider build). The
110 ms floor is the load-bearing half: no slider position can reproduce the
62 ms failure. Mirrored in `settings-panel.ts`; help text now says "choose a
SLOWER speed if apps launch by accident".

**The repair for configs the broken build already wrote**
(`src-tauri/src/config/mod.rs`): the corrected mapping can never emit a value
below 110 ms, so `rollover_ms < 110` can ONLY be the broken mapping's output —
reset those to 70 wpm / 120 ms with a WARN naming the cause. Configs without
`typing_wpm` adopt the default and KEEP their existing window.

**Exact files.** `src-tauri/src/config/schema.rs` (mapping + constants),
`src-tauri/src/config/mod.rs` (repair), `src/components/settings-panel.ts`,
`src/main.ts`, `src/types.ts`.

**Generalise this.** *Before shipping a mapping, check its direction against
the measured quantity, at both extremes.* My own threshold sweep (PROBLEM 69's
table) contained the disproof: it showed commands fire when the letter lands
LATE, which means slow typing needs MORE window, not less. I built the sweep
and then didn't read it. Also: when a shipped bug wrote bad data into user
configs, the NEXT build must repair that data on load — fixing the code alone
leaves every existing install broken.

---

## PROBLEM 73 — Settings outgrew small laptop screens

**Symptom.** User: "the settings now is very long, so in laptops with lower
screen size it needs to be scrollable."

**Root cause.** `#settings-panel` had no height bound. It lives in
`#gear-dock`, which is anchored to the BOTTOM of the stage — so an over-tall
panel runs off the TOP of the window, and the rows up there are simply
unreachable (nothing scrolls, because nothing constrains the height).

**The fix** (`src/styles.css`):

```css
#settings-panel {
  max-height: calc(100vh - 110px);  /* dock offset + gear + gap */
  overflow-y: auto;
  overscroll-behavior: contain;      /* don't scroll the stage behind it */
}
```

plus a slim themed scrollbar. Works down to the window's 520px minHeight.

**Generalise this.** *Any bottom- or top-anchored popover needs a viewport-
relative max-height + overflow from the day it is born* — it will grow, and
the direction it overflows is the direction nobody can reach.

---

## PROBLEM 74 — "(Not Responding)" during the first seconds of launch

**Symptom.** Both testers, repeatedly: the window appears, titles itself
"(Not Responding)", then recovers after a few seconds.

**Root cause.** The window was shown while WebView2 was still doing its
first-run initialisation. A visible window whose webview cannot pump messages
yet IS the "(Not Responding)" ghost — the OS paints the frame, nothing answers
`WM_PAINT`, Windows brands it unresponsive. Every earlier "fix" (moving
schtasks off the startup path, PROBLEM 55) shortened the gap but kept the
ordering: show first, become responsive later.

**The fix.** Invert the ordering — the window is shown only when the frontend
proves it is alive:

1. `src/main.ts` — bootstrap()'s LAST line: `invoke("dashboard_ready")`.
2. `src-tauri/src/commands.rs` — `dashboard_ready` shows + focuses the window
   (on the main thread via `run_on_main_thread`), unless `--autostart`.
   Guarded by a `DASHBOARD_READY` atomic so a webview reload can't re-show.
3. `src-tauri/src/lib.rs` — a 10s fallback thread shows the window anyway if
   the beacon never arrives (wedged frontend beats no window at all), and the
   PROBLEM 59 rebuild path no longer shows directly — the rebuilt webview's
   own bootstrap ends in the same beacon.

The user's first sight of the dashboard is now a window that can already
paint and respond.

**Generalise this.** *Never show a window before the thing inside it can
answer messages.* "Show, then boot" reads as broken; "boot, then show" reads
as fast — even when the total time is identical.

---

## PROBLEM 75 — the friend's machine: a stale task nobody can silently fix

**Symptom.** Friend restarted; dashboard opened at logon — on a machine where
1.0.7+ was installed, which should never do that.

**Root cause.** The self-elevating 1.0.0–1.0.2 era created a Scheduled Task
AS AN ELEVATED PROCESS. That task survives every upgrade (installers do not
touch it), points at the install path with no `--autostart`, and — MEASURED
on this machine with a probe task created elevated then attacked non-elevated:

```
schtasks /Delete            -> ERROR: Access is denied.
schtasks /Create /F (over)  -> ERROR: Access is denied.
schtasks /Change /DISABLE   -> ERROR: Access is denied.
Disable-ScheduledTask       -> Access is denied.
```

A non-elevated process has NO way to remove, replace, or even disable it.
PROBLEM 71's flag fix is unreachable on such machines — the task cannot be
rewritten.

**The fix — triage + a one-click elevated repair.**

`startup.rs` now classifies the task from its XML (`task_state()`):
- **Healthy** (this exe + `--autostart`): keep, drop the Run key.
- **Mismatched**: try `/Delete` (works for tasks our own non-elevated code
  created). If Windows refuses: set `STALE_TASK`, log loudly, and do NOT
  write a Run key — the stale task already autostarts the app; adding a
  second launcher just races it.
- **None**: create with the flag; on Access denied fall back to the Run key.

The dashboard (`checkStaleTask()` in `src/main.ts`) then shows a persistent
banner: *"A leftover startup entry from an older version opens this window at
every logon. One click fixes it."* The button calls `repair_stale_task`
(`startup.rs`): ONE `ShellExecuteExW("runas", "schtasks /Delete …")` — a
single user-initiated UAC prompt — waits for it, verifies the task is gone,
then registers the clean Run-key autostart. Declining the prompt is treated
as a clean "no": the banner stays for next time, nothing is logged as an
error. The banner is deliberately NOT once-per-set like the conflict banner:
it returns every session until the machine is actually repaired.

**Exact files.** `src-tauri/src/startup.rs`, `src-tauri/src/commands.rs`
(`get_stale_task` / `repair_stale_task`), `src-tauri/src/lib.rs` (handler
registration), `src/main.ts`.

**Generalise this.** *An artifact created elevated can only be removed
elevated — a "fix" that assumes the app can clean up its predecessor's mess
must first test what permissions the predecessor left behind.* And when the
only path is elevation, make it one user-initiated click with the reason
stated, never an automatic prompt at startup.

---

## Upgrade behaviour — verified from the generated installers, for the record

Read from `target/release/wix/x64/main.wxs` and `nsis/x64/installer.nsi`:

- **MSI over MSI**: stable `UpgradeCode` `7a2c4e19-8b3d-4f6a-9c1e-5d8f2b7a4c63`
  + `<MajorUpgrade Schedule="afterInstallInitialize"
  AllowSameVersionUpgrades="yes">` — double-clicking a newer MSI removes the
  old version automatically and installs the new one. No manual uninstall.
- **setup.exe**: enumerates the uninstall registry, detects an existing
  MSI/NSIS Spaceadom, and offers to remove it first (user-confirmed page).
- **`%APPDATA%\Spaceadom`** (config.json, debug.log) is touched by NEITHER
  uninstaller during an upgrade — bindings, settings and logs survive, and
  `config/mod.rs`'s load-time migrations adapt/repair old fields (PROBLEM 69's
  wpm adoption, PROBLEM 72's window repair).
- What upgrades do NOT clean: the stale Scheduled Task (PROBLEM 75's banner
  exists for exactly this) and the HKCU Run value (self-managed by the app:
  healthy-task path removes it, repair path rewrites it).


---

## PROBLEM 76 — the first REAL logon test: silent-start worked, but the app looked dead

**Symptom.** The user restarted BOTH laptops with 1.0.8 installed. Good news
first, in their words: it did not "blast into the whole screen" — the
dashboard-at-logon bug is gone at a real reboot. But: "it should have by
itself come in the tray icon and started working… I had to manually search for
spaceadom and start as administrator."

**Root cause — three stacked, none of them "autostart didn't run".**
debug.log from the user's own reboot proves the mechanism fired:

```
boot 14:20:48 → Run key launched the app 14:22:31 (--autostart)
             → waited its fixed 30s
             → 14:23:01 hook installed, tray built, dashboard hidden ✓
```

1. **The 30s blanket delay stacked on the ~100s Windows already takes** to
   reach the Run key. The user opened the laptop, pressed Space+key into a
   dead hook during that window, and reasonably concluded "didn't start".
2. **Windows 11 hid the tray icon.** Every new tray icon goes into the
   overflow flyout behind the `^` chevron. Verified: our
   `HKCU\Control Panel\NotifyIconSettings\<id>` entry had `IsPromoted` unset.
   The icon existed; it was just invisible without clicking the chevron.
3. **On the friend's laptop, autostart genuinely did not fire** — my own
   1.0.8 decision caused it. The stale `/RL HIGHEST` task from the
   self-elevating era often CANNOT start on a standard account, and my
   Mismatched-undeletable branch deliberately withheld the Run key ("one
   launcher at a time"). Result: NO autostart of any kind, and the friend's
   old run-as-administrator habit was the only thing that worked.

**The fixes.**

- `lib.rs`: autostart wait 30s → **10s**. The blanket sleep predates the two
  real cold-boot defences (webview-existence rebuild, PROBLEM 59; ready
  beacon, PROBLEM 74) and no longer needs to carry the risk alone.
- `startup.rs` Mismatched-undeletable branch: **write the Run key after
  all** (`set_run_key(run_at_startup)`). Autostart resilience beats
  launcher tidiness: if both do fire, single-instance resolves the race; the
  worst case is the old dashboard-at-logon, which the repair banner fixes.
- `startup.rs` + `lib.rs`: **`promote_tray_icon_once()`** — sets
  `IsPromoted=1` on our NotifyIconSettings entry (HKCU, non-elevated,
  Win11-only key). Suffix-matched on `spaceadom\spaceadom.exe` so the
  KNOWNFOLDER-GUID form matches and dev builds (`target\release\…`) do NOT.
  Runs 5s after tray build (the shell creates the entry only after first
  showing the icon), retries next launch until it succeeds once, then a
  `tray_promoted` config flag stops it forever — a user who later hides the
  icon must stay hidden.

**How it was verified.** Real-logon log above for the diagnosis; registry
read for the IsPromoted evidence. The 10s wait + promotion need the NEXT
reboot to observe end-to-end.

**Generalise this.** *"It didn't start" from a user is a claim about what
they could SEE, not about the process list.* Check the log before re-fixing
the mechanism — here the mechanism was fine and the visibility was the bug.
And on Windows 11, a tray-only app that never promotes its icon is invisible
by default — plan for it on day one.

---

## PROBLEM 77 — HUD chips overlapped ("Up/Dn ×2" over "Esc Boss Key")

**Symptom.** User: while holding Space, the "Up/Dn ×2 Scroll Top/Bottom" and
"Esc Boss Key" pills overlap. Visible in this session's own HUD screenshots.

**Root cause — two compounding geometry errors in `buildHud()`.**

1. Arc shares came from WIDTH ESTIMATES: `min(label.len × 6.8, 118) + 64`.
   The cap punished exactly the longest labels, and the KEY BADGE width was a
   flat constant — "Up/Dn ×2" alone renders ~70px. Real pill ≈ 230px,
   estimate ≈ 180px → its arc share was far too small.
2. Shares were proportional in ANGLE on an ELLIPSE (ry ≈ 0.55·rx). Equal
   angle steps cover very unequal distance along an elliptical rim, pinching
   chips together where the rim flattens.

**The fix** (`src/components/toast.ts`): build all chips first, MEASURE their
real `offsetWidth`, then place:

- `arcAngles(ws, rx, ry, gap, off)` — samples the ellipse's cumulative arc
  length (720 steps) and positions each chip centre so its share of the RIM
  DISTANCE is proportional to its measured width + a 14px clearance, inverting
  arc length back to the parameter angle by binary search.
- Ring radii now derive from measured maxima too. `estW()` survives only as a
  fallback for a zero measurement.

**How it was verified.** Hold Space on the installed build and LOOK (the
overlay cannot be validated in a browser harness — its failure modes live in
the OS compositor). Screenshot in PROJECT_STATUS.

**Generalise this.** *Never lay out variable-width content from character
counts — measure the DOM.* And proportional-in-angle is only proportional-in-
space on a circle; on an ellipse, distribute along ARC LENGTH.


---

## PROBLEM 78 — the hook watchdog stormed reinstalls at ERROR level

**Symptom.** Found while verifying 1.0.9, in the shipped log: seven
`hook: WATCHDOG — … hooks reinstalled (silent eviction)` ERROR lines in one
session, including `kb silent 9000ms / mouse 9000ms, user active 0ms ago` —
both hooks "dead" while the user was demonstrably typing.

**Root cause.** Windows **UIPI**: while an ELEVATED window has focus (a UAC
prompt, an admin terminal, an elevated installer — exactly what this
machine's install sessions look like), a non-elevated LL hook receives
NOTHING, but `GetLastInputInfo` still updates because the user is typing into
the elevated window. The watchdog's model ("user active + hooks silent =
evicted") read the app's own documented, accepted limitation as an eviction
and reinstalled every 3s timer tick for as long as the elevated window held
focus. Each reinstall is a sub-ms unhook/rehook — mostly harmless — but the
ERROR spam pollutes every log a tester sends, and a reinstall mid-keystroke
is a needless risk.

Ruled out first: injected test input (LAST_KB_EVENT updates BEFORE the
0x7A7A7A7A cookie filter, hook/mod.rs:388, so our own injections keep the
timestamps fresh).

**Exact file.** `src-tauri/src/hook/mod.rs`, `watchdog_check()`.

**The fix — two guards ahead of the existing rules.**

1. **Elevated-foreground probe.** `OpenProcess(PROCESS_QUERY_INFORMATION)`
   on the foreground window's process FAILS with access denied from medium
   integrity against an elevated process — the LIMITED flavour would succeed
   and is deliberately NOT used. On failure: return, silence is UIPI, not
   eviction.
2. **60s cooldown** (`WATCHDOG_LAST_REINSTALL`). A real eviction is fixed by
   ONE reinstall; if silence persists, repeating 3s later cannot help and
   only converts an unknown repeating cause into an ERROR-spam loop.

**How it was verified.** Compiles clean; the storm scenario (elevated window
focused while typing) no longer meets the reinstall condition by
construction. A genuine eviction still reinstalls within one timer tick, at
most once per minute.

**Generalise this.** *A watchdog's "impossible" state must be checked against
every documented limitation of the thing it watches* — here, "user active but
hooks silent" is the app's own KNOWN normal under UIPI. And every automatic
recovery action needs a cooldown: recovery that can repeat unboundedly is an
outage amplifier, not a safety net.


---

## PROBLEM 79 — WhatsApp/Arc: Space+key "does nothing" on the second press

**Symptom.** User: *"space plus W launches WhatsApp. Then again I press
space W it doesn't minimize it back. This is also happening with Arc."* Then
the correction that changed the diagnosis: *"no no, it doesn't launch again,
it does nothing."*

**Live evidence (this machine, w → `shell:AppsFolder\5319275A.WhatsAppDesktop_…!App`):**

```
aumid_focus: no window matched AUMID … Packaged windows seen: ["brave", …alarms…]
cascade: activating Store app: shell:AppsFolder\…WhatsAppDesktop…
cascade: ShellExecute accepted … (hInstApp=42, process_created=false)
```

while `Get-Process` showed WhatsApp's window plainly alive:
`WhatsApp.Root  pid=6300  MainWindowTitle='WhatsApp'`.

**Root cause.** The `shell:AppsFolder\…` matcher (`aumid_focus_or_minimize`)
found windows ONLY via `SHGetPropertyStoreForWindow` → `PKEY_AppUserModel_ID`.
Two whole classes of Apps-folder apps never carry that window property:

1. **WinUI3 packaged apps** (modern WhatsApp — process `WhatsApp.Root`, NOT
   ApplicationFrameHost): visible, titled window, no AUMID property.
2. **Unpackaged Win32 apps registered in the Apps folder** (Arc): ditto.

So the matcher missed, and the code fell through to ShellExecute
"activation" — which, for an already-running single-instance app, is accepted
(`hInstApp=42`) and *does nothing visible*. Hence "it does nothing": not a
failed launch, a successful no-op.

**The fix — a 3-rung ladder inside `aumid_focus_or_minimize`**
(`src-tauri/src/engine/actions/smart_cascade.rs`), verified against the
windows-0.58 registry sources by a 4-agent audit before writing:

- **Rung 1 (existing):** property-store AUMID, exact then package-family.
- **Rung 2 (new — fixes WhatsApp):** enumerate visible windows; per window:
  not cloaked → titled → unowned → `OpenProcess` →
  `GetPackageFamilyName(hProcess)` (kernel32; feature
  `Win32_Storage_Packaging_Appx` added to Cargo.toml) → compare with the
  binding's family. Candidates are COLLECTED and ranked — foreground first,
  else first-in-Z — never first-hit (a topmost mini-player would otherwise be
  toggled forever while the main window is never touched).
- **Rung 3 (new — fixes Arc):** parse the Apps-folder item itself
  (`SHCreateItemFromParsingName` with the ORIGINAL-case string —
  registered AUMIDs are case-sensitive) → `IShellItem2::GetString(
  System.Link.TargetParsingPath {B9B4B3FC-2B51-4A42-B5D8-324146AFCF25},2 —
  hand-rolled PROPERTYKEY, zero new features)` → target exe path →
  delegate to `try_focus_or_minimize`, which owns the same minimize/restore
  cycle plus the HWND cache. Packaged apps have no TargetParsingPath —
  GetString errs cleanly and the ladder ends at the existing activation.

**Guards that the adversarial audit made mandatory:**
- `DwmGetWindowAttribute(DWMWA_CLOAKED)` skip in BOTH rung 1 and rung 2 —
  suspended UWP windows stay "visible" while composing nothing; restoring one
  moves keyboard focus onto an invisible window (Enter could send a WhatsApp
  message into the void). Rung 1 had this latent bug all along.
- `CoUninitialize` moved BELOW the whole ladder: rung 3's shell parse under
  uninitialised COM fails with an Err identical to "property not found",
  silently disabling the Arc fix.
- Per-window failures (`OpenProcess` on protected processes) skip that window,
  never abort the enumeration. Unpackaged processes return
  APPMODEL_ERROR_NO_PACKAGE (15700) and are skipped — which conveniently
  self-filters ApplicationFrameHost.
- Rung 2 is gated on the AUMID containing `!` (unpackaged registered AUMIDs
  have no family and can never match).

**How it was verified.** Compiles 0/0 first try (the audit pre-verified every
signature). Runtime verification on the installed build: see PROJECT_STATUS —
Space+W cycle with the family-match log line.

**Generalise this.** *"Press does nothing" and "press relaunches" are
different bugs:* activation of a running single-instance app is a silent
no-op, so a matcher miss reads as total deadness. And: a window-matching
strategy keyed on a property some windows simply don't carry needs a
process-level fallback — the process always knows its own package.

---

## PROBLEM 80 — HUD and toasts invisible on ONE machine: dead GPU composition

**Symptom.** User: *"currently I cannot see my HUD anywhere… toast is also
not showing. I can hear sound but I can't see."* Same 1.0.9 painted perfectly
on the friend's laptop. Survived a reboot. Earlier the same day (12:11) the
HUD had painted fine on this same machine.

**The measurements that pinned it (in order):**
1. Log: `overlay_fit_hud … GOT size Ok((1130,572)) pos Ok((289,247));
   visible Ok(true)` — while a screenshot of exactly that rect showed ONLY
   the window behind. JS alive (fits firing), sound playing.
2. Window-flag probe: layered ✓ transparent ✓ noactivate ✓ not cloaked ✓ —
   nothing wrong at the Win32 level.
3. **The experiment that proved it:** relaunch with
   `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--disable-gpu` → 263 of 861
   sampled pixels showed HUD content. GPU on: 0 of 861. Same build, same
   machine, same hold.

**Root cause.** The machine's display driver stack (virtual-display drivers —
spacedesk service running, Samsung DeX / phone-mirroring tools installed —
are the prime suspects) breaks DWM composition of the GPU-rendered
transparent WebView2 surface. Chromium keeps running (JS, audio, IPC);
composition delivers nothing. Rust cannot see it: every readback is healthy.
This is PROBLEM 37's symptom signature with a completely different cause —
and it is machine-state, so no amount of build-side testing elsewhere finds it.

**The fix — detect and self-heal** (config `overlay_compositing:
"auto"|"software"`, `schema.rs`):

1. `lib.rs` (before the Tauri builder, where the env var is read once):
   `"software"` merges `--disable-gpu` into
   `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`.
2. `commands.rs` `compositing_selftest()`, riding on every HUD show while in
   "auto": sample 5 screen pixels around the overlay centre at T0 and T+450ms
   (the HUD pulses — a live overlay changes pixels). Window dismissed
   mid-test → no verdict. All-unchanged → strike; any change → strikes reset
   (the safe direction: video behind causes a MISSED detection, never a false
   one). **Three consecutive strikes** → config flips to "software", saved,
   loud WARN, and a detached self-restart (`cmd /C ping -n 3 & start` so the
   single-instance mutex is released first). The overlay was invisible
   anyway; the restart costs the user nothing they could see.
3. Never auto-reverts. A healed machine skips the test forever (static flag).

**How it was verified.** The broken machine is the test bed: fresh install in
"auto", three HUD holds → strikes 1..3 in the log → self-restart →
`compositing: SOFTWARE mode (--disable-gpu)` → HUD pixel-sampled PAINTING.
See PROJECT_STATUS for the run.

**Generalise this.** *When the same binary behaves differently on two
machines, stop debugging the build and start debugging the machine.* The
`--disable-gpu` A/B is a one-command experiment that separates "renderer
runs" from "composition delivers" — remember it for every
invisible-but-alive webview. And a "visible: true" readback testifies about
window STATE, never about pixels: only `GetPixel` tells the truth.


**Addendum (2026-08-13, live verification).** Both fixes verified on the
installed 1.0.10. P79: Space+W launch → minimize (family match, 2 candidates,
foreground-ranked) → restore. P80: strikes 1-3 → software flip → HUD painting
at 153/861 sampled pixels (was 0/861), healed config alone, no env var.
One flaw found during the P80 run: the detached self-restart was killed with
the parent when the app had been started inside a JOB OBJECT (test harness).
Fix: try CREATE_BREAKAWAY_FROM_JOB (0x01000000) first, fall back to a plain
spawn — jobs that forbid breakaway fail the flagged spawn cleanly.
Generalise: *a detached child is only detached if no job object says
otherwise* — any "relaunch myself after exit" spawn on Windows needs the
breakaway-with-fallback pattern.

---

# The "bulletproof" resilience pass (PROBLEMS 81–90)

**Origin.** The user asked for the app to "just work, no matter what device
configuration, screen size, RAM, power, battery." That is not a wish that can
be granted in one edit — it is a list of failure modes that has to be
enumerated and closed one at a time. A 24-agent audit read the whole tree
across six failure surfaces (process death, display topology, power/session,
storage/memory, WebView2 lifecycle, hostile environments), then adversarially
verified every finding against the real code before anything was written.

**Two things worth copying from the method:**
1. The verifiers REFUTED several of their own team's findings — including a
   claimed `delete_profile` panic that had already been fixed. An audit whose
   findings are all "confirmed" has not been verified.
2. Everything below is a fix for something that has NOT yet happened to a
   user. That is the point: each one is a report the user would otherwise have
   filed as "it just stopped working" with no way to diagnose it.

---

## PROBLEM 81 — the cold-boot rebuild produced a half-configured overlay

**Symptom (latent).** On a machine that hit the PROBLEM 59 cold-boot WebView2
race, the overlay was rebuilt as `WebviewWindowBuilder::new(label, url)
.visible(false).build()` — nothing else. The declaration in tauri.conf.json
(transparent, undecorated, always-on-top, skip-taskbar, no-shadow, unfocused)
and the runtime setup (click-through, no-activate, DWM border/corner fixes)
were BOTH lost. The replacement is an opaque, decorated, focus-stealing,
taskbar-visible rectangle that swallows clicks — appearing only on the
machines already having the worst launch experience.

**Root cause.** A window's configuration lived in two places (the conf file
and an inline block in `setup()`), and the rebuild path reproduced neither.

**Exact file.** `src-tauri/src/lib.rs`

**The code.** All runtime configuration moved into one shared function, and
the rebuild now mirrors the conf declaration field-for-field:

```rust
pub fn configure_overlay_window(overlay: &tauri::WebviewWindow) {
    let _ = overlay.hide();
    let _ = overlay.set_focusable(false);       // v11 "NoActivate"
    // DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_DONOTROUND
    // DWMWA_BORDER_COLOR = 0xFFFFFFFE (COLOR_NONE)
    match overlay.set_ignore_cursor_events(true) {
        Ok(())  => { OVERLAY_DISABLED.store(false, Relaxed); }
        Err(e)  => { OVERLAY_DISABLED.store(true,  Relaxed); }  // fail CLOSED
    }
}
```

```rust
// the rebuild, per label
if label == "overlay" {
    builder = builder.title("Spaceadom Overlay").transparent(true)
        .decorations(false).always_on_top(true).skip_taskbar(true)
        .resizable(false).focused(false).shadow(false)
        .inner_size(600.0, 460.0);
} else {
    builder = builder.title("Spaceadom").inner_size(1220.0, 880.0)
        .min_inner_size(720.0, 520.0).center();
}
match builder.build() {
    Ok(w) => { if label == "overlay" { configure_overlay_window(&w); }
               else { tray::attach_close_to_tray(&w); } }   // PROBLEM 90
    ...
}
```

**Generalise this.** *Any code path that RE-creates a resource must be able to
reproduce its full configuration — so that configuration must live in exactly
one callable place, never inline at the creation site.* A rebuild that
silently produces a differently-configured object is worse than a rebuild that
fails loudly.

---

## PROBLEM 82 — a single panic could kill Space+key forever, silently

**Symptom (latent).** The hook thread IS the app. If it panicked — a driver
feeding a malformed event, an unexpected OS failure — Space+key was dead until
the user restarted the process, with no dialog, no tray change, nothing in the
log to explain it. The engine actor had the same shape: one panic inside any
action and the actor task ended, the channel backed up, and every keypress
silently did nothing.

**Root cause.** `std::thread::spawn(hook_thread_main)` with no supervision,
and a `loop { dispatch(event).await }` where dispatch ran inline on the actor
task. Rust does not abort the process on a thread panic (no `panic = "abort"`
in Cargo.toml), so the app kept "running" while its core was dead.

**Exact files.** `src-tauri/src/hook/mod.rs`, `src-tauri/src/engine/mod.rs`

**The code.** A supervisor that restarts the hook thread body, with a cap so a
persistent crash cannot become a spin loop, and a shutdown flag so a
deliberate exit is not mistaken for a crash:

```rust
pub static HOOK_SHUTDOWN: AtomicBool = AtomicBool::new(false);

// supervisor loop
let result = std::panic::catch_unwind(AssertUnwindSafe(|| hook_thread_main(tx2)));
if HOOK_SHUTDOWN.load(Relaxed) { return; }         // clean exit
log::error!("hook: THREAD PANICKED — restarting it so Space+key keeps working");
restarts.retain(|t| now.duration_since(*t).as_secs() < 600);
if restarts.len() > 5 { /* give up loudly, do not spin */ return; }
std::thread::sleep(Duration::from_secs(2));
```

```rust
// engine: each event is its own task, so one panic drops ONE keypress
let joined = tauri::async_runtime::spawn(async move { dispatch(event, &state2).await }).await;
if let Err(e) = joined {
    log::error!("engine: an action PANICKED ({e}) — that keypress was dropped; \
                 the engine keeps running");
}
```

`stop_hook()` sets `HOOK_SHUTDOWN` before posting WM_QUIT.

**Generalise this.** *Every thread that a feature depends on needs an owner
that notices its death.* And the two failure modes need different handling: a
crash should restart, a crash LOOP should stop and say so — an unbounded
restart is a CPU fire that looks like a hang.

---

## PROBLEM 82b — lock poisoning turned one panic into total app death

**Symptom (latent).** 55 sites did `.read().unwrap()` / `.write().unwrap()` /
`.lock().unwrap()` on the shared config and engine state. Rust POISONS a
`Mutex`/`RwLock` when a thread panics while holding it, and every subsequent
`.unwrap()` on that lock panics too. So a single panic anywhere under the
config lock would cascade: every command, every action, every keypress
panicking forever after — from one transient fault.

**The fix.** All 55 converted mechanically:

```rust
.read().unwrap()   →  .read().unwrap_or_else(|p| p.into_inner())
.write().unwrap()  →  .write().unwrap_or_else(|p| p.into_inner())
.lock().unwrap()   →  .lock().unwrap_or_else(|p| p.into_inner())
```

`into_inner()` on a `PoisonError` returns the guard anyway: the data may be
mid-update, but for this app's data (a config struct, a profile index) a
slightly stale read is infinitely better than a dead application.

**Generalise this.** *In a long-running desktop app, `lock().unwrap()` is a
latent whole-app killer.* Poison recovery should be the default and the
exception should be argued, not the other way round.

---

## PROBLEM 83 — the window could be stranded on a monitor that no longer exists

**Symptom (latent).** Undock a laptop, unplug a projector, or reconnect RDP
with fewer screens: the dashboard's saved position is still a valid-looking
coordinate pair, but no pixels live there. Clicking the tray icon then appears
to do nothing at all — the window IS shown, off-screen.

**Exact file.** `src-tauri/src/lib.rs` (+ callers in `tray.rs`, `commands.rs`)

```rust
pub fn ensure_on_screen(win: &tauri::WebviewWindow) {
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else { return };
    let Ok(monitors) = win.available_monitors() else { return };
    if monitors.is_empty() { return; }   // headless RDP moment — nothing sane to do
    let (cx, cy) = (pos.x + size.width as i32 / 2, pos.y + size.height as i32 / 2);
    let on_screen = monitors.iter().any(|m| { /* centre inside this monitor */ });
    if !on_screen {
        log::warn!("window: centre ({cx},{cy}) is outside every live monitor — re-centring");
        let _ = win.center();
    }
}
```

Called from EVERY show path: `tray::restore_window()` and
`commands::dashboard_ready()`.

**Generalise this.** *A saved window position is only meaningful relative to a
monitor layout that may no longer exist.* Validate against the CURRENT layout
at show time, not at save time.

---

## PROBLEM 84 — the declared minimum window size exceeded small screens

**Symptom (latent).** `tauri.conf.json` declares `minWidth: 720`,
`minHeight: 520`. On a 1024×600 netbook at 125% scaling the LOGICAL work area
is about 819×448 — smaller than the declared minimum in one dimension. The OS
honours the minimum, so the window is larger than the screen and the
bottom-anchored controls (the gear, the Special-keys pill) are unreachable.
The work-area clamp could not save it: a clamp cannot go below a minimum.

**Exact file.** `src-tauri/src/lib.rs`, inside the work-area fit

```rust
if max_w < 720.0 || max_h < 520.0 {
    let _ = win.set_min_size(Some(tauri::LogicalSize::new(320.0, 240.0)));
    log::warn!("setup: work area {max_w:.0}x{max_h:.0} is below the declared 720x520 \
                minimum — min size relaxed so the window fits the screen");
}
```

Safe because the frontend already scales the board to any size
(`wireKeyboardFit`); the minimum was a comfort constraint, not a requirement.

**Generalise this.** *A minimum size is a promise you cannot keep on a screen
smaller than it.* Any hard minimum needs an escape hatch measured against the
actual work area.

---

## PROBLEM 85 — renaming a profile could create duplicates

**Symptom.** `create_profile` has always rejected duplicate names.
`rename_profile` validated only the CHARACTERS, so renaming profile B to
profile A's name produced two profiles called "A". Every name-keyed lookup
then became ambiguous, and `delete_profile` (which uses `retain(|p| p.name !=
name)`) would remove BOTH at once.

Historically this reached a `cfg.profiles[0]` index on an emptied Vec — a
panic while HOLDING the config write lock, which is exactly the poisoning
cascade of PROBLEM 82b. That indexing was already replaced by a remaining-count
guard plus a `profiles.first()` fallback; the verifier confirmed the panic is
now unreachable and REFUTED that half of the finding.

**Exact file.** `src-tauri/src/commands.rs` — `rename_profile`

```rust
// `new_name != old_name` keeps a same-name rename a no-op instead of an error
if new_name != old_name && cfg.profiles.iter().any(|p| p.name == new_name) {
    return Err(format!("Profile '{new_name}' already exists"));
}
```

**Generalise this.** *If one mutation path enforces an invariant, every
mutation path must.* Create checked for duplicates; rename did not — and
rename is the one users reach by accident.

---

## PROBLEM 86 — Space+scroll could fade Spaceadom's own windows

**Symptom (latent).** The opacity action skips "our own" windows by consulting
`OWN_HWNDS`. Two independent bugs made that protection nonexistent: (1)
`register_own_hwnd` was NEVER CALLED — the list was always empty; and (2) it
was a `thread_local!`, so even when called it would have registered on the
MAIN thread while the check runs on the ENGINE thread, which sees its own
empty copy. Space+scroll over the dashboard could therefore fade Spaceadom
itself to the opacity floor.

**Exact files.** `src-tauri/src/engine/actions/opacity.rs`, `src-tauri/src/lib.rs`

```rust
// was: thread_local! { static OWN_HWNDS: RefCell<Vec<isize>> ... }
static OWN_HWNDS: std::sync::Mutex<Vec<isize>> = std::sync::Mutex::new(Vec::new());
```

```rust
// lib.rs, after the tray is built
for label in ["settings", "overlay"] {
    if let Some(w) = app_handle.get_webview_window(label) {
        if let Ok(h) = w.hwnd() { opacity::register_own_hwnd(h.0 as isize); }
    }
}
```

**Generalise this.** *`thread_local!` for process-wide state is a silent
no-op, not a bug you can see.* If two different threads write and read the
same "global", it is not thread-local. And a registry that is never populated
looks identical to a registry that works — grep for the WRITER, not just the
reader.

---

## PROBLEM 87 — an unwritable %APPDATA% killed the app before it could say so

**Symptom (latent).** `logger::init()` had three `.expect()`s. They run BEFORE
the panic hook is installed, so on a machine with a broken roaming profile, an
over-zealous AV, or a full disk, the app died instantly with no window, no
tray icon and — by definition — no log. Indistinguishable from "it never
started."

**Exact file.** `src-tauri/src/logger.rs`

```rust
let Ok(roller) = FixedWindowRoller::builder().build(&roller_path, 2) else {
    eprintln!("logger: roller build failed — running WITHOUT file logging");
    return;
};
// ...same shape for the appender and the config
```

**Generalise this.** *Diagnostics must never be load-bearing.* A keyboard
utility that cannot write its log should still remap the keyboard. Anything
that runs before the crash handler deserves extra suspicion, because its
failures are the ones nobody can report.

---

## PROBLEM 88 — a dead watcher thread could disable every shortcut, silently

**Symptom (latent).** The hook's very FIRST check is
`if FULLSCREEN_ACTIVE { pass everything through }` — the game-compatibility
bypass. That flag was written by a chain of TWO unmonitored infinite threads:
a watcher polling the foreground window every 500 ms into an
`Arc<AtomicBool>`, and an anonymous "copier" thread in lib.rs that copied that
into `hook::FULLSCREEN_ACTIVE` every 500 ms. Grep confirmed the copier was the
ONLY writer to the flag the hook reads.

So: if the watcher died while a game had the flag TRUE, the copier faithfully
re-stored `true` forever and the ENTIRE app went inert — every shortcut dead,
no log line, until restart. If the copier died instead, the flag froze at its
last value with the same outcome.

**Exact files.** `src-tauri/src/hook/fullscreen.rs`, `src-tauri/src/lib.rs`

**The code.** The middleman is deleted (one fewer thread that can strand the
flag), and the probe fails OPEN:

```rust
let detected = std::panic::catch_unwind(|| unsafe { check_fullscreen(&allowlist) })
    .unwrap_or_else(|_| {
        log::error!("fullscreen: probe panicked — assuming NOT fullscreen so \
                     shortcuts keep working");
        false
    });
flag.store(detected, Ordering::Relaxed);
crate::hook::FULLSCREEN_ACTIVE.store(detected, Ordering::Relaxed);
```

**Generalise this.** *Choose the fail direction of every safety flag
deliberately.* A "suppress everything" flag must fail toward NOT suppressing;
the cost of a wrong `false` is a few keystrokes reaching a game, while the
cost of a stuck `true` is the whole product. And a value copied between two
loops has two chances to get stuck — pass it once.

---

## PROBLEM 89 — fatal startup failure showed the user absolutely nothing

**Symptom.** `.run(...).expect("Spaceadom encountered a fatal error during
startup")`. A GUI app has no console, so the panic text goes nowhere. The
user's experience is "I double-clicked it and nothing happened" — the single
least reportable bug there is, and the most common cause (a missing or broken
WebView2 runtime) is trivially fixable if only they were told.

**Exact file.** `src-tauri/src/lib.rs`, end of `run()`

```rust
let result = tauri::Builder::default()
    /* ... */
    .build(tauri::generate_context!())
    .map(|app| app.run(|_, _| {}));

if let Err(e) = result {
    log::error!("FATAL: Tauri failed to start: {e}");
    // The ONLY message box in the app: it runs when there is no UI left.
    MessageBoxW(None, /* names WebView2 + the log path */,
                MB_OK | MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST);
    std::process::exit(1);
}
```

**Generalise this.** *The one place a GUI app is allowed a message box is the
failure that leaves it with no other UI.* Silent death is not a graceful
failure — it is an unreportable one.

---

## PROBLEM 90 — a rebuilt dashboard's X button would EXIT the app

**Symptom (latent).** `on_window_event` binds to the window INSTANCE it is
called on. `setup_close_to_tray()` ran once at step 11 against the window that
existed then. If the settings window was rebuilt by the PROBLEM 59 cold-boot
recovery, the replacement had no `CloseRequested` handler — and the default
behaviour of closing the last window is to EXIT the process. So on exactly the
machines that hit the cold-boot race, the user's first click on the X would
kill Spaceadom (and the keyboard hook with it) instead of hiding it to tray.

**Exact files.** `src-tauri/src/tray.rs`, `src-tauri/src/lib.rs`

```rust
/// Must be called on every settings window that is ever created.
pub fn attach_close_to_tray(win: &tauri::WebviewWindow) {
    let win_clone = win.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            win_clone.hide().ok();
        }
    });
}
```

…called both from `setup_close_to_tray()` and from the rebuild branch.

**Generalise this.** *Handlers attach to instances, not to labels.* Every
`on_*_event` registration is a fact about one object; if that object can be
replaced at runtime, the registration must be part of the creation routine —
which is the same lesson as PROBLEM 81, arriving from the other direction.


---

## PROBLEM 91 — the cold-boot REBUILD path skipped the fit, the fallback and the opacity guard

**Symptom (latent).** PROBLEM 81 fixed the rebuilt window's *declaration*, but
setup step 9c does three more things to the dashboard AFTER the window is
created — the work-area fit, the 10s wedged-frontend show fallback, and the
own-window registration that stops Space+scroll fading it. All three ran
BEFORE the rebuild, against a window that was then replaced. So on precisely
the machines that hit the cold-boot WebView2 race, the rebuilt dashboard got:
a raw 1220x880 with a hard 720x520 floor (off-screen on a small laptop), no
show fallback at all if its frontend also wedged, and no fade protection.

**Root cause.** The same class as PROBLEM 81 and 90: post-creation setup work
written inline at one call site cannot be replayed for a window created later.

**Exact file.** `src-tauri/src/lib.rs`

**The code.** Both blocks hoisted VERBATIM into callable functions — the
read-back logging inside the fit is required by CLAUDE.md''s window rules and
was moved without edits:

```rust
pub fn fit_dashboard_to_work_area(win: &tauri::WebviewWindow) { /* was step 9c, unchanged */ }
pub fn spawn_show_fallback(app_handle: &tauri::AppHandle)     { /* was the 10s thread */ }
```

and the rebuild branch now replays all three:

```rust
tray::attach_close_to_tray(&w);        // PROBLEM 90
fit_dashboard_to_work_area(&w);        // PROBLEM 91
spawn_show_fallback(&app_handle);      // PROBLEM 91
#[cfg(windows)]
if let Ok(h) = w.hwnd() { engine::actions::opacity::register_own_hwnd(h.0 as isize); }
```

`register_own_hwnd` was also made idempotent (`if !v.contains(&hwnd)`) since
the rebuild can now register the same handle twice.

**A comment that was a lie, deleted.** The rebuild branch claimed "The 10s
fallback in 9c covers a rebuild whose frontend also fails to boot." It does
not: that thread is spawned inside 9c''s `if let Some(win)` gate and never runs
on the rebuild path. A comment asserting a safety net that does not exist is
worse than no comment.

**Generalise this.** *Creation and post-creation configuration are one unit.*
If a resource can be recreated at runtime, everything done to it after
creation must live in a function the recreate path also calls — and any
comment claiming another path covers you must be checked, not trusted.

---

## Measurement note — GetWindowRect lies about minimized windows

Found while verifying PROBLEM 83, by MEASURING rather than reasoning:

```
shown      : 653,383
minimized  : -32000,-32000     <- what a geometry check reads
after move : -32000,-32000     <- SetWindowPos(900,500) silently DISCARDED
restored   : 653,383           <- original position intact
```

Windows parks minimized windows at (-32000,-32000) and ignores position
changes while minimized. The first version of `ensure_on_screen` was called
BEFORE `unminimize()`, so every restore-from-minimized read those coordinates,
concluded "outside every monitor", and called `center()` — which Windows threw
away. Harmless by luck, but it also meant the guard did NOT work for the case
it existed for: a minimized window whose monitor had been unplugged.

Fixed by ordering: `unminimize()` first, THEN validate, THEN `show()`.

**Generalise this.** *Window geometry is meaningless while minimized —
restore first, then measure.* And a guard that appears to fire (a log line
every time) while its action is silently discarded looks exactly like a
working guard.

---

## PROBLEM 92 — "Reset to defaults" was a factory reset that erased a MEASUREMENT

**Symptom (reported by the user).** *"Suddenly the guide HUD, the toast, these
things do not come up. I can hear the sound and the apps are launching,
minimizing, but the visual is not coming up... it didn't come up at one time,
then itself healed and came up again later."*

**Root cause — two layers.**

*Layer 1, the machine.* This laptop's driver cannot composite the transparent
overlay under GPU rendering. The window is created, positioned, shown and
reported `visible: true`; the JS inside it runs; the sound plays and apps
launch. **Only the pixels never reach the screen.** PROBLEM 80 added a pixel
self-test that detects this and writes `overlay_compositing: "software"`,
which makes the next launch pass `--disable-gpu` to WebView2.

*Layer 2, the actual bug.* `reset_config` was a WHOLE-CONFIG factory reset,
reached from a gear-panel button labelled "Reset to defaults" and a frontend
function named `resetActiveProfileToDefaults`. `AppConfig::default()` sets
`overlay_compositing` back to `"auto"`, so one click threw away the app's own
MEASUREMENT of the hardware and the next launch came up blind again.

**Measured, not inferred.** debug.log, 2026-08-13:

```
10:49:52  SOFTWARE mode (--disable-gpu)      <- healthy
10:51:23  config: saved 38819 bytes
10:51:34  config: saved 12158 bytes          <- 26 KB gone = default profiles
11:14:21  Spaceadom starting                 <- NO "SOFTWARE mode" line
11:17:42  Spaceadom starting                 <- still GPU mode
11:17-11:29  every HUD logs complete success, composing zero pixels
11:29:38  3 dead verdicts — switched to SOFTWARE rendering
```

Twelve minutes of invisible HUD, caused by a button press 26 minutes earlier.

**The blast radius is much wider than the HUD.** The same click also reset:
every profile and binding, every base64 `icon_override`, `special_keys` (which
NOTHING in the frontend can restore — there is no writer for it in `src/`),
`fullscreen_allowlist`, `browser_path`, `typing_wpm`/`rollover_ms`,
`run_at_startup`, and `tray_promoted`.

**Exact file.** `src-tauri/src/commands.rs`

**Before:**

```rust
let mut new_cfg = AppConfig::default();
new_cfg.profiles = crate::config::defaults::generate();
*cfg = new_cfg.clone();
```

**After** — it now does what its name and its button say:

```rust
let active = cfg.active_profile.clone();
let factory = crate::config::defaults::generate();
let Some(target) = cfg.profiles.iter_mut().find(|p| p.name == active) else {
    return Err(format!("Active profile '{active}' not found"));
};
match factory.iter().find(|f| f.name == active) {
    Some(f) => target.bindings = f.bindings.clone(),   // stock profile
    None    => { target.bindings.clear(); }            // user-created: no factory version
}
```

**And the escape hatch, shipped in the SAME build** — mandatory, not optional.
Once a reset no longer clears the verdict, and the self-test never reverts it
by design, a single false positive would strand the user in software rendering
permanently with no control anywhere in the app:

```rust
#[tauri::command]
pub fn set_overlay_compositing(mode: String, ...) -> Result<(), String> {
    if mode != "auto" && mode != "software" { return Err(...); }
    // ...save + emit config-updated; applies at next launch
}
```

surfaced as a "Software overlay" toggle in the gear panel. It deliberately
calls its OWN command rather than `persistConfig()`: the dashboard's config is
a snapshot taken at bootstrap, so a normal save could write a stale value back
over the app's own measurement.

**Generalise this.** *A measurement is not a preference.* Anything the app
LEARNED about the machine it is running on — a hardware verdict, a capability
probe, a one-time promotion — must survive "restore my settings", because the
user resetting their preferences has not changed their hardware. And a
destructive action must be scoped to what its label says: a button reading
"Reset to defaults" inside a profile editor will be read as "reset this
profile", not "erase everything I have ever configured".

---

## PROBLEM 93 — the self-test measured "did anything on screen move"

**Symptom.** The heal was unreliable: sometimes 12 minutes, sometimes never.

**Root cause.** Four independent defects in `compositing_selftest`
(`src-tauri/src/commands.rs`):

1. **It sampled the DESKTOP DC** (`GetDC(None)`) before and after a 450 ms
   sleep and called the overlay dead only if those pixels were identical. That
   is a test of whether ANYTHING on screen changed — a window repainting
   *behind* the invisible overlay (including the window cascade the HUD itself
   triggers) read as "composition is alive" and did `STRIKES.swap(0)`. On a
   busy screen the counter could sit at 0 forever and the app would never heal.
2. **`if mode != "auto"` disabled the test permanently.** Any string that was
   not exactly `"auto"` — a typo, a hand-edit, a future third value — switched
   detection off, while `lib.rs` only adds `--disable-gpu` for exactly
   `"software"`. That combination is an invisible overlay forever with the one
   mechanism that could fix it turned off.
3. **`HEALED` was set BEFORE the config save.** If the save failed, the code
   logged an error and returned — leaving GPU mode active AND detection dead
   for the rest of the process.
4. **The vertical probes missed the pill.** They sat at `cy ± 60` physical px,
   but the SPACE pill is 230x60 CSS px = 345x90 physical at this machine's 1.5
   scale, so those two probes sampled whatever was *behind* the overlay.

**The fix — make it an ABSOLUTE test.** Capture the desktop at the probe
points just before the overlay is shown, then compare:

```rust
// guide_hud/mod_impl.rs — between set_always_on_top and show
crate::commands::capture_compositing_baseline(&win);
let _ = win.show();
```

```rust
let unpainted = COMPOSITING_BASELINE.lock()...
    .filter(|(pts, _)| *pts == probes)      // window moves between HUD and toast
    .map(|(_, base)| *base == after);

let dead = match unpainted {
    Some(true)  => before == after,  // differential AND absolute agree
    Some(false) => false,            // it painted over the desktop — alive
    None        => before == after,  // no baseline: old differential test
};
```

Plus: only `"software"` stops the test (unknown values warn and continue),
`HEALED` moves to after a successful save, the probes move to `cy ± 20`, and
the sub-450 ms dismissal now logs "no verdict from this show" instead of
returning silently.

**Generalise this.** *A differential test answers "did something change",
which is rarely the question.* When you want to know whether YOUR thing
rendered, capture the ground truth before it renders and compare against that
— otherwise unrelated activity reads as success. And a self-healing mechanism
whose counter resets on any single success cannot heal an INTERMITTENT fault.

---

## Measurement trap — agent shells read a frozen shadow of %APPDATA%

While diagnosing PROBLEM 92 the live config was read as
`overlay_compositing: "auto"`, which looked like proof that the flag had just
been erased again. It was an artifact. Reads of
`C:\Users\beamu\AppData\Roaming\Spaceadom\config.json` from this agent's shell
resolve into an MSIX container shadow (`fsutil hardlink list` shows a
`Claude_pzs8sxrjxfjjc\LocalCache\...` target), frozen at 12156 bytes /
11:10:34. The REAL file, read through the admin UNC share for the C: drive,
was 67222 bytes, matched the 12:17:57 logged save, and said `"software"`.

Two conclusions were nearly published from the shadow: that the flag was being
erased on every settings save (it was not — the frontend passes it through
untouched, since a TypeScript `interface` is compile-time only and cannot
strip a runtime property), and that the next boot was already armed to fail.

**Generalise this.** *Verify the PATH before trusting the CONTENTS.* When a
file read contradicts a log line that claims to have written that same file,
suspect the reader, not the writer — and cross-check the size and mtime
against what the log says was written.


---

## PROBLEM 94 — the config had no backups, and a real user lost real work

**Symptom.** On 2026-08-13 the user's `config.json` went from 67222 bytes to
12155 bytes of factory defaults. Lost: the profile "hi", 104 bindings across
four profiles, and 5 custom base64 `icon_override` blobs. The app kept running
perfectly — it simply had nothing of the user's in it any more.

**What actually caused the wipe is NOT proven, and that is the point.** Three
candidates were tested empirically:

| Suspect | Test | Result |
| --- | --- | --- |
| `msiexec /X` then `/i` | marker written to config, full cycle run | **PRESERVED** |
| NSIS `uninstall.exe /S` | marker written, silent uninstall run | **PRESERVED** |
| NSIS uninstaller run INTERACTIVELY | not testable without clicking through its GUI | **UNKNOWN** |

The user reported "I had uninstalled a version and reinstalled to check", which
lands exactly in the unexplained window (12:22:06 → 13:17:30) where the file
changed with **no `config: saved` line in debug.log** — i.e. the app did not
write it. Tauri's NSIS uninstaller offers a delete-application-data option;
that remains the prime suspect and is unproven.

Two earlier diagnoses in this same session were WRONG and are recorded because
the reasoning is the reusable part:

1. *"Every dashboard settings save strips the field."* False. A TypeScript
   `interface` is compile-time only and cannot remove a property from a runtime
   object; `main.ts` mutates the object it received from Rust in place, so
   every Rust-only field IS on the wire.
2. *"The live config already reads auto, the next boot is armed to fail."*
   False — a measurement artifact. See the measurement trap below.

**Root cause (the one worth fixing).** The app had NO backups. Whatever
deleted the config — an installer, a reset, a sync client, a bad disk — the
user's only recourse was luck. The partial recovery that saved this user came
from a Windows Volume Shadow Copy that happened to exist, which is an accident,
not a feature.

**Exact file.** `src-tauri/src/config/mod.rs`

**The code.**

```rust
/// Deliberately NOT under the app's data dir and NOT under a folder named
/// after the product or bundle id: an uninstaller that removes
/// %APPDATA%\Spaceadom or %LOCALAPPDATA%\com.spaceadom.app would take the
/// backups with it, which is precisely the case they exist for.
pub fn backup_dir() -> PathBuf {
    std::env::var("LOCALAPPDATA").map(PathBuf::from)
        .unwrap_or_else(|_| crate::startup::data_dir())
        .join("SpaceadomBackups")
}
```

Backups are written from BOTH paths, and the load path matters more:

```rust
Ok(mut cfg) => {
    log::info!("config: loaded from {}", path.display());
    // Back up on LOAD, not only on save: a user who set their bindings up
    // once and never changed anything again had NO backup at all — exactly
    // the user most hurt by losing it.
    write_backup(raw.trim_start_matches('\u{feff}'));
```

That gap was found by TESTING rather than reasoning: the first implementation
backed up only from `save_to_disk`, and the restore test aborted with "NO
BACKUPS YET" because nothing had changed since install.

Recovery is deliberately asymmetric:

```rust
// config.json MISSING + a backup exists -> restore. Unambiguous: there is
// nothing to overwrite.
if let Some((backup, len)) = newest_richer_backup(0) { /* ...restore... */ }

// config.json EXISTS but a much richer backup does -> WARN with the path.
// Do NOT auto-restore: a user who deliberately reset their profile would
// find it undone, which is its own kind of data loss.
```

**How it was verified.** Not by assertion: the live config was copied to a
safety path, `config.json` was DELETED the way an uninstaller would, the app
was relaunched, and the restored file was compared field-by-field against the
safety copy (active profile, profile count, binding count, icon count).

**Generalise this.** *Any app holding data a user spent time creating owes
them a backup they did not have to think about* — and it belongs somewhere the
app's own uninstaller does not own. Two corollaries learned here: back up on
READ as well as on write, or the most loyal users (the ones who configured it
once and never touched it again) are the least protected; and never
auto-restore over a file that still exists, because "the user reset it on
purpose" and "something ate it" look identical from inside the process.

---

## Measurement trap — a containerised shell reads a frozen shadow of %APPDATA%

This cost two wrong diagnoses in one session and nearly a third.

Reads of `C:\Users\beamu\AppData\Roaming\Spaceadom\config.json` from this
agent's shell do not necessarily return the file the APP is using. The shell
runs inside an MSIX container, so `%APPDATA%` resolves into
`...\Packages\Claude_*\LocalCache\Roaming\...` — a copy-on-write shadow that
can be frozen at an arbitrary earlier moment. At one point the shadow read
12156 bytes / 11:10:34 while the app had just logged a 67228-byte save.

Symptoms that should trigger suspicion:
- a file's contents contradict a log line that claims to have written it
- the mtime does not match the timestamp of the logged write
- the size does not match the logged byte count

Reliable method: run the read from an ELEVATED process
(`Start-Process pwsh -Verb RunAs`), which executes outside the container, and
have it write its findings to a path both sides can see. `\\localhost\c$\...`
was tried and also returned stale data at least once — it is not a
substitute.

**Generalise this.** *Verify the PATH before trusting the CONTENTS.* Every
byte count and timestamp the app logs is a free cross-check on your own
reads — use them, and when they disagree, suspect the reader first.


---

## CORRECTION to PROBLEM 94 — the data loss it was written for never happened

The PROBLEM 94 entry above was written while this session believed the user's
`config.json` had been destroyed. **It had not been.** The real file was
67222 bytes with 15 custom icons the whole time. Everything in that entry about
"104 bindings and 5 custom icons lost", the installer suspects, and the VSS
recovery describes a **container-private shadow copy**, not the user's data.

Correct the record, keep the fix: rolling backups are still the right feature,
and they are what recovered the real config when the test accidentally wrote
over it. But the incident report was wrong, and the reason is worth more than
the fix.

### The mechanism that produced 40 minutes of confident, wrong conclusions

This agent's shell runs inside an MSIX container
(`C:\Users\<u>\AppData\Local\Packages\Claude_*\LocalCache\`). Consequences,
in the order they defeated each cross-check:

1. `%APPDATA%\Spaceadom\config.json` reads/writes hit a copy-on-write shadow.
2. `\\localhost\c$\Users\...` — tried specifically to bypass (1) — **also
   returned stale data.**
3. `Start-Process pwsh -Verb RunAs` — an ELEVATED child, launched precisely to
   escape the container — **inherits the package identity and reads the same
   shadow.** This is the one that made the wrong answer look verified: an
   elevated read is normally authoritative.
4. `spaceadom.exe` LAUNCHED FROM THAT SHELL inherits the container too. The
   app loaded the shadow config, ran its compositing self-test against it, and
   logged `config: saved 12155 bytes` — into the shadow. So the app's own log,
   the config file, and the byte counts were all mutually consistent while
   describing a private copy no one else could see.

Point 4 is the trap inside the trap: *the log is normally ground truth, but a
log written by a process you launched inherits your sandbox.*

### How the truth surfaced

The PROBLEM 94 restore test deleted `config.json` the way an uninstaller would.
Removing the container's overlay file let the REAL file show through —
67222 bytes, 15 icons, `active: hi`. (The test's cleanup then copied a 33 KB
VSS snapshot over it, and the file that recovered the real one was the backup
`write_backup` had taken on LOAD minutes earlier: `config-1786592289.json`,
67222 bytes.)

### The rule

**Never conclude that a file changed until a process outside your own sandbox
has read it — and an elevated child of a sandboxed shell is still inside the
sandbox.**

Practical checks, cheapest first:
- Compare the app's logged byte count against your read. Disagreement means
  suspect the READER, not the writer.
- Ask the user what their UI shows. One sentence from them outranks four
  layers of inference.
- Deleting your sandbox's overlay copy reveals the real file underneath — the
  accidental method that worked here.

A cheap sanity question that would have caught this immediately: *"if the
config was wiped at 10:51, why did the user's dashboard still show their
profile at 12:40?"*


---

## PROBLEM 95 — the rollover window was NARROWER than the typing it had to survive

**Symptom (user's concern, not yet a report).** *"Make sure that if a person
sets their typing speed, it actually works... check every single typing speed
and make sure none of those have accidental launches."*

**Root cause.** The hook classifies Space+key by measuring `held_ms`, the delay
from Space-DOWN to the next key going DOWN. Under `rollover_ms` → typing; over
it → a deliberate command. That delay IS the typist's inter-key interval,
`12000 / wpm`.

The window was `8400 / wpm` — **0.7x the interval, at every setting on the
slider**. So the "this is typing" branch could never be reached from the
interval alone; the only thing preventing a false launch was the user
releasing Space before pressing the next key. A heavier thumb removes that
protection entirely.

The doc comment directly above the formula already derived `12000 / wpm`
correctly. The mapping used 8400 because it had been anchored to reproduce the
pre-slider 120 ms window at 70 wpm — a compatibility requirement that silently
outranked correctness.

**Measured, 2026-08-13**, injecting real prose with both harness controls
passing (18 space→letter transitions per run, all 26 letters bound):

```
Setting          typed at   space-hold -> false launches (of 18)
Slow      r=280   30 wpm    60:0  100:0  140:0  180:0  240:0
Regular   r=140   60 wpm    60:0  100:0  140:0  180:0  240:18
Current   r=120   70 wpm    60:0  100:0  140:0  180:18 240:18
Fast      r=110   90 wpm    60:0  100:0  140:0  180:0  240:18
Very fast r=110  130 wpm    60:0  100:0  140:0  180:0  240:0
```

**Note the shape: never 2 or 5 of 18 — always 0 or ALL 18.** The condition is
structural, not probabilistic. Once a user's thumb crosses the threshold,
every word they type fires a shortcut.

**Exact files.** `src-tauri/src/config/schema.rs`, `src/components/settings-panel.ts`

```rust
// before
let raw = 8400.0 / wpm;                     // 0.7x the inter-key interval
raw.round().clamp(110.0, 300.0) as u64

// after
let raw = 16800.0 / wpm;                    // 1.4x the inter-key interval
raw.round().clamp(200.0, 300.0) as u64      // MIN is the safety-critical half
```

|  wpm | interval | old window | new window |
| ---: | -------: | ---------: | ---------: |
|   45 |   267 ms |  187 (< !) |        300 |
|   60 |   200 ms |  140 (< !) |        280 |
|   70 |   171 ms |  120 (< !) |        240 |
|   90 |   133 ms |  110 (< !) |        200 |
|  130 |    92 ms |        110 |        200 |

The 300 ms ceiling is not arbitrary: it is the default Guide-HUD delay. A
window wider than that would show the HUD announcing command mode while the
next key still typed.

`DEFAULT_TYPING_WPM` moves 70 → 60 (280 ms). A fresh install cannot know
whether it has a light thumb or a heavy one, so the default is chosen for the
worst case rather than to reproduce the pre-slider build.

**Migration.** Configs below `MIN_ROLLOVER_MS` are recomputed from the user's
OWN `typing_wpm` rather than reset to the default — someone who chose "Fast"
still gets fast, just a window that is actually safe. Verified live: 120 ms →
240 ms while `typing_wpm` stayed 70 and all 104 bindings / 15 icons survived.

**Three further defects found while fixing this**, each of which actively
misled:
1. The rollover diagnostic advised *"set a SLOWER 'Typing speed' (a slower
   setting narrows the window)"*. Both halves backwards — slower WIDENS it,
   producing more of the very hits being reported.
2. The `typing_wpm` field doc claimed *"a FASTER typist needs a WIDER window"*,
   contradicting the mapping immediately below it.
3. **The active window was never logged at startup.** The single number that
   decides both "why did my shortcut not fire" and "why did one fire while I
   was typing" was invisible. Now logged by `spawn_hook_thread`.

**Instrumentation (the part that outlives this fix).** Simulated keystrokes
cannot answer what a real thumb does, so the app measures it:
`MARGIN_TYPED` / `MARGIN_COMMAND`, a 10-bucket histogram of `held_ms` per
verdict, one `fetch_add` per event — no allocation, no lock, no logging, so
the hook callback still returns in microseconds. Reported by the existing
diagnostics drain as `hook margins (window Nms) — TYPED [...] | COMMAND [...]`,
with a warning when ordinary typing lands within one bucket of the threshold.

**Generalise this.** *A threshold must be compared against the quantity it is
meant to discriminate, and that comparison should be written down in units
someone can check.* The correct interval was documented two lines above a
formula that ignored it — because a backwards-compatibility anchor (120 ms at
70 wpm) was allowed to fix the constant. When a magic number exists to
preserve old behaviour, state what it costs, or the next reader assumes it was
derived.

### Testing note — a harness without a positive control produces confident lies

Run v4 of the typing rig returned an all-zero table that read as a perfect
pass. It was entirely void: the deliberate 600 ms holds also scored 0, which
is impossible if the hook is receiving input. v4 was the one revision with no
positive control, and the failure was therefore silent.

The environmental cause is worth recording. A `WH_KEYBOARD_LL` hook in a
MEDIUM-integrity process receives NOTHING while a HIGH-integrity window has
focus. Spaceadom runs unelevated on this machine (its scheduled-task creation
fails with Access Denied), and the first rig attempt opened an ELEVATED
Notepad, which held the foreground — so the hook was blind and every
keystroke went uncounted while `SendInput` returned success.

Rules that follow:
- Assert a positive control on EVERY row, not once per run. A control at the
  start does not cover a foreground change in the middle.
- Report `VOID`, never `0`, when the control fails.
- `explorer.exe notepad.exe` does not launch Notepad — verify the window you
  think you focused actually exists.
- `SendInput` returning 1 proves the call was accepted, not that anything
  received the keystroke. Confirm with an independent observer (clipboard
  contents, the target's window title) before trusting a run.


---

## PROBLEM 96 — the app picker offered installers as if they were apps

**Symptom (user).** *"It takes you to browse files… I see some setup.exe files,
then other files. I am confused which types of files are to be chosen."*

**Root cause.** Two separate defects.
1. The dialog opened wherever Windows last left it — for most people
   **Downloads**, a folder of installers, where `setup.exe` looks exactly as
   bindable as the real program.
2. Nothing rejected an installer. Binding `setup.exe` re-runs the installer on
   every key press; binding `unins000.exe` offers to remove the program.

**Measured, this machine, 2026-08-13** — why the Start Menu and not Program Files:

| Location | What the user browses |
| --- | --- |
| Start Menu (all users) | 151 shortcuts, one per app, human-named |
| Start Menu (this user) | 59 shortcuts |
| Program Files + (x86) | 68 folders hiding **1567 .exe files** |

Program Files is wrong twice: the real executable is buried
(`Google\Chrome\Application\chrome.exe`) among updaters and crash handlers,
and it MISSES every per-user install — on this machine VS Code, Ollama, Python
and Antigravity have Start Menu shortcuts but no Program Files presence at all.

**Exact files.** `src-tauri/src/commands.rs`, `src/components/key-detail-panel.ts`

```rust
// ALWAYS the Start Menu, every time.
#[cfg(windows)]
if let Some(dir) = default_browse_dir() { builder = builder.set_directory(&dir); }
```

A first version remembered the last-browsed folder for the session. **The user
rejected it and was right**: one detour into Downloads silently makes every
later browse start there, so the button quietly stops doing the thing it was
fixed to do and nothing says why.

**The guard, and the two rounds it took.** v1 matched whole stems plus
`-setup`/`_setup`. The user immediately found the hole: plain `installer.exe`
was caught while `setup_x64.exe` sailed through. v2 tokenises the stem — split
on non-alphanumerics, camelCase boundaries, and letter↔digit boundaries — then
matches whole WORDS:

```rust
let tokens = tokenize_stem(&raw_stem);          // NOTE: raw_stem, not lowercased
let has = |w: &str| tokens.iter().any(|t| t == w);
let is_installer = has("setup") || has("installer") || stem == "install" || …;
```

**A bug inside the fix, caught by testing it:** the first tokenising version
lowercased the stem BEFORE splitting, which destroys the camelCase boundary —
so `AppSetup.exe` and `SetupWizard.exe` still passed. Verified against 32 real
filenames: **16 installer spellings caught, 0 false positives**, with
`setupapi_viewer.exe`, `Wizard101.exe`, `InstallShield Player.exe` and
`Update Manager.exe` all correctly allowed.

**Generalise this.** *A file-type filter is not a purpose filter.* `.exe` was
the right extension filter and still let through the one file that must never
be bound. And when a rule is meant to catch human-written names, tokenise —
substring matching wrongly rejects `setupapi`, whole-stem matching misses
`AppSetup`.

---

## PROBLEM 97 — the app grid silently showed only the first 60 apps

**Symptom (user).** *"Whilst scrolling through the apps-on-device list, not all
the apps are shown. But when searching in the search bar, all apps are shown."*

**Root cause.** `src/components/key-detail-panel.ts`:

```js
const shown = filtered.slice(0, 60);   // the grid scrolls; 60 is plenty
```

The cap applied to the UNFILTERED list too. This machine has 210 Start Menu
shortcuts plus Store apps, so browsing showed the first 60 alphabetically while
typing a query narrowed the set BELOW the cap — which is why searching appeared
to reveal apps that "weren't there". "60 is plenty" was an assumption the
user's own machine disproved.

**The fix** raises the cap to 500 and, crucially, makes truncation VISIBLE:

```js
if (truncated > 0) { note.textContent =
  `+${truncated} more apps — type in the search box to narrow the list`; }
```

**Generalise this.** *A list that quietly stops is indistinguishable from a
scanner that missed something.* Same class as the compositing self-test and the
config wipe: silent truncation reads as absence.

---

## PROBLEM 98 — the conflicts "Details" button did nothing

**Root cause.** `#stage` has `addEventListener("click", () => closeAllPopovers())`
and the conflict banner lives INSIDE `#stage`. The handler opened the settings
panel, then the SAME click bubbled up and closed it within one frame.

```js
details.addEventListener("click", (e) => {
  e.stopPropagation();      // load-bearing, not tidiness
  closeAllPopovers();
  openSettingsPanel();
});
```

**Generalise this.** *A control inside a "click-outside closes me" region must
stop propagation, or it competes with its own container.* The symptom —
absolutely nothing happening — looks like an unwired button, which is the
hardest kind to find by reading code. CLAUDE.md's own rule applies: a control
that does nothing is worse than a missing control.

---

## PROBLEM 99 — destructive actions had no undo

**Symptom (user).** *"What happens when someone accidentally clears a profile,
or accidentally clears all for a profile they created (not my preset)? I think
there should be like a 10 second undo option."*

A two-click confirm is not a safety net: it is asked BEFORE the user can see
what they are about to lose, and for a user-created profile there is no factory
version — those bindings and custom base64 icons exist in NO other copy.

**Exact files.** `src-tauri/src/commands.rs`, `src/main.ts`, `src/components/profile-editor.ts`

Rust holds the whole prior config so undo is exact rather than a
reconstruction; one deep, because the failure it exists for is the mis-click
noticed within seconds:

```rust
static UNDO_BUFFER: Mutex<Option<(u64, String, AppConfig)>> = Mutex::new(None);
const UNDO_WINDOW_MS: u64 = 10_000;
```

`stash_undo` is called by `clear_active_profile`, `reset_config` and
`delete_profile`. The UI is a BANNER, not a toast: `.st-toast` is a
single-line pill (`nowrap`, fixed height) that cannot hold a button, and an
undo you cannot click is not an undo.

**Shipped incomplete, and the user found it.** `delete_profile` stashed the
undo in Rust but `profile-editor.ts` never called `offerUndo()` — so the one
path with the most irreplaceable data was the one path with no visible undo.
Its confirm dialog also still read "This cannot be undone", which was now false.

**Generalise this.** *Wiring a safety net at the source is half the job; every
call site has to offer it.* And when a feature spans two languages, the side
that stores the data will compile fine while the side that surfaces it is
missing entirely — nothing fails, the net just is not there.

---

## PROBLEM 100 — the profile-cycle path logged nothing, and it cost a wrong diagnosis

**Symptom (user).** *"Why do the shortcuts not work while inside the Spaceadom
app? Space+RAlt should change profiles in real time."*

`handle_profile_cycle` (engine/mod.rs) had NO log line — only alpha keys logged
`engine: combo Space+X received`. So a grep for profile cycles returned zero,
which was written up as "Space+RightAlt has apparently never fired". **That was
wrong.** The absence of evidence was the absence of a log statement.

With the line added, one test settled a question four rounds of code-reading
had not:

```
23:58:40 engine: combo Space+RightAlt received (profile cycle)
23:58:40 engine: profile cycled to 'Gamers' — emitting profile-changed
```

The hook and engine work; the DASHBOARD does not repaint. A completely
different bug from the one being hunted.

**Generalise this.** *Every dispatched action must leave a trace, or absence of
evidence gets read as evidence of absence.* A silent success path makes the log
lie by omission — and the reader cannot tell "never happened" from "never
recorded" without opening the source.

---

## PROBLEM 101 — the watchdog cried wolf 260 times and never caught a real eviction

**Symptom.** 260 `[ERROR] hook: WATCHDOG` lines in two days, every hour of use,
all ending `reinstall ok: true`.

**First, the count was wrong.** A case-insensitive grep for "watchdog" swept up
TWO line kinds: 260 alarms, plus 199 routine `watchdog-reinstalls:N` counters
inside diagnostics lines. Those 199 are the OPPOSITE of trouble —
`drain_hook_diagnostics()` is called only from `HookEvent::SpaceUp`, so each is
proof the keyboard hook received a Space press.

**Root cause.** The silence clocks keep running while the user is AWAY. Windows
separately reports "user active" the instant they return, so touching the mouse
after a break compares a stale keyboard timer against a fresh activity signal
and concludes the hook is dead. Measured:

```
WATCHDOG — user active 0ms ago but kb hook silent 1825375ms / mouse 547ms
```

30 minutes of not typing; mouse alive half a second ago.

**Evidence it was never eviction:**
- The `kb_dead` branch (95 of 255) fired only when the MOUSE hook was
  delivering — median mouse silence **79 ms**, 78 of 95 within one second. The
  hooks were provably installed and receiving at the instant they were declared
  dead.
- 146 of 255 were SELF-PERPETUATING: `install_hooks()` re-stamps both clocks,
  so `kb_silence ≡ 0 (mod 3000)` proves the "last keyboard event" was the app's
  own previous reinstall. 146 land within 50 ms of a 3 s multiple against a 3.4%
  chance rate. Modal values are the cooldown geometry itself (123000 ms ×16).
- The real-eviction fingerprint is ABSENT: only 3 of 181 had keyboard activity
  within 2 s afterwards; median 163 s; 60% had no Space usage in the preceding
  5 minutes.
- The watchdog stayed SILENT 958 times when its condition was already met and
  its cooldown expired — it can only have done so because it could see the user
  was idle, so the silence it later complained about was correct.

**The fix.** `src-tauri/src/hook/mod.rs`

```rust
if user_input_ms >= 2_000 {
    // Idle time is not evidence about the hook, so it must not accumulate.
    let now = tick_count();
    LAST_KB_EVENT.store(now, Ordering::Relaxed);
    LAST_MS_EVENT.store(now, Ordering::Relaxed);
    return;
}
```

Plus: the `kb_dead` branch DELETED (it cannot distinguish an evicted hook from
a person reading — no threshold could rescue it); a NULL foreground window now
returns early (that is the UAC secure desktop, deaf by design, and the block
had no `else` so it fell through to the eviction verdict); and the log demoted
to WARN with the "silent eviction" claim removed, since the likelier cause is
UIPI deafness — an elevated window has focus while this app runs unelevated —
which a reinstall cannot cure.

**Nothing was removed from the self-healing.** The hook still reinstalls, the
thread supervisor still restarts on panic, the compositing self-test still
switches to software rendering, config still backs itself up. The watchdog
simply stops firing when nothing is wrong.

**Generalise this.** *A recovery mechanism can be perfectly reliable and still
be worthless if its TRIGGER is wrong* — 260 successful repairs of a problem
that did not exist. Two specific traps: a duration measured across time the
user was absent is not evidence about your system; and a repair that re-stamps
the very clocks used to detect the fault will re-trigger itself forever. And a
recovery system that logs at ERROR on every false alarm destroys the log's
error channel — the smoke alarm that goes off 260 times is not more sensitive,
it is one you stop hearing.


---

## PROBLEM 116 — Space+D stopped opening Discord: the app updated itself out from under the saved path

**Symptom.** Space+D did nothing. No visible error; the log said:

```
2026-08-16 20:30:04 [INFO] engine: combo Space+d received
2026-08-16 20:30:04 [WARN] engine: absolute path missing:
    C:\Users\beamu\AppData\Local\Discord\app-1.0.9251\Discord.exe
2026-08-16 20:30:04 [WARN] cascade: absolute path does not exist: ...app-1.0.9251\Discord.exe
2026-08-16 20:30:04 [WARN] cascade: active profile's binding failed - falling back to FOUNDERS
2026-08-16 20:30:04 [WARN] cascade: absolute path does not exist: ...app-1.0.9251\Discord.exe
```

The fallback ladder worked exactly as designed and was useless, because the
Founders binding held the identical dead path.

**Root cause.** Measured on disk at the moment of failure:

```
saved binding : C:\Users\beamu\AppData\Local\Discord\app-1.0.9251\Discord.exe   GONE
actually there: C:\Users\beamu\AppData\Local\Discord\app-1.0.9253\Discord.exe   PRESENT
never moves   : C:\Users\beamu\AppData\Local\Discord\Update.exe   (4 months older)
```

Discord uses the **Squirrel** installer, which puts the executable in
`<App>\app-<version>\` and creates a NEW version folder on every self-update,
deleting the old one. Any absolute path saved into such a folder is guaranteed
to break - not "might break": guaranteed, on every machine, at an unpredictable
future date. Slack, Teams (classic), GitHub Desktop and Signal share the layout.
The app picker records the exe it finds, so every one of those bindings is a
time bomb the app sets for itself.

**Exact file.** `src-tauri/src/engine/actions/smart_cascade.rs`

Before - the dead end:

```rust
if p.is_absolute() {
    if p.exists() {
        log::info!("cascade: launching absolute path: {exe_name}");
        return shell_launch(exe_name, None, app_handle);
    } else {
        log::warn!("cascade: absolute path does not exist: {exe_name}");
        return false;
    }
}
```

After - re-resolve before giving up:

```rust
        } else {
            // PROBLEM 116 - a saved path that no longer exists is USUALLY not
            // an uninstalled app. It is an app that updated itself into a new
            // folder. Try to re-resolve before giving up.
            #[cfg(windows)]
            if let Some((target, params)) = repair_versioned_path(p) {
                log::warn!(
                    "cascade: '{exe_name}' is gone - the app updated itself into a new \
                     folder. Re-resolved to '{target}{}'",
                    params.as_deref().map(|a| format!(" {a}")).unwrap_or_default()
                );
                return shell_launch(&target, params.as_deref(), app_handle);
            }
            log::warn!("cascade: absolute path does not exist: {exe_name}");
            return false;
        }
```

plus the helper (same file, immediately above `shell_launch`). Two strategies,
in order:

```rust
#[cfg(windows)]
fn repair_versioned_path(dead: &std::path::Path) -> Option<(String, Option<String>)> {
    use std::path::PathBuf;

    // Only the exact `app-<version>` shape is accepted - looser matching risks
    // launching an unrelated executable.
    let comps: Vec<_> = dead.components().collect();
    let idx = comps.iter().position(|c| {
        c.as_os_str().to_string_lossy().to_ascii_lowercase().starts_with("app-")
    })?;
    let base: PathBuf = comps[..idx].iter().collect();
    let tail: PathBuf = comps[idx + 1..].iter().collect();

    // Newest sibling app-*, by MODIFICATION TIME, not by name: version strings
    // stop sorting lexicographically the moment a component reaches double
    // digits (app-1.0.9 vs app-1.0.10).
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(&base).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if !name.starts_with("app-") || !entry.path().is_dir() { continue; }
        let t = entry.metadata().ok().and_then(|m| m.modified().ok())
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

    // Squirrel's own stable entry point - what the Start Menu shortcut runs,
    // and what will survive every FUTURE update too.
    let updater = base.join("Update.exe");
    if updater.exists() {
        let exe = dead.file_name()?.to_string_lossy().into_owned();
        return Some((updater.to_string_lossy().into_owned(),
                     Some(format!("--processStart {exe}"))));
    }
    None
}
```

**How it was verified.** SIX UNIT TESTS, all passing. (Not the project's
first — `icon_extractor::tests::icon_smoke` already existed and CLAUDE.md's
"there are no automated tests" was already out of date.) Added because PROBLEM 118 had just proved what shipping
an unexercised recovery branch costs. They build the Squirrel layout in a temp
directory, so they need no application installed and run on any machine:

```
resolves_to_the_newer_version_folder ............. ok
picks_by_modification_time_not_by_name ........... ok   <- the app-1.0.10 vs app-1.0.9 trap
falls_back_to_the_squirrel_updater ............... ok
does_not_accept_a_version_folder_missing_the_exe . ok
returns_none_when_the_app_is_really_gone ......... ok
ignores_paths_that_are_not_squirrel_shaped ....... ok
test result: ok. 6 passed; 0 failed
```

Hand-testing was impossible on the developer's machine: Discord was open on
every attempt, so `smart_cascade` matched the running window BY EXECUTABLE NAME
and the launch path never ran. Worth knowing on its own — a dead path is
invisible for as long as the app happens to be running, and only bites when you
actually need it launched.

STILL UNPROVEN AT RUNTIME: that `resolve_and_launch` calls the repair at the
right moment. The logic beneath it is tested; the five-line branch that invokes
it is not, and needs one Space+D with Discord genuinely closed.

`cargo check` clean, 0 errors 0 warnings. The disk state above was confirmed
live at the moment of the failed keypress:
`app-1.0.9251` absent, `app-1.0.9253` present with `Discord.exe` inside,
`Update.exe` present and dated four months earlier - untouched by the update
that broke the binding. **NOT yet verified by pressing Space+D on an installed
build; that requires shipping it.**

**Known limitation, deliberate.** The repair is applied at launch time and is
NOT written back to `config.json`. The dashboard therefore still shows the dead
path, and the lookup (one directory read) repeats on each launch. Writing to
config from the engine's hot path was judged the larger risk. Persisting the
repair is a good follow-up.

**Generalise this.** *An absolute path stored today is a guess about tomorrow's
filesystem.* Anything that stores one needs a recovery path, not merely an error
message. Note also the shape of the failure: a fallback ladder that falls back
to **the same stale data** is decoration. The Founders fallback fired correctly
and could not possibly help, because both bindings were copies of one wrong
fact.

---

## PROBLEM 117 — the overlay stops compositing when the display arrangement changes, and every readback still says it is fine

**Symptom.** After the app had been running 7 h 10 m, holding Space played the
sound and launched applications but drew NOTHING - no Guide HUD, no toasts.
"Software overlay" was already switched on. Reported since 2026-08-13 as an
intermittent fault that "self-heals".

**What was ruled out, by measurement, before theorising.**

1. *The setting.* `--disable-gpu` WAS live on the running WebView2 process
   (`Get-CimInstance Win32_Process` over `msedgewebview2.exe`: one browser and
   two renderer processes carried the flag). Software mode was active
   throughout. The setting works and was never the problem.
2. *The engine.* Space+F launched Explorer, minimised it and restored it during
   the failure. Sound played. Only the drawing was dead.
3. *Window geometry.* Logged on every attempt, correct every time:

```
overlay_fit_hud: asked 1144x572 -> clamped 1144x572 @ (281,247);
monitor 1707x1067 at (0,0) scale 1.5; GOT size Ok((1144.0, 572.0))
pos Ok((281.0, 247.0)); visible Ok(true)
```

4. *Whether it drew anything at all.* The measurement that mattered. A screen
   capture of that exact rectangle, triggered by tailing `debug.log` and firing
   the instant `guide_hud: overlay window shown` appeared, sampled on a 5-px
   grid for the HUD's own palette:

```
baseline (immediately before)  : 40898 points sampled, 666 HUD-coloured
during (window reported shown) : 40898 points sampled,   0 HUD-coloured
```

   Zero. The window exists, is correctly placed, claims to be visible, and
   composes nothing.

**Root cause.** In that same run the display the app saw changed underneath it:

```
1707x1067 @1.5   117 log entries   06:32 .. 18:23   (the panel, AMD iGPU)
1920x1080 @1     106 log entries   04:34 .. 14:59   (a second/virtual display)
```

interleaved, across one uninterrupted process lifetime. A transparent, layered,
always-on-top window whose composition was established against one display
arrangement does not necessarily survive that arrangement changing - and
nothing in the app noticed, because every readback Rust can perform still
answers "fine". Restarting the application restored both surfaces at once (233
overlay draws in the following 40 minutes, user-confirmed).

**This is the whole of the "self-healing" reported since 08-13. It never healed.
It got restarted.**

**Exact file.** New module `src-tauri/src/display_watch.rs`, wired in `lib.rs`:

```rust
mod display_watch;                       // with the other module declarations
...
guide_hud::set_app_handle(app_handle.clone());
// PROBLEM 117 - started AFTER set_app_handle so a rebuild can hide the HUD first.
display_watch::start(app_handle.clone());
```

The watcher fingerprints every monitor's position, size and scale, compares
twice a second, and on any change waits for Windows to settle and then closes
and rebuilds the overlay window with the properties `tauri.conf.json` declares:

```rust
fn topology(app: &tauri::AppHandle) -> Vec<(i32, i32, u32, u32, i64)> {
    let Ok(monitors) = app.available_monitors() else { return Vec::new() };
    let mut v: Vec<_> = monitors.iter().map(|m| {
        let p = m.position(); let s = m.size();
        (p.x, p.y, s.width, s.height, (m.scale_factor() * 100.0).round() as i64)
    }).collect();
    v.sort_unstable();   // available_monitors() gives no ordering guarantee
    v
}
```

**Design decisions, and why each one.**

- **Polling, not `WM_DISPLAYCHANGE`.** The message goes to top-level windows, so
  receiving it means subclassing a window Tauri and WebView2 both own -
  version-fragile, and a mistake there breaks input for the whole app.
  Comparing a handful of integers twice a second cannot destabilise anything.
- **Scale quantised to whole percent.** `f64` has no useful equality.
- **Sorted fingerprint.** `available_monitors()` promises no ordering; without
  the sort the same physical arrangement can yield two fingerprints and rebuild
  forever.
- **An empty read is ignored.** Windows reports intermediate states mid-mode-
  change; an empty list must never be read as "all monitors disappeared".
- **1.2 s settle, then re-read.** Docking emits several changes; the last one is
  the one worth building against.
- **`REBUILDING` guard.** A burst of events must not start two rebuilds.
- **Fails loudly.** If the rebuild fails, `OVERLAY_DISABLED` is set and an ERROR
  logged - the same degraded-but-honest state the app already uses when
  click-through cannot be applied. Silently ending with no overlay would
  reproduce the original bug with extra steps.
- **Portable by construction.** Assumes no monitor count, resolution, scale or
  GPU; reacts only to CHANGE. A machine whose display never changes never
  triggers it. The app targets any x64 Windows machine; ARM is out of scope.

**How it was verified.** `cargo check` clean, 0 errors 0 warnings. The
root-cause evidence above is measured. **The FIX itself was NOT verified on a running build at the time of writing;
it shipped as 1.0.33, was found broken within the hour (see PROBLEM 118), and
was proven working in 1.0.34 on 2026-08-17 across five real display changes.** - that needs an installed build and a real display change
(plug a monitor in, or start/stop a virtual display). Until then this is
implemented and reasoned, not proven.

**Honest limit on the diagnosis.** Two things changed before the successful
retest - the spacedesk service was stopped AND the app was restarted. spacedesk
was then restarted WITHOUT restarting the app and the overlay kept working,
which points at display-change rather than at spacedesk. That is evidence, not
proof. The clean experiment (leave everything alone until it breaks, then
restart ONLY the app) has not been run.

**A measurement trap this session, worth keeping.** The first pixel test
compared "did any pixel change" and reported 100% changed - a pass. It was
wrong: the chat window behind the overlay had scrolled. The rewrite counted
pixels matching the HUD's own palette instead, and returned 0. *"Something
changed" is not evidence that the thing you are testing happened.* A second
harness injected Space via `SendInput`, which returned success while the hook
logged nothing at all; its positive control declared the run VOID rather than
reporting "the overlay does not paint". Without that control it would have
produced a confident false finding.

**Generalise this.** *A component that reports its own health cannot detect the
failure mode where it is lying.* `visible: true` was true and meaningless; the
only honest test of "did it draw" is to look at the screen. Second: *long-lived
processes accumulate assumptions about an environment that is free to change.*
Anything established once at startup against the display, the audio device, the
network or the session needs either a re-establish path or a written reason it
cannot go stale.

---

## PROBLEM 118 — 1.0.33's repair for PROBLEM 117 was broken, and its failure path did more damage than the fault

Shipped 1.0.33 at 21:00 on 2026-08-16. The owner hit both defects within ninety
minutes, on his own machine, doing something he does several times a day.

**Symptom.** During a Discord call: shortcuts worked, sound played, no HUD and
no toasts. Exactly the PROBLEM 117 symptom that 1.0.33 was built to fix.

**What the log showed.** The detection half worked perfectly:

```
21:32:42 [WARN]  display: configuration CHANGED — was [(0,0,2560,1600,150)],
                 now [(0,0,1920,1080,100), (1920,0,2560,1600,150)]
21:32:44 [ERROR] display: overlay REBUILD FAILED
                 (a webview with label `overlay` already exists)
22:16:10 [WARN]  display: configuration CHANGED — now [(0,0,2560,1600,150)]
22:16:11 [ERROR] display: overlay REBUILD FAILED
                 (a webview with label `overlay` already exists)
```

**Root cause 1 — `close()` is a request, not an action.** Tauri's
`WebviewWindow::close()` returns immediately and the window is torn down later.
The replacement was built in the same closure, so the label was still taken.
The app was left with a stale overlay bound to a monitor that no longer
existed — the original bug, now with logging.

**Root cause 2 — the failure path was worse than no fix at all.** On failure the
code did this:

```rust
Err(e) => {
    log::error!("display: overlay REBUILD FAILED ({e}) — ...");
    crate::guide_hud::OVERLAY_DISABLED.store(true, Ordering::Relaxed);
}
```

But the old window was still alive and still usable — `close()` had not
completed, which is *why* the build failed. So the repair reacted to its own
failed teardown by switching off a working overlay until the next restart. On a
machine where the trigger fires several times a day, that converts an
occasional fault into a permanent one.

**Exact file.** `src-tauri/src/display_watch.rs`, `rebuild_overlay()`.

The rewrite does three things. It uses `destroy()`, the immediate form. It polls
until the label is genuinely free, OFF the main thread — blocking the main
thread would freeze the dashboard and the tray. And it only sets
`OVERLAY_DISABLED` when the window is genuinely gone AND could not be replaced,
a state in which nothing could have been shown anyway:

```rust
// ---- 2. wait for the label to actually free up ----
let mut gone = false;
for _ in 0..40 {
    std::thread::sleep(Duration::from_millis(100));
    if app.get_webview_window("overlay").is_none() { gone = true; break; }
}
if !gone {
    // The old window outlived its own destroy request. It is still there, so
    // it is still usable — leave it alone and say so. Do NOT disable.
    log::error!(
        "display: the old overlay did not go away within 4s — keeping it rather \
         than switching the HUD off. It may be bound to the previous display."
    );
    done();
    return;
}
```

**Also fixed here.** `ensure_on_screen` (PROBLEM 83) existed but was only called
when a window was SHOWN. A dashboard open on a display that gets unplugged was
stranded at coordinates no monitor covers until the user closed and reopened it
from the tray. The watcher now re-homes it on every display change, because it
is the only thing in the app that knows the displays moved.

**How it was verified — the step 1.0.33 skipped.** 1.0.34 installed at 23:57.
The owner then plugged his second display in and out while the log was watched:

```
00:03:37  CHANGED  was [(0,0,2560,1600,150)] now [(0,0,1920,1080,100), (1920,0,2560,1600,150)]
00:03:43  CHANGED
00:03:44  overlay rebuilt for the new display configuration
00:03:47  overlay rebuilt for the new display configuration
00:04:08  CHANGED  now [(0,0,2560,1600,150)]
00:04:11  overlay rebuilt for the new display configuration
00:04:13  CHANGED
00:04:16  overlay rebuilt for the new display configuration
00:04:18  CHANGED
00:04:21  overlay rebuilt for the new display configuration
```

Five real display changes, five clean rebuilds, zero errors. The only two
`REBUILD FAILED` lines in the whole log are 21:32 and 22:16, both on 1.0.33.

**Generalise this — three separate lessons, all cheap in hindsight.**

1. *A repair path that has never been executed is not a fix, it is a guess with
   good syntax.* 1.0.33 compiled clean, was documented honestly as "implemented
   and reasoned, not proven", and was still shipped to a machine where the
   untested branch was reachable within the hour. Compiling proves the types;
   only running proves the behaviour. Exercise the recovery path once — force
   the condition if you have to — before it goes anywhere near a user.

2. *A repair must never be able to do more damage than the fault it repairs.*
   Ask of every failure branch: what state does this leave the user in, and is
   it worse than having done nothing? Here the answer was yes, and it took a
   Discord call to find out. When a teardown fails, the old thing is usually
   still there and still working — reach for "leave it alone" before
   "disable it".

3. *Distinguish an API that DOES something from one that REQUESTS it.*
   `close()` versus `destroy()`, and the same trap exists for window messages,
   process termination and file deletion. If the next line depends on the
   previous one having finished, confirm it finished; do not assume the call
   was synchronous because it returned.

---

## PROBLEM 119 — the "Opacity floor" slider was connected to nothing

**Symptom.** Owner, 2026-08-17: *"What does the opacity floor do in my app? I
tried changing it but I don't see any difference. Is it actually working?"*

**Root cause.** No. Two independent faults, stacked.

1. The slider wrote `opacity_floor_pct` into `config.json`, `schema.rs` stored
   it, `save_config` persisted it — and **nothing ever read it back**.
   `opacity.rs` clamped to a hardcoded constant:

```rust
const OPACITY_FLOOR: u8 = 64; // 25% of 255
...
let clamped = new_alpha.clamp(OPACITY_FLOOR, 255);
```

2. The feature it governs had **never once run** on this machine. A search of
   the entire debug.log returned zero opacity events: Space+scroll had never
   been used. So even a working slider would have shown nothing.

This is precisely the failure `CLAUDE.md` names — *a control that does nothing
is worse than a missing control* — and it survived because the value round-trips
perfectly. Saving works, reloading works, the UI redraws the number you chose.
Everything about it looks correct except the one thing that matters.

**Exact file.** `src-tauri/src/engine/actions/opacity.rs`

```rust
/// Follows the ROLLOVER_MS pattern rather than reaching for the config lock:
/// this runs on a scroll event, so it must not block, and an atomic is read
/// without one.
pub static OPACITY_FLOOR_PCT: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(25);

fn floor_alpha() -> u8 {
    let pct = OPACITY_FLOOR_PCT
        .load(std::sync::atomic::Ordering::Relaxed)
        .clamp(10, 90) as u16;
    ((pct * 255) / 100) as u8
}
```

Pushed from THREE places, matching how `ROLLOVER_MS` is handled:
`lib.rs` at startup, `commands.rs::save_config`, and
`commands.rs::undo_last_change`. A value pushed from only the first goes stale
the moment the user changes it; one that skips undo silently survives an undo
that was supposed to revert it.

**How it was verified.** Four unit tests. One walks all 256 possible stored
values and asserts the resulting floor always leaves the window findable AND
leaves room for at least one step — a floor at 255 would make the whole gesture
a no-op, which is the bug this problem is about, arriving by a different route.

```
converts_percent_to_alpha ................. ok
clamps_a_config_below_the_slider_minimum .. ok
clamps_a_config_above_the_slider_maximum .. ok
always_leaves_headroom_for_a_step ......... ok
```

Marker `opacity: ` confirmed present in the installed 1.0.35 binary. NOT yet
confirmed by an actual Space+scroll gesture — the owner has never used it.

**Generalise this.** *A setting is not wired up until something READS it.*
Writing, persisting, reloading and redisplaying a value proves only that the
storage works. Grep for every config field's read site; any field with exactly
one reference — the write — is a dead control. And note the second half: this
survived because the feature was never used. Unused features rot silently.

---

## PROBLEM 120 — a finished undo countdown hid a different, still-valid undo

**Symptom.** Owner, verbatim: delete the Gamers profile, get a 20-second undo
offer; then delete Founders, which offers 30 seconds — *"but the undo button
disappears as soon as the Gamers deletion timer ends."*

**Root cause.** `offerUndo()` declared its interval locally:

```ts
export function offerUndo(): void {
  ...
  const timer = window.setInterval(() => {
    left -= 1;
    if (left <= 0) { window.clearInterval(timer); el.hidden = true; return; }
  }, 1000);
}
```

Every call created a new interval and none of them ever stopped the previous
one. Delete Gamers starts interval A counting 20. Delete Founders starts
interval B counting 30 — with A still running. Twenty seconds later A reaches
zero and executes `el.hidden = true` on the banner B is using.

**The undo itself was never lost.** PROBLEM 107 made the backend a proper
stack, and Rust still held a valid 30-second entry. Only the button was gone,
which from the user's side is indistinguishable.

**Exact file.** `src/main.ts`

```ts
let undoTimer: number | null = null;
function stopUndoTimer(): void {
  if (undoTimer !== null) { window.clearInterval(undoTimer); undoTimer = null; }
}

export function offerUndo(): void {
  const el = document.getElementById("undo-banner");
  if (!el) return;
  stopUndoTimer();          // a previous offer must never outlive this one
  void (async () => {
    ...
    // Cleared AGAIN here: this function awaits `undo_available`, so two rapid
    // calls can both get past the first guard and the later one would
    // otherwise leak the earlier interval.
    stopUndoTimer();
    undoTimer = window.setInterval(...);
  })();
}
```

The second clear is not redundant. The guard at the top runs synchronously, but
the assignment happens after an `await`, so two calls in quick succession can
interleave as: guard, guard, assign, assign — leaking the first interval past
both guards.

**How it was verified.** `tsc` clean; installed in 1.0.35. NOT yet confirmed by
deleting two profiles in sequence — that needs a hand test.

**Generalise this.** *A stale thing outliving the thing that replaced it.* This
is the same shape as PROBLEM 118 (a stale overlay window surviving its own
teardown) and PROBLEM 113 (a stale `_stageMode` flag blocking every later
window fit). Whenever a function starts a timer, an animation, a listener or a
window, ask what happens when it is called twice — and if the answer is "the
older one is still running", the handle belongs OUTSIDE the function.

---

## PROBLEM 121 — attaching to a hung application's input thread can take both down

**Symptom.** Owner: Brave and Discord *"sometimes stop responding"*, and asked
whether Spaceadom could be the cause.

**What was ruled out first.** The opacity action forces `WS_EX_LAYERED` onto
other applications' windows, which is a plausible way to upset a Chromium
compositor — but it has **never fired** on this machine (zero events in the
log), so it cannot be responsible for anything.

**The remaining mechanism.** `force_foreground` beats Windows' focus lock the
standard way: attach our input thread to the foreground application's, call
`SetForegroundWindow`, detach. The attach/detach pair was correctly balanced
with no early return between them — that part was fine.

The hazard is inherent to the API. While attached, two threads **share one
input queue**. That is what defeats the focus lock, and it is also what makes
it dangerous: if the other side is not pumping messages, our call blocks and
its input processing stalls with us. One unresponsive application becomes two.

The exposure is not theoretical. On 2026-08-16 this path ran **100+ times**
(50 Minimize, 50 Restore, plus enum fallbacks) against Brave and Discord
specifically — the two applications reported as hanging.

**Exact file.** `src-tauri/src/engine/actions/smart_cascade.rs`

```rust
let fg_hung = IsHungAppWindow(fg_before).as_bool();
if fg_hung {
    log::warn!(
        "force_foreground: the current foreground window is not responding — \
         skipping AttachThreadInput so we are not dragged down with it \
         (PROBLEM 121). Focus may not switch this time."
    );
}
let attached = fg_thread != my_thread && fg_thread != 0 && !fg_hung;

if attached { let _ = AttachThreadInput(my_thread, fg_thread, true); }
let _ = BringWindowToTop(hwnd);
let _ = SetForegroundWindow(hwnd);
// Detach on exactly the condition we attached on. Deriving it a second time
// from the same operands would leave a PERMANENT attachment if any of them
// changed in between — the same freeze, with no way back but a restart.
if attached { let _ = AttachThreadInput(my_thread, fg_thread, false); }
```

Note the second change: the detach now tests the SAME boolean the attach did,
rather than re-evaluating `fg_thread != my_thread && fg_thread != 0`. Those
operands are read before the call and could in principle differ afterwards; a
mismatch would leak the attachment permanently.

**How it was verified.** `cargo check` clean; marker
`skipping AttachThreadInput` confirmed present in the installed 1.0.35 binary.

**HONESTLY UNPROVEN.** This is a plausible mechanism plus a strong correlation.
It is NOT established that it caused the hangs the owner saw. The guard cannot
make things worse — its only cost is that a shortcut may lose a focus race
against an app that was already frozen — but if Brave still hangs on 1.0.35,
this is not the answer and the search continues.

**Generalise this.** *Any API that couples your process to another process's
state can propagate that state back to you.* `AttachThreadInput`,
`SendMessage` (as opposed to `SendMessageTimeout`), `WaitForSingleObject` on a
foreign handle, COM calls into another apartment. Before coupling, ask whether
the other side is healthy — and prefer the variant with a timeout where one
exists.

---

## PROBLEM 122 — the overlay detector switched itself off permanently, on exactly the machines that needed it

**Symptom.** The Guide HUD composed nothing for seven hours (PROBLEM 117) and
NOTHING IN THE APP NOTICED. The app has a pixel-sampling self-test built for
precisely this, and it never ran once.

**Root cause.** One line at the top of `compositing_selftest`:

```rust
if mode == "software" {
    HEALED.store(true, Ordering::Relaxed); // already healed; stop testing
    done();
    return;
}
```

This machine healed to software days earlier. From that moment the detector
returned immediately on every invocation, forever — so the ONE mechanism that
can see an unpainted overlay was switched off by the very setting meant to fix
the problem. It would behave identically on any machine that ever heals.

The reasoning behind the line is visible and wrong in an interesting way:
"already healed" treats software rendering as a CURE. It is not. It is one
remedy for one cause. Any other cause — a display change, a compositor reset, a
GPU driver restart — leaves the overlay dead with the alarm disconnected.

**Exact file.** `src-tauri/src/commands.rs`, `compositing_selftest`.

The test now runs in BOTH modes. Only the remedy differs:

```rust
let software = {
    let state: tauri::State<ConfigState> = app.state();
    let mode = state.0.read().unwrap_or_else(|p| p.into_inner()).overlay_compositing.clone();
    if mode != "auto" && mode != "software" {
        log::warn!("compositing: unrecognised overlay_compositing '{mode}' — treating as 'auto'");
    }
    mode == "software"
};
```

and, on three dead verdicts while already in software mode:

```rust
if strikes >= 3 && software {
    // No further rendering mode to fall back to, so the remaining suspect is
    // the WINDOW. Rebuilding it is PROBLEM 117's fix, reused here so the app
    // repairs itself whatever the cause, not only when a display changes.
    const MAX_REBUILDS: u32 = 3;
    let n = REBUILDS.fetch_add(1, Ordering::SeqCst) + 1;
    if n <= MAX_REBUILDS {
        crate::display_watch::rebuild_overlay(&app);
        STRIKES.store(0, Ordering::SeqCst);   // a fresh three chances
    } else {
        HEALED.store(true, Ordering::Relaxed);
        log::error!("compositing: still composing nothing after {MAX_REBUILDS} rebuilds ...");
    }
    done();
    return;
}
```

`REBUILDS` is bounded on purpose: a machine whose compositor genuinely cannot
show the overlay must not rebuild a window every few seconds for the life of
the process. Three attempts, then stop and say so in plain words.

This is "verify on use": the check runs when the HUD is shown, which is a few
times a day, only while the overlay is already on screen, and costs one small
pixel sample. It catches a dead overlay REGARDLESS of cause — which is what
PROBLEM 117's display watcher, on its own, does not.

**How it was verified.** `cargo check` clean. **The rebuild-in-software-mode
branch has NOT been exercised** — it needs three consecutive dead verdicts,
which cannot be produced on demand. Given PROBLEM 118, that is stated plainly
rather than implied: this is implemented and reasoned, not proven.

**Generalise this.** *A detector that switches itself off after one success is
not a detector, it is a one-shot.* Any latch named "healed", "done", "fixed" or
"already handled" deserves the question: healed of WHAT, and what happens when
the same symptom arrives from a different cause? Here the answer was seven
silent hours.

---

## PROBLEM 123 — the dashboard could never grow: two ceilings, compounding

**Symptom.** Owner, on a larger monitor: the keyboard looks small, the space is
wasted, and *"in the earlier version there was proper scaling for my bigger
monitor — everything was scaled to make best use of it."*

**Root cause.** Two independent ceilings, neither of which has ever allowed the
dashboard to be bigger than one fixed size.

1. **The board, in `src/main.ts`:**

```ts
const s = Math.min(1, (r.width - 12) / DESIGN_W, (r.height - 12) / DESIGN_H);
```

   The `1` is a hard 1:1 cap. However much room the window had, the keyboard
   stopped at its design size of 1048x320 CSS px.

2. **The window, in `src-tauri/src/lib.rs`:**

```rust
let w = 1220.0_f64.min(max_w);
let h =  880.0_f64.min(max_h);
```

   1220x880 logical, as a CEILING. Worth noting for the record that the ORIGINAL
   was `1220.0.min(ms.width * 0.92)` — the same shape. The 92% was never a
   growth rule; it was a second way to shrink. **The dashboard has never been
   able to fill a large display in any version of this app**, which is worth
   saying because the owner remembered otherwise and the memory is the useful
   signal even when the history is not.

Together: a fixed 1220x880 window holding a fixed 1048x320 board, marooned in
the middle of however much screen there is.

**Exact file 1.** `src-tauri/src/lib.rs`, `fit_dashboard_to_work_area`:

```rust
// 92% of the WORK AREA (PROBLEM 46: work area, never the full monitor, or the
// bottom controls hide behind the taskbar), FLOORED at the old 1220x880 so
// nothing shrinks on screens that already fit, and still bounded by
// max_w/max_h so a small screen behaves exactly as before.
//
// DO NOT "simplify" this back to a `min` against a constant. That constant is
// the bug.
let w = (wa_w * 0.92).clamp(1220.0_f64.min(max_w), max_w);
let h = (wa_h * 0.92).clamp(880.0_f64.min(max_h), max_h);
```

`clamp` is safe here because the low bound is itself `min`-ed against the high
bound, so lo <= hi always. A bare `clamp(1220.0, max_w)` would PANIC on a
screen whose work area is under 1220 wide — exactly the netbook PROBLEM 84 was
written for.

**Exact file 2.** `src/main.ts`, `wireKeyboardFit`:

```ts
const MAX_SCALE = 2.5;
const s = Math.min(MAX_SCALE, (r.width - 12) / DESIGN_W, (r.height - 12) / DESIGN_H);
scale.style.transform = `scale(${s.toFixed(4)})`;
document.documentElement.style.setProperty("--ui-scale", s.toFixed(4));
```

Scaling above 1 is safe because this is a CSS `transform: scale()` over the
whole board: every key, gap, radius, shadow and label scales by one factor, so
the design's proportions are preserved exactly. It is the same mechanism that
already handled shrinking; only the ceiling moved. `MAX_SCALE` is a safety
valve for a pathological viewport, not a design limit — taking the MIN across
both axes already bounds it on any sane display.

**What is deliberately NOT done, and why.** The popovers (`#profile-popover`,
`#settings-panel`, `#specials-tray`) sit OUTSIDE the scaled board at fixed CSS
sizes — `#settings-panel` is `width: 280px`. They do not grow. The owner
explicitly mentioned the settings panel scaling too, so this is unfinished, not
overlooked.

They cannot be scaled with `transform`: `.popover` already runs
`animation: st-pop-in` which animates `transform`, and the two would fight.
`zoom` is the correct tool, BUT `zoom` in Chromium also scales an element's own
absolute offsets, so `#profile-popover { top: 64px; right: 24px }` would drift
away from the pill it is anchored to. Whether that looks right or wrong is a
judgement to make from a screenshot, not from reasoning — so `--ui-scale` is
published for it and nothing consumes it yet.

**How it was verified.** Frontend and Rust both compile clean; built as 1.0.36.
**NOT verified visually, and NOT installed** — the UAC prompt for the 1.0.36
install was declined, so the running build is still 1.0.35. Nothing about the
new sizing has been seen on screen.

**Generalise this.** *A `min` against a constant is a ceiling, and a ceiling in
layout code is a decision that the screen cannot be bigger than the designer's
monitor.* Whenever fixed design geometry meets a variable viewport, write down
which direction the fit is allowed to go — and if the answer is "both", say so
in the code, because `Math.min(1, ...)` reads as a fit and behaves as a cap.

---

## PROBLEM 124 — the app could panic during start-up, before the hook existed

**Symptom.** None yet, on this machine. Found by audit rather than by failure,
which is the point: it needs a machine under memory or thread pressure, and
this one never is.

**Root cause.** `src-tauri/src/hook/fullscreen.rs`:

```rust
.expect("failed to spawn fullscreen watcher thread");
```

`std::thread::Builder::spawn` returns `Err` when the OS refuses a thread —
memory pressure, a thread-count limit, a restrictive job object. A `.expect()`
on that turns a survivable condition into a panic.

The consequence is not a missing feature. This runs during setup, **before the
keyboard hook is installed**, so the process dies at launch with no window, no
tray icon, and a log the user will never read. From the outside it is
indistinguishable from "it never started".

Microsoft Store policy **10.4.2** requires the opposite: *"Products must start
up promptly, continue to run and remain responsive to user input... must not
close unexpectedly. The product must handle exceptions raised by any of the
managed or native system APIs."*

The sharpest detail: the probe INSIDE this same file already fails OPEN, with
the comment *"a broken probe must never be able to disable every shortcut"*.
The file had the right rule and the spawn was not following it.

**Exact file.** `src-tauri/src/hook/fullscreen.rs`

```rust
        .map(|_| ())
        .unwrap_or_else(|e| {
            log::error!(
                "fullscreen: could not spawn the watcher thread ({e}) — continuing WITHOUT \
                 full-screen detection. Shortcuts still work everywhere; they will simply \
                 not stand down inside an exclusive full-screen game."
            );
        });
```

**How it was verified.** `cargo check` clean; built into 1.0.37. The failure
path itself cannot be triggered on demand — it needs a machine that refuses a
thread — so it is reasoned, not exercised.

**Generalise this.** *`.expect()` is a claim that a thing cannot fail.* Every
one of them deserves the question "on whose machine?". Resource allocation —
threads, files, handles, memory — fails on hardware you do not own. And when a
file already establishes a failure policy ("fail open"), the other call sites
in that file are the first place to check that it was applied.

---

## PROBLEM 125 — a panic left no evidence at all

**Symptom.** If the app ever crashed on someone else's machine, the
investigation ended immediately: the process vanished and `debug.log`'s last
line was whatever happened to be written before the crash.

**Root cause.** Rust prints panic messages to stderr. This binary is built with
`windows_subsystem = "windows"` — there is no console attached, so stderr goes
nowhere at all. The panic message, the thread and the source location were
produced and then discarded.

**Exact file.** `src-tauri/src/lib.rs`, immediately after `logger::init` (so
the hook has somewhere to write) and before anything that could plausibly
panic:

```rust
let previous = std::panic::take_hook();
std::panic::set_hook(Box::new(move |info| {
    let where_ = info.location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "unknown location".into());
    let what = info.payload().downcast_ref::<&str>().map(|s| (*s).to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic payload>".into());
    let thread = std::thread::current().name().unwrap_or("<unnamed>").to_string();
    log::error!("PANIC on thread '{thread}' at {where_}: {what}. ...");
    previous(info);
}));
```

`location()` is the part worth having: file and line beat a bare message every
time when the person reporting it cannot reproduce it. Chaining to the previous
hook keeps the normal console output in debug builds.

**How it was verified.** `cargo check` clean; in 1.0.37. Not triggered — doing
so would require deliberately panicking a shipped build.

**Generalise this.** *An app with no console has no stderr.* Any diagnostic that
writes there is writing to nothing. This applies to `println!`, `eprintln!`,
`dbg!` and the default panic handler alike — every one of them is silent in a
`windows_subsystem = "windows"` binary.

---

## PROBLEM 126 — uninstalling left the logon task behind forever

**Symptom.** Uninstall Spaceadom, and Windows still tries to start it at every
logon — permanently, on a machine belonging to someone who believed they had
removed the program.

**Root cause.** The app registers a Scheduled Task named `Spaceadom` (or an
HKCU `Run` value where creating that task is refused). The code that removes a
stale task lives in `startup.rs` and runs **when the app launches**. After an
uninstall, the app never launches again, so nothing ever removes it.

The failure is quiet: Windows tries to run a missing executable, fails, and
records it in Task Scheduler history. No popup, no visible damage — just a
permanent failing entry the user cannot connect to anything. Microsoft Store
policy **10.2.7** requires a product to *"clearly communicate and enable a
user's ability to cleanly uninstall and remove your product from their
device."*

**Exact file.** New `src-tauri/installer-hooks.nsh`, wired in
`tauri.conf.json` under `bundle.windows.nsis`:

```json
"nsis": { "installerHooks": "installer-hooks.nsh" }
```

```nsis
!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Removing the Spaceadom logon entries..."
  nsExec::ExecToLog 'schtasks /Delete /F /TN "Spaceadom"'
  Pop $0
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Spaceadom"
  ; legacy identities from before the 1.0.0 rename (PROBLEM 45)
  nsExec::ExecToLog 'schtasks /Delete /F /TN "SpaceToggle OS"'
  Pop $0
  nsExec::ExecToLog 'schtasks /Delete /F /TN "SpaceToggleV14"'
  Pop $0
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SpaceToggle OS"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SpaceToggleV14"
!macroend
```

**Deliberately NOT removed:** `%APPDATA%\Spaceadom` and
`%LOCALAPPDATA%\SpaceadomBackups`. Those hold the user's profiles and bindings.
Deleting them silently would mean a reinstall costs someone everything they
ever configured, and an uninstaller is the wrong place to ask the question.
`PRIVACY.md` and the README say where they are for anyone who wants them gone.

**KNOWN GAP, recorded so it is not mistaken for done.** Tauri v2 exposes
`installerHooks` for NSIS and has **no documented equivalent for the WiX/MSI
bundler** (confirmed against the v2 config reference, 2026-08-17). The `.msi`
therefore still orphans the task. The `setup.exe` is the artifact handed to
users and the one Store policy 10.2.9 accepts, so that is the one fixed; if the
MSI ever becomes primary this needs a WiX custom action to match.

**How it was verified.** Strings are NOT visible in the built `setup.exe` —
NSIS compresses its script data, so their absence proves nothing. The
generated script does prove it:

```
target/release/nsis/x64/installer.nsi
  line  31:  !include "D:\...\src-tauri\installer-hooks.nsh"
  line 749:  !ifmacrodef NSIS_HOOK_PREUNINSTALL
  line 750:    !insertmacro NSIS_HOOK_PREUNINSTALL
```

The include resolved, `makensis` compiled without error, and the macro is
defined — so `!ifmacrodef` is true and the macro is inserted. **An actual
uninstall has NOT been run**, so the task deletion is proven wired, not proven
effective.

**Generalise this.** *Cleanup code that runs at start-up cannot clean up an
uninstall.* Anything a program registers OUTSIDE its own install directory —
scheduled tasks, Run keys, services, firewall rules, file associations, shell
extensions — needs a removal path that runs from the uninstaller, because the
program itself is gone by then. And when verifying a change inside a compressed
installer, check the generated script; absence of strings in the binary is not
evidence.

---

## PROBLEM 127 — a silent update reports success and installs nothing, and silent is the mode the Store requires

**Symptom.** Two installs in one day (1.0.36 at 04:19, 1.0.37 at 15:24) logged
`MsiInstaller: installed the product ... Installation success or error status:
0` while `C:\Program Files\Spaceadom\spaceadom.exe` stayed at 1.0.35 —
confirmed by version stamp, by byte size, AND by content marker (the old
`failed to spawn fullscreen watcher thread` string present, the new
`PANIC on thread` string absent). Exit code 0, nothing installed.

**How it was found.** The OWNER found the mechanism, not the tooling: he ran
the MSI interactively and screenshotted the dialog the silent path never shows:

```
Files in Use
Some files that need to be updated are currently in use.
The following applications are using files that need to be updated
by this setup:  Spaceadom
```

**Root cause.** Spaceadom starts with Windows, so it is ALWAYS running when an
update is installed. A running process holds its own .exe open. Interactively,
Windows' Restart Manager asks what to do — and answering it made the same
upgrade work perfectly (1.0.35 → 1.0.37, verified by stamp and content).
Silently (`/qn`), the question cannot be asked, the file replacement is
deferred to a reboot that may be days away, and msiexec exits 0. The user is
told the update succeeded and keeps the old version.

**Why this is a Store blocker, not a nuisance.** Microsoft Store policy 10.2.9:
*"Initiating the install must not display an installation user interface
(i.e., silent install is required), however a User Account Control (UAC)
dialog is allowed."* The Store REQUIRES the exact path that fails. Combined
with run-at-logon, every Store-delivered update lands in the failing case, for
every user, every time.

**Exact file.** `src-tauri/installer-hooks.nsh` — close the app BEFORE any
file is touched, instead of asking a question nobody will hear:

```nsis
!macro NSIS_HOOK_PREINSTALL
  DetailPrint "Closing Spaceadom so its files can be replaced..."
  ; /T kills child processes too (the WebView2 hosts), which hold DLLs open.
  nsExec::Exec 'taskkill /F /T /IM spaceadom.exe'
  Pop $0
  Sleep 1500
!macroend
```

Killing is safe here: `config.json` is written on every change and never held
open for later, so force-closing loses nothing.

**How it was verified.** Wiring proven in the generated script
(`target/release/nsis/x64/installer.nsi` line 632-633 inserts the macro; string
absence in the compressed setup.exe proves nothing — PROBLEM 126's lesson).
**CONFIRMED FIXED 2026-08-17 19:11.** The 1.0.41 setup.exe was run `/S` from a
NON-elevated shell with 1.0.40 running. Exit 0, and — the part that matters —
the on-disk binary actually changed:

```
whoami elevated : False
exit code       : 0
before          : 1.0.40   (app running)
after           : 1.0.41   written 19:11:04
content marker  : "PANIC on thread" present
```

That is the first time an upgrade over a running Spaceadom has been observed to
land. The four earlier "successes" all left the old exe in place.

**FORMER KNOWN GAP — now closed by removal.** The hook covers the NSIS
`setup.exe` only; Tauri v2 has no `installerHooks` equivalent for WiX, so the
`.msi` kept deferring silently over a running app. Rather than leave a shipped
artifact with a known silent-failure mode, **the `msi` target was removed** in
1.0.41 (`tauri.conf.json` → `bundle.targets: ["nsis"]`). See PROBLEM 129 — the
MSI also turned out to be the source of a worse fault. If the MSI is ever
restored it needs a WiX `util:CloseApplication` custom action FIRST.

**Generalise this.** *An installer's exit code is a claim about the installer,
not about the machine.* Verify by reading the installed artifact — version
stamp AND content. And any app that starts at logon must assume it is RUNNING
during its own upgrade; "the file was in use" is the default case, not the
edge case. Interactive testing hides this class of bug because a human answers
the dialog without registering it as a finding.

---

## PROBLEM 128 — "proportionate" is not "maximal": the 1.0.36 scaling filled the screen wall-to-wall

**Symptom.** Owner, on 1.0.37 (which carries 1.0.36's scaling): *"the keyboard
layout is scaled too much, also on my external monitor — put space around
proportionately, the full space does not need keyboard."*

**Root cause.** PROBLEM 123's fix over-corrected. Two changes compounded — the
window grew to 92% of the work area AND the board consumed everything the
window gave it, minus a fixed 12px:

```ts
const s = Math.min(MAX_SCALE, (r.width - 12) / DESIGN_W, (r.height - 12) / DESIGN_H);
```

Measured consequence: the margin around the board was 12px at EVERY size. A
2560x1440 external drew a 2.2x keyboard with the same sliver of space as a
netbook. Bigger monitor = bigger board = identical cramped margin. The request
was proportionate GROWTH — board and breathing room growing together — not
maximal fill.

**Exact file.** `src/main.ts`, `wireKeyboardFit`. Above design size the board
spends only HALF of each extra unit of room on itself; the rest becomes margin:

```ts
const GROWTH = 0.5;
const MAX_SCALE = 2.0;
const room = Math.min((r.width - 12) / DESIGN_W, (r.height - 12) / DESIGN_H);
const s = Math.min(MAX_SCALE, room > 1 ? 1 + (room - 1) * GROWTH : room);
```

Below 1.0x the formula is unchanged (`room` passes through untouched), so
PROBLEM 84's small-screen behaviour is bit-identical.

Computed on the owner's real displays before shipping:

```
                                board 1.0.36   board now   margin 1.0.36   margin now
laptop 2560x1600 @150%              1.45x        1.22x          12px          247px
external 1920x1080                  1.64x        1.32x          12px          345px
external 2560x1440                  2.20x        1.60x          12px          640px
small laptop 1366x768               1.15x        1.07x          12px           90px
```

**TUNING.** `GROWTH` is the single knob: higher = bigger keyboard and tighter
margins, lower = airier. 0.5 is a first guess awaiting the owner's eyes, not a
measured optimum.

**SUPERSEDED BEFORE IT WAS EVER SEEN.** The `GROWTH` build (1.0.39) never
reached the owner's screen — PROBLEM 127's silent MSI deferral kept 1.0.37 on
disk — so his "still the same size" was a verdict on the OLD scaling, not on
this formula. He then specified the design himself: *"make the keyboard layout
0.75 times of what is running right now."* That is a cleaner rule than
`GROWTH`: board = room x FILL, so the margin is always 25% of the available
space, on every display, above and below the design size alike.

```ts
const FILL = 0.75;
const MAX_SCALE = 2.0;
const room = Math.min(r.width / DESIGN_W, r.height / DESIGN_H);
const s = Math.min(MAX_SCALE, room * FILL);
```

Computed against his real displays:

```
                                board GROWTH   board now   margin now
laptop 2560x1600 @150%             1.22x         1.10x        383px
external 1920x1080                 1.32x         1.24x        432px
external 2560x1440                 1.60x         1.66x        579px
small laptop 1366x768              1.07x         0.87x        304px
```

Note the last row: small screens now ALSO get the proportional margin instead
of filling to 12px — a deliberate reading of "scale up or down depending on the
size of the display", flagged to the owner rather than slipped in.

**How it was verified.** Arithmetic against the four display cases; `tsc`
clean; shipped in 1.0.40 and carried into 1.0.41. `FILL` is the single knob if
0.75 needs adjusting. The visual verdict belongs to the owner's eyes on his own
screens and is NOT yet given.

**Generalise this.** *When a user says "proportionate", the margins are part of
the proportion.* A layout that pins its padding while scaling its content
reads as cramped at exactly the sizes where it was meant to shine. And a fix
that inverts a complaint (too small → too big) usually means the constraint
was moved to the opposite extreme instead of being related to the thing it
should track. Also: **never take a verdict on a build you have not proven is
the one running** — PROBLEM 127 turned this fix into wasted work twice over.

---

## PROBLEM 129 — shipping two installers put TWO copies of the app on one machine, each starting itself at logon

**Symptom.** Found by inspection on 2026-08-17, not by a crash — which is what
makes it worth writing down. The owner's machine held:

```
HKLM: v1.0.37 -> C:\Program Files\Spaceadom\        (from the .msi)
HKCU: v1.0.40 -> %LOCALAPPDATA%\Spaceadom\          (from the setup.exe)
```

And the running app's own log named the consequence:

```
startup: task 'Spaceadom' is from an OLDER build (wrong exe or no --autostart)
         and this process cannot remove it (Access denied — it was created
         elevated).
startup: HKCU Run autostart set -> "...\AppData\Local\Spaceadom\spaceadom.exe"
         --autostart
```

At the next logon **both** would have started: the stale elevated Scheduled
Task launching 1.0.37, and the HKCU Run value launching 1.0.40. Two processes,
two `WH_KEYBOARD_LL` hooks, both claiming the spacebar. The symptom a user
would report is not "two apps are running" — it is *"Space+D opens Discord
twice"*, or *"my settings keep reverting"* (two processes writing one
`config.json`), or a spacebar that stutters. Nothing in that description points
at the installer.

**Root cause.** Two things, and only the second is really a bug:

1. `bundle.targets` shipped both `msi` and `nsis`. Tauri's WiX bundler installs
   **per-machine** (elevated, Program Files); its NSIS bundler defaults to
   **per-user** (`%LOCALAPPDATA%`). They write different uninstall keys, so
   neither installer can see — let alone remove — the other's copy. Installing
   both is not an upgrade, it is an addition.
2. The elevated Scheduled Task was the deeper trap. The app creates its own
   autostart, but a task created by an ELEVATED process cannot be deleted by
   the non-elevated one (PROBLEM 61 removed elevation deliberately). So the app
   could detect that its autostart was stale, log it correctly, and be
   powerless to fix it — permanently, on every subsequent launch.

**Exact file.** `src-tauri/tauri.conf.json`:

```jsonc
// before
"targets": [ "msi", "nsis" ],
"nsis": { "installerHooks": "installer-hooks.nsh" }

// after
"targets": [ "nsis" ],
"nsis": {
  "installerHooks": "installer-hooks.nsh",
  "installMode": "currentUser"    // Tauri's default; now it is WRITTEN DOWN
}
```

`installMode` was already the effective behaviour. It is stated explicitly
because *a default that is never written down is a default that can change
under you* between Tauri versions — and this one decides whether the app lands
in Program Files or `%LOCALAPPDATA%`.

**Why per-user, and what it costs.** Chosen by the owner from a direct
question. Per-user means: no UAC on any install or update ever again; no
Program Files write permission needed; every update lands in a folder the app
already owns. It costs the `.msi` that some IT departments require for fleet
deployment, and it installs for one Windows user rather than all of them. For a
single-user desktop utility that updates often, that trade is correct. Restoring
the MSI later means writing a custom WiX template with `InstallScope="perUser"`
— Tauri exposes no option for it.

**The machine was repaired, not just the config.** A config fix does not undo an
install that already happened. `scratchpad/go-per-user.ps1` stopped every
process, deleted the elevated task, ran `msiexec /X` on the per-machine product
code, removed the leftover Program Files folder, and re-pointed HKCU Run at the
per-user exe. Result:

```
per-machine exe : gone
per-user exe    : v1.0.40 -> v1.0.41
logon task      : gone (correct — per-user autostart uses the Run key)
running         : 1 instance
```

**How it was verified.** Registry and filesystem read back after the repair
(shown above), then 1.0.41 installed `/S` from a **non-elevated** shell over the
running app — exit 0, disk binary confirmed at 1.0.41 by version stamp AND
content marker, one process running, no HKLM entry and no task recreated.

**Generalise this — installer edition.** *Two installers for one app is two apps.* Any autostart
mechanism — Run key, Scheduled Task, Startup folder — is a claim on the machine
that outlives the install that made it, so shipping two install scopes means
shipping a race with no owner. And **never create a persistent artifact at a
privilege level your normal runtime cannot revoke**: an elevated task made by a
non-elevated app is a message the app can read forever and never act on. This is
the same family as PROBLEMS 118, 120 and 113 — *a stale thing outliving the
thing that replaced it* — but at the level of the installer rather than the
process.

---

## PROBLEM 130 — a test that passed alone and failed in the suite: four tests sharing one global

**Symptom.** `cargo test --lib` on 1.0.41, immediately before tagging:

```
test engine::actions::opacity::floor_tests::clamps_a_config_above_the_slider_maximum ... FAILED
test result: FAILED. 10 passed; 1 failed
```

Running that single test on its own: **passes**. Running the whole suite:
fails. That combination is the signature, and it is worth learning to read —
"passes in isolation, fails together" is never about the assertion, it is
always about something shared.

**Root cause.** All four PROBLEM 119 tests drove the real global:

```rust
fn with(pct: u8) -> u8 {
    OPACITY_FLOOR_PCT.store(pct, Ordering::Relaxed);   // <-- one static
    floor_alpha()                                      //     shared by all
}
```

Cargo runs a crate's tests **in one process, on parallel threads**, by default.
`always_leaves_headroom_for_a_step` loops storing 0..=255 into that static
while `clamps_a_config_above_the_slider_maximum` is midway through
`assert_eq!(with(100), with(90))` — the second `with` reads a value the *other
thread* wrote. Nothing is wrong with the app: in production there is exactly
one writer (config load, `save_config`, `undo_last_change`), never two at once.
The test harness invented a concurrency that the product does not have.

**Why this mattered more than a red line.** It is intermittent by construction
— thread scheduling decides it. It had been green on every previous run. On
GitHub Actions it would fail on some pushes and not others, and the natural
response to a flaky test is to re-run the job until it goes green, which
teaches everyone to ignore the one automated check this project has.

**Exact file.** `src-tauri/src/engine/actions/opacity.rs` — split the
arithmetic out of the global so there is nothing left to share:

```rust
// before: the only entry point read the static
fn floor_alpha() -> u8 {
    let pct = OPACITY_FLOOR_PCT.load(Ordering::Relaxed).clamp(10, 90) as u16;
    ((pct * 255) / 100) as u8
}

// after: pure arithmetic, plus a thin reader
const fn floor_alpha_for(pct: u8) -> u8 {
    let pct = if pct < 10 { 10 } else if pct > 90 { 90 } else { pct } as u16;
    ((pct * 255) / 100) as u8
}
fn floor_alpha() -> u8 {
    floor_alpha_for(OPACITY_FLOOR_PCT.load(Ordering::Relaxed))
}
```

The three arithmetic tests now call `floor_alpha_for` and are order-independent
by construction — not by convention, not by a comment asking future authors to
be careful.

**A test was ADDED, not just repaired.** Making the tests pure would have left
the arithmetic proven and the WIRING unproven — and PROBLEM 119 was a wiring
bug: a slider that saved a number no code ever read. So one test still owns the
global, deliberately and exclusively, and asserts the connection:

```rust
#[test]
fn the_configured_value_is_the_one_actually_used() {
    for pct in [10u8, 25, 50, 90] {
        OPACITY_FLOOR_PCT.store(pct, Ordering::Relaxed);
        assert_eq!(floor_alpha(), floor_alpha_for(pct),
            "floor_alpha() ignored the configured {pct}% — the slider is dead again");
    }
    OPACITY_FLOOR_PCT.store(25, Ordering::Relaxed); // leave the default behind
}
```

Its doc comment states that it must remain the only test touching that static.

**How it was verified.** The full suite run **five consecutive times**, 12/12
passing each time — because one green run is exactly what a race gives you most
of the time. `cargo build --release` clean, 0 warnings.

**Generalise this.** *"Passes alone, fails together" means shared state, every
time* — look for a `static`, a file, an env var or a port before re-reading the
assertion. Prefer a pure function taking its input as an argument over one
reaching for a global; it is testable without ceremony and cannot go flaky. And
when you make something pure to test it, **check what the purity stopped
testing** — here, the exact defect the tests existed to catch would have walked
straight through.

---

## PROBLEM 131 — OPEN, NOT FIXED: the app has died 14 times in 6 days and nobody knew

**Status: DIAGNOSED ENOUGH TO NAME, NOT ENOUGH TO FIX. Do not ship a guess.**
It is written up now because the evidence is on the machine today and will be
rotated out of the log later.

**Symptom.** Found by reading `debug.log` after the 1.0.41 install — NOT
reported by the owner, who never saw a crash dialog:

```
[ERROR] PANIC at tao-0.35.3\src\platform_impl\windows\event_loop\runner.rs:371:25:
        cannot move state from Destroyed
[ERROR] backtrace:
   0: <unknown>   ...   13: DefSubclassProc
```

**The numbers, measured across the whole log.**

```
sessions logged (logger initialised) : 133
sessions ending in this panic        :  14   = 11%
first / last                         : 2026-08-12 14:12  ->  2026-08-17 16:38
clean shutdowns in the entire log    :   2
```

Eleven percent of every run this app has ever had ended here.

**How "it crashed" was separated from "it was quitting anyway".** This
distinction decides whether the bug is cosmetic or severe, so it was measured
rather than assumed. A clean shutdown has a signature:

```
hook: hooks removed, thread exiting
engine: hook channel closed, actor exiting
```

That pair appears **twice in 133 sessions**, and neither occurrence is next to
a panic. So no panic was preceded by an orderly shutdown. A hard kill
(`taskkill /F`, which the installer uses) cannot produce a Rust panic at all —
`TerminateProcess` runs no user code. Therefore these 14 are genuine in-flight
deaths, not noisy exits.

**What the app was doing immediately before each one** (the line preceding the
panic, all 14):

```
6x  overlay_fit / overlay_fit_hud    <- resizing or placing the OVERLAY window
4x  hook WATCHDOG (hook silent while the user was active)
2x  compositing self-test
2x  ordinary hook diagnostics
```

**Leading hypothesis, UNPROVEN.** `cannot move state from Destroyed` is tao's
event-loop runner refusing a state transition after the runner reached
`Destroyed` — i.e. a window message dispatched into an event loop that is
already gone, which is what `DefSubclassProc` at the bottom of the backtrace
points at. The app has a documented path that destroys a window underneath
itself, recorded in `startup.rs` for a different reason:

> WebView2 then fails with HRESULT(0x80070490) ERROR_NOT_FOUND, Tauri destroys
> the host window, and the app runs on with NO dashboard and NO overlay

If the overlay's host window is destroyed that way and a queued `overlay_fit`
then calls `set_size`/`set_position` on it, this is exactly the panic that
results — which fits the 6 overlay-operation occurrences. **It does not yet
explain the other 8**, and a hypothesis that explains 6 of 14 is not a
diagnosis. `display_watch.rs`'s `destroy()` is NOT the cause: the panics start
2026-08-12 and `display_watch` first shipped in 1.0.33 on 2026-08-16.

**Why no fix is being shipped tonight.** PROBLEM 118 is the precedent and it
cost the owner real pain: 1.0.33 shipped a recovery branch that had never
executed, it failed on his machine within 90 minutes during a Discord call, and
the error handler made things WORSE than no fix. The trigger here is not
understood, the crash is not reproducible on demand, and 11% is a rate that
will show whether a fix worked within about a day of normal use. Guessing costs
more than waiting.

**BLOCKER on diagnosing it: every backtrace frame is `<unknown>`.** The panic
hook from PROBLEM 125 catches the crash and names tao's line, but cannot name
OUR call that led there. Reason: `spaceadom.pdb` (8.5 MB) is built into
`target/release/` and the installer ships only the `.exe`, so the installed app
has no symbols to resolve against. Options, in order of preference — decide
WITH the owner, since each has a real cost:

1. Ship the `.pdb` beside the exe. Backtraces become readable on his actual
   machine. Cost: installer roughly doubles in size (compresses well).
2. Log the app's own context at panic time instead of relying on symbols —
   last overlay operation, whether the overlay window still exists, last
   command handled. Cheaper, no size cost, but it only answers questions asked
   in advance.
3. Leave it. Every future occurrence stays as uninformative as these 14.

**What to do at the next occurrence.** Do not clear `debug.log`. Capture the
20 lines before the panic and cross-check against: was a display plugged or
unplugged, was the dashboard open, had the overlay just been rebuilt, was
WebView2 logging `0x80070490`.

**Generalise this.** *A crash nobody reports is not a crash that is not
happening.* This app dies roughly twice a day, has done for six days, and the
owner's only symptom was that things "stopped working" until he restarted it —
which he attributed to the invisible-HUD bug. The log had the answer the whole
time; nothing was reading it. **A panic handler that produces
`0: <unknown>` is a smoke alarm with the battery out** — it fires correctly and
tells you nothing, and it looks like diligence in the code review.

### PROBLEM 131, part 2 — shipped in 1.0.42: making the next crash readable

The owner chose **both** options. Neither of these fixes the crash; they make
the fifteenth one worth reading, where the first fourteen were not.

**1. The symbols now ship.** `spaceadom.pdb` is installed BESIDE the exe, which
is where dbghelp looks.

There is an ordering trap here that cost a build to find, and it is the reason
this is not a one-line config change. Tauri validates `bundle.resources` while
the Rust crate COMPILES (`generate_context!`) — before the linker has produced
the pdb. So the resource path must already exist at compile time, and the real
file cannot exist yet.

The obvious workaround is the dangerous one: staging the PREVIOUS build's pdb
satisfies the check and is far worse than shipping nothing. **Mismatched
symbols do not fail — they resolve to confidently wrong function names and line
numbers**, and someone will act on that. So the staging is two-phase:

```
src-tauri/build.rs           writes an obviously-invalid STUB if the path is
                             missing. Runs on EVERY cargo invocation.
beforeBundleCommand          scripts/stage-symbols.mjs --real copies the
                             freshly-linked pdb over it, after the link and
                             before packaging. Missing pdb => build FAILS.
```

`build.rs`, not `beforeBuildCommand`: Tauri's before-hooks run only for
`tauri build`, so with the staging in the config a plain `cargo test` died with
`resource path symbols\spaceadom.pdb doesn't exist`. Found by running the
tests, which is the argument for having them.

**Cost, measured rather than guessed.** The owner was told the installer would
"roughly double". It did not: 4.6 MB → **5.6 MB**. The 8.2 MB pdb compresses to
about 1 MB in the NSIS payload. The warning was honest but pessimistic, and the
real number is the one to quote from now on.

**Verified end to end, not assumed.** The exe's CodeView (RSDS) record names
`spaceadom.pdb`, and the installed pdb carries the identical build GUID:

```
exe expects pdb : spaceadom.pdb
exe build GUID  : 85597d0a-065e-433a-b4b1-068cba3d2e65   age 1
pdb GUID match  : YES (offset 20492)
```

That is the check that distinguishes "a pdb is present" from "the RIGHT pdb is
present", and it is the whole point — see the mismatched-symbols warning above.

**2. Crash context: what the app was DOING.** `src-tauri/src/crash_context.rs`
records a handful of breadcrumbs that the panic hook prints before the
backtrace. A backtrace says which code was on the stack; it does not say the
overlay had been rebuilt twice in the last minute.

```
app context at panic:
    last overlay op : overlay_fit 520x282 (toast, bottom-centre) (43ms ago)
    last action     : Space+d (1204ms ago)
    last display evt: overlay rebuild started (...) (4102ms ago)
    overlay rebuilds this session: 2
```

Two design rules, both from bugs in this project. Every read is `try_lock`,
because this runs INSIDE the panic hook and `lock()` would deadlock if the
panicking thread held it — turning a logged crash into a silent hang, which is
strictly worse. And a busy lock is reported as `<lock busy — the panic may be
INSIDE this path>`, which is itself a clue rather than a gap. The module is
never called from the keyboard-hook callback, which may not touch the heap.

The rebuild counter is deliberately a **falsifiable test** of the leading
hypothesis: if crash reports keep showing a rebuild moments earlier, that is the
answer; if the counter is 0 in every report, the hypothesis is dead and should
be written off here.

**3. A REAL BUG found on the way in: there were TWO panic hooks, and one had
never run.** `std::panic::set_hook` REPLACES. PROBLEM 125 installed a hook at
`lib.rs:371` that chained politely to its predecessor; the older PATCH 5d block
called `set_hook` again ~50 lines later without chaining, discarding it. So
everything PROBLEM 125 added — the thread name, the "this is a crash" wording —
has never once appeared in a log. The log format is what proves it: all 14
crashes report `PANIC at`, hook #2's wording, never `PANIC on thread`.

They are now one hook. **Do not add a second `set_hook` anywhere**: the last one
installed wins silently, and the loser leaves no trace of having lost. Same
class as PROBLEMS 118/120/129 — *a stale thing outliving the thing that
replaced it* — and it neatly explains why a fix "shipped" in 1.0.37 changed
nothing about how crashes were reported.

**4. A NEW LEAD, from the conflict detector, unprompted.** The 1.0.42 startup
log on the owner's machine:

```
conflicts: spacedesk is running (spacedeskservice.exe)
conflicts: PowerToys is running (powertoys.exe) - PowerToys Keyboard Manager
display: watching 2 monitor(s) for configuration changes
```

**spacedesk is a VIRTUAL DISPLAY driver** — it adds and removes monitors in
software, at any time, without a cable being touched. That reframes "sometimes
I add my 2nd display, sometimes I disconnect it" from an occasional event into
a software-driven one that can fire unpredictably, and display changes are the
input to the one code path that deliberately destroys a live window. It does
not prove anything yet — the crashes predate `display_watch` by four days — but
it is the first plausible mechanism for why THIS machine sees them and it is
now directly measurable via the rebuild counter.

PowerToys Keyboard Manager is a separate, known concern: it can capture Space
before Spaceadom's hook sees it. Worth telling the owner about independently of
the crash.

**Still open.** The root cause. 1.0.42 changes nothing about how often the app
dies — expect roughly two more per day until it is actually fixed. What changes
is that the next one names its own cause instead of printing `<unknown>` twelve
times.

---

## PROBLEM 132 — "shortcuts do not work inside the app": the watchdog spent 20 minutes performing a repair that could not work, and logged success every time

**Reported by the owner, again, on 2026-08-17 21:12** — *"Now again the
shortcuts are not working while inside the app... this is repeating, sometimes
it's working, sometimes it's not."* He was right to be annoyed: this symptom
has been reported repeatedly, investigated twice, and closed neither time.
PROJECT_STATUS 2026-08-16 explicitly left it open with the instruction *"Do not
close this out; it needs the condition it fails under to be captured, not a
theory."* This is that capture.

**The measurement that had never been taken.** Every previous look sampled the
log AFTER the fact. This time the log covered the failure as it happened:

```
20:36:22  WATCHDOG  kb 12000ms / mouse  9375ms   reinstall ok: true
20:37:22  WATCHDOG  kb 60000ms / mouse 30734ms   reinstall ok: true
20:38:22  WATCHDOG  kb 60000ms / mouse 60000ms   reinstall ok: true
   ... one alarm every 60s, unbroken ...
20:55:22  WATCHDOG  kb 60000ms / mouse 60000ms   reinstall ok: true
```

**Twenty consecutive minutes in which neither hook saw a single event**, while
`GetLastInputInfo` reported the user active 0-16ms earlier, and the repair
reported success twenty times. Session totals: `watchdog-reinstalls:24`, and
`0 of them while the Spaceadom window itself had focus` on every reading of the
PROBLEM 104 counter — the keys it did see came from his other apps.

**Root cause — three defects, all in the recovery path.**

**(1) The repair could not work, and could never report that.** Re-hooking was
the watchdog's ONLY move. But a low-level hook proc fires on the thread that
INSTALLED it, so if that thread's message pump is what is wedged, a fresh hook
on the same wedged pump is a fresh hook that never fires. `reinstall ok: true`
means only that `SetWindowsHookEx` returned a handle; it says nothing about
whether events will arrive. The watchdog therefore repeated a failing move once
a minute, indefinitely, announcing success each time.

**(2) The log named a cause the code had already excluded.** The alarm read
*"Usually means an elevated window has focus (UIPI), which a reinstall cannot
fix"* — but the block immediately above it RULES ELEVATION OUT and returns
early when the foreground process is elevated. If that sentence is ever
printed, UIPI is the one thing it cannot be. Two separate investigations read
that line and went looking at elevation. **A log that asserts a cause the code
has already excluded is worse than a log that says nothing: it is a signpost
pointing away from the answer, and it carries the authority of the program.**

**(3) The one case the owner keeps reporting was the one case with no
diagnosis at all.** The elevation discriminator is guarded by
`pid != std::process::id()` — so when *Spaceadom's own window* is in the
foreground, the check is skipped entirely and control falls straight through to
the "evicted" verdict with no evidence gathered either way. The exact scenario
in the bug report was the exact scenario the instrumentation ignored.

**Exact file.** `src-tauri/src/hook/mod.rs`.

*Escalation — stop repeating a move that has already failed twice:*

```rust
let streak = BLIND_REINSTALLS.fetch_add(1, Ordering::Relaxed) + 1;
if streak >= 2 {
    BLIND_REINSTALLS.store(0, Ordering::Relaxed);
    let own = BLIND_WHILE_OWN_FG.swap(0, Ordering::Relaxed);
    log::error!(
        "hook: {streak} reinstalls in a row and STILL no events - re-hooking has \
         failed, so the ENTIRE hook thread is being restarted (fresh message pump). \
         Foreground: {fg}. Alarms while Spaceadom's OWN window had focus: {own}."
    );
    ESCALATE_RESTART.store(true, Ordering::Relaxed);
}
```

*The pump acts on it. Leaving the pump IS the repair:*

```rust
if ESCALATE_RESTART.swap(false, Ordering::Relaxed) {
    let _ = KillTimer(None, timer_id);
    let _ = UnhookWindowsHookEx(kb_hook);
    let _ = UnhookWindowsHookEx(ms_hook);
    HOOK_INSTALLED.store(false, Ordering::Relaxed);
    return;               // HOOK_SHUTDOWN stays false -> PROBLEM 82's
}                         // supervisor rebuilds the thread from scratch
```

`HOOK_SHUTDOWN` staying false is load-bearing: it is exactly what distinguishes
this from a deliberate exit, so the supervisor treats it as a crash and builds a
NEW thread with a NEW message queue and NEW hooks — the only repair that can
survive a wedged pump. The supervisor's existing 5-restarts-per-10-minutes cap
bounds it, so escalation cannot become a spin loop. Streak resets to 0 the
moment any event arrives, so this only ever fires for CONTINUOUS blindness
(~2 minutes), never for a one-off.

*And the alarm now names the window instead of guessing:*

```rust
fn foreground_desc() -> String { ... }   // "chrome.exe (pid 1234)"
                                         // "spaceadom.exe <- SPACEADOM'S OWN WINDOW"
                                         // "<none - secure desktop or desktop switch>"
```

`PROCESS_QUERY_LIMITED_INFORMATION`, deliberately, not the full flavour: the
LIMITED one SUCCEEDS against an elevated process from medium integrity, which is
the whole point — we want the name even when that window is the reason we are
deaf. Safe here and only here, on the WM_TIMER branch of the pump rather than in
a hook callback, so its round-trip cannot trip LowLevelHooksTimeout.

**How it was verified — and what is NOT verified.** Compiles clean, 0 warnings;
13 tests pass; built, installed unelevated over the running 1.0.42, and both
new strings confirmed present in the INSTALLED binary by ASCII scan
(`reinstalls in a row`, `SPACEADOM'S OWN WINDOW`).

**The escalation branch has never executed.** That is PROBLEM 118's exact
shape — shipping a recovery path that has never run once — and it is stated
here rather than glossed. What makes it acceptable this time: the branch's
failure mode is bounded (worst case, the hook thread restarts unnecessarily,
which costs microseconds and is what the supervisor already does on a panic),
where PROBLEM 118's branch could DISABLE a working overlay. It is not called
fixed. It is called shipped and instrumented. Confirmation requires the next
occurrence to show either the escalation line followed by recovery, or the
alarm naming a foreground window we did not expect.

**Generalise this.** *A repair that reports success without checking that it
worked is not a repair, it is a ritual.* `reinstall ok: true` asked the API
whether it accepted the call, never whether events resumed — and a health check
that can only ever say "fine" will happily narrate a 20-minute outage. Verify
the OUTCOME, and when a repair fails twice, escalate instead of repeating: the
second identical failure is evidence about the repair, not about the fault.
Also: **guard clauses hide their exceptions.** `if pid != self` looked like an
optimisation and was actually a blind spot aimed precisely at the case being
investigated.

---

## PROBLEM 133 — the anti-freeze guard was watching the wrong window, and had never fired once

**Reported by the owner 2026-08-17 21:33:** *"something made brave freeze,
check if its my app which was the culprit"* — the "say so if it happens again"
that PROBLEM 121 explicitly asked for.

**First, the answer to his actual question: for THAT freeze, no.** The log is
unambiguous:

```
21:09:52          last Spaceadom interaction with Brave (Restore/Minimize)
21:15 .. 21:28:52 no Brave activity of any kind
21:28:52          Space+B -> "cascade: LAUNCHING brave.exe"   <- already gone
21:33:41          every Brave process Responding = True
```

Spaceadom LAUNCHED Brave at 21:28:52, so Brave had already died or been closed,
and nothing in this app touched it for the preceding 19 minutes. The cascade
only runs when a shortcut fires, and no shortcut targeted Brave in that window.

**But checking produced a worse finding than the one being investigated.**

```
has the PROBLEM 121 hung-app guard EVER fired?   ->   NEVER, in the entire log
```

Not once, across 100+ focus/restore operations, almost all of them Brave and
Discord — the exact two applications reported as freezing. A guard with that
much exposure and zero firings is not a guard that found nothing. It is a guard
aimed at the wrong window.

**Root cause.** `force_foreground` checks the OUTGOING foreground:

```rust
let fg_hung = IsHungAppWindow(fg_before).as_bool();     // window we leave
let attached = fg_thread != my_thread && fg_thread != 0 && !fg_hung;
if attached { AttachThreadInput(my_thread, fg_thread, true); }

let _ = BringWindowToTop(hwnd);        // <-- the TARGET
let _ = SetForegroundWindow(hwnd);     // <-- never checked for hang
```

That check is correct for what it covers: `fg_thread` is whose input queue we
join, so refusing to attach to a wedged one is right. It is also incomplete in
the direction that matters. `BringWindowToTop` and `SetForegroundWindow` are
aimed at `hwnd`, the TARGET, and nothing asked whether the target was alive. A
call into a wedged window's thread blocks the caller — and we may be attached to
a second thread while it happens, so the stall can reach a third party.

**Why it could never fire.** `fg_before` is normally the healthy window the
owner is looking at. The sick one is whatever he just aimed a shortcut at — you
press Space+B *because* Brave is not responding to clicks. The guard therefore
inspected the well window on every single call and reported all clear, while the
unguarded line below reached straight into the sick one.

**Exact file.** `src-tauri/src/engine/actions/smart_cascade.rs`, before the
attach:

```rust
let target_hung = IsHungAppWindow(hwnd).as_bool();
if target_hung {
    log::warn!(
        "force_foreground: the TARGET window is not responding - not touching it \
         (PROBLEM 133). Raising a wedged window cannot succeed and risks dragging \
         Spaceadom down with it. The app is stuck on its own; this shortcut is a \
         no-op until it recovers."
    );
    return;
}
```

`IsHungAppWindow` only reports true after ~5s of a thread not pumping, so this
cannot trigger on a merely busy app. When it does say hung, raising the window
was never going to work; the only open question was whether we hung too.

**How it was verified.** Compiles clean, 0 warnings; 13 tests pass; installed
unelevated over the running 1.0.43 and the string confirmed present in the
INSTALLED binary (`TARGET window is not responding`).

**HONEST LIMITS, both of them.** (1) Like PROBLEM 121 before it, this branch has
not been observed firing — it needs a genuinely wedged target, which cannot be
manufactured on demand. Unlike 121, we now know it is pointed at the right
window, and the same log query that exposed 121 will confirm or refute this one.
(2) `ShowWindow(SW_RESTORE)` on a minimised target is NOT covered by this guard.
It runs earlier in the cascade and can also block on a wedged thread. It is left
alone deliberately rather than guessed at — this fix addresses the two calls
that are documented blockers, and widening it further without evidence is how
PROBLEM 118 happened.

**Generalise this.** *A safety check that has never fired deserves an
investigation, not confidence.* The natural reading of "zero occurrences" is
"the problem is rare"; the equally likely reading is "the check cannot see the
problem". They are distinguishable only by asking what the check actually
inspects versus what the dangerous line actually touches — and those had drifted
one variable apart here (`fg_before` vs `hwnd`). It also passed code review
twice, because a guard named for the right bug reads as covering it.

---

# BACK-FILLED ENTRIES (2026-08-17) — 17 problems the code implements that this file never recorded

**Why this section exists.** The owner asked which documentation was still
outstanding. A mechanical audit — every `PROBLEM n` cited in `src-tauri/src/**`
and `src/**`, compared against every `## PROBLEM n` heading here — found **84
problem numbers referenced in code, 67 documented, 17 with no entry at all**.
Sixteen of the seventeen were missing from `PROJECT_STATUS.md` too.

**Source and its limits, stated up front.** These are reconstructed from the
code comments at each site, not from live investigation. The comments are
unusually detailed, so symptom and root cause are trustworthy. But most entries
CANNOT carry a "how it was verified" line, because that evidence was never
written down and cannot be recovered now. Treat them as accurate about WHAT and
WHY, and silent about HOW IT WAS PROVEN. That gap is the cost of documenting
late, and it is the argument for doing it at the time.

---

## PROBLEM 53 — Space+key did nothing for most bindings: only hardcoded paths resolved

**Symptom.** A tester's log, three lines in a row:
`could not resolve path for Battle.net.exe` / `NVIDIA GeForce Experience.exe` /
`HaloInfinite.exe`. Space+key did nothing for most of his bindings while the
hook and engine were working perfectly.

**Root cause.** Resolution only handled apps with a hardcoded install path, an
`App Paths` registry key, or a presence on `PATH`. That is a small minority of
installed Windows software.

**Exact file.** `src-tauri/src/engine/actions/smart_cascade.rs` (~L1629) — fall
back to searching both Start Menu trees, which is where the app picker already
finds things, so the two agree by construction. `ShellExecute` launches a `.lnk`
directly.

**Generalise this.** *When a lookup has three sources and all three are
opt-in registrations, the default case is failure.* The fix was to use the
source that is populated for nearly every installed app.

---

## PROBLEM 54 — "ShellExecute launched" did not mean a window appeared, and I reported it as success

**Symptom.** A tester's log showed five `ShellExecute launched brave.exe` lines
followed by `url_focus - Titles seen: []` — zero Brave windows, seconds later.

**Root cause.** `ShellExecuteEx` returns success for activations that create no
process at all, and the legacy error code rides in `hInstApp`. The log line
claimed success unconditionally.

**This one has a confession attached, and it is kept deliberately:** *"I read a
tester's log and told the user apps 'were opening' when nothing had appeared on
his screen."*

**Exact file.** `smart_cascade.rs` (~L296) — report `process_created` alongside
`hInstApp`. `hInstApp <= 32` is the classic error range; `5` (ACCESS DENIED) is
the one to watch when an elevated process launches a per-user app.

**Generalise this.** *Never log a launch without logging whether a PROCESS was
created.* An API's success value describes the call, not the world — the same
lesson PROBLEM 127 relearned for installers and PROBLEM 132 for hook reinstalls.

---

## PROBLEM 55 — the app froze for a moment on every launch

**Symptom.** Both testers: "not responding for a few moments when first
opening".

**Root cause.** `ensure_startup_task()` ran INLINE on the startup path. It
shells out to `schtasks` three times (`/Query`, `/Create`, `/Change`) at
100-300ms per spawn, stacked on top of WebView2's own first-run init.

**Exact file.** `src-tauri/src/lib.rs` (~L517) — moved to a background thread.
The logon task only matters at the NEXT logon, so nothing needs it before the
window exists.

**Generalise this.** *Work whose result is not needed until the next boot must
never be on the path to the first paint.*

---

## PROBLEM 56 — apps "launched" and never appeared: an elevated parent cannot start a per-user app

**Symptom.** Space+letter reported a launch; no window ever appeared. Same
evidence as PROBLEM 54.

**Root cause.** Chromium browsers launched by an ELEVATED parent de-elevate
themselves by re-launching through the shell, and on some machines that handoff
dies silently.

**Exact file.** `smart_cascade.rs` (~L116) — hand the launch to the desktop's
explorer via `IShellDispatch2::ShellExecute`, the Microsoft-documented approach
(Raymond Chen, "How can I launch an unelevated process from my elevated
process", plus the ExecInExplorer SDK sample). Explorer runs at medium
integrity, so the app starts as if double-clicked.

**STALE PREMISE — FLAGGED 2026-08-17, NOT REMOVED.** The comment at this site
still asserts *"Spaceadom runs ELEVATED (the keyboard hook needs it)"*. **Both
halves are now false.** PROBLEM 61 removed elevation, and `WH_KEYBOARD_LL` never
required it. The CODE is still correct and worth keeping — launching via
explorer is harmless when unelevated and remains the right call if elevation
ever returns — but the stated reason is wrong, and a future reader could
reasonably conclude from it that this app elevates. Left in place with this
flag rather than silently edited, because the comment is evidence of what was
believed when the code was written.

---

## PROBLEM 57 — "it only starts if I right-click Run as administrator"

**Symptom.** Exactly that, from a tester.

**Root cause.** After an install moved the app, the Scheduled Task still pointed
at the previous path. `schtasks /Run` then "succeeded" launching a stale or
deleted exe; the stub exited; nothing appeared.

**Exact file.** `src-tauri/src/startup.rs` (~L540) — verify the task's target
matches THIS exe before running it; a mismatch falls through to one UAC prompt,
after which the task is rewritten to the current path.

**LARGELY SUPERSEDED.** Autostart is the HKCU Run value now, not a task
(PROBLEM 129). The lesson outlives the mechanism: *a launcher that points at a
path is a launcher that can point at the wrong path*, which is also PROBLEM
116's shape (self-updating apps moving out from under a saved path).

---

## PROBLEM 102 — ten backups, all from the last four minutes

**Symptom.** The user's 84 KB config from 23:16 was already gone by the time
anyone looked for it.

**Root cause.** A plain 10-deep ring. While actively binding keys, ten saves
happen within minutes — so all ten copies covered the last few minutes. **A
count-based ring has no time depth exactly when the user is most active, which
is exactly when mistakes get made.** Raising the count only buys a bigger
constant.

**Exact file.** `src-tauri/src/config/mod.rs` (~L296) — bucketed retention:
every save from the last hour, one per hour for 24 hours, one per day for 7
days. ~30-40 files, a couple of MB. The newest file in each bucket wins.

**Generalise this.** *"Keep the last N" and "keep history" are different
requirements.* Whenever N events can arrive in a burst, a count-based ring is a
burst-shaped blind spot.

---

## PROBLEM 103 — one button, two very different outcomes, and the destructive one was hidden behind the gentler word

**Symptom.** The user clicked "Reset this profile" four times on their own
profile `hi` and cleared all 26 bindings.

**Root cause.** On a stock profile, reset restores factory bindings. On a
user-created profile there are none to restore, so the same button simply
EMPTIES it. One label; the worse outcome was the unlabelled one.

**Exact file.** `src/components/settings-panel.ts` (~L72) — the label states
what will actually happen for the profile in hand.

**Generalise this.** *A control whose meaning depends on hidden state must say
which meaning is active.* Related: PROBLEM 119's slider that did nothing.

---

## PROBLEM 104 — the counter that answers "does the hook see ANY key?"

**Symptom.** The owner's long-running report: with the Spaceadom window focused,
nothing works — no HUD, no toasts, no launches.

**Root cause of the BLIND SPOT (not of the bug).** Every existing counter only
recorded keys the hook DECIDED something about. So "a hook that never fires" and
"a hook that fires and passes everything through" looked identical — both leave
zeros everywhere. The instrumentation could not distinguish the two hypotheses
it was being used to choose between.

**Exact file.** `src-tauri/src/hook/mod.rs` (~L69) — `KB_EVENTS_SEEN`
incremented on ENTRY, before any branch, so a still-zero value is positive proof
the callback is not being invoked; plus `KB_EVENTS_OWN_FG` for the subset
arriving while our own window holds the foreground.

**What it went on to prove — and this is why it earns a full entry.** On
2026-08-16 it REFUTED the standing "Windows does not deliver our own window's
keys to our own hook" theory: seven non-zero readings, with the engine acting on
those keys in the same second. On 2026-08-17 the same counter supplied the
evidence for PROBLEM 132, reading `0 of them while the Spaceadom window itself
had focus` through a 20-minute outage.

**Generalise this.** *Instrument the OBSERVATION before the component you
suspect.* One counter, one log line, refuted weeks of work aimed at the wrong
layer — and then served a second investigation a day later.

---

## PROBLEM 105 — deleting the fallback profile silently breaks every other profile

**Symptom.** Keys stop working later, in a DIFFERENT profile, with nothing
connecting the failure to the act that caused it.

**Root cause.** Every key left unassigned in every remaining profile is rerouted
to the fallback. Deleting it is categorically unlike deleting any other profile.

**Exact file.** `src-tauri/src/commands.rs` (~L1426) — the warning rides on the
UNDO LABEL, not a confirm dialog.

**Why not a dialog, recorded so it is not retried.** `window.confirm` was tried
first and NEVER APPEARED: this webview does not render native script dialogs.
That is why every destructive control here uses a two-step button instead. *A
warning the user cannot see is worse than none, because it looks like the job
was done.*

---

## PROBLEM 106 — undo windows scaled to what the action costs to rebuild

**Root cause.** One duration for every destructive action ignores that the
actions are not equally expensive to reverse. A profile the user just made by
hand is cheap; the stock profiles carry 26 curated bindings each; the fallback
additionally breaks every other profile's unassigned keys and needs the longest
explanation — so it needs the longest window to read it in.

**Exact file.** `src-tauri/src/commands.rs` (~L502). 10s / 20s / 30s tiers.

---

## PROBLEM 107 — undo was one slot, and the second delete silently destroyed the first

**Symptom.** Delete A (undo armed), delete B seconds later, undo — B comes back,
**A is gone permanently, with nothing saying so.**

**Root cause.** A single-slot buffer holding "the whole config before the
action". The second action overwrote it with a state that ALREADY had A missing.

**Exact file.** `src-tauri/src/commands.rs` (~L488) — a stack, newest last, each
entry carrying its OWN deadline so a 10s and a 30s undo can be pending
simultaneously and expire independently.

**Related:** PROBLEM 120 — the timer that hid the button was a separate bug in
the same feature; the undo itself was never lost, only its button.

---

## PROBLEM 108 — two levels of friction, matched to the consequence

**Root cause.** Uniform friction is wrong in both directions: heavy
confirmation on a cheap action trains people to click through, and light
confirmation on a catastrophic one is no protection.

**Exact file.** `src/components/profile-editor.ts` (~L210) — ordinary profiles
get the red "Delete?" pill (a light second click). The FALLBACK gets the full
panel, because its consequence is not about that profile at all and needs
sentences the user has time to read.

---

## PROBLEM 109 — deleting a preset profile was a one-way door

**Symptom.** A user who removed Founders, Gamers or Professionals — to tidy up,
or just to see what happened — had to rebuild 26 bindings by hand or dig through
backups.

**Exact file.** `src-tauri/src/commands.rs` (~L617) — "Restore preset profiles",
deliberately **ADDITIVE**: it restores only what is ABSENT and never touches a
preset the user still has.

**That distinction is the whole reason this is not `reset_config`.** Someone who
has spent months customising Founders must not have it silently reverted by a
button labelled "restore".

---

## PROBLEM 112 — the overlay's landed rect, returned to the frontend

**Root cause.** The warp handover animates a pill between its slot and the SPACE
key, but the WINDOW moves at the same time and that move is instant and
un-animatable on Win32. The frontend must convert a pill's position from before
the move into the coordinate space after it, which needs both rects.

**Exact file.** `src-tauri/src/commands.rs` (~L20) — every fit hands back where
it ACTUALLY landed, not what was asked for. That doubles as the read-back the
window rules require.

---

## PROBLEM 113 — a stale flag hid toasts that arrived outside a hold

**Root cause.** `_stageMode` legitimately suppresses window fits while pills
live inside the HUD window. If it is still set when a toast arrives outside a
hold, the HUD is gone and the flag is stale — and a suppressed fit means a
hidden window, i.e. **a toast the user never sees**.

**Exact file.** `src/components/toast.ts` (~L553) — clear it at that point,
where clearing is always safe.

**Generalise this.** Another instance of the family named in PROBLEMS
118/120/129/131: *a stale thing outliving the thing that replaced it.*

---

## PROBLEM 114 — the "peel out of the SPACE pill" branch was unreachable in the only case it existed for

**Root cause.** The engine cancels the HUD BEFORE running the action
(`engine/mod.rs`: `HookEvent::KeyCombo` calls `s.cancel_hud()` and only then
dispatches), so by the time the toast arrives `_hudActive` is already false. The
branch could therefore never run for the case it was written for: firing a
shortcut while the HUD is up.

**Exact file.** `src/components/toast.ts` (~L121) — the overlay REMEMBERS the
SPACE geometry for a short grace period; a toast arriving inside it is the same
gesture continuing.

**The rejected alternative is the interesting part.** Reordering the engine was
not done: cancel-first is deliberate, so a slow action cannot leave the HUD
stranded on screen. *When a fix would undo a deliberate ordering, carry the
state forward instead of reversing the order.*

---

## PROBLEM 115 — transform and opacity only

**Root cause.** Animating width, height, background and border colour forces
layout and paint every frame — impossible to make smooth, and worst on this
owner's laptop, where the overlay runs in software rendering.

**Exact file.** `src/components/toast.ts` (~L810). Morph became two faces
cross-fading rather than one box resizing.

---

## What this audit did NOT cover

Honest scope statement, so the next reader knows what is still unknown:

- `PROBLEM 1-52` and the rest were spot-checked only for PRESENCE of a heading,
  not for accuracy of content. A heading exists; whether it still matches the
  code was not re-verified.
- `PROJECT_STATUS.md` was not back-filled for these 17. This file is the
  technical record and is the one CLAUDE.md says must let another AI apply the
  fix without opening the codebase; duplicating 17 entries into the dev log adds
  length without adding recoverability.
- The `Spaceadom-Developer-Guide.docx/.pdf` predate 1.0.41-1.0.44 and still
  describe the `.msi` install path that no longer exists. **Regenerating them is
  outstanding work.**

---

## PROBLEM 134 — "nothing fires while I am using the app": the hook callback broke the one rule this project wrote down for it

**The owner, for at least the fourth time, and increasingly bluntly:** *"While I
am using the Spaceadom app, if I press space plus any letter, or I am holding
the space, why isn't anything firing up? Why am I not seeing the overlay? This
was a problem we faced before and we solved. Read documentations."*

He was right that the answer was in the documentation. It was not in
`PROJECT_STATUS.md` or in this file — both of which record the symptom as OPEN
and unexplained. **It is in the project's own skill reference**,
`references/win32-keyboard-hook.md` section 2, which has described this exact
failure the entire time:

> "This produces a symptom that is almost always misdiagnosed: **the keyboard
> stops responding after a while, and restarting the app fixes it.** That reads
> like a memory leak or a state bug. **It is hook eviction.** In a Tauri or
> Electron app it is close to guaranteed if you get this wrong, because a
> WebView2 garbage collection or layout pass can stall the UI thread well past
> 1000ms."

And the rule immediately below it:

> "Inside the hook, do only this: read the event, check your `dwExtraInfo` tag,
> **consult an atomic or lock-free structure**, decide pass-or-suppress, return."

**Root cause 1 — the callback did Win32 window queries on every keystroke.**
PROBLEM 104's diagnostic counter, added to investigate this very symptom:

```rust
// BEFORE - on the hook path, once per key event
let fg = GetForegroundWindow();
if !fg.0.is_null() {
    let mut pid = 0u32;
    GetWindowThreadProcessId(fg, Some(&mut pid));
    if pid == std::process::id() { KB_EVENTS_OWN_FG.fetch_add(1, ...); }
}
```

Its own comment defended it: *"no allocation, no lock, no logging, so the
callback still returns in microseconds."* **That audit counted the wrong costs.**
Neither call is on the permitted list, and the lock they take is inside the
window manager, not in our code — USER32/win32k state that the foreground
application's UI thread also touches. The cost is therefore paid exactly when
that thread is busiest, which is when our own dashboard is focused and
rendering. **The instrument added to diagnose the bug sat on the bug's own
critical path.**

```rust
// AFTER - one atomic read; the Win32 sampling moved to the watchdog's 3s timer
if FG_IS_SELF.load(Ordering::Relaxed) {
    KB_EVENTS_OWN_FG.fetch_add(1, Ordering::Relaxed);
}
```

Up to 3s stale, which is meaningless for a counter reported once a minute.

**Root cause 2 — the hook thread ran at the same priority as the renderer.**
`LowLevelHooksTimeout` (capped at 1000ms since Win10 1709) is a deadline on
RETURNING, measured in wall-clock. **A callback merely waiting for a CPU slice
misses it identically to a slow one**, and this thread was competing with
WebView2 at equal priority. Three facts compound on this machine specifically:

```
--disable-gpu          the overlay/dashboard composite in SOFTWARE (GPU
                       composition is dead here), so rendering is CPU work
1766x964 dashboard     PROBLEM 123 grew the window to 92% of the work area,
                       and software compositing cost scales with pixel count
focused == busiest     the heaviest rendering happens while the dashboard is
                       in front, which is precisely the reported condition
```

```rust
SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL)
```

ABOVE_NORMAL deliberately, not TIME_CRITICAL: this thread must beat a rendering
pass, not the kernel. The callback is bounded work, so it cannot monopolise
anything even when scheduled aggressively.

**Exact file.** `src-tauri/src/hook/mod.rs` — the callback's counter block, a
new `FG_IS_SELF` atomic refreshed in `watchdog_check`, and the priority call at
hook-thread start.

**Why this reconciles every earlier observation, including the contradictory
ones.**

- *2026-08-16 FINDING 1 proved the hook DOES receive our own window's keys.*
  Correct, and still correct. Eviction is a load-dependent race, not a
  capability limit — which is why the same counter shows non-zero readings on
  some days and zero on others, and why "sometimes it works" was always the
  honest description.
- *PROBLEM 132's 20-minute blackout with `reinstall ok: true` twenty times.*
  A reinstall restores the hook; it does nothing about the load that evicts it
  again seconds later. Both fixes are needed: 132 recovers, 134 attacks the
  cause.
- *`0 of them while the Spaceadom window itself had focus` on every recent
  reading, while hundreds of events were seen.* Exactly what eviction-under-load
  looks like from inside: the callback is not invoked at all in the window where
  our UI is busy, so the counter that would have recorded it never runs.
- *It got worse after 1.0.36.* PROBLEM 123 grew the dashboard to 92% of the work
  area. More pixels, more software compositing, more starvation. A layout change
  degraded the keyboard hook, which is not a connection anyone would look for.

**How it was verified.** Compiles clean, 0 warnings; 13 tests pass; built and
installed unelevated over 1.0.44; marker `thread priority raised` confirmed in
the INSTALLED binary; and the running process confirms it took effect:

```
hook: thread priority raised to ABOVE_NORMAL so a WebView2 render pass cannot
      starve the callback past LowLevelHooksTimeout (PROBLEM 134)
live check: threads above normal priority = 39 (SetThreadPriority returned Ok)
```

**NOT YET VERIFIED — and this is the whole question.** Whether the owner can now
hold Space inside the dashboard and see the HUD. That is a load-dependent race,
so absence of the symptom for an hour is weak evidence and absence for a day is
strong evidence. The instrument to watch is already in place: `KB_EVENTS_OWN_FG`
should now go NON-ZERO whenever he uses shortcuts with the dashboard focused. If
it stays 0 while he reports the symptom, this diagnosis is wrong and should be
written off here, in this entry, rather than left to look plausible.

**Generalise this.** *A diagnostic can be a load-bearing part of the fault it
was added to measure.* PROBLEM 104's counter was added specifically to answer
"does the hook see anything while our window is focused" — and by asking the
question on the hook path it made the answer "no" more likely. Any probe on a
latency-critical path must be a plain atomic read, with the expensive half
sampled somewhere that is allowed to block. And second: *a rule with a written
rationale still needs the rationale re-checked when the code changes around it.*
"Returns in microseconds" was true of our instructions and false of the system
calls they made, and the reassuring comment is exactly why nobody looked again.

---

### PROBLEM 134 — STATUS CORRECTION, 2026-08-17 22:30. Not fixed. Read this before building on it.

PROBLEM 134 above committed to a falsifiable test: *"KB_EVENTS_OWN_FG should now
go NON-ZERO whenever he uses shortcuts with the dashboard focused. If it stays 0
while he reports the symptom, this diagnosis is wrong and should be written off
here."* Honouring that in both directions, because the result was split:

**The counter moved.** `saw 64 key event(s), 21 of them while the Spaceadom
window itself had focus` — the first non-zero reading in days, after being 0 on
every sample through every failure. Space+B fired from inside the app at
22:01:35 and launched Brave. So the two changes were real: the Win32 calls did
not belong on the hook path, and the thread priority did lift.

**The symptom did not go away.** A baseline-differenced probe keyed to the
overlay's actual HWND, run for 45s while the owner held Space with the dashboard
focused, observed **0 shows** — the overlay never became visible, and the app
logged nothing at all in that window. So 1.0.45 improved a measurement without
fixing the fault. **It is NOT the answer, and must not be cited as one.**

**RULED OUT by the owner, on evidence better than the log:** Mouse Without
Borders and PowerToys. The hypothesis was strong on paper — MWB hooks BOTH
keyboard and mouse, swallows input destined for another machine, and its helper
started at 19:04:42 the same evening, which fit every log symptom including the
otherwise-inexplicable simultaneous mouse blindness. The owner refuted it
directly: *"this thing was solved, and I had Mouse Without Borders even back
then... the app launching, closing, using my shortcuts inside my app, and Space
+ right alt would change the profile, holding space would show the HUD."*
**It worked, on this machine, with these same programs running.** A hypothesis
that requires those programs to be new cannot stand.

**THE REFRAME THAT MATTERS, and the reason this entry exists at all: THIS IS A
REGRESSION, NOT A LIMITATION.** Every investigation so far — mine and the
2026-08-16 one — treated "shortcuts do not work while our own window is focused"
as a property to be explained. It is not. It is a behaviour that WORKED and then
STOPPED. That changes the whole method: the question is not "what about Windows
prevents this" but "what did WE change". Everything above was looking for a
mechanism when it should have been looking for a diff.

**The cheap decisive experiment, NOT YET RUN.** `all-versions/` holds every
installer ever built, and they install per-user with no admin prompt since
1.0.41. Installing an older build and testing the one gesture — focus the
dashboard, hold Space, look for the HUD — bisects this to a version range in
minutes. Candidates: 1.0.27 (the documented clean baseline), 1.0.34, 1.0.36
(where PROBLEM 123 grew the window and software compositing costs jumped).
Config is untouched by version changes, so rollback is safe.

**Deprioritised at the owner's explicit request** 2026-08-17 22:30: *"If we
cannot figure out the solution of it, let it be. It's OK... except when my app
is focused, everywhere else it is working, so that is good enough for now."*
Recorded as OPEN, with the bisect as the named next step, so nobody re-derives
the refuted theories. Do not close this. Do not treat PROBLEM 134 as its fix.

---

## PROBLEM 135 — the slingshot arrival, and why it was invisible for three consecutive builds

**Owner's request, 2026-08-17 night.** He kept the HUD's entrance ("there is a
very good transition, the ripple effect") and asked for the HANDOVER to become a
real, visible move: *"from the guide HUD disappearing to the toast, there's not
enough good transition. I want to see that transition."* He supplied a full
implementation patch — "Slingshot arrival" — and two constraints that shaped
everything below:

1. *"Do not go back to version 1.0.33... do not implement anything of that one.
   Implement newly. Whatever I am giving."*
2. *"I do not care if there is a bit of slowness because of visually seeing the
   animation work. Because I remember last time, as soon as I left the space
   key, then the guide actually disappeared, but the toasts were there and from
   the guide to coming to the toast, there was no time. **Ensure you give enough
   time for the animation this time.**"*

Constraint 2 turned out to be the entire bug, stated by the owner in plain
language before a line was written. It took three failed builds to hear it.

---

### PART A — what the slingshot is

A toast fired mid-hold no longer fades in. It tears out of that app's own chip
in the ring, arcs around the OUTSIDE of the ring, and decelerates into its slot.
The chip leaves a dashed socket that fills back in when the pill lands.

**Exact files.** `src/components/toast.ts`, `src/styles/overlay-earthy.css`.

**Gated on its own flag — `WARP` stays `false`.** Honouring constraint 1: the
1.0.33 warp machinery (toast→SPACE absorb, return-on-release) is NOT switched
back on. Only the arrival is new code.

```ts
const SLING = true;
const SLING_MS = 940;      // door to door - slow on purpose, this is the set piece
const SLING_BOW = 150;     // px the arc swings out past the chord
const SLING_SAMPLES = 22;  // bezier keyframes
const SLING_STRETCH = 1.9; // nose-first stretch at mid-flight
const SLING_MID = 0.5;
const SLING_T0 = 0.16;     // stretch ramps from here - AFTER the face fade (0.17)
const CAPTURE_EASE = "cubic-bezier(.3,0,.08,1)";
```

New functions, all in `toast.ts`: `chipFor()` (find the launched app's chip by
leading-word match), `arcPoints()` (sampled quadratic bezier with tangent angle
per sample), `tearOut()` / `refill()` (the dashed socket), `flightSling()`.
Chips are tagged in `buildHud`'s `make()` with `c.dataset.stApp =
a[1].toLowerCase()`, which is the only thing a toast ("Brave launched") and a
chip ("Brave") share.

**DELIBERATE DEVIATION FROM THE SUPPLIED PATCH — do not "restore" it.** The
patch animates `width`/`height` on the pill and `width` on the trail. Both are
banned in this file by **PROBLEM 115**, which removed exactly those because they
force layout every frame and this overlay runs `--disable-gpu` on this machine.
The shape is implemented as specified — same arc, same 22 samples, same
nose-first shear, same fade offsets — but driven by `scale()` / `scaleX()` using
the cross-scale technique `flightWarp` already proved here. A fixed-width trail
scaled on X, never resized.

---

### PART B — three builds, three wrong answers, and what each cost

Recorded because the sequence is the lesson, not the destination.

**1.0.46 — the toast vanished entirely.** Owner: *"the toast is not visible
anymore... everything just went away as soon as I left the space."*

Self-inflicted, and a straight re-creation of **PROBLEM 113**. The slingshot
sets `_stageMode = true` to park the toast inside the HUD's window during the
flight — and `_stageMode` blocks every `overlay_fit`. For the WARP handover
skipping the fit is correct, because the pills go on living in that window. For
the slingshot it is wrong: `hideGuideHud` has already waited for the flight to
land and the HUD window is collapsing, so leaving the flag set means the toast's
window is never fitted — and `overlay_fit` is what SHOWS it.

The measured signature was identical to PROBLEM 113's, five months of notes
apart:

```
05:54:22  combo Space+b received   -> no overlay_fit
05:54:25  combo Space+f received   -> no overlay_fit
```

Fix, in the non-WARP tail of `hideGuideHud`:

```ts
// before
setToastLayerHidden(false);
if (!_stageMode) relayout();

// after - always un-stage, always fit
setToastLayerHidden(false);
if (_stageMode) { _stageMode = false; setStageAnchor(false); anchorGlow("toast"); }
relayout();
```

**1.0.47 — toast back, still no animation.** Added the decision log rather than
guessing again:

```ts
invoke("overlay_log", { msg: `sling: text="${text}" chip=${c ? "FOUND" : "none"} ...` });
```

It answered its question immediately and ruled out the whole "naming mismatch"
branch:

```
overlay-js: sling: text="Brave" chip=FOUND hudActive=false hudBusy=true chips=34
```

**1.0.48 — still no animation.** Found a real second defect: I had put
`background` and `border` on the two FACES rather than on one shared box. The
chip face fades out at 17% and the toast face does not arrive until 60% — so for
roughly 400ms mid-flight **the pill had no visible box at all**. Restructured to
the patch's actual design (one persistent box, faces cross-fade inside it) and
added a geometry log:

```
overlay-js: sling-geo: from=(251,-158 106x45) to=(0,239 166x43) win=1050x525 bow=150
```

Every number in bounds. Flight firing, chip found, geometry correct, no JS
errors — and the owner still saw nothing. **That combination is the finding**:
when everything measurable inside the page is healthy and the result is still
invisible, the fault is outside the page.

---

### PART C — the actual root cause

`hide_guide_hud()` hid the **OS window**, unconditionally, the instant a combo
cancelled the HUD:

```rust
pub fn hide_guide_hud() {
    if HUD_VISIBLE.swap(false, Ordering::Relaxed) {
        if let Some(handle) = APP_HANDLE.get() {
            if let Some(win) = handle.get_webview_window("overlay") {
                let _ = win.hide();          // <-- here
            }
            let _ = handle.emit("guide-hud-hide", ());
        }
    }
}
```

The engine calls `cancel_hud()` BEFORE it dispatches the action
(`engine/mod.rs`, `HookEvent::KeyCombo`). So the ordering on every single
shortcut was:

```
combo pressed
  -> cancel_hud()      -> win.hide()          WINDOW GONE
  -> action dispatched -> ShellExecute...     (500-1000ms)
  -> toast arrives     -> slingshot flies     inside an invisible window
  -> toast's overlay_fit re-shows the window  toast "pops" with no transition
```

Which is precisely what the owner described three times: ring vanishes
instantly, pause, toast appears with nothing in between. **Every slingshot since
1.0.46 executed perfectly, in a window nobody could see.**

Why three builds missed it: a page cannot observe that its own window is hidden.
`getBoundingClientRect`, `IsWindowVisible` on child elements, WAAPI state, the
JS error bridge — all report health. And this `win.hide()` was **the only window
operation in the app that logged nothing**, while `overlay_fit` and
`overlay_fit_hud` log size, position and visibility on every call. The window
rules in CLAUDE.md already say never to remove that logging, for exactly this
reason; the rule had simply never been applied to `hide`.

**The fix: thread one bit of truth from the component that knows it.**

`src-tauri/src/guide_hud/mod_impl.rs`:

```rust
pub fn hide_guide_hud() { hide_guide_hud_pending(false); }

/// `action_pending` = the engine cancelled the HUD because a COMBO fired, so a
/// toast is about to arrive in this same window.
pub fn hide_guide_hud_pending(action_pending: bool) {
    if HUD_VISIBLE.swap(false, Ordering::Relaxed) {
        if let Some(handle) = APP_HANDLE.get() {
            if action_pending {
                log::info!("guide_hud: hide with action pending - window stays up for the handover");
            } else if let Some(win) = handle.get_webview_window("overlay") {
                log::info!("guide_hud: overlay window hidden (no action pending)");
                let _ = win.hide();
            }
            let _ = handle.emit("guide-hud-hide", action_pending);
        }
    }
}
```

`src-tauri/src/engine/mod.rs` — the caller states what it knows:

```rust
fn cancel_hud(&mut self, action_pending: bool) { ... }

HookEvent::KeyCombo(combo) => { s.cancel_hud(true);  }   // a toast is coming
HookEvent::SpaceUp { .. }  => { s.cancel_hud(false); }   // plain release
HookEvent::WheelUp/Down    => { s.cancel_hud(false); }   // opacity, no toast
```

The window is still hidden eventually — by `overlay_toasts_done` when the stack
empties, the same terminal path every toast already uses. Nothing leaks.

`src/components/toast.ts` — the flag rides the event, and the grace is sized to
reality:

```ts
await listen<boolean>("guide-hud-hide", (e) => hideGuideHud(e.payload === true));

/* 1200 because the gap is real launch latency, MEASURED from this machine's
   log: Brave ~500ms, VLC ~1000ms from combo to toast. The 380 this replaced
   lost the race to every cold launch. */
const SLING_HANDOVER_MS = 1200;
```

And the ring now folds away UNDER the flight instead of being held rigid until
it lands — the flight lives in `#st-flight`, not `#st-hud`, so collapsing the
ring is pure visuals and cannot disturb the pill:

```ts
if (flightLeft > 0) {
  if (_hudEl) _hudEl.classList.add("hidden");   // ring collapses beneath the pill
  sweep(760, 280, 150);
  window.setTimeout(() => { if (!_hudActive) hideGuideHud(false); }, flightLeft + 40);
  return;
}
```

---

### How it was verified

`tsc` clean, `cargo check` clean, 0 warnings. Installed unelevated over 1.0.48;
marker `window stays up for the handover` confirmed present in the INSTALLED
binary. **And confirmed by the only instrument that can see this overlay — the
owner's eyes: "the sling is working now, i can see it."** That is the project's
own rule for overlay work (hold Space and LOOK); no harness can substitute,
because the failure mode lived in the window manager, not the page.

**NOT DONE — the owner's words: "it still needs work, we will do that."** The
motion is visible and correct in shape; tuning is open. The knobs are `SLING_MS`
(940), `SLING_BOW` (150), `SLING_STRETCH` (1.9) and `SLING_HANDOVER_MS` (1200).

### Generalise this

*When every measurement inside the box is healthy and the result is still wrong,
the box is the wrong box.* Three rounds of in-page instrumentation — decision
log, geometry log, error bridge — all returned "fine" while the window they drew
into was hidden. The page is structurally incapable of observing its own
window's visibility, so no amount of better logging inside it could ever have
found this.

*Any call that can make the user's screen change must say so in the log.* This
project already learned that for `overlay_fit` and wrote it into the window
rules as "never remove that logging". `hide()` is the same class of operation
and was silent, and that silence cost three builds and three of the owner's test
cycles. The rule was right; its scope was too narrow.

*And when the user describes the mechanism, that is data, not colour.* "As soon
as I left the space key the guide disappeared... there was no time" named the
window-hide ordering exactly, before implementation started. It was read as a
request for a longer animation rather than as a report of when the window went
away.


---

## PROBLEM 136 — the landing pad: a re-fit after a slingshot threw the toast back out from under it

**Written 2026-08-20, long after the fix.** An audit found this number cited
four times in `src/components/toast.ts` (lines 91, 467, 615, 1913) and twice in
`src-tauri/src/commands.rs` (the `overlay_fit_handover` doc-comment), with **no
entry here at all.** An AI reading that code hits a named problem number, comes
to the index-of-record, and finds nothing — which reads as "never written up"
rather than "number skipped". Reconstructed from the code and the 2026-08-18
log entries.

**Symptom, from the owner:** *"pausing in the middle before jumping to toast"* —
the slingshot arrived beautifully and then the toast leapt sideways.

**Root cause.** A slingshot LANDS on the staged stack, so once it touches down
the toast is already at its final screen position. Re-fitting the overlay after
that does two things at once: `setStageAnchor(false)` re-anchors the stack
(`top: 50% + 239px` → `bottom: 74px`), and the WINDOW moves under it. Between
them the toast is thrown out from under the place the pill just landed.

**Exact file.** `src/components/toast.ts`.

```ts
_slingStaged = true;    // line 615 — this stack is a landing pad now
…
_slingStaged = false;   // line 467 — the landing pad is gone
```

While `_slingStaged` is set, every `overlay_fit` is suppressed: the geometry
that the flight computed IS the answer, and re-deriving it can only disagree.

**How it was verified.** Owner-confirmed on 1.0.49 — *"the sling is working
now"*.

**Generalise this.** *An animation that computes a final position owns that
position until it is over.* Any layout pass that runs mid-flight is a second
opinion about something already decided, and the two will differ by exactly the
amount the user sees as a jump.

**Related trap, same family:** `_stageMode` left set blocks every subsequent
`overlay_fit` — that is PROBLEM 113, and 1.0.46 re-created it exactly. A
suppression flag needs a guaranteed clearing path on every exit, including the
error ones.

---

## PROBLEM 137 — the handover window: flying to a midpoint instead of the real slot

**Also written 2026-08-20.** Cited at `toast.ts` 618, 1651, 1778 and in
`commands.rs`. Unlike 136 this one IS explained in prose — inside PROBLEM 138,
under a heading that does not name it, so a grep for "PROBLEM 137" lands there
by luck rather than by index. This entry exists so the number resolves.

**Symptom.** On release, the pill flew to a point that was not its slot, then
settled — a two-stage motion where there should have been one.

**Root cause.** A toast's final home is BELOW the HUD window's bottom edge. The
overlay window is sized for the HUD, so "fly to the bottom-centre slot" aimed
at a position outside the window and got clamped to a midpoint.

**Exact file.** `src/components/toast.ts` + `overlay_fit_handover` in
`src-tauri/src/commands.rs`.

The fix is a HANDOVER window: grow the overlay DOWNWARD first — top edge fixed,
new bottom edge = `overlay_fit`'s own bottom (`ms.height - 64.0`) — and pin
`#st-hud` to its original height so the ring does not stretch while the window
does. The flight then targets a slot that genuinely exists inside the window,
and `toast.ts:1651` unpins on the way out.

**The identity that makes it seamless:** the handover window's bottom edge and
`overlay_fit`'s bottom edge are the SAME number. If they ever drift apart the
toast jumps at the moment of handover, which is precisely the artefact this
removed.

**Generalise this.** *You cannot animate to a coordinate outside the window.*
When a flight crosses a window boundary, the window has to move first — and the
two geometries must be computed from one expression, not two that happen to
agree today.

---

## PROBLEM 138 — thruster up, slingshot down: the toast ⇄ HUD handover, both directions

**Owner's spec, 2026-08-18**, delivered as `THRUSTER_SLING.md` inside
`design/Design system overhaul 2 project.zip`, with one instruction attached:
*"Make sure there is no twitching in the middle."* Confirmed working by him on
1.0.52: *"the thruster is working."*

The pairing:

- **Hold Space, toasts on screen → THRUSTER CONVOY.** Each pill squats, ignites
  and burns up to the SPACE key behind a flickering three-layer exhaust plume,
  shedding pressure rings and sparks. One launch every 120ms; the ring blooms as
  the last one lands.
- **Release Space → SLINGSHOT DOWN.** Each pill peels out of SPACE and flies ONE
  continuous curved arc into its own slot. Alternating sides, bows widening per
  pair.

**Exact files.** `src/components/toast.ts` (constants, `mkPlume`,
`shedExhaust`, `flightThruster`, `flightSlingDown`, `absorbIntoSpace`,
`hideGuideHud`, the chip-less mid-hold branch).

---

### `WARP` is back on, and that is not a reversal of 1.0.33

The flag's own header said: *"Switched off rather than deleted... Set to true to
work on the transition again; there are exactly three call sites."* What the
owner rejected in 1.0.33 was the MOTION, not the machinery — the staging,
freezing, parking, rect arithmetic and one-grow-per-handover contract were all
kept, correct, and dormant. Turning `WARP` on now re-activates that machinery to
drive the flights **he designed and supplied himself**. `flightWarp`'s old
straight-line motion survives in exactly one place: the 420ms `fromSpaceExit`
grace ejection. The flag's comment records this so a future reader does not
"restore" 1.0.33 by mistake.

---

### THREE DEVIATIONS from the supplied patch — all deliberate, none cosmetic

**1. PROBLEM 115: no `width`/`height`/`background` animation.** The patch
animates all three per frame, on both flights and both trails. That is banned in
this file, in writing, because it forces layout and paint every frame and this
overlay composites in software — it is why 1.0.29–1.0.31 could never be made
smooth however the easing was tuned. Both flights therefore use `flightWarp`'s
two-face cross-scale construction: each face is built at its OWN natural size
and never resized, and the box morph is a `scale()` between them. Same squat,
same plume, same arc, same timings, same face-fade offsets — transform and
opacity only. The trails are fixed-width bars driven by `scaleX`.

**2. The descent lands at the TRUE bottom slot, not the staged one.** The patch
has the pills land in the staged mid-window position, which is precisely the
mid-screen stop the owner had already rejected one round earlier ("no need that
lower middle center... make it fly to the final position"). So the descent
reuses PROBLEM 137's handover window:

```ts
invoke<Rect | null>("overlay_fit_handover", { width: window.innerWidth, height: ringH })
  .then(() => {
    _hudEl.style.top = "0px";              // pin: the ring must not move a pixel
    _hudEl.style.bottom = "auto";
    _hudEl.style.height = ringH + "px";
    const from = spaceBox();               // AFTER the grow, BEFORE .hidden scales to .93
    _hudEl.classList.add("hidden");        // ring folds away UNDER the arcs
    setStageAnchor(false);                 // normal anchor = the true final slots
    void document.body.offsetWidth;
    relayout();                            // depth attrs only - stage guard holds
    back.forEach((t, i) => { ... flightSlingDown({ from, to: settledBox(t.el), ... }) });
  })
```

**Why this produces no twitch, which was the owner's one instruction.** There
are three seams where a jump could hide, and each is closed by construction:

```
seam 1  window grows for the descent   top edge FIXED + ring pinned to ringH
                                       -> the ring cannot move on screen
seam 2  stack un-stages to the slots   happens while the pills are PARKED
                                       -> nothing visible is moved
seam 3  window shrinks after landing   handover bottom == overlay_fit bottom
                                       (`ms.height - 64.0`, same expression)
                                       -> the toast's screen position is
                                          identical before and after
```

`spaceBox()` is measured AFTER the grow deliberately: the pills' slots and the
SPACE key must be read in the SAME viewport, or the arc starts from a stale
origin — the failure PROBLEM 113 recorded as "a flight sometimes began off to
one side".

**3. Chip-less mid-hold toasts now fly too.** Volume, clipboard and unlisted
apps have no chip to tear out of, so `SLINGSHOT_ARRIVAL.md`'s fallback was a
plain fade. They now launch from the SPACE key via `flightSlingDown` — the same
descent, the same true-bottom landing — so every mid-hold toast is a flight.

---

### The no-pause guarantee, and where it lives

The spec is explicit that the descent must never be split: *"any seam reads as
the pill stopping mid-air, which is exactly what was rejected."* In
`flightSlingDown` the position is ONE `mover.animate()` over `SLING_SAMPLES`
arc keyframes with ONE easing (`CAPTURE_EASE`). The shear, the faces and the
trail are separate animations, but they animate DIFFERENT elements and different
properties — none of them touches `mover`'s transform. If a future change ever
adds a second position animation to `mover`, or chains two, the pause comes
back.

The thruster's squat is the same rule in the other direction: the 14px dip is a
keyframe at offset 0.11 INSIDE the single position animation, never a chained
segment.

---

### How it was verified

`tsc` clean, `cargo check` clean, 0 warnings; built and installed unelevated
over 1.0.51; the installed exe confirmed newer than the edited source (added
after 1.0.50 shipped a version bump whose patch script had silently failed).
**Behaviour confirmed by the owner** — the only instrument that can see this
overlay, per this project's own rule.

**Timings, all tunable in one block near `WARP_MS`:** `THRUST_MS` 640,
`THRUST_STAGGER` 120, `THRUST_DIP` 14, `THRUST_STRETCH` 2.35, `EXHAUST_EVERY`
64, `SLING_DOWN_MS` 820, `SLING_DOWN_BOW` 170, `SLING_DOWN_STRETCH` 1.9.

### Generalise this

*A supplied patch is a specification of INTENT, not of implementation.* This one
was precise about shape and timing and wrong about mechanism for this codebase —
it prescribed the exact per-frame property animation that this file had already
banned by measurement. Implementing it literally would have produced the correct
choreography and the same jank that got the previous attempt rejected. Read the
patch for what it wants to look like; read the codebase for how it is allowed to
be built.

*And when a spec's own fallback contradicts a decision the user made an hour
ago, the user wins.* The patch's descent lands where the stack is staged. The
owner had already rejected that landing point in as many words. Following the
document there would have been obedience to the wrong authority.


---

## PROBLEM 139 — a per-user .msi cannot be built with Tauri's WiX bundler. BLOCKED, with the wall located exactly.

**Owner's request 2026-08-18:** *"publish the msi version of this too."* Asked
which way, given the .msi was removed in 1.0.41, he chose **"Fix it first, then
ship"** — a per-user .msi landing in the same folder as the setup.exe, with the
update problem solved. That is the right answer. It is also, with this bundler,
not currently possible. Recorded so nobody spends the evening rediscovering it.

**What was built and works.** `src-tauri/wix/main.wxs`, a fork of
tauri-bundler 2.9.4's stock template with four marked changes, KEPT IN THE REPO
even though it is not wired up:

```
InstallScope="perUser" + InstallPrivileges="limited"   (was perMachine)
<Directory Id="LocalAppDataFolder">                    (was ProgramFiles64Folder)
xmlns:util=".../UtilExtension"                         (auto-loads WixUtilExtension)
<util:CloseApplication Target="spaceadom.exe" ... />   (PROBLEM 127's fix, WiX side)
```

`candle` compiles it. The generated WXS was verified to contain all four
changes. The extension loads itself: the bundler scans the WXS for
`"http://schemas.microsoft.com/wix/(\w+)"` and passes `Wix$1.dll` to candle, so
declaring the namespace is the whole wiring.

**Where it stops: ICE38, at link time.**

```
error LGHT0204 : ICE38: Component Path installs to user profile.
                 It must use a registry key under HKCU as its KeyPath, not a file.
error LGHT0204 : ICE38: Component I3d755866... (spaceadom.pdb)     same
error LGHT0204 : ICE38: Component I164d3ab3... (space_toggle_os_lib.dll)  same
```

A per-user MSI must install to the user profile; ICE38 then requires EVERY
component there to be keyed on an HKCU registry value rather than on a file.

**Why it cannot be fixed from the template.** The three offending components are
not written in the template. Two arrive through `{{resources}}`, a pre-rendered
blob emitted by the bundler's Rust code, and the third through the binaries
loop. The template can place them; it cannot change their KeyPath.

**Why it cannot be suppressed.** `light` accepts `-sice:ICE38`, and Tauri never
offers it. The complete field list of `WixConfig` (tauri-utils 2.x):

```
language  template  fragment_paths  component_group_refs  component_refs
feature_group_refs  feature_refs  merge_refs  skip_webview_install  license
enable_elevated_update_task  banner_path  dialog_image_path
```

No extra-args, no skip-validation. And a `tauri build` failure on the msi target
fails the WHOLE build — so a broken .msi would take the working setup.exe and
the entire GitHub release pipeline down with it. That is the reason this was
reverted rather than left switched on.

**Routes that would actually work, for whoever picks this up:**

1. Patch `tauri-bundler` (a fork or an upstream PR adding e.g.
   `wix.lightArgs`), then `-sice:ICE38` makes this a two-line change.
2. Build the MSI outside `tauri build`: the `main.wixobj` is already produced,
   so a post-build script could run `light -sice:ICE38` itself. Costs a
   hand-rolled step in CI and diverges from the bundler.
3. Ship the .msi per-MACHINE, as before. **Explicitly rejected by the owner**
   and by PROBLEM 129 — it is what put two Spaceadoms on his machine.

**Delivered instead.** `share-spaceadom/` refreshed to 1.0.52 with a rewritten
READ-ME. The 1.0.27 setup.exe AND its .msi were removed from that folder — both
are archived in `all-versions/`, and the .msi in particular is the two-installs
trap, which the old README actively invited by offering it as an equal choice.

**Generalise this.** *Check the escape hatch before building the thing that
needs it.* The template fork was the interesting part and it was finished before
anyone asked whether the linker's validation could be waived — which is the one
question that decided the outcome. Ten minutes reading `WixConfig`'s fields
first would have reordered the whole evening. And: **a build target that can
fail takes every other target with it**, so an experimental bundle format does
not belong in a release pipeline until it links.

---

## PROBLEM 140 — patching tauri-bundler to pass `-sice:ICE38` WORKS. It is documented here and deliberately not shipped.

**Owner, 2026-08-18:** *"patch tauri-bundler to add lightArgs and get the msi
working."* It was done, it compiles, and it is recorded as a recipe rather than
as vendored code. Read PROBLEM 139 first for why the .msi needs this at all.

### The patch, in full — two hunks in `tauri-bundler` 2.9.4

`src/bundle/windows/msi/mod.rs`, in `build_wix_app_installer`:

```rust
// 1. the argument vector must be mutable
-   let arguments = vec![
+   let mut arguments = vec![
      format!("-cultures:{}", ...),

// 2. immediately before `let msi_output_path = output_path.join("output.msi");`
+   if let Ok(extra) = std::env::var("TAURI_WIX_LIGHT_ARGS") {
+     let extra: Vec<String> = extra.split_whitespace()
+       .filter(|a| !a.is_empty()).map(|a| a.to_string()).collect();
+     if !extra.is_empty() {
+       log::info!(action = "Running"; "light with extra args: {extra:?}");
+       arguments.extend(extra);
+     }
+   }
```

Built as a `cargo-tauri` binary via a four-line driver crate:

```toml
[dependencies]
tauri-cli = "2.11.4"
[patch.crates-io]
tauri-bundler = { path = "../tauri-bundler" }
```

```rust
fn main() { tauri_cli::run(std::env::args_os().skip(1), None); }
```

`cargo build --release` → `cargo-tauri.exe` in 3m40s. Then
`TAURI_WIX_LIGHT_ARGS=-sice:ICE38` and the per-user .msi links.

### Why an env var and NOT a `wix.lightArgs` config field

Asked for as `lightArgs`; built as an env var, on purpose. `WixConfig` in
tauri-utils is `#[serde(deny_unknown_fields)]`, and **`tauri-build` parses the
same `tauri.conf.json` during the application's own compile**. Adding the field
would therefore break `cargo build` for anyone on a stock tauri-utils, and drag
a forked dependency into the shipped app rather than leaving it in the build
tool where it belongs. Same capability, blast radius confined to the tool.

### Why it is NOT shipped — the cost the owner asked to be told about

He said to do both *"if it is not coming at any cost... if it is, ask me
again"*. It comes at a cost, and the cost is not the build time:

**`-sice:ICE38` does not fix ICE38, it silences it.** The check exists because
components installed into a user profile should be keyed on a registry value
rather than a file — that is what makes MSI *repair* and *uninstall* behave
correctly per-user. Suppressing it yields an .msi that installs cleanly and may
then misbehave on repair or uninstall, leaving files behind. That is shipping a
known-defective installer with its warning light unscrewed, which is a worse
failure mode than not shipping one: the defect is silent and arrives later.

Secondary costs: a vendored `tauri-bundler` fork in the repo, CI having to build
it, and a re-base on every Tauri upgrade.

**Weighed against PROBLEM 141's banner**, which covers the actual risk AND
reaches the old .msi copies already on other people's machines — something a new
per-user .msi can never do — the .msi earns nothing. Decision: recipe kept,
`tools/` deleted (it had grown to **1.6 GB** with its Cargo build tree, which is
its own argument against vendoring), `src-tauri/wix/main.wxs` kept parked.

**Generalise this.** *"It builds" is not "it is correct."* A suppressed
validation is a defect that has been made quiet, and quiet defects surface at
uninstall time on someone else's machine. And **vendoring a third-party crate to
change two lines is a bad trade** — the diff is the asset; the 1.6 GB of build
tree around it is a liability.

---

## PROBLEM 141 — the conflict banner now detects a SECOND INSTALL, not just a stale task

**The owner remembered this, and he was right.** Mid-way through the .msi work
he asked what I was even doing: *"I think I had this resolved by... there would
be something on the top of the dashboard saying that there is a conflict, there
is an old version installed, just press this and a prompt will come up... and it
will delete the old version. I think it should still be there in the app."*

It is still in the app. It detects the wrong thing.

**What existed (PROBLEM 75):** a banner for a stale *Scheduled Task* — an old
logon task, created elevated, pointing at a moved or deleted exe. One click, one
UAC prompt, `schtasks /Delete`. `get_stale_task` / `repair_stale_task`.

**What was missing:** a second *install*. The .msi installed per-machine into
`C:\Program Files\Spaceadom`; the setup.exe installs per-user into
`%LOCALAPPDATA%\Spaceadom`. Windows treats them as two unrelated programs — two
uninstall entries, two autostart registrations, and at logon two processes each
installing a `WH_KEYBOARD_LL` hook and fighting over the spacebar. Measured on
this machine (PROBLEM 129):

```
HKLM: v1.0.37 -> C:\Program Files\Spaceadom\        (from the .msi)
HKCU: v1.0.40 -> %LOCALAPPDATA%\Spaceadom\          (from the setup.exe)
```

No stale task is involved, so the existing banner never fired.

**Why it must be NAMED rather than diagnosed.** Nobody experiences this as "two
apps are running". They experience Space+D opening Discord twice, or settings
that keep reverting because two processes write one `config.json`. Nothing in
that description points at an installer.

**Exact files.** `src-tauri/src/rival_install.rs` (new), plus two commands in
`commands.rs`, the module + a startup call in `lib.rs`, and the banner in
`src/main.ts`.

Detection is deliberately one-directional — it only ever offers to remove the
**per-machine** copy, and returns `None` if we ARE that copy:

```rust
// an app must never offer to delete itself
if me.starts_with(rival.parent()?) { return None; }
```

The scan rides the existing background thread next to `ensure_startup_task`,
never the startup path: it stats Program Files and reads a PE version resource,
and PROBLEM 55 established that no file I/O belongs before the first paint.

Repair is ONE elevated `runas` — the same shape as PROBLEM 75's — that stops any
process running *from the rival directory* (never ours), uninstalls every
per-machine registration via `msiexec /X`, removes the leftover folder, and
clears the HKLM Run value. Then it **verifies against the disk**, not against an
exit code:

```rust
let gone = detect().is_none();
RIVAL_FOUND.store(!gone, Ordering::Relaxed);
```

That is PROBLEM 127's lesson applied: an installer's exit code is a claim about
the installer, not about the machine.

`Win32_Storage_FileSystem` was added to the windows crate features for
`GetFileVersionInfoW`, so the banner can say *"v1.0.37 is installed at…"*
instead of pointing at a bare path. Per-API feature gating, exactly as CLAUDE.md
warns: without it the import fails as "could not find FileSystem in Storage" on
a path that plainly exists in the docs.

**Both banners share one element**, so the rival check runs first and the
stale-task check stands down if it already claimed the slot — two installs is
the worse fault.

### How it was verified — END TO END, both branches

Not left as an unexercised recovery path, because PROBLEM 118 is this project's
record of what that costs. A real decoy was planted in Program Files (a genuine
signed exe, so the version resource had to actually parse), the app restarted,
and the owner clicked the button:

```
09:18:09  a SECOND copy of Spaceadom is installed at C:\Program Files\
          Spaceadom\spaceadom.exe (v1.0.27) ... offering a one-click removal
09:18:32  removal cancelled at the UAC prompt          <- DECLINE path
09:19:02  the second copy is gone — one Spaceadom remains   <- SUCCESS path
```

Afterwards: `C:\Program Files\Spaceadom` absent, HKLM Run value absent, one
instance running. **The decline path executed too**, which is the branch that
usually ships untested — it returns a clean `false`, the button re-arms, and the
banner stays until the machine is actually repaired.

**Generalise this.** *When the user says "I think I already solved this", find
out what they solved before building anything.* He had solved the stale-task
conflict, and remembered the SHAPE of the solution correctly — a banner, a
click, a prompt. I was three hours into forking a build toolchain to make a
second install impossible, when the cheaper and strictly better answer was to
extend the mechanism he already had, which also reaches every machine that is
already broken. A new installer can only protect future installs; a detector
protects the ones already out there.


---

### PROBLEMS 139/140 — RESOLVED 2026-08-18, and the resolution was the owner's, not mine.

He asked the question that ended three hours of wrong work: *"You made MSI many
times before, at that time there was no issue. Why is this issue coming up? I
thought this new issue you solved using the UAC prompt and the user can just
give the permission."*

Both halves are correct.

**Why the old .msi always built:** it was PER-MACHINE. ICE38 is a rule about
installing into the USER PROFILE — it never fires for a Program Files install.
Every .msi up to 1.0.40 built cleanly because none of them tried to be
per-user. **The ICE38 wall in PROBLEM 139 was self-inflicted**: it appeared the
moment I set `InstallScope="perUser"`, and it existed only because I was trying
to make a second install physically impossible.

**And it no longer needs to be impossible.** PROBLEM 141's banner detects a
second install and removes it with one permission prompt — the mechanism he had
designed and remembered. Once the conflict is *detected and repairable*, the
whole reason for a per-user .msi evaporates.

**What shipped in 1.0.54.** The stock per-machine .msi, exactly as before, with
ONE change to the WiX template: `util:CloseApplication`. That fixes the real
defect the old .msi always had — PROBLEM 127's silent update deferral over a
running app — and it needs only `wix.template`, a stock Tauri feature. **No
forked bundler, no `-sice`, no suppressed validation.** `tools/` and the
patched-CLI tree are deleted; PROBLEM 140's recipe stays recorded in case a
per-user .msi is ever genuinely wanted.

Verified from the MSI's own tables rather than by scanning its bytes — an .msi
is a compound document, so string absence proves nothing (PROBLEM 126's lesson,
which an ASCII scan here duly "failed" before the table query confirmed the
opposite):

```
WixCloseApplication : CloseSpaceadom | spaceadom.exe | 33
CustomAction        : WixCloseApplications (1), WixCloseApplicationsDeferred (3073)
InstallExecuteSeq   : WixCloseApplications @ 3999   (immediately before InstallFiles)
Scope               : ALLUSERS=1, INSTALLDIR under ProgramFiles64Folder
```

**NOT verified: a live silent install over a running app.** The UAC prompt for
that test was cancelled, so PROBLEM 127's cure is confirmed structurally
(the action exists and is sequenced before file copy) and NOT behaviourally.
Say so rather than implying otherwise.

**And a second occurrence of the same config trap, worth its own line.**
`tauri.conf.json` ended up with TWO `"wix"` blocks — twice — because a regex
insert added one while a block already existed. Duplicate JSON keys are legal:
the parser silently keeps the last, so the template pointer vanished and the
build produced a stock .msi while the config appeared to say otherwise. Both
times it was caught only by reading the *effective* parsed value, never by
reading the file. The fix now parses, mutates the structure, and re-serialises,
with an explicit duplicate-key detector that printed `duplicate keys found:
['wix']` on the way through.

**Generalise this.** *When a constraint appears that never existed before, ask
what changed on YOUR side first.* ICE38 was not a Tauri limitation discovered;
it was a rule I walked into by changing the install scope. And *a validated
config is not a verified config* — check the value the program actually
received, not the text you believe you wrote.

---

## PROBLEM 142 — the tray icon went back into the overflow when the installer moved, and a one-shot latch made it permanent

**Owner, 2026-08-18:** *"It was shown, it was pinned. I didn't have to press the
show hidden icons thing. Especially in 1.0.15... Get that back."*

He was right that it used to work, right about the version, and the cause is
exactly the thing he could not have known about.

**Root cause.** Windows 11 keys notification-icon visibility to the
**EXECUTABLE PATH**, in `HKCU\Control Panel\NotifyIconSettings\<id>\IsPromoted`,
where `<id>` derives from that path. New icons default to the overflow flyout.
Measured on his machine: **116 known icons, 2 promoted.**

PROBLEM 76 had already solved this — and the log proves it ran:

```
2026-08-12 14:37:06  startup: tray icon promoted to the visible taskbar corner
                     ({6D809377-6AF0-444B-8957-A3…}\Spaceadom\spaceadom.exe)
```

That GUID is `FOLDERID_ProgramFilesX64`. It promoted the **Program Files**
identity and set `tray_promoted: true` in config. Then PROBLEM 129 moved the
install to `%LOCALAPPDATA%\Spaceadom` in 1.0.41 — to Windows a **different
icon**, freshly hidden. The latch said "already done", so promotion never ran
again.

**The latch was the bug, not the promotion.** A bare bool cannot express WHICH
icon was promoted, so it could not notice that the answer had gone stale. It
exists for a good reason — a user who drags the icon back into the overflow must
not be overridden — and that reason survives if the latch stores the path:

```rust
// config/schema.rs
/// PROBLEM 142 — the exe path the tray icon was last promoted FOR.
#[serde(default)]
pub tray_promoted_for: String,

// lib.rs — gate on the PATH, not a bare bool
let me = std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
let done_for = cfg_arc.read().map(|c| c.tray_promoted_for.clone()).unwrap_or_else(|_| me.clone());
if !me.is_empty() && done_for.eq_ignore_ascii_case(&me) { return; }
```

Within one install location it still runs exactly once. Move the app and it
re-promotes once for the new identity.

**Second defect in the same code: a single 5-second sleep.** The shell writes
the `NotifyIconSettings` entry only after it has SHOWN the icon, and a cold
logon is still settling at 5s. One sleep meant a silent no-op on a slow boot,
with `promoted=false` and a retry deferred to the next launch.

```rust
// lib.rs, inside the st-tray-promote thread — BEFORE
std::thread::sleep(std::time::Duration::from_secs(5));
if startup::promote_tray_icon_once() { /* save flag */ }
// returned false -> entry not there yet; retry next launch

// AFTER — poll, and say so if it never appears
for _ in 0..8 {
    std::thread::sleep(std::time::Duration::from_secs(3));
    if startup::promote_tray_icon_once() {
        if let Ok(mut c) = cfg_arc.write() {
            c.tray_promoted = true;
            c.tray_promoted_for = me.clone();
            let snapshot = c.clone();
            drop(c);
            let _ = config::save(&snapshot);
        }
        return;
    }
}
log::info!(
    "tray: no NotifyIconSettings entry for this exe after ~24s — the icon stays      where Windows put it; it can still be dragged out of the overflow by hand,      and this retries on the next launch."
);
```

**Third: match OUR exe, not just the suffix.** `promote_tray_icon_once` matched
any entry ending `spaceadom\spaceadom.exe`, which also hits STALE entries for
install locations we have moved away from. Promoting those is harmless, but it
makes the log say "promoted" without telling you whether the icon the user is
actually looking at was the one promoted.

```rust
// startup.rs — prefer an exact match on our own path, keep the suffix rule
// as the fallback for the shell's KNOWNFOLDER-GUID spelling of the same path.
let me = std::env::current_exe()
    .map(|p| p.to_string_lossy().to_lowercase().replace('/', "\\"))
    .unwrap_or_default();

let norm = path.to_lowercase().replace('/', "\\");
if (!me.is_empty() && norm == me) || norm.ends_with(r"spaceadom\spaceadom.exe") {
    entry.set_value("IsPromoted", &1u32)?;
}
```

`promote_tray_icon_once` also now prefers an EXACT match on `current_exe()`
before the `…spaceadom\spaceadom.exe` suffix rule, so the log says whether the
icon the user is actually looking at got promoted rather than some stale entry.

**How it was verified.** On the real machine, from outside the agent container:

```
tray IsPromoted   : 1
tray_promoted_for : C:\Users\beamu\AppData\Local\Spaceadom\spaceadom.exe
20:17:19  startup: tray icon promoted to the visible taskbar corner (…\Spaceadom\spaceadom.exe)
```

**Generalise this.** *A "do this once" flag must record WHAT it did it to.* A
bare bool is a cache with no key: the moment the underlying identity changes it
becomes a permanent lie, and the failure is invisible because the flag looks
correct. This is the same family as PROBLEM 116 (a saved path outliving the app
that moved) and PROBLEM 118 (a stale window outliving its replacement).

---

## PROBLEM 143 — every install I "verified" for a whole session went into an agent sandbox

**Symptom, from the owner:** *"I restarted my laptop and it didn't come up. The
app didn't restart with my laptop."*

**Root cause, and it is a tooling failure, not an app bug.** The agent shell
runs inside an MSIX container (`Claude_pzs8sxrjxfjjc`) which redirects
`%LOCALAPPDATA%` and virtualises `HKCU`. Every `setup.exe /S` run from that
shell installed into the container. Read back from the same shell, everything
looked perfect — exe present, right version, right content marker, Run key set.
Read from OUTSIDE the container:

```
HKCU\...\Run  Spaceadom              -> ERROR: value does not exist
%LOCALAPPDATA%\Spaceadom\spaceadom.exe -> ABSENT
%ProgramFiles%\Spaceadom\spaceadom.exe -> ABSENT
StartupApproved\Spaceadom            -> 02 (enabled)   [leftover]
```

**The app was not installed on the real machine at all.** It could not start at
logon because there was nothing to start.

**Why it went unnoticed for hours.** After every build I launched the app
myself, so it was always running and the owner tested real, working builds. Only
persistence was fake, and only a reboot could reveal that. The container's
redirect is also read-through, so both paths showed the same size and timestamp
— the copies were indistinguishable by inspection.

**The tell, in hindsight:** Windows had recorded a tray-icon entry for
`…\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Local\Spaceadom\spaceadom.exe`. The
container path was sitting in the registry the whole time.

**The escape hatch.** `explorer.exe` runs outside the container, and anything it
starts is un-virtualised. Every real install/verify in this session was done as:

```bash
powershell -Command "Start-Process explorer.exe -ArgumentList 'C:\...\script.cmd'"
```

**CLAUDE.md already warned about half of this** — "an agent shell may read a
STALE containerised copy of that folder while File Explorer shows the live one"
— for the LOG folder. The warning was right and its scope was too narrow. It now
covers installs and the registry.

**Generalise this.** *A sandbox that lies consistently is worse than one that
fails.* Every check I had — version stamp, byte size, ASCII content marker,
registry read-back — is a check performed BY the same sandboxed process, so all
of them agreed and all of them were wrong. The only cure is to verify through a
channel outside the sandbox. **"I verified the install" means nothing unless the
verifier and the installer are in different worlds.**

---

## PROBLEM 144 — a settings panel full of controls nobody could read

**Symptom, from the owner:** *"In my settings panel there's a lot of stuff and
some stuff need more description… a user who didn't use this ever doesn't know
how to use this, or what those settings do… at the same time the place doesn't
look clumsy."*

**Root cause.** Nothing was broken. Every switch worked and none of them said
what they did. "Software overlay" and "Opacity floor" are unguessable, and the
obvious fix — a line of help under every row — is exactly the clumsiness he
ruled out.

**The design's answer (design/design-system-overhaul-3.md §1): nothing is added
to a row until it is asked for.** The label becomes a button; pressing it slides
its description open underneath.

**Exact files.** `src/components/settings-panel.ts`, `src/styles.css`.

The collapse is a CSS grid, not a height animation — a height animation needs a
measured pixel value and this text wraps differently in every theme:

```css
.set-desc      { display: grid; grid-template-rows: 0fr;
                 transition: grid-template-rows 380ms cubic-bezier(.3,.9,.3,1); }
.set-desc.open { grid-template-rows: 1fr; }
.set-desc-in   { overflow: hidden; min-height: 0; }
```

**The bug the owner caught in the first build:** *"in place of descriptions, in
the settings, when the descriptions are not expanded, there's a visual bug."*
The margin and border were on the GRID ITEM. At `0fr` a grid item is zero-height
but its margins and borders still paint, so every collapsed description left a
stray hairline and a gap. They moved to a nested `.set-desc-body`:

```css
/* WRONG — paints at 0fr */        /* RIGHT — inside the clipped child */
.set-desc-in { margin-top: 7px;    .set-desc-body { margin: 7px 0 6px;
               border-left: 2px; }                  border-left: 2px solid …; }
```

**Generalise this.** *A collapsing container must have nothing on it that paints
at zero size.* Margin, border, padding and outline all survive `0fr`/`0px`; only
the CLIPPED child may carry them.

**Two more things live in this problem number,** both from the same brief:

- **`theme` replaced `dark_mode` as the source of truth,** with the serde
  default deliberately EMPTY so migration can tell "never set" from "set to
  earthy" (`src-tauri/src/config/schema.rs`, `config/mod.rs`):

  ```rust
  let migrated_theme = cfg.theme.is_empty();
  if migrated_theme { cfg.theme = if cfg.dark_mode { "starry" } else { "earthy" }.to_string(); }
  cfg.dark_mode = cfg.theme != "earthy";   // ONE setting still drives the overlay
  dirty |= migrated_theme;                 // persist now, or it re-migrates every launch
  ```

- **"Hide the keyboard layout so a person could enjoy the blank sky"** —
  `applySkyMode()`. Two things are non-negotiable when the entire UI can vanish:
  Esc always returns, and a corner control is always painted, so a user who
  never thinks to press Esc is not stranded in an empty window.

---

## PROBLEM 145 — Warcry looked like a blue night, and Starry night had no stars

**Symptom, from the owner:** *"in warcry you kept bluish background and it looks
so bad! warcry is supposed to be giving out warrior, war, fight, bloodshed,
kings, kingdom etc type vibe, not this!"* — and, separately, no stars appeared
in Starry night.

**Root cause, ONE line, causing BOTH.** The stage's background was hardcoded:

```css
/* src/styles.css — before */
#stage { background: linear-gradient(160deg, #1a2138 0%, #10141f 55%, #070a11 100%); }
```

Every theme repainted its tokens and then the stage painted that same blue-grey
over the whole window. Warcry's iron and blood never reached the screen, and the
starfield — which is a background-image on `#stage` — was painted over by the
gradient that shipped with it.

```css
/* after — the stage asks the theme */
#stage { background: linear-gradient(160deg, var(--st-stage-a) 0%,
                                             var(--st-stage-b) 55%,
                                             var(--st-stage-c) 100%); }
```

with `--st-stage-a/b/c` per theme, and Starry night setting all three to
`transparent` so `--st-starfield` shows through.

**Generalise this.** *A themeable surface cannot own a literal colour.* If a
value appears as a hex literal outside the palette file, it is not themeable, no
matter how many tokens the theme defines. Grep for hex literals in layout CSS
after any theming work.

**The palette itself was rebuilt to the owner's words** (`src/styles/themes.css`):
blood crimson `#b83024` and COLD IRON `#7b8792` as the second voice, on a
`#2a100c / #140b09 / #070303` stage. Gold survives only as a rare warning edge —
*"i dont like golden color much"*.

**And the gating rule was revised by him after seeing it** — recorded because it
reverses an earlier instruction in the same session: *"keep the starry sky of
v1.0.57 as the default of starry night with fun mode off, with fun mode on the
rest you are building now."* So the drifting star tile belongs to the THEME, and
Fun mode adds the living scene on top.

---

## PROBLEM 146 — the constellations were not pressable, and the reason was in the spec I had just read

**Symptom, from the owner:** *"Constellations are not pressable"*.

**Root cause.** My pointer carve-out re-enabled `pointer-events` on
`#keyboard-outer` — a fully transparent wrapper spanning `inset: 70px 30px 84px`,
i.e. most of the window. A transparent element still takes every press inside
its box.

**The spec warns about this in the same section I transcribed the scene from**
(design-system-overhaul-3.md §5b): *"Both content wrappers also need
`pointer-events:none`, with `pointer-events:auto` on only the opaque panels
inside them — a transparent full-page wrapper above the sky layer blocks hits
across its whole box, not just where the panels are."* I read that line and
shipped the thing it describes.

**Exact file.** `src/styles/themes.css`, the carve-out:

```css
/* Every WRAPPER is transparent to the pointer … */
body.nocturne[data-theme="starry"][data-fun="on"] #stage,
… #topbar, … #keyboard-outer, … #gear-dock, … #specials-dock { pointer-events: none; }

/* … and only the OPAQUE LEAVES take it back. */
… #keyboard-scale, … #key-detail-panel, … #toast-container,
… #topbar > *:not(.spacer), … #gear-dock > *, … #specials-dock > * { pointer-events: auto; }
```

**Verified:** the owner — *"constellations working now"*.

**Generalise this.** *Hit-testing follows the box, not the paint.* Any wrapper
that exists only to position its children must be `pointer-events: none`, and
the children opt back in one by one. And: **reading a warning is not the same as
applying it** — when a spec calls out a failure mode, check your own diff against
that sentence before shipping.

---

## PROBLEM 147 — the sound kit arrived and nothing called it

**Symptom, from the owner:** *"did you not get the sound files? where the sound
effects?"* — then he supplied `design/sounds.js`.

**Root cause.** There were no sound assets to find: the design synthesises every
sound in WebAudio, so the "files" are one module. It had to be wired.

**The module is copied BYTE-IDENTICAL** (`src/sounds.js`, verified with
`diff -q` against `design/sounds.js`). CLAUDE.md's *transcribe, never paraphrase*
applies to a handed-over design asset, and a retyped copy is one that silently
drifts. Its types live beside it in `src/sounds.d.ts` rather than being woven in.

**Exact files.** `src/sfx.ts` (new), `src/main.ts`,
`src/components/settings-panel.ts`, `src/components/starry-sky.ts`,
`src/components/special-cards.ts`.

**Why the instance is its own module.** Three modules make sounds and `main.ts`
already imports two of them, so hanging it off main would have made
`starry-sky` import `main` while `main` imports `starry-sky` — a cycle whose
failure mode is a silently `undefined` binding at first use, not a build error.

```ts
// src/sfx.ts — a leaf. It imports nothing from the app.
let _get: () => AppConfig | null = () => null;
export function bindSfxConfig(get: () => AppConfig | null): void { _get = get; }

export const sfx = new Sfx({
  enabled: () => _get()?.sound_enabled === true,
  fun:     () => _get()?.fun_mode !== false,
  volume:  () => 40,
});
```

**The gates are closures over a GETTER, not values.** Flipping "Sound ticks" or
"Fun mode" takes effect on the very next sound with nothing to re-register, and
`appConfig` can be replaced wholesale (reload, reset to defaults) without leaving
a stale object captured here gating sounds by a config nothing else reads.

**WebAudio refuses to start before a real user gesture,** so `wireSfxUnlock()`
rides the first pointer/key event in the capture phase and then removes itself.

**Two call sites needed the module's own special cases, not logic of my own:**

```ts
// The switch you just enabled has to confirm itself — enabled() reads false
// until the line above it, so sounds.js forces past its own mute gate.
if (appConfig.sound_enabled) sfx.toggleOn("sound"); else sfx.toggleOff("sound");

// Fun mode is the one switch that must be audible in EITHER gate state;
// toggleOn/Off("fun") is special-cased inside sounds.js to do exactly that.
if (appConfig.fun_mode) sfx.toggleOn("fun"); else sfx.toggleOff("fun");
```

**Generalise this.** *When a handed-over module documents where its sounds
belong, follow that map instead of inventing one.* `sounds.js` ends with a
"WHERE EACH SOUND BELONGS" table; every call site here comes from it. The two
places where I first wrote my own equivalent were the two the module already
handled better.

---

## PROBLEM 148 — the personality layer: characters, sliders, and the cards that explain a special key

**Symptom.** Spec §2, §3 and §4 were unimplemented, and one part of that was a
real usability hole rather than decoration: the bottom tray had been a row of
INERT labels since V14. `␣ ⌫ — Force Close` names the keys and leaves the user
to guess what it does. Nowhere in the app said.

**Exact files.** `src/styles/characters.css` (new), `src/components/controls.ts`
(new), `src/components/special-cards.ts` (new),
`src/components/settings-panel.ts`, `src/components/keyboard-matrix.ts`,
`src/main.ts`, `src/preview.ts`, `src/styles/design-system.css`.

### §2 — the toggle characters

Each switch performs a character when Fun is on: the engine ignites with a
thruster flame, Fun mode and Run at startup hop over their tracks, the sound and
overlay switches ping a sonar ring, Visual effects smears through a warp. The
keyframes are the v3 lab's, transcribed:

```css
@keyframes thrustOn { 0% { transform: translateX(0); }  55% { transform: translateX(19px); } 100% { transform: translateX(16px); } }
@keyframes orbitOn  { 0% { transform: translate(0,0); } 50% { transform: translate(8px,-16px) scale(1.18); } 100% { transform: translate(16px,0); } }
@keyframes warpKOn  { 0% { transform: translateX(0) scaleX(1); } 45% { transform: translateX(6px) scaleX(2.3) scaleY(.6); } 100% { transform: translateX(16px) scaleX(1) scaleY(1); } }
```

**The one change the app's own CSS needed, and why it is load-bearing:** the
thumb used to travel with `left: 2.5px → 18.5px`. A keyframe that animates
`transform` cannot drive an element positioned by `left` — they fight, and
`left` wins at the end, so the knob would snap back the instant the animation
finished. `design-system.css` now travels by `transform: translateX(16px)` in
EVERY mode, fun or not, so there is one truth about where the thumb is.

**"First render = no animation" is enforced by a one-shot latch,** because
`render()` re-runs after every toggle and without it, flipping one switch would
replay all eight characters at once:

```ts
function wireToggle(id: string, onChange: () => void | Promise<void>): void {
  const el = panelEl?.querySelector<HTMLInputElement>(`#set-${id}`);
  el?.addEventListener("change", () => { markFlipped(id, el.checked); void onChange(); });
}
// …and toggleRow only stamps data-anim when the row ENDED where the user
// pushed it. Several handlers revert on backend failure, and without this
// guard the knob would finish its journey to ON and stay there — on a switch
// that reads OFF:
_flipped?.id === id && _flipped.on === on ? (on ? "on" : "off") : undefined
```

### §3 — the slider characters

Typing speed grows a comet tail that flips to whichever side it is trailing;
Guide HUD delay gets a planet with an elliptical orbit ring that spins in 5s at
rest and 1.1s while dragging; Opacity floor becomes a starfield with a moon for
a handle.

**The sliders stay NATIVE `<input type="range">`.** The lab builds its own from
a div and pointer listeners; copying that would have cost keyboard control, the
arrow keys, screen-reader semantics and every existing input/change listener, to
buy three decorations. The input keeps the job; a wrapper carries the
personality, and one custom property positions all of it:

```html
<span class="sld" data-char="comet" style="--p:.34">
  <input type="range" …>          <!-- still the real control -->
  <i class="sld-tail"></i>        <!-- decoration, pointer-events:none -->
</span>
```

```css
.sld { --x: calc(7.5px + var(--p,0) * (100% - 15px)); }   /* the handle's centre */
```

**MEASUREMENT TRAP, recorded because it cost a round trip.** Checking this in
`preview.html` appeared to show every decoration drifting ~4px across the track.
It was not drifting. The preview scales its layout with a transform, so
`getBoundingClientRect()` returns POST-transform pixels while `clientWidth` and
every CSS length stay pre-transform — comparing the two is comparing different
units. A JS-measured pixel `--x` was added to "fix" it and then reverted; both
approaches had agreed all along. *When a verification says a value is wrong,
check the units of the verification before you change the value.*

### §4 — the special-key cards

Pressing a tray chip, or the special key on the board, opens a 240px card above
it: the combo, the name, what it actually does, and how to press it. Copy is
verbatim from the lab's SPECIALS array. Eight entrance animations cycle by
index, `ANIMS[i % 8]`, with tray index `i` and board index `i + 3` so the same
key never performs the same entrance in both places. Fun OFF = `plainIn` 180ms.
The sound is index-matched by `sfx.cardOpen(i)`, so the genie card gets the
genie sound.

**The card is `position: fixed` and placed in screen coordinates**, because the
board key lives inside `#keyboard-scale`, which is TRANSFORMED to fit the
window. A card positioned relative to that would be scaled with it, text and
all. `getBoundingClientRect()` already reports post-transform screen pixels, so
one code path serves both triggers.

**A bug caught in verification, and worth keeping:** replacing one card with
another left the outgoing card fading for 400ms — still in the DOM, still
hit-testing, still findable by its own marker. Pressing eight chips in a row
stacked eight cards. A REPLACE now removes the old card instantly and silently;
only a real close fades, and the moment it starts fading it drops its
`data-spec-card` marker and its pointer events.

```ts
export function closeSpecialCard(silent = false): void {
  …
  if (silent || reduced()) { card.remove(); return; }   // replaced -> leaves at once
  sfx.cardClose();
  card.removeAttribute("data-spec-card");
  card.style.pointerEvents = "none";
  …
}
```

**Generalise this.** *An element that is leaving is still an element.* Anything
kept alive for an exit animation must be removed from hit-testing, and from
every selector that identifies the live one, on the same tick the exit starts.

### The regression 1.0.59 shipped, and the harness that caught it (1.0.60)

Making the tray chips pressable meant turning each `<span class="special-item">`
into a `<button>`. **This app has no global button reset** — `.set-row-label`
and `#specials-btn` each carry their own — so the eight chips along the bottom
of the dashboard rendered as grey Windows buttons: `rgb(240,240,240)` fill, a
1.8px `outset` black border, and Arial instead of Figtree. That shipped in
1.0.59 and was installed on the owner's machine before it was found.

```css
.special-item {
  …
  appearance: none; background: none; border: 0; padding: 0;
  font-family: inherit; text-align: left;
}
```

**How it was caught:** by reading the computed style of the real element in the
harness — `getComputedStyle(chip).backgroundColor` — not by looking at it. The
same pass confirmed `.set-row-label` was already clean, which is why nobody had
noticed the missing reset before.

**Generalise this.** *Changing an element's TAG changes its default styling, and
a rule written for the old tag will not mention the difference.* `span` → `button`,
`div` → `a`, `span` → `input` all inherit a chrome the existing rule never had to
override. After any tag change, read the computed background, border and font of
the result before believing the class still describes it.

### The dev harness had drifted, and that is part of why this was possible

`preview.html` was still rendering a hand-written "Dark mode" switch — three
versions after the theme pill replaced it — so it could not have caught any of
this. The switch markup and the slider shell now come from
`src/components/controls.ts`, and the cards from
`src/components/special-cards.ts`, which BOTH the app and the preview import, so
they cannot disagree again.

Those are LEAF modules for a concrete reason: the first attempt exported them
from `settings-panel.ts`, and importing that into `preview.ts` dragged `main.ts`
in behind it. Main's bootstrap ran inside the harness, failed on a missing Tauri
`invoke`, and blanked the page with the fatal-error screen — the preview
rendered nothing at all.

**Verified in the harness** (`preview.html?gear&fun&specials`), by sampling the
animations through the Web Animations API rather than by eye:

| Check | Result |
| --- | --- |
| thrustOn at 0 / 231 / 420ms | `translateX(0)` → `19px` → `16px` |
| orbitOff at 0 / 240 / 480ms | `16px` → `(8,-16) scale 1.18` → `0` |
| ring on the sound switch | `ringOn`, `rgba(122,138,94,.55)`, at the knob's destination |
| thruster track (Earthy) | `linear-gradient(90deg, #f6e2cf, #c67139)` — the lab's own values |
| planet orbit vs handle, p = 0 / .5 / 1 | `7.5 / 123.4 / 239.3` vs `7.5 / 123.5 / 239.5` |
| 8 tray cards | all 8 entrances in spec order, one card at a time, centred on the chip, 10px above |
| board key Backspace (tray index 2) | `sky-unfurlIn` = index 5 — the +3 rule |

---

### Measurement trap (2026-08-20) — the ASCII-marker check no longer sees the frontend

`CLAUDE.md` has told every session since PROBLEM 42 that "bundled CSS names are
ASCII-searchable inside the exe", with `st-hud-glow` as the example. Measured on
the 1.0.59 binary:

```
st-hud-glow      False        <- the file's own example
toggle-thumb     False
keyboard-scale   False
spec-card        False        <- shipped in this very build
Boss Key         True         <- only because it is ALSO a Rust string
```

Tauri v2 compresses the embedded `dist2` assets. Nothing frontend is findable in
the exe any more, and the one marker that *is* found is found for the wrong
reason — which is worse than a clean miss, because it looks like the check is
working.

**What to use instead**, and why each link is needed:

1. grep the marker in `dist2/assets/*` — proves the BUNDLE has it;
2. exe `LastWriteTime` > newest file in `dist2` — proves the bundle it embedded
   is that one and not a previous build's;
3. installed exe version stamp, read from OUTSIDE the agent sandbox — proves the
   machine has that exe (PROBLEM 143);
4. `dashboard-js: frontend ready` in the log after the new start — proves the
   new modules loaded rather than throwing at import.

`scripts/install-real.cmd` runs the install and steps 2-3 in one pass and writes
its findings to `D:\`, a drive the container does not redirect.

**Generalise this.** *A verification technique has a shelf life, and it expires
silently.* This one did not start failing loudly — it started returning False
for things that were present. Any check that has not been re-validated against a
KNOWN-GOOD case is not a check; it is a habit. Re-run it against something you
are certain about before trusting a negative.

---

## PROBLEM 149 — nothing closed on an outside press in Starry night, and the profile popover could not be pressed at all

**Symptom, from the owner:** *"after opening settings panel, pressing elsewhere
doesn't close it… I was not being able to press the other profiles. And when
trying to press the other profiles, as there was a constellation behind the
profile, I was getting the card of constellation instead. Pressing elsewhere
wasn't closing the profiles tab either. The same not-closing thing, in case of
starry night theme only."*

**Root cause — BOTH are children of PROBLEM 146's fix.** The starry-night
carve-out sets `#stage` to `pointer-events: none` so constellation presses can
pass through the transparent wrappers. Two consequences were missed:

1. The close-everything listener lived ON `#stage`
   (`document.getElementById("stage").addEventListener("click", closeAllPopovers)`).
   An element that does not take pointer events never fires a click — so in
   Starry night, "press elsewhere" reached NOTHING and no popover ever closed.
2. `#profile-popover` is a direct child of `#stage` (NOT of `#topbar` — the
   markup places it after the topbar), and `pointer-events: none` **inherits**.
   The opt-in list re-enabled `#topbar > *` and the two docks, but not the
   popover — so its rows were transparent to the pointer, and every press fell
   through to the constellation drifting behind it, which opened its card.

**Exact files.** `src/main.ts` (wirePopovers), `src/styles/themes.css`.

```ts
// BEFORE — dead in starry night, #stage takes no pointer events there:
document.getElementById("stage")!.addEventListener("click", () => closeAllPopovers());
// AFTER — on the document. Everything that must survive its own click already
// stops propagation (PROBLEM 98), so only genuine "elsewhere" clicks arrive:
document.addEventListener("click", () => closeAllPopovers());
```

```css
/* themes.css opt-in list gains: */
body[data-theme="starry"][data-fun="on"] #profile-popover,
body[data-theme="starry"][data-fun="on"] #sky-return,
```

**Generalise this.** Two reusable classes here:
- *A listener on an element that can lose pointer-events is a listener that
  can silently stop existing.* Anything that means "anywhere" belongs on
  `document`, not on a surface.
- *When a carve-out disables pointer events on a subtree, every interactive
  thing inside it must appear in the opt-in list — grep for `popover`,
  `button`, `input` under that subtree when adding one, and add new popovers
  to the list in the same commit.*

---

## PROBLEM 150 — the night scene, v4: moon, twenty constellations, a real sea, a rigged galleon, and a storm

**What the owner supplied (2026-08-20):** `night-scene4.md` + `moon.md` (a
delta spec against the 1.0.59 scene), `Help Lab v2-4.dc.html` (the lab, source
of record), and `constellations.js` (the 20 figures' geometry, extracted by him
from the lab). Plus one verdict that CONTRADICTED the spec: the lab scales the
galleon UP 40%, and he wants the whole background *"smaller… smaller ocean and
smaller ship… the sky will also scale out so more stars and these new
constellations have space."*

**The scale resolution — one factor, applied once.** The ocean band renders
the lab's ENTIRE 200px coordinate space verbatim inside a wrapper, and the
wrapper is scaled by CSS:

```css
.sky-ocean       { height: 150px; }                    /* was 200px */
.sky-ocean-world { width: 133.3334%; height: 200px;
                   transform: scale(0.75); transform-origin: bottom left; }
```

So every number in the extracted markup IS the lab's number, checkable against
the spec value for value, and the owner's "smaller" is exactly one declaration.
The ship's width attribute is additionally `379 → 320` (→ 240px on screen);
the moon's numbers are pre-multiplied by the same 0.75 (its geometry is tied
to the waterline, which moved from 174px to 130.5px above the bottom);
constellation SVGs render at 0.85; the star tile was regenerated denser and
finer (178 stars r 0.4–1.4, seed 20260820 — was 115 at up to r 1.9) on the
SAME 1400×900 tile, because `starDrift` and the constellation bands share that
width and changing one desynchronises the sky.

**Exact files.** `src/components/starry-sky.ts` (rewritten),
`src/components/night-markup.ts` (new — the lab's ocean subtree extracted by
script, six live slots turned into ids), `src/styles/starry-sky.css`
(rewritten), `src/constellations.js` (byte-identical copy) + `.d.ts`,
`src/styles/themes.css` (star tile), `src/preview.ts` (`?sky` harness mode).

**What each spec section became:**

- **§2 Constellations** — 20 figures, each in exactly ONE of the three 1400px
  drift bands (shuffle → round-robin 7/7/6 → slot placement, lab-verbatim), so
  nothing is ever visible twice and the full 4200px loop takes ~9 minutes.
  ~45% start hidden; every 5.2–12s one crosses over on a 3.4s fade, the hidden
  count floating between 30% and 55%; the highlight beat is 2.6–6s and picks
  only from visible figures. Hidden figures take no presses.
- **§3 Sea** — the old Bezier ribbon tiles and `waveA/B/C`/`waveBob` are GONE.
  One water gradient, then three fields of individual crest marks
  (`seaField`) and a silhouette horizon (`seaWave`), generated fresh each
  launch; vertical motion comes from `injectHeave()` — four keyframe sets of
  48 stops from summed sines, two uncorrelated signals per layer, co-prime
  durations 71/59/47/37s (~42h before the layers' relative phase repeats).
- **§4 Galleon** — same hull, crew, cannons, flag; now with generated rigging
  (65 paths: 25 shrouds + 32 ratline rungs + 8 stays, alphas .27–.37 — the
  readable range on this sky), 12 rip triangles, 5 shot holes, and four loose
  ribbons on staggered `sailFlap`.
- **§5 Storm** — six blurred cloud masses on lopsided radii drifting ±26px,
  LIGHTER than the sky in their mid-tones (dark-on-dark is invisible), plus a
  17s two-flicker lightning span. The whole container's opacity is driven by…
- **§1 moonPow** — a 13–30s weather cycle weighted to the extremes (36% cloud
  wins, 36% the moon blazes, 28% ordinary night), transitioning moon-group
  brightness, cloud opacity and moonbeam strength over 13s LINEAR so it reads
  as weather, not a switch.
- **moon.md** — an 88px disc with four edgeless glow layers (every gradient
  runs to 0 alpha at 100% — stopping short leaves a ring), nine clustered
  maria over a blurred wash (unevenly sized, upper-left, so no face can form),
  and a 7-leg wander: ~64s at home, slow 34–60s legs down the sky, ~62s sunk
  behind the galleon — occluded by nothing more clever than the ocean band's
  higher z-index.

**How it was verified** (preview harness `preview.html?sky`, measured through
the DOM and the Web Animations API, not by eye): bands 7/7/6 with 9/20 hidden
at start; ship bounding box exactly matches 240×172.5 pre-rotation; heave
keyframes seamless (first stop == last stop on all four); rigging counted 65
paths, splash droplets 13 (6+4+3), crash bursts 7, cloud masses 6; card opens
freeze every ocean animation and outside-press unfreezes; hidden figures
report `pointer-events: none`. Plus a 7-agent adversarial audit of every spec
section against the code (see PROJECT_STATUS entry for its findings).

**Generalise this.** *When a spec and its owner disagree about scale, wrap the
spec's coordinate space and scale the wrapper.* Rewriting fifty numbers by
hand produces fifty chances to drift; one CSS transform produces none, keeps
the file diff-able against the design forever, and makes the next scale
verdict a one-line change.

---

## PROBLEM 151 — the settings panel: the wave that buried the characters, the descriptions that vanished, and the sky-black slider

**Symptoms, from the owner (all 2026-08-20):** *"toggling the things in
setting on or off refreshes the toggles in a wavy type way — no need that,
because the toggle animation cannot be seen"*; *"I had Show me around on, then
I turned on fun mode, then the descriptions of Show me around disappeared"*;
*"the opacity floor slider full is sky black when using earthy… you can use
grey"*; *"the typing speed description and the conflict description is not
required — only show those when show me around is on"*; and *"ensure the first
time it opens, it opens with fun mode off and show me around off."*

**Root causes.**

1. **The wave:** `.set-item`/`.set-row` carried `animation: st-pop-in` at all
   times, and `render()` rebuilds the whole panel after EVERY toggle — so each
   flip replayed the full entrance cascade, drowning the character animation
   it existed to show. The entrance now plays only under `#settings-panel.pop`,
   set for the first render after opening and removed after it.
2. **The vanishing descriptions:** the open state lives in the DOM (`.is-open`)
   and `render()` replaces the DOM — so every re-render silently closed every
   description. Flipping Fun re-renders; hence "turning on fun mode wiped
   show-me-around". `render()` now snapshots the open ids before `innerHTML`
   and restores them after, with each box's transition suppressed for one
   frame so the restore is instant rather than a replayed slide.
3. **The sky-black slider:** the starfield character painted BOTH sides of the
   track in near-identical night tones (#3b3550 / #2b2733) — in Earthy that
   reads as one solid black bar with no position. The filled side is night
   (#2b2733), the empty side is now the same `var(--st-border)` grey every
   other slider uses, and the three stars only exist once the fill has been
   dragged past them: `opacity: calc((var(--p) - .25) * 40)` — the browser
   clamps to [0,1], so the calc is a clean threshold with no JS. **Trap
   inside the fix:** the stars' twinkle animated OPACITY, and a running
   animation overrides the declaration — a hidden star would have twinkled
   itself visible. They now twinkle on a scale-only keyframe (`sld-twinkle`).
4. **Teaching prose:** the typing-speed hint, the conflicts detail/explainer
   and a new specials-tray note ("Press any of these to read what it does —
   and try them out") carry `.sma-note`, shown only under `body.show-around`,
   which is kept in sync at bootstrap, on the toggle, and on panel render.
5. **First-install defaults:** `fun_mode` and `show_me_around` serde defaults
   flipped to false (and the `Default` impl), and every frontend read flipped
   from `!== false` to `=== true` — a missing field must read as OFF now. The
   owner's own config already carries explicit values, so nothing changes for
   him; new installs meet plain, quiet controls and opt INTO the personality.

**Exact files.** `src/components/settings-panel.ts`, `src/styles.css`,
`src/styles/characters.css`, `src/main.ts`, `src/sfx.ts`,
`src/components/special-cards.ts`, `src-tauri/src/config/schema.rs`.

**Generalise this.** Three classes:
- *State that lives in the DOM dies with the DOM.* Anything `innerHTML`-
  rebuilt must snapshot/restore its open/selected/scrolled state, or that
  state silently resets on every render.
- *An entrance animation on a re-rendered subtree replays on every render.*
  Gate entrances on an explicit "fresh" marker, never on element creation.
- *A CSS animation overrides same-property declarations.* If a property must
  stay under declaration control (a threshold, a toggle), the animation may
  not touch that property.

---

## PROBLEM 152 — Smart Search did what v11 did, which is not what the owner wanted; and its '/' never actually pressed a key

**Symptom, from the owner:** *"smart search description is wrong; in WhatsApp
it goes to the box of start-a-new-chat instead of going to type a message; in
Discord it goes to 'where would you like to go' instead of the message box;
inside Brave even though I am using YouTube, pressing it doesn't go to the
search of YouTube, nor does it work on hundreds of other sites."* Plus:
*"where's scroll bottom in special keys?"* and the board's ↓ key opened Scroll
TOP's card.

**Three distinct faults.**

1. **WhatsApp/Discord behaved AS DESIGNED — and the design was v11's.** The
   gold standard's `FocusInputEngine()` sends Ctrl+F (WhatsApp chat search)
   and Ctrl+K (Discord switcher); the Rust port mirrored it faithfully, and
   the card copy ("searches the web for the text you've highlighted") was the
   lab's and described neither. The owner has now redefined the feature: chat
   apps must land in the MESSAGE BOX. Neither app has a focus-compose
   shortcut, so the engine sends ESC — both apps return focus to the compose
   input once transient panels close. **This diverges from v11 on the owner's
   explicit order** — recorded so nobody "fixes" it back to match v11.
2. **YouTube '/' was injected as TEXT, not as a KEY.** v11's AHK `Send("/")`
   presses the physical key; the Rust port used `KEYEVENTF_UNICODE`, which
   synthesises a text character on VK 0 — and sites that bind their shortcut
   to a physical keydown ignore text input. `send_slash_class_key()` now maps
   the char through the CURRENT layout with `VkKeyScanW` (Shift wrapped when
   the layout needs it, unicode fallback only when the layout cannot type the
   char at all) and sends a real down/up, cookie-tagged as always.
3. **Ordinary sites now get the ADDRESS BAR** (Ctrl+L — the owner's pick):
   there is no universal "focus this site's search box" key, and '/' silently
   dies on most of his "hundreds of sites". YouTube/Spotify keep '/',
   Gemini keeps its sequence, Explorer keeps Ctrl+E, generic apps keep Ctrl+F.
   Every press now logs `smart_search: proc= title= -> decision`, because
   "does nothing on YouTube" and "never fired" used to read identical.

**Scroll Bottom** joined the tray (␣ ↓↓, its own card, 9 chips now) and the
board's ↓ key maps to it — it had been sharing Scroll Top's card via
`down: "up"` in `BOARD_TO_SPECIAL`.

**Exact files.** `src-tauri/src/engine/actions/focus_engine.rs`,
`src/components/special-cards.ts`.

**Honest limit:** injection cannot be exercised from the agent shell (UIPI +
container, PROBLEM 143), so the retargeted behaviour is **hand-test items**
for the owner: ␣, on YouTube-in-Brave, on a plain site, on a new tab, in
WhatsApp, in Discord. The log line will name the decision either way.

**Generalise this.** *"Injected the character" and "pressed the key" are
different events, and web pages can tell them apart.* Anything meant to
trigger a page's keyboard shortcut must send the layout-mapped virtual key,
not a unicode packet. And: *when behaviour is a faithful port of a gold
standard the owner has since outgrown, record the divergence AS a decision* —
otherwise the next session restores the old behaviour in the name of fidelity.

---

## Measurement trap (2026-08-20) — a hit-test check that read the wrong element

Verifying PROBLEM 150's "hidden constellations take no presses", the harness
asked `getComputedStyle(hiddenSvg).pointerEvents` and got `"none"`. It was
right about that element and wrong about the behaviour: `buildCon` puts an
**inline** `pointer-events: auto` on the hit rect and on every halo circle —
that is what makes a 3px star pressable — and an inline style beats a plain
rule, so the CHILDREN still took every press. The fix is one selector
(`.sky-con-wrap.is-hidden .sky-con *` with `!important`); the lesson is the
check, not the CSS.

**Generalise this.** *Hit-testing happens on the deepest element under the
cursor, so a pointer-events check must be made on the element that actually
receives the press, not on its container.* When the container's children carry
their own inline pointer-events, the container's computed value tells you
nothing.

An 8-agent adversarial audit of this session's work against the specs found
six further real defects — the moon's wash/maria blurs left unscaled while
every sibling px was multiplied by 0.75, the moonbeam missing the 13s weather
transition its own docstring claimed, the constellation fade using `linear`
where the lab says `ease-in-out`, the stars never growing +0.5 when lit, and
cards on `<body>` closing the popover underneath them. All are fixed in
1.0.61. The audit also REFUTED several confident-sounding findings (the ship's
320px width, the 0.75 world scale, the 0.85 constellation scale), which is the
point of running the verify pass rather than acting on the first list.

---

## PROBLEM 153 — Smart Search: every app where the shortcut was a GUESS failed, so stop guessing and find the box

**Symptom, from the owner, testing 1.0.61:** *"space+, worked on discord,
youtube inside browser, worked on browser new page, but not on whatsapp, nor in
gemini chat inside browser"* — and then *"also didn't work in spotify app"*.

**Root cause, and the pattern is exact.** Split the results by where the key
came from:

| App | Key sent | Where the key came from | Result |
| --- | --- | --- | --- |
| Discord | Esc | tested, real | works |
| YouTube in browser | `/` | documented by YouTube | works |
| New browser tab | Ctrl+L | documented by the browser | works |
| WhatsApp | Esc | **a guess** | fails — backs out to the chat list |
| Spotify app | Ctrl+F | **a guess** | fails — not its search |
| Gemini in browser | Esc then `i` | **v11's guess, ported faithfully** | fails |

Every documented shortcut worked; every guess failed. **There is no better
guess available**: WhatsApp and Spotify publish no shortcut for their main
input, and a web app's input is not the browser's to focus — no browser key
can reach Gemini's prompt box.

**The fix is to ask the accessibility tree where the box IS.** UI Automation
is how screen readers find inputs, and both Electron (WhatsApp, Discord,
Spotify) and Chromium (every browser page) expose theirs through it.

**Exact file.** `src-tauri/src/engine/actions/focus_engine.rs`, plus
`Win32_UI_Accessibility` in `src-tauri/Cargo.toml`.

```rust
unsafe fn focus_text_input_uia(hwnd: HWND, spot: InputSpot) -> bool {
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
    let root = uia.ElementFromHandle(hwnd)?;

    // Edit FIRST, Document second: a Chromium contenteditable — Gemini's
    // prompt, WhatsApp's compose box — reports as Document, not Edit.
    for ctype in [UIA_EditControlTypeId, UIA_DocumentControlTypeId] {
        let cond = uia.CreatePropertyCondition(UIA_ControlTypePropertyId,
                                               &windows::core::VARIANT::from(ctype.0))?;
        for el in root.FindAll(TreeScope_Descendants, &cond)? {
            if !el.CurrentIsKeyboardFocusable()? { continue }   // caret can go there
            if el.CurrentIsOffscreen()?          { continue }   // user can see it
            let r = el.CurrentBoundingRectangle()?;
            if r.right - r.left < 40 || r.bottom - r.top < 12 { continue }  // not a template
            // InputSpot::Bottom -> largest r.top (compose box);
            // InputSpot::Top    -> smallest r.top (search box).
        }
        if best.is_some() { break }
    }
    best.map(|el| el.SetFocus().is_ok()).unwrap_or(false)
}
```

**Three details that are load-bearing:**

1. **Document as well as Edit.** Searching only for `Edit` finds nothing in
   WhatsApp or Gemini — their inputs are contenteditables, which UIA reports
   as `Document`.
2. **`InputSpot`.** With several text boxes on screen there is no
   app-independent way to pick, so position decides: chat and prompt apps use
   the BOTTOM-most box (the compose field), everything else the TOP-most (the
   search field).
3. **The three filters are not tidiness.** Chromium trees are full of
   keyboard-unreachable, offscreen and zero-sized inputs; focusing one of them
   looks exactly like the feature doing nothing.

**Where it runs matters.** `FindAll` over a Chromium descendant tree costs
tens to a few hundred milliseconds. That is fine on the ENGINE actor and would
be fatal on the hook thread, where anything near a second gets the hook
evicted by Windows (PROBLEM 134's law). This is engine-side only.

**Everything confirmed working keeps its fast path** — Discord's Esc,
YouTube's `/`, the address bar — so a tree walk is only paid where a key
cannot work, and UIA failure falls back to the old shortcut, which means this
can only add behaviour.

**Generalise this.** *When a table of results splits cleanly along "was this
value researched or invented?", the fix is not a better invention — it is a
mechanism that does not need one.* Six apps, six guesses, three failures, and
the three failures were exactly the three guesses.

**Honest limit:** injection cannot be exercised from the agent shell (UIPI +
container, PROBLEM 143), so this is a HAND-TEST item. Every press logs its
branch — `smart_search: proc= title= -> UIA -> bottom-most input` or
`-> UIA found nothing -> Ctrl+F` — so a failure names its own cause.

---

## PROBLEM 154 — settings with descriptions nobody could open, and an invisible keyboard that still took presses

**Symptoms, from the owner:** *"guide hud and opacity floor needs [description]
and there are other stuff in the settings which need description too"*; and
*"when keyboard hidden, pressing in the place of keyboard still gets keyboard
presses"*.

**Root cause 1 — the copy existed and had no trigger.** `DESC` already carried
all sixteen entries (wpm, huddelay, opacity, conflicts, reset, clear, presets,
logs among them). But only `toggleRow()` and the theme pill emit a
`data-desc` label; `sliderRow()`, `typingSpeedRow()`, the Conflicts heading and
the four action buttons render a plain `<span>` or a `<button>` that does
something else. Eight descriptions were written, shipped, and unreachable.

Sliders and the Conflicts heading now render a `data-desc` button plus their
`descBox`. **The four action buttons deliberately do NOT become their own
trigger** — pressing them already resets, clears, restores or opens a folder,
and a destructive control must never double as its own help. They share one
`ⓘ What do these buttons do?` row that opens all four as a small convoy.

**Root cause 2 — a child's `pointer-events: auto` beats its parent's `none`.**
Sky mode sets `pointer-events: none` on `#stage`'s direct children, which
covers `#keyboard-outer`. But the starry-night carve-out (PROBLEM 146)
re-enables it on `#keyboard-scale`, a descendant — and CSS has no inheritance
contest here: the deeper declaration simply wins. So the invisible board went
on taking every press.

```css
/* the rule that only reached the direct children */
body.sky-mode #stage > *:not(#sky-return) { pointer-events: none; }
/* …and the one that reaches what the carve-out re-enabled */
body.sky-mode #stage > *:not(#sky-return) * { pointer-events: none !important; }
```

**Generalise this.** *`pointer-events: none` on an ancestor is a default, not a
lock.* Any descendant may opt back in, and two features that each manage
pointer-events over the same subtree will silently fight — the one that runs
deeper wins, regardless of which is conceptually "more important". When a mode
must make a subtree inert, it has to say so about the descendants too.

---

## PROBLEM 155 — Spaceadom can close the conflicting program now, and the reason it must ask is written down

**The owner's request, 2026-08-20:** *"I was wondering of giving a button
asking to temporary or permanently close those when show me around on or
someone presses conflicts… just to make the experience seamless for users who
don't know how to close the conflicting thing, confirm before closing, let them
know if any prompt they have to accept."*

**This REVERSES a documented decision, and that is the point of the entry.**
`renderConflicts()` carried this comment since PROBLEM 96: *"Spaceadom never
closes another program for you… it is malware behaviour besides."* That was
right as a DEFAULT and wrong as an absolute — a user who does not know what
PowerToys is cannot act on a banner telling them to close it. What makes the
difference is not the action but the consent around it.

**Exact files.** `src-tauri/src/hook/conflict_close.rs` (new),
`src-tauri/src/hook/conflicts.rs` (`known_process_names()`),
`src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`,
`src/components/settings-panel.ts` (`conflictActions()`), `src/styles.css`.

**Three rules the backend will not break, and why each exists:**

```rust
// 1. A CLOSED LIST. This command is reachable from the webview, so without
//    this it is a "terminate any process by name" primitive.
fn is_known_conflict(process: &str) -> bool {
    let p = process.to_ascii_lowercase();
    crate::hook::conflicts::known_process_names().iter().any(|k| *k == p)
}

// 2. ASK POLITELY FIRST. taskkill WITHOUT /F sends WM_CLOSE, so the program
//    saves its state and shuts down properly; /T takes its children, which
//    matters for PowerToys — the Keyboard Manager engine is a child, and the
//    child is what actually holds the hook. Force is the second attempt only.

// 3. NEVER ELEVATE SILENTLY. A non-elevated app cannot end an elevated one,
//    and PowerToys usually IS elevated. That returns needs_permission, the UI
//    changes its button to say a prompt is coming, and only the NEXT press
//    raises it via ShellExecuteExW("runas").
```

**"Permanently" is bounded, and it reports what it did.** `remove_autostart()`
touches exactly two places a user-level program registers itself — the HKCU
`Run` key and the Startup folder — and returns a human-readable list of what
it removed, which the UI shows. **Scheduled Tasks are deliberately not
touched**: PowerToys' task is created by its installer under the machine
account, deleting it needs elevation, and getting it wrong breaks a program the
user chose to install.

**The UI is slow and loud on purpose.** Two presses, never one; the armed label
states the consequence (`Yes — close PowerToys now` vs
`Yes — close it and stop it starting`) so the second press is informed; the
other button hides while one is armed, so a stray press cannot fire the wrong
action; the result sentence is whatever Rust reported, **including the
failures** ("the permission prompt was declined, or Windows refused").

**Generalise this.** *A default of "we never do X" is not the same as "X is
wrong".* When the reason for the ban is consent, the fix is to build the
consent, not to keep the ban — but then the consent machinery IS the feature,
and it must be as carefully built as the action.

---

## PROBLEM 156 — four small ones the owner found in 1.0.62

**1. "After pressing clear this profile it shows Confirm, then after confirming
it still shows Confirm — which feels like a bug."** It was one. `arm()` set the
state and called `render()`; `disarm()` set the state and did not — so the
button kept the armed label after the action had already fired.

```rust,ignore
function disarm(): void { _armed = null; window.clearTimeout(_armTimer); render(); }
//                                                                      ^^^^^^^^ was missing
```

*Generalise: a state-changing pair must be symmetric about its side effects.
If one half re-renders, the other half has to, and the missing one is always
the "undo" — it gets written second and tested least.*

**2. "The Show me around button when turned off, all the descriptions
minimising takes too much time. It wasn't the problem in other builds."** It
was not: PROBLEM 154 took the description count from 8 to 16, and the convoy
stagger was a flat 80ms per row — so closing went from ~0.6s to ~1.3s of
stagger before the last row even started. Closing is now BUDGETED rather than
per-row: `min(CONVOY_STAGGER_MS * 0.65, CONVOY_OUT_MS / rows)`, so the whole
convoy is out inside 300ms however many rows exist.

*Generalise: a per-item delay is a hidden multiplication by a count that will
grow. Budget the total, then divide.*

**3. "You might have compromised on the animations when moving between themes.
There was a cool animation which I think you missed."** Correct — spec §5's
*"whole app cross-fades background/color 450ms"* was never ported. CSS custom
properties do not transition, so a token swap is instant by nature and the
theme change read as a flicker. `body.theme-xfade` is added by `applyLook()`
for 450ms and removed, transitioning the properties the tokens feed:

```css
body.theme-xfade, body.theme-xfade *:not(.toggle-thumb):not(.theme-seg-ind) {
  transition: background-color 450ms linear, background-image 450ms linear,
              color 450ms linear, border-color 450ms linear, fill 450ms linear !important;
}
```

Two details: `background-image` is included because `#stage`'s gradient is the
largest surface in the app and all three themes declare it with the same
structure (3 stops, same angle), which is the condition for a browser to
interpolate one gradient into another. And it is applied ONLY when the theme
actually changes — `applyLook()` also runs at boot and on the fun switch, and a
450ms transition on every surface during first paint fades the dashboard in
from nothing.

**4. "In describing you said twice Spaceadom doesn't close program for them."**
The Conflicts description and the conflicts hint said the same sentence, and
PROBLEM 155 made both of them false. Deduplicated and rewritten. The Conflicts
description is also **no longer gated behind "Show me around"**, per the owner:
a live fault on this machine should explain itself whether or not you asked for
a tour.

---

## Measurement note (2026-08-20) — Smart Search, closed by the owner

He tested 1.0.62's UI-Automation focuser and reported: *"it still doesn't work
in whatsapp, spotify, it's okay, just leave it. just ensure it works on google
search while inside browser."*

**So UIA did not fix WhatsApp or Spotify, and the reason is not diagnosed.**
Recorded rather than quietly dropped, with what is known: the tree walk finds
*something* (the fallback path did not log), so the likely causes are that the
element found is not the compose box (WhatsApp's is one of several
contenteditables), or that `SetFocus()` succeeds at the UIA layer while the app
re-routes focus itself. **Do not re-attempt from scratch** without first
reading the `smart_search:` log line, which names the branch taken.

Google was added to the DOCUMENTED-shortcut class it belongs in — `/` is
Google's own focus-search key, on both the home page and results pages — which
is the same class as YouTube, and that class has a 100% success record.

---

## PROBLEM 157 — the close button that silently refused, and three animations the owner could feel were wrong

**Symptoms, from the owner testing 1.0.63:** *"I tried pressing it and it did
nothing. It's still running. It's doing nothing… and I was not given any prompt
to approve."* Plus: *"this thing always staying there in the settings isn't
worth it — this thing can just pop up when someone presses the thing that is
conflicting"*, *"the satisfying animation of the slider of the themes moving
between the names sliding smoothly is not there anymore"*, *"when closing the
wait is still too long and the animation is not smooth enough, it feels
laggy"*, and *"I still didn't feel the smooth transition between the themes"*.

### 1. The close silently refused, because two matchers had to agree and lived apart

`Conflict.process` carries the **real running exe name**. For spacedesk that is
`spacedeskservice.exe`, while its entry in `KNOWN` is the prefix `spacedesk` —
`detect()` matches it with `name.starts_with(...)`. The guard in
`conflict_close` compared for **equality** against the list keys:

```rust
// BEFORE — refused every spacedesk close, silently
known_process_names().iter().any(|k| *k == process.to_ascii_lowercase())
// "spacedeskservice.exe" == "spacedesk"  ->  false
```

The refusal was one line of small grey text under the button, which is why it
read as "it did nothing".

```rust
// AFTER — conflicts.rs owns ONE matcher and both callers use it
pub fn is_known_process(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    KNOWN.iter().any(|(p, _, _)| if *p == "spacedesk" { n.starts_with("spacedesk") } else { n == *p })
}
```

**And no prompt appeared** because the elevation flow needed a THIRD press: the
unelevated attempt returned `needs_permission`, the button changed its label,
and only the next press raised UAC. The user has already confirmed they want
the program closed — the prompt is now raised on the same press, and the
confirm text says in advance that Windows may ask.

*Generalise this: two matchers that must agree and live in different files
will disagree, and the failure will be silent because the second one is a
guard — guards only speak when they refuse, and a refusal looks like nothing
happening.* One matcher, exported, both callers.

### 2. The buttons moved out of Settings and became a prompt

Two permanent buttons under every conflict, for an action you take once, in a
panel you open often. The conflict ROW is the trigger now
(`role="button"`, Enter/Space, a "Press to close it →" cue), and it raises
`conflict-prompt.ts` at top centre — where every other transient message in
this app already appears. Entrance is transform/opacity only, because this is
the one surface that shows up on a machine already in trouble.

**When it cannot close the program it now GUIDES instead of refusing** (the
owner's instruction): the buttons are replaced by *"Open Windows Start-up
settings"*, which runs `taskmgr.exe /0 /startup` — Task Manager's own switch
for that tab — and the text tells the user to find the program, choose Disable,
and restart.

**Also restored:** the per-app one-liner (`c.detail`), which had been gated
behind "Show me around". It is the thing that actually explains the conflict,
which is why the long Conflicts description could be cut to one sentence and
put back behind the tour.

### 3. The theme slider stopped sliding because the element stopped surviving

`.theme-seg-ind` has always carried `transition: transform 480ms
cubic-bezier(.34,1.45,.36,1)`. The CSS never broke. The handler called
`render()`, which rebuilds the panel — **destroying the indicator and creating
a new one already at the new position.** A brand-new element has nothing to
transition FROM.

```ts
// AFTER — update in place, never re-render
const seg = panelEl?.querySelector<HTMLElement>(".theme-seg");
seg?.style.setProperty("--seg-i", String(["earthy","warcry","starry"].indexOf(next)));
seg?.querySelector<HTMLElement>(".theme-seg-ind")?.setAttribute("data-seg", next);
panelEl?.querySelectorAll<HTMLElement>("[data-theme-set]").forEach((o) => { … });
// no render()
```

*Generalise this: a transition needs the SAME element to exist before and
after. If a state change re-renders the subtree, every transition inside it
becomes a jump — and the CSS will look perfectly correct while you debug it.*
This is the third bug in this family (the toggle characters in PROBLEM 148, the
open descriptions in PROBLEM 151, this).

### 4. The two "not smooth" animations were both too much work, not too little

**The convoy:** a `grid-template-rows` transition costs a LAYOUT PASS PER
FRAME, and 1.0.63 ran sixteen of them staggered, each also running a 460ms
keyframe entrance on its child. Closing is now **unstaggered** — sixteen boxes
collapsing together is the same total layout work as sixteen staggered, one
pass per frame either way, but it is over in 240ms instead of a second of
waiting. The row transition went 380 → 240ms, and the child's entrance 460 →
230ms so it finishes INSIDE the row transition rather than animating a box that
has already stopped moving.

**The cross-fade:** 1.0.63 applied it as `body.theme-xfade *`, five properties
on every element in the document. On a machine compositing in software that is
thousands of simultaneous interpolations, and the result was slower, not
smoother. It is scoped to the ten surfaces that actually carry the palette.

*Generalise this: when motion feels laggy, the first question is how many
elements are animating and whether the property forces layout — not what the
curve is.*

### 5. Low-power mode, because this has to run on any laptop

The owner's standing requirement: *"the whole app needs to be running in any
Windows laptop… optimized enough to run on any device which may or may not have
that much of graphical capabilities."*

The night scene's real cost is not its element count — it is `filter: blur()`.
Seven blurred surfaces (six storm masses at 10–15px, plus the lightning), all
of which MOVE on `cloudDrift`, so the compositor re-blurs them every frame.
`body.lite-scene` drops those blurs and lets the gradient stops carry the
softness, which is CLAUDE.md's own overlay rule applied to the dashboard.

It is decided once at boot from the three signals that mean "nothing spare":

```ts
const lite = appConfig.motion === "reduced"
  || appConfig.overlay_compositing === "software"
  || window.matchMedia("(prefers-reduced-motion: reduce)").matches;
```

…and logged, so a "the sky is stuttering" report can be answered without
guessing which branch the machine took.

### 6. A hidden window never gets a frame

`openConflictPrompt` added its `.is-in` class inside `requestAnimationFrame`.
**rAF does not fire in a window that is not compositing** — the class would
never land and the prompt would sit at `opacity: 0` forever, which is
indistinguishable from the feature not existing. Reading `host.offsetWidth`
flushes layout synchronously, which is all a transition needs for a "from"
state, and it does not depend on a frame ever arriving.

*Generalise this: never gate VISIBILITY on rAF. Use it for work that can wait
for a frame, never for the step that makes something appear.* This is the same
family as PROBLEM 135 (a page cannot observe that its own window is hidden).

### 5b. …and the low-power trigger was wrong, within the hour

**Symptom, from the owner:** *"you messed up the clouds and storms animation
now."*

**Two mistakes in one feature.**

1. **Software compositing was a trigger.** His machine composites in software
   as its NORMAL state — the overlay self-test set that back in PROBLEM 92 — so
   lite mode switched itself on for the one person it was not meant for.
   *Software compositing means "no GPU path", not "no headroom".* The trigger is
   now only the two signals the USER controls: Windows' reduced-effects setting
   and this app's own Visual effects switch.
2. **`filter: none` was too blunt.** The storm masses are gradients whose
   SOFTNESS is their shape; removing the blur leaves hard elliptical edges that
   do not read as cloud at all. That is not an optimisation, it is a different
   picture. Lite mode uses `blur(5px)` instead — blur cost grows with radius,
   so 5px is a fraction of 10–15px while the masses keep their form.

*Generalise this: a performance signal is not the same as a user preference.
Deciding to show someone less should be driven by what they asked for, not by
what their hardware happens to report — and an optimisation that changes what
a thing LOOKS LIKE is a design change wearing a performance costume.*

### 5c. "First install should be quiet" — it is, and here is why the owner's own machine is not

**His report:** *"when starting at first install, it was supposed to start with
fun mode off and show me around off, the user turns on if they want."*

**It does. His machine is not a first install.** Read from outside the sandbox
on 2026-08-20, his live `config.json`:

```
fun_mode=True  show_me_around=False  theme=starry  dark_mode=True
```

Those are HIS choices, made while testing 1.0.56–1.0.59, and they are stored.
A default only applies where a value is absent — overwriting a stored value
with a new default would mean every future default change silently rewrites
what the user picked, which is a far worse bug than the one being reported.

**This is now enforced rather than asserted** (`config/schema.rs`,
`first_install_tests`), because the owner has asked for it twice and it is
three flags among thirty that anyone adding a feature could flip:

```rust
#[test] fn first_install_is_quiet_and_earthy() {
    let d = AppConfig::default();
    assert!(!d.fun_mode);  assert!(!d.show_me_around);
    assert_eq!(d.theme, "earthy");  assert!(!d.dark_mode);
}
#[test] fn a_config_missing_the_new_fields_also_lands_quiet_and_earthy() {
    // Fixture built by DELETING the three fields from a serialised default —
    // a hand-written old config would need every field lacking a serde
    // default, and would rot the moment someone adds another.
    …
}
```

Both paths are checked because they can drift APART: `Default` is what a fresh
install writes; the serde defaults are what an OLD config missing the field
falls back to. A mismatch means the same person gets a different app depending
on when they installed.

**To see the true first-run experience** on a machine that already has a
config, rename `%APPDATA%\Spaceadom\config.json` and start the app — it will
write a fresh one. Renaming rather than deleting keeps the bindings.

### 5d. The storm, third attempt — and the diff that ended the guessing

**The owner, after two wrong fixes:** *"the clouds and storms are still messed
up and does not look as good as previous"*, then *"the storm was supposed to be
behind the ship to give it scary atmosphere, never for sky"*, then *"change
just the clouds and storm based on this [the lab], don't touch anything else."*

**Stop guessing, diff it.** Extracting the lab's `{{ cloudStyle }}` subtree and
comparing it span for span against `night-markup.ts`:

```
lab spans: 7   current spans: 7
span 0: identical … span 6: identical
```

**Nothing about the clouds was ever wrong.** Every gradient stop, radius, blur
radius and drift duration was already the lab's. The only thing altering them
was the world's `transform: scale(0.75)`, which rendered the bank at 525×195
instead of 700×260 — which is why it stopped looming and started reading as a
few small puffs.

**The two constraints only LOOK contradictory.** "Behind the ship, never for
sky" means it must stay a child of the scaled world, so it paints behind the
galleon and belongs to the ship's scene. "Match the lab" means it must render
at the lab's size. Both hold if the container counter-scales in place:

```css
#st-clouds {
  left: -146.667px !important;   /* -110 / 0.75 */
  bottom: 200px !important;      /*  150 / 0.75 */
  transform: scale(1.33333);     /*    1 / 0.75 */
  transform-origin: left bottom;
}
```

Measured after: `left -110, 700×260, 150px above the viewport bottom` — the
lab's numbers exactly — with all seven masses at their authored sizes and the
ship still 273×231.

**The failed attempt is recorded because its reasoning was seductive:** moving
the storm OUT of the world gave the right size and the wrong scene. A full-size
bank towering over a 0.75 ship reads as sky, which is precisely what the owner
rejected. Size and scene are separate properties, and the scaled-wrapper trick
is what buys both.

**Generalise this.** *When the owner says a transcribed thing "looks wrong",
diff it against the source BEFORE theorising.* Two builds were spent on
plausible theories — blur cost, low-power mode, layer ownership — for a
component whose markup was byte-identical to the design the whole time. The
diff took one command and pointed straight at the only thing that was
different: the coordinate space it was measured in.

### 5e. The storm, re-authored to the ship — and the moon spots that were never missing

**The owner, after the size fix landed (2026-08-20):** *"the clouds have gone
too far around, the clouds need to come behind and beside the ship… the main
job of the cloud and storms lightnings is to make the ship atmosphere look
scary, whereas you spread it too far, and the clouds need to be a little bit
more greyish, and the clouds right now have visible big big gaps in between
instead of overlapping which makes them look like spots rather than helping
make the atmosphere scarier."*

**This is the point at which the lab stops being the answer.** Its six masses
are spread across 700px, which is right on a lab canvas and wrong here,
because this app's galleon renders at 0.75. Restoring the lab's size (5d) gave
a bank that reached far past the ship on both sides — technically faithful,
and it lost the thing the storm exists for.

`STORM_MASSES` in `starry-sky.ts` is now **owner-directed and marked as such**,
with the lab's version left byte-identical in `night-markup.ts` so the original
stays diff-able. Three changes, one per sentence of the brief:

| Sentence | Change | Measured result |
| --- | --- | --- |
| "behind and beside the ship" | container 500×240 at screen −120px (was 700×260 at −110) | storm spans −120..380, ship −72..201 — covers it, reaches just past the bow, rises to 360px against the ship's 231px |
| "gaps… look like spots" | masses repositioned so every one shares area with others | overlaps per mass: 3, 4, 4, 5, 4, 2 — **minimum 2**, one connected body |
| "a little bit more greyish" | ramp desaturated navy → slate: `10,15,30 → 18,20,26`, `24,34,60 → 38,42,52`, `48,62,96 → 72,78,92`, `60,76,112 → 96,102,118`, rim `158,182,228 → 178,186,202` | gradient STRUCTURE untouched — same two radials, same stop positions, so night-scene4.md's "lighter than the sky in its mid-tones" still holds |

Blur radii and drift durations are the lab's, unchanged. The overlap check is
arithmetic (rectangle intersection over the six live bounding boxes), not an
eyeball — "they look like spots" is exactly the kind of judgement that needs a
number behind it.

### The moon's maria were never missing

**Owner:** *"the moon random spots disappeared, bring them back."*

Measured before touching anything: **9 maria and 2 wash shapes present, correct
sizes, correct positions.** They were being washed out, not removed.

`moonPow` multiplies the whole moon group's brightness by up to **1.66×**
(night-scene4.md §1 — and that is correct, the halo has to blaze rather than
the disc just going white). A 25%-opacity dark spot does not survive being
multiplied by 1.66, and this disc is also 0.75 of the size `moon.md` assumed.
Both effects push the same way.

```css
/* moon.md's values, pre-divided by the brightness cycle's mean */
.sky-moon-wash  { opacity: .23; }   /* was .17 */
.sky-moon-maria { opacity: .34; }   /* was .25 */
```

**Generalise this.** *"It disappeared" and "it is present but invisible" are
different bugs with different fixes, and the DOM can tell you which in one
query.* Reaching for "bring them back" would have re-added elements that were
already there. Also: when two independent scalings act on the same appearance —
here a 0.75 geometry scale and a 1.66 brightness multiplier — a value copied
from a spec that assumed neither is going to be wrong, and it will be wrong
quietly.

### 5f. The storm, settled — `design/storm-clouds.md`, transcribed

**The owner, after three failed attempts here:** *"use this, you got wrong
enough times."* He wrote a standalone spec, `design/storm-clouds.md`, and that
file is now the authority for this component. My 1.0.68 deviations — a 500px
bank, repositioned masses, a greyed ramp — are gone.

**What each attempt got wrong, because the sequence is the lesson:**

| # | What I did | Why it was wrong |
| --- | --- | --- |
| 1.0.64 | Blamed blur cost, added a low-power mode | Triggered on software compositing — which is his machine's NORMAL state. Wrong signal entirely. |
| 1.0.66 | Moved the storm out of the scaled world | Right size, wrong scene: a full bank over a 0.75 ship reads as sky, not as the ship's weather. |
| 1.0.68 | Re-authored the masses myself to "cluster on the ship" | Invented geometry to satisfy a description, when a spec for it already existed. Also greyed the ramp, which attacks the one thing §2 is emphatic about. |
| 1.0.69 | Transcribed `storm-clouds.md` | — |

**Two rules in that spec that a well-meaning tidy-up destroys, and 1.0.68
destroyed one of them:**

- **§2: storm cloud on a night sky must be LIGHTER than the sky in its
  mid-tones, not darker.** *"The first attempt used near-black masses and they
  were completely invisible against a #131a2e sky."* The `48,62,96` and
  `60,76,112` steps are what make the massing read; only the innermost core is
  darker than the ground. My "a little more greyish" change pulled exactly
  those steps toward slate.
- **§2: lopsided radii are load-bearing.** A cloud on `border-radius: 50%` is a
  smudged circle. All four corners must differ.

Plus §2's falloff reaching 0 alpha at **88%**, before the element edge — stop it
short and the blur reveals a circular seam.

**Scale.** The container counter-scales out of the ocean world's 0.75, so
§1's `left: -110px; bottom: 150px; 700x260` are REAL screen pixels and the blur
radii are not resampled by an ancestor transform. It stays a CHILD of the
world, in DOM order after the water and before the ship, which is §1's
requirement and gives the checklist's *"masts and rigging read in front of the
cloud; water reads behind it"*.

**Verified against the spec's own §6 checklist**, measured in the harness:

| Check | Result |
| --- | --- |
| container geometry | `left -110, 150px above the bottom, 700x260` — §1 exactly |
| six masses | `320x158 / 360x142 / 268x110 / 300x128 / 230x104 / 214x98` — §2 table exactly |
| blur radii | `12 / 15 / 10 / 13 / 12 / 14` — §2 exactly |
| "the six masses do not pulse together" | durations `23/31/19/27/35/29`, all distinct |
| masses form one bank, not spots | overlaps per mass `3, 5, 4, 3, 2, 3` — minimum 2 |
| "masts read in front, water behind" | DOM order water(0) → clouds(9) → ship(10) |
| lightning | `360x158`, `lightning 17s` |
| moonlight thins the bank | container `opacity 0.782` at pw 0.34 = `1 − .34×.64`, `transition 13s` |
| reduced motion | the bolt is REMOVED, not frozen — a stopped strike is a lamp over the ship |

**Generalise this.** *When a component has been wrong three times, the problem
is the absence of a spec, not the quality of the attempts.* Each fix here was a
reasonable response to the last complaint and none of them converged, because
"looks scary", "too far around" and "like spots" are descriptions of a result,
not of a target. The moment a file existed with numbers in it, the work took
one pass and was checkable line by line. **Ask for the spec earlier.**

### 5g. …then halved, with one number

**Owner:** *"ugh, just scale the clouds down by half and closer to the ship."*

`storm-clouds.md` was written for a full-size galleon; ours renders at 0.75, so
the spec's 700px bank out-reached the ship on both sides even when transcribed
perfectly. The fix is the container's transform, and **nothing inside the storm
changes**:

```css
#st-clouds {
  left: -106.667px; bottom: 173.333px;   /* screen -80px / 130px, pre-divided by .75 */
  transform: scale(0.66667);             /* 0.5 (halve) / 0.75 (undo the world) */
  transform-origin: left bottom;
}
```

Measured: **350x130 at left -80**, spanning -80..270 against a galleon at
-72..201, rising to 260 just past mastheads at 231.

**Why a transform and not six edited masses.** Rewriting the table would mean
changing eighteen numbers and permanently losing the ability to diff this
component against `storm-clouds.md` — which is the thing that finally ended
four rounds of guessing. Scaling the container keeps every authored value
intact and checkable, and makes the next size change one number instead of
eighteen.

*Generalise this: when a spec's geometry is right but its SCALE does not suit
its surroundings, scale the container. Editing the contents to compensate
destroys the only copy you can verify.*

---

## PROBLEM 158 — the archive and the share folder went stale because keeping them fresh was my habit, not the build's

**Symptom.** Asked whether 1.0.70 was ready to hand to a friend, three things
were wrong at once:

- `all-versions/` stopped at **1.0.65** — five versions behind — while its own
  header promised *"Every installer ever built lives in this folder."*
- `all-versions/WHAT-CHANGED.md` had no row for **1.0.64** or **1.0.66**.
- `share-spaceadom/` was still handing out **1.0.65**, with a README describing
  features that had since changed, and referencing a `PRIVACY.md` that was not
  in the folder.

**Root cause, and it is not carelessness in the interesting sense.** Archiving
and refreshing the share folder were separate manual commands I ran after each
build. They were run faithfully for 1.0.59 through 1.0.65 and then skipped for
five consecutive versions — **because the build cycle sped up**. During the
storm iterations a version took four minutes end to end, and the steps that
survive that pace are the ones the build performs, not the ones the operator
remembers.

*A manual step gets skipped exactly when the cycle speeds up, which is when it
matters most.*

**Exact files.** `scripts/archive-build.mjs` (new),
`src-tauri/tauri.conf.json` (`afterBundleCommand`).

```jsonc
"build": {
  "beforeBuildCommand":  "npm run build",
  "beforeBundleCommand": "node scripts/stage-symbols.mjs --real",
  "afterBundleCommand":  "node scripts/archive-build.mjs"   // <- new
}
```

The script copies both installers into `all-versions/`, replaces whatever is
in `share-spaceadom/` with the current pair, copies `PRIVACY.md` in beside them
(the share README tells friends to read it), and **warns without failing** when
the README or the changelog has no mention of the version just built.

**Two deliberate design choices:**

1. **It can never fail the build.** The whole body is wrapped in one
   `try/catch` that logs and swallows. A broken bookkeeping step must not cost
   a working installer — that trade is always wrong in this direction.
2. **The share folder is emptied of every non-current installer**, rather than
   just having the new one added. Leaving old ones there is precisely how
   someone sends a friend a build from five versions ago.

**The config edit went through parse → mutate → re-serialise with a
duplicate-key detector**, not a text substitution — PROBLEM 139's lesson, where
a duplicated `"wix"` key was legal JSON, the parser kept the last one, and a
template pointer vanished silently. Verified after writing: 3 lines changed,
nothing reformatted.

**Generalise this.** *If a step must happen after every build, it belongs IN
the build.* And when a folder's README states an invariant — "every installer
ever built lives here" — something has to enforce it, or the README becomes a
lie at the exact moment the project gets busy.

---

## PROBLEM 159 — a corrupt config factory-reset the user while a good backup sat unread

**Symptom (found by audit, not by a user — which is the point).** If
`config.json` fails to parse, the old code preserved it as `.json.corrupt`,
logged a line, and called `generate_defaults()`. Meanwhile
`%LOCALAPPDATA%\SpaceadomBackups\` holds a timestamped copy of every save from
the last hour, one per hour for a day, one per day for a week — maintained
since PROBLEM 102 and, until now, **never read back by anything**.

The log told the user a backup existed and left them to copy it by hand. A
friend who has never opened that folder will not do that; they will see an app
that forgot every binding they ever made.

**Exact file.** `src-tauri/src/config/mod.rs`.

```rust
// BEFORE — preserve the broken file, then discard the working one
let backup = path.with_extension("json.corrupt");
let _ = std::fs::copy(&path, &backup);
generate_defaults()

// AFTER
match newest_valid_backup() {
    Some((cfg, from)) => {
        log::warn!("config: recovered from backup {} — the unreadable file is at {}",
                   from.display(), backup.display());
        let _ = save_to_disk(&cfg, &path);   // if this launch crashes, the next
        cfg                                  // one must not decide again
    }
    None => { log::error!("config: no usable backup either — regenerating defaults");
              generate_defaults() }
}
```

**Two details that are the whole difficulty:**

1. **Sort by MODIFIED TIME, not by filename.** The names carry a timestamp
   today; sorting by a naming convention breaks silently the day that changes.
2. **Skip a backup that does not parse and keep looking.** Corruption tends to
   hit the most recent write — which is exactly the file a naive "restore the
   latest backup" would restore.

**Tested, because this is a RECOVERY branch** — the class of code that ships
unexercised and fails the one time it runs (PROBLEM 118's lesson, and
CLAUDE.md's stated rule for when a test is warranted). `newest_valid_backup_in`
takes the directory so it can be driven from a temp dir; four tests cover
newest-wins, corrupt-newest-falls-back, no-backups, and all-corrupt. Each uses
its own scratch directory because `cargo test` runs them on parallel threads
(PROBLEM 130 was a flaky test caused by four tests sharing one static).

**Generalise this.** *A backup nothing reads is not a backup, it is a
reassuring file.* If recovery requires the user to know the folder exists, find
it, identify the right file and copy it over the broken one, then for most
users the feature does not exist.

---

## PROBLEM 160 — I fixed the "manual step gets skipped" problem with a build hook that broke the build

**Symptom.** PROBLEM 158 wired `scripts/archive-build.mjs` into
`tauri.conf.json` as `afterBundleCommand`, and the next `cargo check` died:

```
thread 'main' panicked at build.rs:54:10:
failed to run tauri-build with the app manifest: unknown field `afterBundleCommand`,
expected one of `runner`, `devUrl`, `frontendDist`, `beforeDevCommand`,
`beforeBuildCommand`, `beforeBundleCommand`, `features`, ...
```

**There is no `afterBundleCommand` in Tauri v2.** `beforeBundleCommand` exists;
its counterpart does not. And `tauri-build` rejects unknown config keys by
PANICKING at build-script time, so the mistake does not degrade the build — it
stops every build, including `cargo check` and `cargo test`.

**Root cause of the ROOT CAUSE:** I verified the script by running
`node scripts/archive-build.mjs` and watching it archive correctly. **I never
re-ran a build.** The thing I changed was the build, and the thing I tested was
not. It shipped in one commit and would have blocked the very next build.

**The fix** — npm's `posttauri`, which runs after any `npm run tauri …`, the
command CLAUDE.md documents:

```json
"scripts": { "tauri": "tauri", "posttauri": "node scripts/archive-build.mjs" }
```

That also fires on `npm run tauri dev`, so the script now exits early when
there are no installers for the current version, rather than emptying the
share folder on a dev run.

**Verified the way it should have been the first time:** a full
`npm run tauri build`, which printed the archive step's own output — including
its two warnings that the README and changelog had no mention of the new
version, both of which were then true and are now fixed.

**Generalise this.** *Test the thing you changed, through the interface you
changed it in.* A build-system change is verified by running a build; a script
that a build calls is not verified by calling the script. This is the same
family as PROBLEM 143 ("a verification performed by the sandboxed process
cannot detect the sandbox") — in both cases the check ran somewhere the fault
could not appear.

Second lesson, smaller: **a config parser that rejects unknown keys is a
feature.** Tauri panicking here is why this cost one commit instead of shipping
silently as a hook that never ran.

---

## PROBLEM 161 — a dead keyboard hook was completely invisible

**Symptom.** If `SetWindowsHookExW` fails, or Windows evicts the hook and the
watchdog cannot get it back, **nothing works and nothing says so.** No
shortcut fires, the dashboard looks perfectly healthy, the tray icon is normal,
and the only evidence is a line in `%APPDATA%\Spaceadom\debug.log`. On a
friend's laptop that reads as "this app just doesn't do anything".

**Root cause.** Rust has always known — `HOOK_INSTALLED` is stored by
`install_hooks()` and exposed as `HookStatus.installed` (that was PROBLEM 66's
fix). The dashboard fetched the struct and **used one field of it**:

```ts
const status = await invoke<HookStatus>("get_hook_status");
setPausedState(status.bypass_active);   // and `installed` was dropped
```

**Exact files.** `src/main.ts` (`applyHookState`), `src-tauri/src/hook/mod.rs`
(`request_hook_rebuild`), `src-tauri/src/commands.rs` (`reinstall_hook`),
`src-tauri/src/lib.rs`.

A banner, not a toast — a toast announces an EVENT, and this is a persistent
STATE. It stays until the state changes, and it carries two buttons: *Try
again*, and *Open log folder*.

**"Try again" asks for a THREAD rebuild, not a re-hook**, and that distinction
is PROBLEM 132's whole lesson: re-hooking from a thread that is itself wedged
produces a hook that looks healthy and receives nothing. So the button sets the
same `ESCALATE_RESTART` flag the watchdog raises after two failed attempts.

**Banner ownership.** The conflicts banner and this one share
`#conflict-banner`, so both now stamp `dataset.owner`. A conflicts refresh that
finds nothing no longer wipes a hook warning, and a detected conflict — the
likelier explanation of a dead hook, with more useful text — is not overwritten
by it.

**Generalise this.** *A status field that nothing reads is a status field that
does not exist.* The backend had been reporting this correctly for dozens of
versions. Grep for the fields of any status struct and check each one has a
consumer; the ones that do not are silent failures waiting.

**Honest limit:** the banner cannot be triggered on demand — making
`SetWindowsHookExW` fail is not something a test can arrange here — so it is
verified by wiring and by code review, **not by having seen it appear.**

---

## PROBLEM 162 — the key editor could not be used on the commonest cheap laptop

**Symptom.** `#key-detail-panel` had no `max-height` and no `overflow`. It is
centred with `translate(-50%, -50%)`, so a panel taller than the window grows
off **both** edges — taking Assign/Done with it.

**Measured:** at 1366×768 with 150% scaling — 911×512 CSS pixels, the most
common cheap Windows laptop — a key with a long detected-apps list could not be
finished at all.

**Exact file.** `src/styles.css`.

```css
max-height: min(560px, calc(100vh - 32px));
overflow-y: auto;
overscroll-behavior: contain;
```

`min()` so nothing changes on a large screen; `overflow-y` on the PANEL rather
than an inner box so the padding scrolls with the content instead of the last
row hiding beneath it.

**Verified** in the harness at 911×512: `max-height` computes to 480px, the
panel's rect is fully inside the viewport, and `scrollHeight > clientHeight`.

**Generalise this.** *Anything centred with a translate must be bounded.* A
centred element that outgrows its container escapes in two directions at once,
and the half that leaves the top is the half nobody notices in testing.

---

## PROBLEM 163 — the sea was redrawn from scratch on every theme toggle

**Symptom.** `seaField`/`seaWave` generate ~129 KB of SVG across four tiles.
They ran on every `buildStarrySky()`, and the scene is rebuilt whenever the
theme or Fun mode changes — so each flip paid full generation AND a full image
decode, under a **new `data:` URL each time**, which means the image cache
could never help.

**Exact file.** `src/components/starry-sky.ts` — a module-level `_seaCache`,
filled on first build and reused.

**A second, better reason than performance:** the randomness is now per-LAUNCH
rather than per-rebuild. The sea should not silently become a different sea
because you opened the settings panel.

**Generalise this.** *A generated `data:` URL defeats every cache by design* —
the cache key is the content, and the content is new every time. Anything
expensive that is regenerated identically-in-spirit but differently-in-bytes
should be generated once and held.

---

## PROBLEM 164 — the webview could run any program on the machine, for no caller

**Symptom.** `src-tauri/capabilities/default.json` granted
`shell:allow-execute` — permission for the WEBVIEW to execute arbitrary
processes — with **no caller anywhere**. Every shell-out in this app
(`conflict_close`, the log folder, Task Manager) is a Rust command, which needs
no webview permission at all.

Not exploitable today: the webview loads only local assets and the CSP allows
no external hosts. It is a standing grant with no user, which is the definition
of unnecessary attack surface, and a Store reviewer looking at a keyboard-hook
app will ask about exactly this.

**Fix:** removed, with the reason recorded in the capability file's own
description so it is not re-added by reflex.

**Generalise this.** *A permission with no caller is not "harmless", it is
unaudited.* Grep every permission for a consumer before shipping; the ones with
none cost nothing to remove and everything to explain later.

---

## PROBLEM 165 — the Store build and the friend build had the same filename

**Symptom (caught before it shipped).** `npm run store` writes an installer
that embeds the entire WebView2 runtime — **209.8 MB** versus the friend
build's 5.6 MB — to the *same path*:
`bundle/nsis/Spaceadom_<v>_x64-setup.exe`.

Two silent failures follow. `scripts/install-real.cmd` installs whatever is at
that path, so a local install after a Store build quietly installs the 210 MB
variant. And the 210 MB file gets copied into `share-spaceadom/` or attached to
a release, and friends download 210 MB for nothing.

**Fix.** `scripts/label-store-build.mjs`, wired as npm's `poststore`, renames
the output to `…-setup-STORE.exe` and leaves the normal path EMPTY — so the
next `tauri build` is the only way to get a friend installer back. An obvious
failure instead of a silent substitution.

It also **refuses to label a build under 100 MB**: a small file means the
offline config did not apply, and that installer would be rejected by the
Store as a downloader stub. Better to say so than to label it convincingly.

**Verified:** both files now coexist, `5.6 MB` and `209.8 MB`, correctly named.

**Generalise this.** *Two build variants that share an output path will be
confused, and the confusion is silent because the filename is the only thing
anyone checks.* Give every variant a distinct name at the moment it is
produced, and make the wrong one impossible to pick up by accident.

---

## PROBLEM 166 — the publish folder, and why it is built rather than assembled

**Not a bug — a request, with a bug's worth of lesson attached.** The owner
asked for *"a file named 'to-publish-in-microsoft-store'* keeping the
publishable version inside it".

**Why it is generated by `poststore` and not filled by hand.** This is the
third time in three days that a folder someone must keep in step with a build
has drifted:

- `all-versions/` stopped at 1.0.65 while its header promised every installer
  was there (PROBLEM 158).
- `share-spaceadom/` was handing out a five-version-old build with a README
  describing features that had changed (same).
- And the Store variant and the friend variant shared a filename, so the wrong
  210 MB binary could be picked up silently (PROBLEM 165).

A publish folder is the worst possible place for that failure, because **the
Microsoft Store pins a submission to a URL whose bytes must never change**
([app package requirements](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msi/app-package-requirements)).
Uploading the wrong file is not a mistake you quietly correct; it is a new
version and a new review.

**Exact file.** `scripts/label-store-build.mjs`, wired as npm's `poststore`.

```js
// Exactly ONE installer, always the current one. Anything else is removed.
for (const f of readdirSync(pub)) {
  if (/\.exe$/i.test(f) && !f.includes(version)) unlinkSync(join(pub, f));
}
copyFileSync(to, join(pub, `Spaceadom_${version}_x64-setup-STORE.exe`));
copyFileSync(join(ROOT, "PRIVACY.md"), join(pub, "PRIVACY.md"));
if (!readFileSync(notes, "utf8").includes(version)) {
  say(`WARNING: SUBMIT-CHECKLIST.md does not mention ${version}`);
}
```

**What is in the folder, and what is deliberately not:**

| File | In git? | Why |
| --- | --- | --- |
| `SUBMIT-CHECKLIST.md` | yes | The steps, in order, with the signing detail that catches people out |
| `LISTING.md` | yes | Description, features, certification notes, age-rating answers — all paste-ready |
| `Spaceadom_<v>_x64-setup-STORE.exe` | **no** | 210 MB; `*.exe` is already ignored repo-wide |
| `PRIVACY.md` | **no** | A COPY. The source of truth is `../PRIVACY.md`, and a folder-local `.gitignore` says so — editing the copy is a mistake that survives until the next build overwrites it |

**The signing instruction that would otherwise be got wrong.** Policy 10.2.9
says "the binary **and all of its PE files**". Signing only the installer
leaves an unsigned `spaceadom.exe` inside it, which is a rejection. The order
is: sign the exe → rebuild the installer so it wraps the signed exe → sign the
installer. That is stated at the top of the checklist rather than buried.

**Generalise this.** *A folder whose contents must match a build is a build
output.* Every time one has been maintained by habit in this project it has
drifted, and the drift is always discovered by someone else — a friend with an
old installer, or, here, a reviewer with the wrong binary.

---

## PROBLEM 167 — PiP: no restore-on-exit, no handle validation, an animation that skipped every vertical hop, and threads fighting each other

**Symptom.** The owner, 2026-08-24: *"i noticed pip isnt working properly"*, and
when asked what that looked like: *"it behaves oddly and doesnt go to the 4
corners properly eithwr"* + *"Loses its title bar and won't come back"*. Then,
live, mid-session: *"i tried pip , it didnt work for the claude desktop app,
then space hud sound appeared but showed nothing ,it appeared behind claude"*.

**Evidence, from `%APPDATA%\Spaceadom\debug.log` 2026-08-24.** The cycle itself
was reaching every state, so "PiP is broken" was never the whole story:

```
22:11:26.735 pip: hwnd 0x40f9e -> corner 3 at (0,824)
22:11:27.135 pip: restoring hwnd 0x40f9e to original frame
22:11:27.527 pip: entering PiP for hwnd 0x40f9e
22:11:28.062 pip: hwnd 0x40f9e -> corner 1 at (1280,48)
22:11:28.532 pip: hwnd 0x40f9e -> corner 2 at (1280,824)
```

Two facts fall straight out of those timestamps:

1. `corner 1` then `corner 2` is **470 ms apart** against an animation that runs
   up to `120 x 8ms = 960 ms`. Two spring threads were driving one window.
2. `corner 1 (1280,48)` to `corner 2 (1280,824)` changes **only Y**.

**Root causes — four, all in `engine/actions/pip.rs`.**

**(a) The exit test read one axis.**

```rust
if (x - tx as f64).abs() < 0.5 && vx.abs() < 0.5 {
    break;
}
```

Top-Right to Bottom-Right is a purely vertical move: `x` is already at target and
`vx` is 0, so this is TRUE on iteration 1. That tap snapped instantly while the
horizontal hops either side of it glided — "doesn't go to the 4 corners
properly", exactly.

**(b) Overlapping animations.** `animate_to` spawned a thread per press with no
cancellation, so a fast cycle had two or three threads calling `SetWindowPos` on
the same HWND toward different targets.

**(c) Nothing ever released a window except the 5th tap.** `restore_window` was
reachable only at `position_index >= 4`, and the cache was memory-only, with no
`IsWindow` validation and no exit handler. Quit the app mid-cycle and the window
stayed borderless, topmost and quarter-sized **forever**, with the only control
that could undo it gone.

**(d) That orphan is also why the Guide HUD disappeared.** PiP sets
`HWND_TOPMOST` and cleared it only on that same 5th tap. A window that could not
be cycled out of stayed topmost for the rest of its life — and the overlay's own
"re-assert topmost" was a no-op (PROBLEM 168), so it could never climb back over
it. One failed PiP permanently hid the HUD behind an ordinary-looking app.

**The fix.** `pip.rs` rebuilt. The owner's decisions, taken 2026-08-24:

- **Stop stripping the frame entirely** — *"Stop stripping entirely — corner-snap
  only."* On Electron/Chromium windows (Claude, Discord, VS Code, Spotify)
  `WS_CAPTION|WS_THICKFRAME` are not what draws the frame, so stripping them
  changed nothing visible while arming (c). PiP is now move + resize +
  stay-on-top, and **nothing that can fail to be put back is ever changed.**
- **Keep the CURSOR's monitor** — *"the monitor the cursor is on — keep as is."*
  Offered the alternative and declined it: PiP doubles as throw-this-window-to-
  the-screen-I-am-pointing-at. Do not "fix" it.
- **Rescue orphans + restore all on exit** — *"Rescue them + restore all on
  exit."*

Both axes now decide convergence:

```rust
let settled_x = (x - tx as f64).abs() < 0.5 && vx.abs() < 0.5;
let settled_y = (y - ty as f64).abs() < 0.5 && vy.abs() < 0.5;
if settled_x && settled_y { break; }
```

A generation ticket retires a superseded flight *onto its own target*, so it
neither fights the new one nor abandons the window mid-air:

```rust
static ANIM_GEN: AtomicU64 = AtomicU64::new(0);
// in animate_to:
let ticket = ANIM_GEN.fetch_add(1, Ordering::SeqCst) + 1;
if ANIM_GEN.load(Ordering::SeqCst) != ticket { place(tx, ty); return; }
if !unsafe { IsWindow(h()).as_bool() } { return; }
```

The cache became process-global so shutdown can read it, and `restore_all()` is
called from the Tauri exit handler in `lib.rs`:

```rust
.map(|app| {
    app.run(|_, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit) {
            engine::actions::pip::restore_all();
        }
    })
});
```

`looks_orphaned()` repairs a window an OLDER build stripped, and is deliberately
narrow — topmost **and** no caption **and** no thick frame **and** within 4px of
half the work area on both axes **and** parked in one of the four corners. A bare
"borderless and topmost" test would also match games, full-screen players and
every app with custom chrome, and handing `WS_CAPTION` to a Chromium window that
never had one would wreck its layout.

**How it was verified.** `cargo test --lib` — four new tests covering the corner
arithmetic at a zero origin and at a negative-x second monitor, the shape of the
vertical-hop bug, and that `new_cache()` hands out the SAME map (or `restore_all`
silently restores nothing). 23 pass. The rest needs the owner's hands: this
cannot be exercised without a real foreground window.

**Generalise this.** *A convergence test must cover every dimension the thing
converges in.* And: *if the only way out of a state is a control the user might
never reach, the state is a trap — give it an exit that does not depend on them.*

---

## PROBLEM 168 — "re-assert topmost on EVERY show" never called SetWindowPos, because tao diffs the flag first

**Symptom.** *"i also noticed space hud doesnt appear all the time or not on top
of everything"* — and, specifically, *"it appeared behind claude"*.

**The log said the window was fine.**

```
22:11:32.306 guide_hud: overlay window shown
22:11:32.326 overlay_fit_hud: asked 908x572 -> clamped 908x572 @ (399,247);
             GOT size Ok((908.0, 572.0)) pos Ok((399.0, 247.0)); visible Ok(true)
```

Right size, right position, `visible: true`, sound played — nothing on screen.
That is the signature of *the window is underneath something*, not *the page
failed*, and CLAUDE.md's window rules already say to suspect the WINDOW when
everything measurable inside the page is healthy (PROBLEM 135's lesson).

**Root cause.** Three places re-asserted topmost, each with a comment stating
exactly what it meant to do:

```rust
// guide_hud/mod_impl.rs — "Re-assert topmost on EVERY show: other always-on-top
// windows appearing since the last show can end up above us in the topmost
// band, and the user requires the HUD over everything."
let _ = win.set_always_on_top(true);

// commands.rs overlay_fit + overlay_fit_handover — "Toasts must sit above
// everything, same rule as the HUD."
let _ = win.set_always_on_top(true);
```

**None of them did anything.** tao caches window flags and diffs before touching
the OS — `tao-0.35.3/src/platform_impl/windows/window_state.rs`:

```rust
fn apply_diff(mut self, window: HWND, mut new: WindowFlags) {
    let mut diff = self ^ new;
    if diff == WindowFlags::empty() { return; }            // line 321
    ...
    if diff.contains(WindowFlags::ALWAYS_ON_TOP) {         // line 339
        SetWindowPos(window, HWND_TOPMOST, ..., SWP_ASYNCWINDOWPOS | ...);
```

The overlay is created `always_on_top(true)` and never turned off, so the flag is
already set, the diff is empty, and it returns at line 321 without ever reaching
line 339.

At the OS level `SetWindowPos(HWND_TOPMOST)` on an already-topmost window is
**not** a no-op — it moves the window to the top of the topmost band. That is the
entire behaviour these three call sites wanted, and none of them got it. Anything
that entered the band after us stayed above us permanently: a pinned media
player, an installer, or — routinely on this machine — a window PiP had marked
topmost and failed to release (PROBLEM 167).

**The fix.** `commands.rs`, one helper, straight to Win32:

```rust
pub(crate) fn raise_overlay_topmost(win: &tauri::WebviewWindow) {
    let Ok(hwnd) = win.hwnd() else { ... return };
    let raw = windows::Win32::Foundation::HWND(hwnd.0 as *mut _);
    unsafe {
        if let Err(e) = SetWindowPos(raw, HWND_TOPMOST, 0, 0, 0, 0,
                                     SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) { ... }
    }
}
```

Called from all three old sites plus `overlay_fit_hud`, which is the last
placement before the ring is revealed. `SWP_NOACTIVATE` matters: the overlay is
NoActivate/click-through and must never take focus. Not `SWP_ASYNCWINDOWPOS`
(which tao uses) — the caller shows the window immediately afterwards and wants
the z-order applied, not posted.

**How it was verified.** tao's source read directly from the vendored crate, not
from memory — `apply_diff`'s early return is quoted above at its real line
numbers. Behaviour needs the owner's hands.

**Generalise this.** *Setting a flag to the value it already holds is a no-op, so
any "re-assert" expressed as a setter is a lie.* Same family as PROBLEM 161 ("a
status field nothing reads does not exist") — code that reads as covered in
review because the COMMENT describes the intent and the CALL cannot deliver it.
When you want an ACTION, call the action.

---

## PROBLEM 169 — every overlay placement used primary_monitor(), and failed silently when there was no monitor

**Symptom.** *"Happens both ways but worse with two displays."*

**Root cause, two of them.** `overlay_fit`, `overlay_fit_handover`,
`overlay_fit_hud` and `place_overlay_centred` all resolved the display with
`primary_monitor()`. The HUD was documented as primary-monitor-only "by the
user's explicit decision, 2026-08-10" — but this owner works on an external
screen much of the day, so the ring was being drawn correctly on a panel he was
not looking at. Indistinguishable, from where he sat, from "it did not appear".

Worse, `place_overlay_centred` did this:

```rust
fn place_overlay_centred(win: &tauri::WebviewWindow, w: f64, h: f64) {
    if let Ok(Some(mon)) = win.primary_monitor() {
        ...
    }
}
```

No `else`. When `primary_monitor()` returns `None` — which is exactly what
happens for a moment during a hotplug, a lid close or a resolution change, and
this machine does that several times a day — it positioned nothing and the caller
went on to `show()` anyway. The ring then painted into whatever box the last
toast had left behind.

**The fix.** One resolver, `commands::overlay_monitor`, with a chain that always
lands somewhere and says which rung it used:

```rust
pub(crate) fn overlay_monitor(win: &tauri::WebviewWindow) -> Option<tauri::Monitor> {
    if let Ok(pos) = win.cursor_position() {
        if let Ok(Some(mon)) = win.monitor_from_point(pos.x, pos.y) { return Some(mon); }
    }
    if let Ok(Some(mon)) = win.primary_monitor() { ...debug...; return Some(mon); }
    let first = win.available_monitors().ok().and_then(|m| m.into_iter().next());
    ...warn or error...
    first
}
```

The owner reversed the primary-only decision on 2026-08-24, choosing the
**cursor's** screen over the foreground window's. That matches what PiP already
does (`MonitorFromPoint(GetCursorPos())`, kept deliberately in the same
conversation), so both features answer "which screen?" the same way and the rule
only has to be learned once.

**Generalise this.** *A placement that cannot find a monitor must say so and still
land somewhere.* An `if let` with no `else` around a positioning call is a silent
failure whose symptom appears later, somewhere else, as "it looked wrong".

---

## PROBLEM 170 — nothing brought a JUST-LAUNCHED app to the front

**Symptom.** *"when apps launched , they do not come up , they sometimes launch
minimized in the taskbar"*. Asked directly whether it also happens when the app
is already running: **"Only when it has to launch it."**

**Root cause.** `force_foreground` is called from four places in
`smart_cascade.rs` — lines 867, 1073, 1130, 1179 — and **every one of them is
inside a focus-an-existing-window path**. Nothing ran after a launch at all.
`shell_launch` ended at:

```rust
log::info!("cascade: ShellExecute accepted {file} (hInstApp={inst}, process_created={pid_created})");
```

and returned `true`. The log bears this out — every launch in the file ends there
with no follow-up line:

```
21:46:32.561 cascade: launching C:\Program Files\Google\Chrome\Application\chrome.exe
21:46:32.601 cascade: ShellExecute accepted ... (hInstApp=42, process_created=true)
```

So the CORE_AIM contract "**If closed**: Launch the app" was half-implemented:
the process started and where its window ended up was left to Windows. Windows
says no, for a concrete reason: Spaceadom is not the foreground process when a
shortcut fires (our hook swallowed the keystroke, so the OS never credited us
with the input), and a process launched by a background process inherits that
refusal. Its window opens behind, or the taskbar button flashes. Apps that
restore their last session's state come back minimised.

**The fix — two halves, neither sufficient alone.**

1. `AllowSetForegroundWindow(ASFW_ANY)` immediately before the shell call, in
   both `shell_launch` and `run_browser`. This is the documented way to hand our
   foreground privilege to the process we are about to start. It applies only to
   a process the shell creates for us, and expires quickly.
2. `raise_after_launch(exe_stem)` — a bounded watcher, because cold-start latency
   varies wildly (measured on this machine: Brave ~500 ms, VLC ~1 s, Electron
   apps several seconds), so one delayed attempt would miss exactly the slow apps
   that fail today.

The owner's choice for how hard to try: *"keep trying ~8s, but stand down if you
touch anything."* Two abort signals:

```rust
// You typed. Baseline taken AFTER SETTLE_MS, because releasing the Space of
// the very combo that started this launch is itself a keyboard event.
std::thread::sleep(std::time::Duration::from_millis(SETTLE_MS));
let kb_baseline = crate::hook::last_keyboard_event_tick();
...
if crate::hook::last_keyboard_event_tick() > kb_baseline { ...stand down...; return; }
// You switched to a THIRD window (not the launch origin, not the target).
let fg = unsafe { GetForegroundWindow() };
if !fg.0.is_null() && fg != started_fg { ...stand down...; return; }
```

Mouse MOVEMENT deliberately does not abort — drifting the pointer while an app
loads is not a decision.

`find_window_by_exe_stem` is **find-only**, deliberately not
`try_focus_or_minimize`: that one MINIMISES a window that is already foreground,
so calling it here would hide an app that had opened correctly — a worse bug than
the one being fixed. `launch_app` was split into a thin wrapper over
`launch_app_inner` so the raise happens on every successful route rather than
being remembered at each of the five `return shell_launch(...)` sites.

Store/UWP targets are excluded: their windows belong to
`ApplicationFrameHost.exe`, so an exe-stem search can never find them, and the
shell foregrounds those itself.

**How it was verified.** Compiles clean, 23 tests pass. The behaviour needs the
owner's hands — an agent shell cannot make a real cold launch land in front.
`raise_after_launch:` log lines were added at every decision so the next log
answers it without guesswork.

**Generalise this.** *A follow-up step that must be repeated at every `return` is
a step that will be missed when the next return is added.* Wrap the function.

---

## PROBLEM 171 — the compositing self-test scored strikes for being COVERED

**Symptom.** Not reported by the owner — found while reading his log to check
PROBLEM 168.

```
22:08:55.833 compositing: overlay pixels did not change across 450ms while visible (strike 1/3)
22:08:57.910 ... (strike 2/3)
22:10:12.396 ... (strike 1/3)
22:10:14.503 ... (strike 2/3)
22:11:32.841 ... (strike 1/3)
```

It reached 2-of-3 twice in one evening. **Three strikes silently flips the
machine to software rendering and restarts the app.** His config confirms it has
happened before: `overlay_compositing` was `"software"` on 20 Aug and `"auto"`
now.

**Root cause.** The test samples screen pixels with `GetPixel` on the desktop DC
and asks whether they changed. `GetPixel` returns *whatever is visible at that
coordinate* — our overlay if we are on top, and the window above us if we are
not. **"I painted nothing" and "someone is covering me" produce identical
readings, and both scored a strike.** The strike at 22:11:32.841 lands on the
exact HUD show the owner described as *"showed nothing, it appeared behind
claude"*, with a PiP'd permanently-topmost window over it. The overlay was
compositing perfectly.

**The fix.** `commands.rs` — abstain when covered:

```rust
if unsafe { overlay_is_occluded(&win, &probes) } {
    log::info!("compositing: another window is covering the overlay at the probe points — NO VERDICT ...");
    done();
    return;
}
```

Checked AFTER the 450 ms wait, not before: a window can be raised over the HUD
during the sample, and it is the state at the moment of judgement that decides
whether the pixels mean anything.

**`WindowFromPoint` is the obvious call and it is WRONG here.** The overlay is
click-through (`WS_EX_TRANSPARENT`, from `set_ignore_cursor_events(true)`), and
`WindowFromPoint` deliberately skips transparent windows — it would never return
our overlay, so every show would look occluded and the test would abstain
forever. That is silent disablement, which is precisely the PROBLEM 122 failure
this file already records once. So `overlay_is_occluded` walks the z-order from
the top with `GetTopWindow` + `GetWindow(GW_HWNDNEXT)`: if we meet the overlay
before any visible, un-cloaked window covering a probe point, nothing is above
us. Cloaked windows are skipped (`DWMWA_CLOAKED`) — suspended UWP apps still
report visible and still have rects, and treating them as cover would abstain
constantly. An inconclusive walk returns `false`, so the test still runs.

**Generalise this.** *A measurement that cannot distinguish two causes must not be
allowed to pick one.* A remedy applied to a misdiagnosis is worse than no remedy:
this one was about to disable GPU compositing to fix a z-order bug.

---

## PROBLEM 172 — the app stood down inside full-screen VIDEO, not just games

**Symptom.** Contributory to "the HUD doesn't appear all the time".

**Root cause.** `hook/fullscreen.rs` detects an exclusive full-screen app
structurally: `WS_POPUP` + `WS_EX_TOPMOST` + covers the whole monitor. When it
matches, `hook::FULLSCREEN_ACTIVE` goes true and the hook's very first line
passes EVERYTHING through — no shortcuts, no Guide HUD, nothing. That test is a
decent description of a game and, unavoidably, also of a full-screen video, a
PowerPoint presentation and a screen share.

**The fix.** Owner's call: *"stand down for games only, not video."* There is no
cheap, honest way to ask Windows "is this a game?", so name the things that are
not:

```rust
const NOT_A_GAME: &[&str] = &[
    "brave.exe", "chrome.exe", "msedge.exe", "firefox.exe", ...
    "vlc.exe", "mpv.exe", "potplayermini64.exe", "spotify.exe", ...
    "powerpnt.exe", "zoom.exe", "ms-teams.exe", "discord.exe", "claude.exe",
];
```

UNION with the user's `fullscreen_allowlist`, not a replacement: a curated list
keeps every entry, and an upgrading user gets the browsers and players without a
config migration having to run correctly first.

**Generalise this.** *A structural test that matches the right thing and three
wrong ones needs a name list, not a cleverer structural test.*

---

## PROBLEM 173 — Windows evicted the keyboard hook 17 times in one day, and the app was deaf for ~11s each time

**Symptom.** *"space hud doesnt appear all the time"*. Found in the log, not
reported — the owner had no way to see it.

```
22:03:02 WATCHDOG — user active 0ms ago but NEITHER hook saw anything (kb 9000ms / mouse 9000ms). Foreground: claude.exe
22:08:20 ... Foreground: claude.exe
22:24:17 ... Foreground: brave.exe
22:28:44 ... Foreground: Discord.exe
```

17 on 2026-08-24 alone. And at every startup:

```
conflicts: spacedesk is running (spacedeskservice.exe)
conflicts: PowerToys is running (powertoys.exe)
```

**Root cause.** Windows enforces `LowLevelHooksTimeout` (300 ms default) as a
hard deadline and silently unhooks anyone who overruns it — no error, no event.
Our callback is microseconds of work, but the deadline is measured across the
whole hook CHAIN, so a slow hook ahead of us evicts US. With two other low-level
keyboard hooks resident, that is routine rather than exceptional.

The watchdog *detects and repairs* this, but its timings were a 3000 ms timer
against an 8000 ms silence threshold: **~11 seconds of worst-case deafness**,
during which holding Space does nothing at all and nothing tells the user why.

**The fix, three parts.**

1. **Detect faster** — `WATCHDOG_TICK_MS = 1000`, `BLIND_MS = 3_000`. Worst case
   ~4 s instead of ~11 s. Not lower: the threshold is what separates "our hook is
   deaf" from "this person is not typing right now", and PROBLEM 101 is the
   record of getting that wrong (260 false alarms in two days). The asymmetry
   justifies the tightening — a false positive costs one reinstall, bounded by
   the existing 60-second cooldown; a miss costs a dead app in the user's hands.
2. **Count it where a human can see it.** `HOOK_EVICTIONS_TOTAL` — a second,
   never-drained counter. `HOOK_REINSTALLS` is `swap(0)`-ed on every Space
   release by `drain_hook_diagnostics`, which is right for a log line and useless
   for a UI: anything asking "how often has this happened?" would read zero,
   because the user pressed Space between the eviction and the question.
3. **Fix the CAUSE, on request only.** `set_hook_timeout(raise)` writes
   `LowLevelHooksTimeout = 5000` to `HKEY_CURRENT_USER\Control Panel\Desktop`.
   HKCU only (no elevation, no UAC, cannot affect other users), never automatic,
   fully reversible, and the UI states the sign-out requirement up front —
   Windows reads the value at logon, and a setting that appears to do nothing for
   an hour is worse than no setting.

Surfaced in Settings under Conflicts, and hidden entirely when the count is 0 and
the timeout has not been raised: there is no point offering a registry change to
someone whose hook has never been evicted.

**Generalise this.** *A counter that is reset by an unrelated event cannot answer
a question about history.* And: *an intermittent fault the user cannot see is
reported as "the app is flaky"; the same fault with a number beside it is a bug
report.*

---

## PROBLEM 174 — "Guide-to-toast motion" is now a setting, and it is OFF

**Symptom.** The owner, 2026-08-24: *"the space hud to toast motion should have
an on off switch because some people gave me feedback that they found it
disturbing, too much time consuming and doesnt add much to the functionality, so
, just have an on off switch in the settings for this specific space hud to toast
, off by default, if off then it has toasts and space hud like before , but
ensure no twitches or buggy feel"*.

**Not a bug — but note this is the THIRD time the flight has changed state.** It
went off for 1.0.33 (*"he tried 1.0.32, preferred 1.0.27's plain behaviour"*),
came back for 1.0.51 with new flights he supplied himself, and is now off again.
Making it the user's choice retires the argument permanently.

**The change.** `toast.ts`'s `WARP` and `SLING` were build-time `const`s. They are
now `let`, default `false`, driven by `hud_toast_flight` in the config.

ONE switch drives BOTH, deliberately. Asked whether "off" should keep the inbound
absorb, the owner chose *"Everything — full 1.0.27"*. That is also the safer
answer: WARP and SLING share `_stageMode`, `_slingStaged` and `_hudBusy`, so
leaving one on leaves the whole state machine live — and that state machine is
what latched `hudBusy=true` for three minutes in his log (PROBLEM 175).

**Nothing was reconstructed from memory.** The plain path both flags fall back to
has been present and correct all along; `absorbIntoSpace` documents its own
fallback as *"always correct, just less pretty"*. OFF is the code that shipped in
1.0.27.

Plumbing, following the theme rule exactly (`CLAUDE.md`: the overlay learns about
a setting two ways and needs both):

```rust
// commands.rs save_config
let _ = app.emit("flight-changed", new_config.hud_toast_flight);
```

```ts
// overlay.ts — seed from get_config, because an event that only fires on CHANGE
// leaves a freshly-created overlay with the wrong value
applyFlight(cfg?.hud_toast_flight === true);
```

`=== true`, never `!== false`: the key is absent from every config written before
1.0.73, and those users are precisely the ones who asked for the motion to stop.

`applyFlight` also cleans up when the switch is turned off MID-HOLD, which would
otherwise strand the machinery it was driving — `_stageMode` blocks every window
fit and `_absorbed` pills do not age, so a flight in the air at that moment would
leave the overlay unfittable and a pill frozen wearing SPACE's identity.
`_flying` is deliberately NOT cleared: it counts flights physically mid-animation
which decrement themselves, and zeroing it would let a fit run underneath one.

**How it was verified.** Two tests pin the default on BOTH paths — `Default`
(what a fresh install writes) and the serde default (what an old config missing
the field falls back to), because they can drift apart and a mismatch means the
same user gets a different app depending on when they installed.

---

## PROBLEM 175 — `_hudBusy` latched true and blocked every toast for the rest of the session

**Symptom.** *"nothing shows up but sounds"*, and — asked whether restarting
fixes it — **"Yes — restart fixes it until it happens again."**

**Evidence, measured.** The overlay's own instrumentation, in the owner's log:

```
22:08:31.493 overlay-js: sling: text="PiP: Top-Left"    hudActive=false hudBusy=true chips=8
22:08:31.993 overlay-js: sling: text="PiP: Top-Right"   hudActive=false hudBusy=true chips=8
22:11:16.709 overlay-js: sling: text="PiP: Top-Right"   hudActive=false hudBusy=true chips=8
22:11:34.665 overlay-js: sling: text="Frame Restored"   hudActive=false hudBusy=true chips=8
```

At 22:10:13.950 it was still `hudBusy=false`; one second later it flipped and
never came back. `hudActive=false` with `hudBusy=true`, held across **three
minutes** and many keypresses. Session-scoped — exactly what a restart clears.

**Root cause.** `hideGuideHud` sets the flag on entry:

```ts
function hideGuideHud(actionPending = false): void {
  _hudActive = false;
  _hudBusy = true;
```

and clears it in exactly ONE place — inside the `setTimeout` at the bottom. But
both SLING branches `return` before reaching it:

```ts
if (flightLeft > 0) {
  ...
  window.setTimeout(() => { if (!_hudActive) hideGuideHud(false); }, flightLeft + 40);
  return;                                    // <-- _hudBusy still true
}
if (actionPending && !_slingHeld) {
  _slingHeld = true;
  window.setTimeout(() => { if (!_hudActive) hideGuideHud(false); }, SLING_HANDOVER_MS);
  return;                                    // <-- _hudBusy still true
}
```

Each schedules another `hideGuideHud(false)` — **guarded by `if (!_hudActive)`**.
Re-hold Space inside that grace window and `_hudActive` is true, the rescheduled
call is skipped, and nothing ever clears the flag.

`_hudBusy` gates `fitToStack()` and `requestFit()`, and `overlay_fit` is what
SIZES AND SHOWS the overlay window. Latched, it means no toast and no HUD
placement for the rest of the session.

**The fix — a deadline, not more `return` bookkeeping.** Every deferral is
bounded, so if the flag is still set well past the longest of them, no path is
coming to clear it:

```ts
const armBusyDeadline = (): void => {
  window.clearTimeout(_hudBusyGuard);
  _hudBusyGuard = window.setTimeout(() => {
    if (!_hudBusy || _hudActive) return;
    _hudBusy = false; _stageMode = false; _slingStaged = false; _slingHeld = false;
    if (_toasts.length) requestFit();
    else if (_isOverlay) invoke("overlay_toasts_done").catch(() => {});
  }, SLING_HANDOVER_MS + SLING_MS + 600);
};
```

Clearing early costs nothing: `_hudActive` is checked independently everywhere
`_hudBusy` is, so an early clear can only permit a fit the HUD's own state
already allows. Every legitimate clear now also `clearTimeout`s the deadline, and
`showGuideHud` clears both — a fresh hold is proof the previous one finished, and
without that the user's own attempt to make the HUD reappear was the very event
being blocked.

**How it was verified.** The root cause is confirmed by the log above, which is
stronger than a repro: it is the fault happening on the owner's machine, with the
flag values printed. The fix compiles and the frontend builds; whether the latch
recurs needs a day of his use. With PROBLEM 174's switch OFF by default, none of
this code runs for most users anyway.

**Generalise this.** *A flag set on entry and cleared on ONE exit path is a latch
waiting for a second exit path to be added.* Clear it on every return, or give it
a deadline — this one now has both.

---

## PROBLEM 176 — a held Space swallowed every OS chord: Win+Shift+S became "launch Spotify"

**Symptom.** Owner, 2026-08-24: *"when holding the space if I press on the Print
Screen button then Spotify comes up. For no reason. It doesn't let me take
screenshot while holding the space bar."*

**Root cause.** Not Print Screen at all — `VK_SNAPSHOT` (0x2C) matches no combo
arm and always passed through. The owner's screenshot key is the **Win+Shift+S**
snip chord (`PrintScreenKeyForSnippingEnabled = 1`, verified live). The combo
gate in `hook/mod.rs` asked only `MODIFIER_ACTIVE && is_down`; the rule
*"Ctrl+Space / Alt+Space / Win+Space are real OS shortcuts, never swallow them"*
existed only on the Space-DOWN path, so it protected Space-pressed-last and
nothing else. The `S` of Win+Shift+S became `KeyCombo::Alpha('s')`, suppressed
with `LRESULT(1)`. Then the engine chain, proven in the log:

```
23:41:20.192 engine: combo Space+s received
23:41:20.210 cascade: could not resolve "slack.exe" ...            (Professionals: s = Slack)
23:41:20.210 cascade: ... falling back to the FOUNDERS binding     (Founders: s = Spotify)
23:41:20.211 Event: Space+? | Target: Spotify.exe | Action: Restore (Enum)
```

Both halves of the report from one missing check. Dates to 1.0.27 (`e1f8103`),
not a recent regression.

**Fix.** `hook/mod.rs` — in the combo block, before ANY branch that can consume
the key:

```rust
if vk != VK_RMENU && other_modifier_down() {
    PASSED_TO_OS.fetch_add(1, Ordering::Relaxed);
    return CallNextHookEx(None, n_code, w_param, l_param);
}
```

Three details that are each load-bearing:
- **Placement ABOVE the rollover branch.** The first version sat below it;
  with `ROLLOVER_MS = 200` on this machine, any chord within 200ms of
  Space-down was eaten by `in_rollover` and retyped as a literal `" s"` before
  the gate ever ran.
- **`VK_RMENU` exempt** — Space+RightAlt cycles profiles and Right Alt IS Alt;
  guarding it would silently delete profile cycling.
- **Shift stays out of `other_modifier_down`** — Win+Shift+S is caught by the
  Win. Space+Shift is not an OS chord.

Accepted knowingly: the pass-through returns before `SPACE_ABORTED` is set, so
Space-held + Ctrl+C now performs the copy AND types a space on release. The
chord working is worth the space. `passed-to-os(...)` was added to the drained
diagnostics so "Spaceadom ate my key" and "my chord did nothing" stay
distinguishable.

**Generalise.** *A rule enforced on one entry path is not a rule.* The invariant
was "OS chords always win"; it was implemented as "OS chords win when Space
comes last".

---

## PROBLEM 177 — the deferred HUD show raced its own cancel; the loser could win

**Symptom.** *"At the present moment it is stuck. Like I'm not holding the space
but the space hud is still stuck."* Screenshot: the ring on screen, Space up.

**Root cause.** The show is deferred by `guide_hud_delay_ms` (500ms here). The
engine's guard — `if !*cancel_rx.borrow() { …locks, Vec-building… show }` — is
not atomic with the show. A cancel landing in that gap is honoured, then forgotten,
and the show proceeds with nothing left to undo it. Caught live, 81ms apart:

```
23:41:07.793 guide_hud: hide with action pending - window stays up for the handover
23:41:07.874 guide_hud: overlay window shown        <- the loser, winning
```

Compounded by 1.0.73 moving `HUD_VISIBLE.store(true)` to the START of
`show_guide_hud` (its comment reasoned about a false positive, never a lost
update): the hide consumed a flag belonging to a show that had shown nothing,
terminal state `window visible, HUD_VISIBLE == false`, from which no Rust hide
path could recover — every one is gated on that flag. Result measured: 3m36s
with zero fits, zero hides, nine PiP actions and a launch all invisible.

**Fix.** `guide_hud/mod_impl.rs` + `engine/mod.rs`, four pieces:

1. `HOLD_EPOCH` — `begin_hold()` stamps each Space-hold; every cancel
   (`end_hold`, OUTSIDE the `HUD_VISIBLE` guard — the race being closed is
   exactly the case where the flag is still false) and every new hold advances
   it.
2. The stamp is re-checked **inside** `show_guide_hud`, three times: on entry,
   immediately before `win.show()`, and before the `guide-hud-show` emit — the
   entry check alone leaves ~80ms of window work after it, which is the
   original race in a smaller coat.
3. `VISIBLE_EPOCH` records WHICH hold published the flag, so a late-aborting
   show clears only its OWN flag — an unconditional `store(false)` would let
   stale show A clobber newer show B's flag and recreate the bug.
4. A reconciliation branch in `hide_guide_hud_pending`: swap returned false but
   `SHOW_OUTSTANDING` says a show completed unconsumed → force the hide and the
   `guide-hud-hide` emit (the page's `_hudActive` is cleared ONLY by that
   event).

Two hazards found on re-review and fixed before shipping:
- The reconciliation originally opened with `win.is_visible()` — a **blocking**
  getter (`rx.recv()`, no timeout) reached on every typed space under the
  engine lock and from the hook thread. It now gates on one atomic
  (`SHOW_OUTSTANDING`) and touches Tauri only in the real failure.
- `abort_if_stale`'s hide is gated on `SHOW_OUTSTANDING.swap(false)`: at the
  "before show" checkpoint this invocation has shown nothing, and the window
  may be legitimately up from a previous hold's action-pending toast handover —
  hiding it unconditionally would take the window down under a toast (PROBLEM
  135's class, reintroduced by the fix's first draft).

Second half: the watchdog now calls `hide_guide_hud()` on eviction — a mid-hold
eviction loses the Space-UP forever, and every piece of key state was repaired
on reinstall except the one the user can see.

**Generalise.** *Every gap between a check and the work it guards is a race;
re-check at the point of no return, and give the state an owner (epoch) so late
losers can tell their own writes from newer ones.*

---

## PROBLEM 178 — the new-profile name box had no way back

**Symptom.** *"What happens if I don't write anything in it? The add-new-profile
thing doesn't come up again… if I had not pressed anything after some time, it
should go back how it was before."*

**Root cause.** `profile-editor.ts` `wireNewProfile`: `open()` set
`openBtn.hidden = true` and the only routes back were Escape or a SUCCESSFUL
create. The state also outlived the popover closing, so the ＋ button stayed
missing for the session.

**Fix.** Three exits, one per way of changing your mind: Escape (unchanged);
focus leaving the row (deferred one tick — clicking Add blurs the input FIRST,
and a synchronous revert would tear the row down before Add's click handler ran,
silently not creating the profile); and a 15s idle timer re-armed per keystroke.
Blur and idle revert ONLY when the box is empty — typed text is the user's work.
`resetNewProfileRow()` is called from `closeProfilePopover` so reopening always
shows the button.

**Generalise.** *A control that can only be undone by completing the action you
decided against is a trap. Every transient state needs an abandon path.*

---

## PROBLEM 179 — the keyboard could not react to a fast cursor sweep, and the first fix measured in the wrong coordinate space

**Symptom.** *"When the cursor is moved in a very fast way over the keyboard it
should show some motion and a reaction to it."*

**Root cause.** `.key { transition: all 200ms }` + `:hover { translateY(-4px) }`.
A sweep hovers each key ~20–40ms; a 200ms ease covers under a third of its
travel and reverses. The animation was correct and never got to happen.

**Fix.** `src/key-wake.ts` — a velocity-driven wake on the STANDALONE CSS
`translate`/`rotate` properties, which compose with `transform` instead of
fighting it, so the deliberate-hover lift keeps its 200ms softness untouched.
`styles.css` switched `.key` from `transition: all` to an explicit property
list OMITTING translate/rotate (with `all`, the browser would ease every wake
write over 200ms and reintroduce the lag). Instant attack, ~300ms spring
release, strength scaled by speed, smoothstep falloff, loop parks when settled.

Reviewed adversarially before shipping; five real defects fixed:
1. **Coordinate-space mismatch** — measured viewport px, wrote board-local px
   under `#keyboard-scale`'s `transform: scale()`. Now ONE space (board-local
   design px), pointer mapped per frame, `_scale = rect.width / offsetWidth`.
2. **16px baked-in offset** — bootstrap measures during the intro cascade,
   whose `backwards` fill holds every key at `translateY(16px)`; centres cached
   16px low, wake rendered above the cursor, row above reacting 2.1× the row
   below. No observer could see the cleanup (transform ≠ ResizeObserver;
   inline-style ≠ childList MutationObserver). Fixed by re-measuring from the
   cascade cleanup timeout in `keyboard-matrix.ts`.
3. **Frame-rate dependence** — lerp factors were per-frame; now normalised to a
   60Hz reference from the rAF timestamp.
4. **Tilt strobe** — `Math.sign()` of a quantity that is zero on the line of
   travel flipped per sub-pixel jitter on the keys under the cursor;
   `rotate` has no transition, so they snapped 2×TILT per frame. Now a clamped
   continuous `cross/22`, latched with the direction.
5. Runs while the board is blurred behind the key editor or hidden in sky mode;
   `enabled()` now checks both, and `applyMotion` tears the wake down when
   Visual effects turns off mid-session.

Re-measure strips inline translate/rotate first (rects would otherwise include
the live displacement), and `measureKeyWake` is re-entrant across rebuilds.

**Generalise.** *Any effect that reads geometry and writes transforms must name
its coordinate space once and stay in it — and never measure an element that is
mid-animation.*

---

## PROBLEM 180 — sixteen special keys were destroyed unconditionally, in every build since 1.0.27

**Symptom.** None reported — that is the point. Space+Enter, Tab, Left, Right
and F1–F12 did nothing for any user ever, silently.

**Root cause.** The combo arms dispatched `KeyCombo::Special(...)`
unconditionally and destroyed the key with `LRESULT(1)`, directly under a
comment claiming *"only dispatch if the user has bound them in special_keys
config"*. The config check lived downstream in `engine::handle_special`, AFTER
the keystroke was gone; its `// key passes through — not configured` comment
described something that cannot happen. `special_keys` is `{}` in the owner's
config and no UI can write it, so all 16 were dead everywhere.

**Fix.** `BOUND_SPECIALS: AtomicU32` (bits 0–11 = F1–F12, 12 Enter, 13 Tab,
14 Left, 15 Right), consulted in the match arms (`VK_TAB if special_bound(13)`
…). One relaxed load + shift — PROBLEM 58 respected. Published from BOTH the
startup config load in `lib.rs` AND `config::save` (the single mutation
funnel); the atomic starts at 0, so the startup publish is what stops the same
bug reappearing with a smaller window. Both false comments fixed in the same
commit. No `BOUND_LETTERS`: Founders maps all 26, the mask would be a constant.

Honest scope note: this makes the keys stop VANISHING (Tab alt-tabs again while
Space is held); it does not make Space+F1 launch anything — `special_keys` has
no UI, which is separate work.

**Generalise.** *A comment describing a check is not the check. Enforce a
precondition where the irreversible action happens, not downstream of it.*

---

## PROBLEM 181 — the keyboard hook alone was evicted in 11–189s bursts, and the watchdog structurally could not see it

**Symptom.** *"All these used to work inside the Spaceadom app itself. Right now
it works only if we minimize that app."* Measured — the report captured live:

```
23:35:17.905 WATCHDOG — (kb 19000ms / mouse 3515ms). Foreground: spaceadom.exe <- OWN WINDOW
23:35:29.017 guide_hud: overlay window shown
23:35:31.468 engine: combo Space+b received      (and three more within seconds)
```

19 seconds deaf at the focused dashboard; re-hook; everything works 11s later.
24 of 46 alarms on 2026-08-24/25 had mouse silence <5s with keyboard silence
5–29s. Worst episode: 23 consecutive minutes (2026-08-20 02:55–03:18) through
six full thread rebuilds. NOTE: focus-specificity is NOT proven — only 51/444
alarms name spaceadom.exe; the eviction is machine-wide (spacedesk + PowerToys
resident, `LowLevelHooksTimeout` unset → 300ms wall-clock default).

**Root cause of the blindness.** The only detector was
`both_dead = kb_silence > BLIND_MS && ms_silence > BLIND_MS` — and the mouse
hook keeps firing, holding it false. The keyboard-only branch was deleted by
PROBLEM 101, correctly (it could not tell "evicted" from "not typing"), but the
deletion left NO detector for the failure that actually happens.

**Fix.** A REFERENCE `WH_KEYBOARD_LL` (`ref_kb_hook_proc`) that does exactly one
relaxed store and `CallNextHookEx` — it cannot overrun the timeout, so it cannot
be evicted for being slow. Discriminator:

```rust
let kb_only_dead = kb_silence > BLIND_MS && ref_silence < BLIND_MS;
```

Reference firing + primary silent = eviction, CERTAIN — the ambiguity PROBLEM
101 died on is gone. Same thread as the primary deliberately: a wedged pump
stops both, which is `ESCALATE_RESTART`'s case, and a cross-thread reference
would blur the two failure modes. The WARN now names which failure it is.
`install_hooks` swaps the reference alongside the primaries.

**Generalise.** *When a signal is ambiguous, do not tune thresholds — add a
reference that isolates one cause. A do-nothing probe that cannot fail the way
the subject fails turns an unanswerable question into arithmetic.*

---

## PROBLEM 182 — the watchdog's own throttles were a bigger source of deafness than the eviction

**Symptom.** Six alarms reporting 58,000–60,016ms of silence — not a plausible
eviction duration; the 60s cooldown's own length showing up in its instrument.
Ten alarms reading exactly `kb 4000ms / mouse 4000ms` — identical clocks, the
signature of `install_hooks()` stamping both and NEITHER hook then firing
(`reinstall ok: true` only means SetWindowsHookExW returned a handle — PROBLEM
132, still reproducing).

**Root cause.** (a) The cooldown early-return sat ABOVE the silence
computation: after any repair, unrepairable and unmeasured for 60s. (b) The
cooldown assumed the previous repair worked; often it demonstrably had not.

**Fix, with two guard rails that the first drafts each violated:**
- Measure first, throttle second. If the previous repair delivered events, hold
  the full 60s; if it delivered NOTHING, retry after **5s — a floor, not
  zero**. Zero would spin and, worse, burn the supervisor's budget.
- `previous_worked = LAST_KB_EVENT > WATCHDOG_LAST_REINSTALL` — and the stamp
  is therefore stored AFTER `install_hooks()` with a fresh tick. Stored before
  (the first draft), install_hooks' own clock-stamp lands a few ms later and
  satisfies the comparison by itself: the adaptive path could never fire. Found
  by re-deriving the arithmetic, not by testing.
- Escalation (thread rebuild) is rate-limited SEPARATELY at 120s
  (`LAST_ESCALATION`): the supervisor in lib.rs gives up FOREVER above 5
  rebuilds in 10 minutes ("Space+key is DEAD until the app is restarted"), and
  the historical 2-minute cadence is precisely what kept 7 consecutive rebuilds
  inside that cap. Letting the 5s retry drive escalation would have converted
  intermittent deafness into permanent deafness — the fix worse than the bug.

**Generalise.** *A retry policy has three rates — detect, repair, escalate —
and they must be budgeted independently. And a "did it work?" comparison must
be stamped after the work, or the work satisfies it.*

---

## PROBLEM 183 — the diagnostics instrument could only report while the hook was alive

**Symptom.** I argued from *"saw N key events, 0 while the Spaceadom window had
focus"* (12 samples) that the hook was deaf when the app was focused. The
argument was worthless.

**Root cause.** `drain_hook_diagnostics()` had exactly ONE caller: the engine's
`SpaceUp` arm. A SpaceUp only arrives if the hook received BOTH halves of a
Space press — so every line ever printed was written at an instant the hook
demonstrably worked. The failure episodes produced no line at all; the counter
has read non-zero 35 times historically (`2026-08-16 … 341 of them`), disproving
my reading outright. Bonus defects: the seen-block swapped counters to zero
without printing when `seen == 0` AND without advancing `LAST_SEEN_REPORT`
(silently discarding an accumulated own-focus count), and "in the last minute"
was a floor, not a window (two consecutive lines 8.5 hours apart both claimed
it).

**Fix.** A 30s `tokio::interval` on the ENGINE thread also drains (never the
hook callback — PROBLEM 58); `LAST_SEEN_REPORT` always advances when the window
elapses; the line prints when `seen > 0` **or the user was active within 60s**
(`millis_since_last_input`, non-windows stub added), so a deaf-while-active
minute finally leaves a trace while idle nights stay quiet; and the wording
reports the true elapsed window.

**Generalise.** *An instrument whose trigger requires the system to be healthy
cannot observe the system being sick. Check what has to be true for a
measurement to be RECORDED before believing its absence — or its zeros.*

---

## PROBLEM 184 — six win32k syscalls per keystroke inside the hook callback, added by the fix for PROBLEM 176

**Symptom.** None yet — caught in review before it shipped, by the codebase's
own precedent.

**Root cause.** `other_modifier_down()` ran up to six `GetAsyncKeyState` calls,
and PROBLEM 176 put it on the path of EVERY combo key. `GetAsyncKeyState`
enters win32k and contends on USER32 state the foreground app's UI thread also
touches — PROBLEM 134's argument verbatim, which removed `GetForegroundWindow`
from this same callback and left these behind. With `LowLevelHooksTimeout`
unset, the 300ms deadline is WALL-CLOCK: a callback merely waiting on a
contended lock misses it like a slow one. The mouse hook survives every
eviction on this machine; `ms_hook_proc` makes no win32k calls at all. That
contrast is the whole argument.

**Fix.** `MODS_DOWN: AtomicU32` maintained from the hook's OWN event stream —
it sees every Ctrl/Alt/Win transition (`track_modifier`, called before every
early return so a release during bypass cannot latch; the injected-cookie check
runs earlier still, so our own synthetic Alt tap never pollutes the mask).
`other_modifier_down()` is now one relaxed load. Self-healing for a key-up lost
to an eviction: `resync_modifiers()` (the GetAsyncKeyState sweep) runs from the
watchdog timer and from `install_hooks` — off the callback, where it is free.
The PROBLEM 100-family hazard ("bookkeeping lied about key state and broke
every shortcut") does not apply: that failure was specific to Space, which we
SUPPRESS; Ctrl/Alt/Win are never suppressed, so the OS and our ledger see the
same events — and the resync bounds any drift at ~1s regardless.

**Generalise.** *When a file documents why a class of call was removed from a
hot path, grep for the survivors of that class before adding another. The
comparison instrument was free: the hook that never fails is the one that makes
no syscalls.*

---

## PROBLEM 185 — the Warcry theme's HUD and toasts wore Starry Night colours instead

**Symptom.** The Guide ring and toast messages displayed the wrong palette: cold
blues and a starry theme, not the blood crimson and cold iron the owner had set
as Warcry.

**Root cause.** The theme system relied on a boolean: `body.nocturne` on the
dashboard meant "dark mode" — but both Warcry and Starry Night use a `nocturne`
base, differing only in a secondary tint on top. The dashboard's CSS could tell
them apart with `body.nocturne[data-theme="warcry"]` and `body.nocturne[data-theme="starry"]`
selector chains, but the overlay — a separate window — never had `data-theme`
set on its body. Every overlay rule with a nocturne qualifier therefore
matched for both themes. Toasts and HUD wore Starry Night's palette inside
Warcry's session.

**Fix.** Four changes, each one load-bearing:

1. **`commands.rs` now emits the theme value as a STRING.** Instead of a boolean
   change-flag, `theme-name-changed` carries the full enum value ("Earthy",
   "Warcry", or "Starry Night"). The single event is sufficient because themes
   are not changed from Rust — only from the dashboard, which re-emits via
   `save_config`.

2. **`overlay.ts` seeds `document.body.dataset.theme` at startup** from
   `get_config().theme`, translating the enum value into the same string the
   CSS expects. This is mandatory because the event only fires on CHANGE — a
   freshly opened overlay window sees no event and would wear the previous
   session's palette without it.

3. **`toast.ts` gained `applyThemeName(value: string)`** that sets the dataset
   and runs on every theme-name-changed event. No longer does the toast miss
   changes because it was looking for a boolean.

4. **`overlay-earthy.css` gained a complete `body.nocturne[data-theme="warcry"]`
   block** transcribed from `themes.css`: crimson `#b83024` for the ring accent,
   cold iron `#7b8792` for outlines, surfaces `#1f100d` and `#2b1713` for the
   deeper layering. Without this block the selector chain never matched.

**Generalise.** *A boolean that distinguishes two cases by the absence of a
third case cannot scale to three. Store the actual value, not a signal, and
broadcast it to all windows that need to observe it.*

---

## PROBLEM 186 — "Give Shortcuts More Time" was unexplained and the owner did not understand what it did

**Symptom.** The owner asked: *"even I do not understand what that means… what
would happen if more time is not given? and why keep it as an option rather
than the default?"*

**Root cause.** The setting existed but carried no documentation. The note field
was unused.

**Fix.** `settings-panel.ts` now displays two notes:

1. **The mechanism:** "Windows gives keyboard-watching apps 0.3 seconds per
   keypress and cuts them off entirely if they overrun."

2. **Why it is not the default:** "This is a Windows setting affecting every
   keyboard app on the PC. It needs a sign-out to take effect. The trade-off
   is that a genuinely hung keyboard app could hold keys for 5 seconds instead
   of 0.3 seconds — so disable it if you trust your apps."

The button label now names the numbers in both directions:
- On: "Raise Windows' limit from 0.3 to 5 seconds"
- Off: "Put back Windows' 0.3 second limit"

These words translate the setting from abstract ("more time") to concrete (how much
time), from "option" to trade-off (what you are choosing between), and explain
the Windows plumbing instead of leaving it to guesswork.

**Generalise.** *Name a setting by its numbers and its real constraint, not by
the quantity of change. "Raise from 0.3 to 5 seconds" is immediate and
testable; "give more time" is abstract and invites confusion.*

---

## PROBLEM 187 — elapsed-time window computed against zero-initialized timestamp measures UPTIME, not elapsed

**Symptom.** A log line reported: `hook: saw 1 key event(s) in the last 38539s, 0 of them while the Spaceadom window itself had focus`. 38,539 seconds is 10.7 hours — the app reported it had been running for more than 10 hours when it had been running for a few minutes and the owner was actively using it. The same line then appeared again at 07:10:58.

**Root cause.** `drain_hook_diagnostics()` computed elapsed time as `now - LAST_SEEN_REPORT`, where `LAST_SEEN_REPORT` was an `AtomicU32` initialized to 0. The first call to drain arrived when `tick_count()` (machine uptime in milliseconds) was actually 38,539,000 ms. Subtracting zero from that gave 10.7 hours regardless of how long the diagnostic window actually was. The counter was then zeroed, so the next drain 8.5 hours later started from zero again — producing the duplicate.

**Fix.** Lines 128–189 in `src-tauri/src/hook/mod.rs`, in `drain_hook_diagnostics()`:

```rust
if last == 0 {
    LAST_SEEN_REPORT.store(now, Ordering::Relaxed);
    KB_EVENTS_SEEN.store(0, Ordering::Relaxed);
    KB_EVENTS_OWN_FG.store(0, Ordering::Relaxed);
} else if now.saturating_sub(last) >= 60_000 {
    // ... drain the counters and print
}
```

On the first call, seed the reference point to the current tick and zero the counters. Only on subsequent calls — when `last != 0` — compute the elapsed window and decide whether to print. This moves the zero timestamp from the log output into the clock.

**Generalise.** *An elapsed-time window computed against a zero-initialized timestamp measures UPTIME, not elapsed. Seed the reference on first use. Print nothing on that first call — print only when the window has matured enough to mean something.*

---

## PROBLEM 188 — Space released while another modifier is held silently dropped instead of typing a space

**Symptom.** Holding Alt and pressing Space intended to type a space (Alt is not a bound modifier). Instead, nothing happened. No error, no space typed, nothing in the log. The window menu did not open because Space was suppressed and Alt+nothing-else does nothing.

**Root cause.** The Space-UP handler at lines 1422–1440 in `src-tauri/src/hook/mod.rs` checked whether another modifier was physically down and, if so, dropped Space entirely:

```rust
if !other_modifier_down() {
    inject_space();
}
```

This is correct for SPACE-DOWN — Alt is a pass-through modifier and Space should not launch a command when Alt is held. But the release path inherited the same gate. When a modifier is held at release time, Space-UP should type a space anyway, because nothing was launched and the key must complete its normal purpose.

**Fix.** Lines 1422–1440 now store the modifier state AT PRESS time and honour it at release time. When Space is pressed with another modifier held, the combo is aborted immediately (`SPACE_ABORTED` set). At release, if no combo was aborted, Space types a space — UNLESS another modifier is STILL held, in which case a new counter `SPACE_DROPPED_MODIFIER` records the event for diagnostics:

```rust
if !SPACE_ABORTED.load(Ordering::Relaxed) {
    if !other_modifier_down() {
        inject_space();
    } else {
        SPACE_DROPPED_MODIFIER.fetch_add(1, Ordering::Relaxed);
    }
}
```

The counter is drained and included in the diagnostic log alongside the other event counts.

**Generalise.** *When a gate is added on the DOWN edge of a key, audit the UP edge. Asymmetry between press and release — a check that applies to one but not the other — is its own bug class and will surface only when the user presses while in state X and releases while in state Y. Do not assume the DOWN logic is complete for UP.*

---

## PROBLEM 189 — diagnostic log misreported when the keyboard hook last received input

**Symptom.** Log lines printed: `hook: DEAF for the last 9s — the reference hook fired 3100ms ago (keys ARE reaching the chain) but the primary hook saw 0 of them. This is NOT 'nobody typed'.` At the same time, logs printed `hook: saw 0 key event(s) in the last 0s`. Contradictory: the first line says keys are reaching the chain; the second says the app has been silent for zero seconds (which is obviously false on any long-running session).

**Root cause.** `drain_hook_diagnostics()` used an `idle_ms < 60_000` discriminator based on mouse movement to decide whether "nobody typed" or "the hook is deaf". This was a guess with no evidence. The reference hook existed by then and fires whenever a key reaches the OS, regardless of whether the primary hook saw it — perfect evidence. But the log was not using it. The "0s" came from the same measurement that produced PROBLEM 187's 38,539s: zero-initialized timestamp, no seeding.

**Fix.** Lines 157–187 in `src-tauri/src/hook/mod.rs`, in `drain_hook_diagnostics()`, replace the mouse-based discriminator with the reference-hook clock:

```rust
let ref_silence = now.saturating_sub(LAST_REF_KB_EVENT.load(Ordering::Relaxed));
if seen > 0 {
    log::info!("hook: saw {seen} key event(s) in the last {elapsed_s}s, {own} of them \
                 while the Spaceadom window itself had focus");
} else if ref_silence < 60_000 {
    log::warn!("hook: DEAF for the last {elapsed_s}s — the reference hook fired \
                 {ref_silence}ms ago (keys ARE reaching the chain) but the primary hook \
                 saw 0 of them. This is NOT 'nobody typed'.");
}
// else: reference also silent → nobody typed, stay quiet.
```

The reference hook's silence is now the evidence. If it fired in the last 60 seconds, keys reached the OS — the primary hook failure is REAL, not user silence. Only when both are silent (nobody typed AND we saw nothing) do we stay quiet. The elapsed window now uses the seeded clock from PROBLEM 187, so it reports actual elapsed time, not uptime.

**Generalise.** *An instrument whose trigger requires the system to be healthy cannot observe the system being sick. Before trusting a measurement's absence, check what has to be true for that measurement to be RECORDED. In a deaf-detection log, a line printed only when events arrive cannot report that nobody arrived.*

---

## PROBLEM 190 — frontend diagnostic log printed duplicate warn lines about overlay fit failures

**Symptom.** The log contained multiple identical lines: `overlay-log warn: overlay window size is not a number: (null).` At 1650ms and again at 1751ms, the warning fired twice in 101ms with the exact same arguments — but each fit operation runs once, and they were not about the same overlay function.

**Root cause.** `src/components/toast.ts`, in `buildHud()` around lines 933–955, contained a null-check branch that warned when the overlay's window did not report its fitted size, FOLLOWED by a `.catch()` that warned again when the IPC call itself was rejected (a different failure: the Rust command was not available or failed). Both branches warned with similar wording. When the overlay was first building, it called fit once and got both errors in sequence — the size query failed (warn 1), then the IPC fell back to a retry that also failed (warn 2).

The null warn was redundant: if the IPC succeeded, we have the size; if it failed, the `.catch()` already explains why. Printing both turned one failure into confusing duplicate output.

**Fix.** Lines 933–955 in `src/components/toast.ts`, in `buildHud()`: delete the null-branch warn call that prints "overlay window size is not a number". Keep the `.catch()` branch:

```typescript
.catch((err) => {
    log.warn(`IPC fit failure; falling back: ${err}`);
    // ... fallback logic
});
```

The Rust log already prints with more detail (`overlay_fit_hud` INFO level) when fit succeeds or fails, so the front-end's one job is to handle genuine IPC rejection — a Rust command missing or Rust failing. Deleting the null-branch warn removes the duplicate without losing information.

**Generalise.** *When a diagnostic is generated by two separate code paths both observing the same failure, keep the one closest to the root cause. The `.catch()` on the async call sees the failure at the boundary; a null-check inside the success path cannot — it only ever fires on a path that went wrong somewhere earlier.*


## PROBLEM 191 — hold-Space apps (Photoshop, Figma, Blender) lost their Space gestures; the exception list

**Symptom.** The owner reported: *"In the settings give an option of Exclude list or Exception list where people can add their apps they want to exclude. The app will automatically pause while in there — it won't work inside the apps of the exception list. When people press that, the similar option of choosing apps when pressing letters comes up, and they will be able to choose as many apps as exceptions as they want."* The specific use case: Photoshop, Figma and Blender use hold-Space mouse gestures to pan the canvas. Spaceadom suppresses every Space keydown system-wide, so those gestures are dead in those apps.

**Root cause.** The low-level keyboard hook callback (`kb_hook_proc`, `ms_hook_proc`) intercepts every Space-down and suppresses it globally, preventing the target application from ever receiving the keydown event. Verified 2026-08-25: a Space-down keystroke inside Photoshop produces ZERO keydown events at the application level while Spaceadom is running. There is no way for Photoshop to know Space was pressed, so the hold-Space pan gesture never starts. This is by design — the hook must suppress Space to use it as a modifier — but the design is too absolute: it has no exception path.

**Fix.** Implemented three-layer solution:

1. **Background poller thread** (`src-tauri/src/hook/exclusions.rs`, ~230 lines), modelled line-for-line on the existing `fullscreen.rs` watcher:
   - Polls the foreground window once every 500 ms.
   - Normalizes the window's executable stem (lowercase, with or without `.exe`, handles both `/` and `\` path separators).
   - Checks it against `EXCLUDED_LIST: Mutex<Vec<String>>`, locked only by the poller itself.
   - Sets `EXCLUDED_ACTIVE: AtomicBool` to the result.
   - Uses `Builder::new().name("st-exclusion-watcher")` and `catch_unwind` returning `String::new()` (→ NOT excluded) on panic, matching fullscreen.rs's fail-OPEN design. A broken poller must never disable the app.
   - Logs only on state change: `exclusions: photoshop is foreground — Spaceadom standing down` / `exclusions: left photoshop — Spaceadom resumed`.

2. **Hook-side gate** (`src-tauri/src/hook/mod.rs`), placed immediately after the `FULLSCREEN_ACTIVE` check and **before** the bypass branch:
   ```rust
   pub static EXCLUDED_ACTIVE: AtomicBool = AtomicBool::new(false);
   pub static SUPPRESS_EXCLUDED: AtomicU32 = AtomicU32::new(0);

   if EXCLUDED_ACTIVE.load(Ordering::Relaxed) {
       if is_down { SUPPRESS_EXCLUDED.fetch_add(1, Ordering::Relaxed); }
       return CallNextHookEx(None, n_code, w_param, l_param);
   }
   ```
   In both `kb_hook_proc` and `ms_hook_proc`, deliberately after `LAST_MS_EVENT` liveness stamp and **before** `MODIFIER_ACTIVE` early-return — `MODIFIER_ACTIVE` can still be true from a Space held just before the app switch, so without the gate's position, the first wheel event inside an excluded app would still be swallowed.

3. **Config and startup** (`src-tauri/src/config/schema.rs`, `src-tauri/src/config/mod.rs`, `src-tauri/src/lib.rs`):
   - Added `#[serde(default)] pub excluded_apps: Vec<String>` to the schema with `Vec::new()` default.
   - `publish_excluded_apps` called from both `config::save` and the startup load. Commented with PROBLEM 180 — an atomic that starts empty and is only fed on save means the feature is dead from launch until the first save.
   - `start_exclusion_watcher()` added as setup step 8b beside the fullscreen watcher.

4. **Frontend UI** (`src/components/settings-panel.ts`, `src/components/app-grid.ts`, `src/components/key-detail-panel.ts`):
   - New "App exceptions" section directly above Conflicts, using the same `.set-title .set-row-label .descBox` pattern as other settings.
   - Clicking the setting opens the app-selection grid (identical markup to key-detail-panel's editor grid), allowing users to select multiple excluded apps.
   - `drawAppGrid` carries the tile markup, icon `onerror` letter-disc fallback, and PROBLEM 97's `RENDER_CAP` truncation notice.
   - Diagnostic counter added: `drain_hook_diagnostics` drains `let ex = SUPPRESS_EXCLUDED.swap(0, …)`, includes `&& ex == 0` in the all-zero guard, and appends `excluded-app:{ex}` to the diagnostics line.

**Generalise.** *A system-wide input intercept that suppresses a key cannot safely be made absolute — there must be a surrender path. The check that decides "is this app excluded?" must live off the hot path (the hook callback) where it can do zero allocation, logging, or locking; the verdict is delivered as one atomic boolean updated from a background thread. The background thread's failure mode must be fail-OPEN: a broken probe returns "not excluded" so the app never gets silently disabled. Applying this pattern to other global-suppress features is straightforward — the existing fullscreen watcher demonstrates the architecture.*

---

## PROBLEM 192 — the App exceptions review pass: rows too wide, conflicts had no faces, the picker had no way out but one

**Symptom.** The owner reviewed 1.0.79's App exceptions feature (PROBLEM 191) and gave three notes: (1) *"Instead of the apps accepted... saying their full name and making it row by row — just show their icon, and maybe their name can be beneath the icon in very small font… the apps excepted can be side by side."* (2) *"In the conflicts, it would be good if you could show the app icon when something conflicts — show the app icon of the conflicting app as well."* (3) *"Instead of having to press on Done adding — if someone presses another place it shouldn't stay and wait for pressing Done adding. And after a few seconds it should automatically close."*

**Root cause.** All three are UI-only — nothing in the hook path, config schema, or backend was implicated. (1)/(2) were as-designed layout choices from the first pass that read as heavier than the app-grid picker they sit beside. (3) is the SAME class of bug as PROBLEM 178 (the new-profile name box): a transient UI surface with only one way to close, `_excPickerOpen` toggled solely by its own "Done adding" button — click anywhere else, or walk away, and it just sits there.

**Fix — all in `src/components/settings-panel.ts`, `src/components/app-grid.ts`, `src/styles.css`. No Rust touched; `cargo test --lib` (25/25) is the proof.**

1. **Exception tiles.** `renderAppExceptions()`'s per-app markup changed from one `.exc-row` (icon | full name | ✕, three-column grid, full row width) to a `.exc-grid` of `.exc-tile`s (flex-wrap, ~58px each, icon on top at 30px, name beneath at 10.5px with `text-overflow:ellipsis`, `title=` the full name):
   ```ts
   const tile = document.createElement("div");
   tile.className = "exc-tile";
   tile.title = label;                       // full name discoverable on hover
   const disc = document.createElement("span");
   disc.className = "exc-tile-disc";
   paintAppDisc(disc, hit?.icon, label, i);   // shared with app-grid.ts, not duplicated
   const name = document.createElement("span");
   name.className = "exc-tile-name";
   name.textContent = label;
   const remove = document.createElement("button");
   remove.className = "exc-tile-x";           // corner badge, not a full-width column
   remove.setAttribute("aria-label", `Remove ${label} from exceptions`);
   ```
   CSS: `.exc-tile-x { position:absolute; top:-6px; right:-6px; opacity:0; }` with
   `.exc-tile:hover .exc-tile-x, .exc-tile:focus-within .exc-tile-x, .exc-tile-x:focus-visible { opacity:1; }` —
   `:focus-within` is load-bearing: the badge is invisible at rest, and without
   it a keyboard user tabbing to the (still-focusable) button would land on a
   control they cannot see.

2. **Conflict-row icons.** `Conflict.process` (Rust, `hook/conflicts.rs`) is
   already a bare exe filename (`"autohotkey64.exe"`), never a path — so the
   cheap route the investigation flagged just works: `exeStem()` in
   `app-grid.ts` only splits on a path separator when one is present, so
   calling it on a bare filename returns the same stem it would from a full
   path. Added one small export:
   ```ts
   export function findAppByStem(stem: string): AppInfo | null {
     if (!_apps) return null;
     for (const a of _apps) if (exeStem(a.path) === stem) return a;
     return null;
   }
   ```
   and in `renderConflicts()`:
   ```ts
   const known = findAppByStem(exeStem(c.process));
   paintAppDisc(disc, known?.icon_base64, known?.name ?? c.product, i);
   ```
   No new Rust command — the existing Start-Menu scan (`list_start_menu_apps`,
   already warmed by the exceptions section rendered just above it) is reused.
   Degrades to the letter disc exactly like every other app icon in this app
   when the running process isn't a Start-Menu shortcut Spaceadom has scanned
   (a bare service exe, for instance). `renderConflicts()` also redraws once
   if the scan was still running on first paint:
   ```ts
   void loadApps().then(() => { if (panelEl && !panelEl.hidden) draw(); });
   ```
   CSS: `.conflict-row` gained a leading `auto` grid column for the 22px
   `.conflict-row-disc`; `.conflict-row-why`/`.conflict-row-cta` moved from
   `grid-column: 1 / -1` to `2 / -1` so the wrapped text still starts under
   the name, not under the icon.

3. **The picker closes itself.** Reused the SAME two mechanisms PROBLEM 178
   already established for exactly this trap, deliberately not inventing a
   third:
   - **`registerDismissable()`** (`src/dismissable.ts`) on the picker's wrap
     element — outside-press + Escape, for free.
   - **A 12s idle timer**, re-armed on pointer movement inside the picker,
     scroll, keystroke, and picking an app:
     ```ts
     const EXC_PICKER_IDLE_MS = 12_000;
     function armExcIdle(): void {
       window.clearTimeout(_excIdleTimer);
       _excIdleTimer = window.setTimeout(closeExcPicker, EXC_PICKER_IDLE_MS);
     }
     // wired to: search input, wrap pointermove, scroll container scroll,
     // and drawAppGrid's onPick (armExcIdle() before addException()).
     ```
     12s, not profile-editor's 15s or something shorter: scanning a grid of
     app icons to find the right one is slower than typing a name, and a
     picker that vanishes mid-scan is a worse bug than the one being fixed.
   - **"Done adding" stays** — now one way out among several, not the only
     one, so nobody who already relies on it is stranded.

   **The propagation gotcha, found and fixed while implementing (a) — worth
   recording as its own class of bug.** `main.ts` wires
   `panelEl.addEventListener("click", e => e.stopPropagation())` at bootstrap
   (PROBLEM 98 — required so the settings panel itself survives clicks inside
   it). `registerDismissable`'s "outside press" detection is a
   **document-level** click listener. A press that lands OUTSIDE the whole
   settings panel never passes through `panelEl` at all, so it reaches
   `document` fine. But a press that lands INSIDE the panel and outside the
   picker — another settings row, a slider, blank space in the box — bubbles
   up and dies at `panelEl`'s own stopPropagation listener; it never reaches
   `document`, so `registerDismissable` alone cannot see it and the picker
   would stay open. Fixed by adding a **second** listener, scoped to the panel
   itself rather than document:
   ```ts
   function wireExcPanelOutsideClick(): void {
     if (_excPanelClickWired || !panelEl) return;
     _excPanelClickWired = true;
     panelEl.addEventListener("click", (e) => {
       if (!_excPickerOpen || !_excPickerWrap) return;
       if (e.timeStamp <= _excArmedAt) return;                 // the click that opened it
       if (_excPickerWrap.contains(e.target as Node)) return;  // handled inside the picker
       closeExcPicker();
     });
   }
   ```
   This works because `e.stopPropagation()` only stops an event from reaching
   ANCESTOR elements — it does not stop other listeners registered on the
   SAME element from running. `main.ts`'s stopPropagation listener and this
   new one both live on `panelEl`, so both still fire for every click inside
   the panel; only the trip past `panelEl` to `document` is cut. Wired once
   (guarded by `_excPanelClickWired`), a no-op whenever the picker is closed.

**Verified.** `npx tsc --noEmit` exits 0. `npm run build` exits 0, 0
warnings. `cargo test --lib`: 25 passed, 0 failed (proves the Rust side is
genuinely untouched, not just states it). Confirmed the changes reached the
actual bundle, not just the source: `grep -o "exc-tile\|conflict-row-disc"
dist2/assets/toast-*.css` found both; `grep -o "12e3" dist2/assets/main-*.js`
found the idle constant (`12_000` minifies to `12e3`); `grep -o "from
exceptions" dist2/assets/main-*.js` found the new aria-label text. **Not yet
verified:** installer build, install to the real machine, hand-test of the
tiles, the conflict icons, and the picker's auto-close on the owner's actual
1707×1067 panel.

**Generalise.** *Any transient UI surface that opens on request needs multiple
ways to close, not just one. Outside-press, Escape key, and an idle timeout
(re-armed on user activity) form a complete close pattern. This pattern has
recurred three times in this project (PROBLEM 178 for the new-profile name box,
PROBLEM 192 for the picker, with the same mechanisms reused deliberately
rather than hand-rolled twice). The pattern belongs in a shared helper
(`dismissable.ts`, with idle logic nearby) so future transient surfaces get it
automatically. A secondary lesson worth keeping: when a dismissal mechanism
listens at `document` level but lives inside a container with a
stopPropagation boundary, add a second boundary-scoped listener for the inner
surface — a single global listener cannot see presses that die at a boundary
before reaching the global context.*

---
## PROBLEM 193 — the app picker let you bind a key to an uninstaller

**Symptom.** The owner nearly bound a key to "Uninstall PASCO Capstone" from the app grid (Start Menu \ Tools\), believing it was the application itself — Space+key would have re-run the uninstaller on every press had he not caught it.

**Root cause.** `check_app_path()` already existed and rejected obvious uninstallers, but it was only ever wired to the manual file-browse path (PROBLEM 96); every picker built from the Start-Menu scan — the key editor's app grid, the App Exceptions picker, and any grid added since — walked straight past it, because the Start Menu genuinely contains these shortcuts (installers often add one under a "Tools" subfolder right next to the real app). On top of that, the check itself was too narrow: `stem == "uninstall"` only ever matched a shortcut named EXACTLY "Uninstall.lnk". The name every real installer actually writes is "Uninstall &lt;App Name&gt;.lnk" — Windows' own naming convention — and it slipped straight through.

**Fix.** `src-tauri/src/commands.rs`: `check_app_path()` now tokenises the stem (`tokenize_stem`) and rejects it when the FIRST TOKEN is "uninstall", not just an exact-stem match — this catches "Uninstall PASCO Capstone" without also catching a legitimate app that merely contains the word "uninstall" somewhere in its name. `list_start_menu_apps()` now calls `check_app_path()` on BOTH the shortcut's display name and its resolved target path (a shortcut can be named innocuously while its target is `unins000.exe`, or vice versa) and `continue`s past any match, filtering it out of the scan itself. Because every picker in the app (key binding grid, App Exceptions grid) is built from this one scan's output, filtering here instead of per-picker means no uninstaller or installer can reach any grid, and no future grid can forget to re-add the check.

**Generalise.** *A safety filter that exists somewhere in the codebase is not the same as a safety filter that is WIRED to every path that needs it — grep for every caller before trusting a check is universal.*

---

## PROBLEM 194 — duplicate "Raise Windows' limit" button in Settings > Conflicts

**Symptom.** Settings > Conflicts showed two identical "Raise Windows' limit from 0.3 to 1 second" buttons stacked under one "Re-check now".

**Root cause.** `renderConflicts()`'s `draw()` clears `box` synchronously and then calls `drawHookHealth()`, which is ASYNC (`await invoke("get_hook_health")`). `draw()` is called from two places that race: once when the section first renders, and again from `loadApps().then(() => draw())` once the Start-Menu icon scan lands. `box.innerHTML = ""` only ever runs at the START of `draw()` — so if the first `drawHookHealth()` call is still awaiting its invoke when the second `draw()` clears and repopulates the box, both calls eventually append their own copy of the block once their own `invoke` resolves, and neither knows the other exists.

**Fix.** `src/components/settings-panel.ts`: `drawHookHealth()` now wraps its note, caveat and button in a single container `div` with class `hook-health-block`, and removes any prior `:scope > .hook-health-block` from the box before appending the fresh one. Marked on a wrapping container rather than on the existing `.sma-note` class, because `.sma-note` is shared and generic elsewhere in this panel — querying for it here would risk deleting notes this function never wrote. Whichever `draw()` call resolves last now wins cleanly, converging on exactly one block instead of stacking.

**Generalise.** *An async function that appends into shared DOM must be idempotent against being called twice, because "clear then rebuild" at the SYNCHRONOUS call site does not protect against a slower ASYNC completion landing later.*

---

## PROBLEM 195 — crash reports from other people's machines, without asking anybody to send a log file

**Symptom.** Not a bug — a gap. Spaceadom is on friends' laptops now, and when it crashes there the only record is `%APPDATA%\Spaceadom\debug.log` on *their* disk. Getting it requires asking a non-technical person to find a hidden folder and email a text file, which nobody does, so every crash outside this machine is invisible. PROBLEM 131 made the local crash record excellent (one panic hook, shipped symbols, last-action breadcrumbs) and it still only helps if somebody hands it over. The owner, 2026-08-26, rejected building a telemetry backend for this as work that buys nothing Sentry already gives away free, and asked for Sentry with a switch to turn it off. Second decision the same day, reversing a first one: crashes and errors only, from day one — not a wider "full debug" mode narrowed later.

**Root cause — n/a, this is a feature. The CONSTRAINT it had to respect, and how it was satisfied.**

The `sentry` crate's DEFAULT feature set includes `panic`, and `sentry::init()` with that feature calls `std::panic::set_hook` from inside the dependency. This project has a hard rule against exactly that (CLAUDE.md: *"There is exactly ONE `std::panic::set_hook` call, in `lib.rs`. There were two, and the second silently replaced the first for months (PROBLEM 131) — `set_hook` replaces, it does not chain unless you make it."*).

Two things make the dependency version worse than the original PROBLEM 131 duplication:

1. **It is invisible to the project's own tripwire.** `grep -c set_hook src-tauri/src/lib.rs` is how this repo checks the rule. A hook installed from inside `sentry-panic` does not appear in any grep of this repo's source, so the check would have passed while the rule was broken.
2. **Its failure mode leaves no trace, by definition.** The thing that would have broken is `crash_context.rs`'s breadcrumbs and the thread-name line — i.e. the crash reporting itself. A crash-reporting feature that silently disables the existing crash reporting is the worst possible shape of this bug, and it would have looked like it was working.

(Verified from source rather than assumed: `sentry-panic`'s `PanicIntegration::setup()` does `let next = panic::take_hook(); panic::set_hook(...)`. It *does* chain to `next`, so this would probably have survived — but "probably survives" is not the standard the rule was written to, and the fact that the answer required reading a dependency's source is itself the argument for not relying on it.)

**How it was satisfied:** the feature is off, and the forwarding is done by hand from inside the existing hook.

```toml
# src-tauri/Cargo.toml
# `default-features = false` IS LOAD-BEARING. DO NOT "TIDY" IT AWAY.
sentry     = { version = "0.49", default-features = false, features = ["backtrace", "contexts", "reqwest", "rustls"] }
sentry-log = "0.49"
```

Proof it worked, both halves:

```
$ grep -c set_hook src-tauri/src/lib.rs
5                                    # unchanged from before this feature
$ grep -c 'std::panic::set_hook(' src-tauri/src/lib.rs
1                                    # the one real call site
$ cargo tree | grep sentry
├── sentry v0.49.1
│   ├── sentry-backtrace v0.49.1
│   ├── sentry-contexts v0.49.1
│   ├── sentry-core v0.49.1
├── sentry-log v0.49.1
                                     # sentry-panic is NOT in the graph at all
```

The `cargo tree` line is the stronger of the two: a crate that is not compiled cannot install a hook, so this is a structural guarantee rather than a promise. (One incidental catch: a *comment* mentioning the function name moved `grep -c` from 5 to 6. The comment was reworded. A tripwire that a comment can trip is a tripwire that gets ignored.)

**Fix.**

*New file `src-tauri/src/telemetry.rs`* — the DSN placeholder, the level gate, the kill switch, and the manual panic capture.

```rust
// !!!  PASTE YOUR SENTRY DSN HERE  !!!   (empty => this whole module is inert)
pub const SENTRY_DSN: &str = "";

/// Crashes and errors only. A WARN, an INFO or a DEBUG line never leaves the
/// machine, which is why "ordinary use sends nothing" is a true statement.
pub const SENTRY_MINIMUM_LEVEL: log::Level = log::Level::Error;

/// FALSE AT PROCESS START, DELIBERATELY: the config is not loaded yet when the
/// logger is installed, so the honest state is "we do not know whether this
/// user consented" — and the only safe answer to that is to send nothing.
static SENDING_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn log_filter(metadata: &log::Metadata<'_>) -> sentry_log::LogFilter {
    if !SENDING_ENABLED.load(Ordering::Relaxed) {
        return sentry_log::LogFilter::Ignore;
    }
    // log::Level orders Error(1) < Warn(2) < Info(3): "at least as severe as"
    // is `<=`, not `>=`. Getting this backwards would send everything.
    if metadata.level() <= SENTRY_MINIMUM_LEVEL {
        sentry_log::LogFilter::Event
    } else {
        sentry_log::LogFilter::Ignore
    }
}
```

`init()` guards the empty DSN rather than passing it through — `sentry::init("")` is an *error*, not a disabled client:

```rust
pub fn init() -> Option<sentry::ClientInitGuard> {
    if SENTRY_DSN.is_empty() { return None; }
    // BUILT BY MUTATION, NOT BY STRUCT LITERAL: ClientOptions is
    // #[non_exhaustive] as of 0.49, so `ClientOptions { .. }` is E0639.
    let mut options = sentry::ClientOptions::default();
    options.release = sentry::release_name!();
    options.attach_stacktrace = true;
    options.send_default_pii = false;
    Some(sentry::init((SENTRY_DSN, options)))
}
```

and `capture_panic` builds the event `sentry-panic` would have built, by hand:

```rust
pub fn capture_panic(message: &str, thread: &str) {
    if !SENDING_ENABLED.load(Ordering::Relaxed) { return; }
    sentry::configure_scope(|scope| scope.set_tag("thread", thread));
    sentry::capture_event(sentry::protocol::Event {
        exception: vec![sentry::protocol::Exception {
            ty: "panic".into(),
            value: Some(message.to_owned()),
            stacktrace: sentry::integrations::backtrace::current_stacktrace(),
            ..Default::default()
        }].into(),
        level: sentry::Level::Fatal,
        ..Default::default()
    });
    // Without this the event is still in the transport queue when lib.rs calls
    // process::exit(1), and the crash we most wanted to see never arrives.
    if let Some(client) = sentry::Hub::current().client() {
        client.flush(Some(std::time::Duration::from_secs(2)));
    }
}
```

*`src-tauri/src/lib.rs`* — one line added inside the EXISTING hook closure, after the three `log::error!` lines (debug.log is the record that always works; nothing that talks to a network runs before it):

```rust
        telemetry::capture_panic(&msg, &thread);
```

plus the client guard in `run()`, bound to a name because `ClientInitGuard` closes the client on drop:

```rust
    // `let _ = telemetry::init();` would drop it at the end of the statement
    // and switch reporting off on the line that turned it on.
    let _sentry_guard = telemetry::init();
```

*`src-tauri/src/logger.rs`* — the Sentry bridge WRAPS log4rs instead of replacing it. This was one line before (`log4rs::init_config(config)`, which builds the logger AND installs it globally); only one thing can be the global `log::Log`, so bolting Sentry on afterwards is impossible:

```rust
// BEFORE
let _ = log4rs::init_config(config);

// AFTER
let logger = log4rs::Logger::new(config);       // built, NOT installed
let max_level = logger.max_log_level();
let bridged = sentry_log::SentryLogger::with_dest(logger)
    .filter(|metadata| crate::telemetry::log_filter(metadata));
let _ = log::set_boxed_logger(Box::new(bridged));
log::set_max_level(max_level);                  // init_config used to do this
```

debug.log is completely unaffected: same appender, same pattern, same rotation, same levels. Sentry only ever sees a copy, and only of what `log_filter` allows.

*`src-tauri/src/config/schema.rs`* — the field, with the trap named:

```rust
    /// **TRUE MEANS SENDING IS HAPPENING.** The Settings switch is its
    /// NEGATION ("Don't send logs"), so the switch reads `!send_logs`.
    ///
    /// `default = "default_true"`, NOT a bare `#[serde(default)]`: a bool's
    /// `Default` is `false`, so the bare attribute would silently invert the
    /// intended default for every config written before 1.0.82.
    #[serde(default = "default_true")]
    pub send_logs: bool,
```

*`src-tauri/src/config/mod.rs`* and *`lib.rs`* — published from BOTH ends, the same pattern `BOUND_SPECIALS` and the app-exceptions list use (PROBLEM 180). `save` alone is not enough: the atomic starts false, so reporting would stay off from launch until the user happened to save something — and a crash during startup, the crash worth having most, would never be reported.

```rust
// config/mod.rs, inside save()
crate::telemetry::publish(config);

// lib.rs, immediately after config::load_or_init()
telemetry::publish(&shared_config.read().unwrap_or_else(|p| p.into_inner()));
```

*`src-tauri/src/commands.rs`* — the switch's own command, so the flip is immediate and unconditional rather than a side effect of a save:

```rust
#[tauri::command]
pub fn set_send_logs(send_logs: bool, state: State<'_, ConfigState>, app: tauri::AppHandle)
    -> Result<(), String>
{
    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    cfg.send_logs = send_logs;
    let snapshot = cfg.clone();
    drop(cfg);
    // Flip the atomic FIRST. If the disk write fails, the user's stated wish is
    // still honoured for this session — the reverse order means "you asked me
    // to stop and I kept sending until you restarted".
    crate::telemetry::set_sending_enabled(send_logs);
    config::save(&snapshot)?;
    let _ = app.emit("config-updated", snapshot);
    Ok(())
}
```

*`src/components/settings-panel.ts`* — at the very bottom of the panel, below every existing row and button, behind its own divider. The negation is written out three separate times (schema, command, panel) because an inverted privacy switch is the one bug here a user could never detect: it would look correct and do the opposite.

```ts
  // `send_logs: true` means SENDING IS HAPPENING. So the switch is CHECKED
  // when send_logs is FALSE. `!== false`, not `=== true`: the key is absent
  // from every config written before 1.0.82, and absent means ON.
  const dontSendLogs = appConfig.send_logs === false;
  ...
  ${toggleRow("sendlogs", "Don't send logs", dontSendLogs, 10)}

  wireToggle("sendlogs", async () => {
    const nextDontSend = !(appConfig.send_logs === false);
    const nextSendLogs = !nextDontSend;
    await invoke("set_send_logs", { sendLogs: nextSendLogs });
    ...
  });
```

**Transport.** `reqwest` + `rustls`, not the crate's default `transport` (= reqwest + native-tls) and not the optional `curl`. Nothing on this build machine is installed system-wide — the whole toolchain lives at `D:\RUST-DOWNLOADED-HERE` — so rustls, which needs no OpenSSL, no libcurl and no system certificate stack to link against, is the only one that builds here without a prerequisite. Confirmed absent from `cargo tree`: `native-tls`, `openssl`, `curl`.

**How it was verified.**

- `cargo check --lib` — 0 errors, 0 warnings.
- `cargo test --lib` — 28 passed, 0 failed (25 before; three new: two in `telemetry`, one in `config::schema`). The `telemetry` test asserts the switch actually kills — that an ERROR is dropped while sending is off, that ERROR passes and WARN/INFO/DEBUG/TRACE do not while it is on, and that flipping changes the very next verdict.
- `grep -c set_hook src-tauri/src/lib.rs` = 5, unchanged; `grep -c 'std::panic::set_hook('` = 1.
- `cargo tree` shows no `sentry-panic`.
- Built, installed via explorer.exe (MSIX-container rule, PROBLEM 143) and confirmed running.

**What is NOT verified, and cannot be from here:** that an event actually arrives at sentry.io. `SENTRY_DSN` ships empty, so no client is created and nothing is sent — by design, so that today's build runs with telemetry inert. The path from `log::error!` to the wire is untested until a real DSN is pasted in. Labelled untested rather than implied working.

**Generalise.** *A crate that installs its own global hook (panic handler, logger, signal handler) must be checked against every OTHER thing in the codebase that already owns that hook, and wired to cooperate with it explicitly — never assume a library's "automatic" integration is compatible with an existing single-owner resource.* Two corollaries this one produced: the check for "is the rule still held" must survive the dependency being able to break it invisibly (`cargo tree` proves what `grep` cannot), and a default feature set is a set of decisions somebody else made about your codebase — read them before accepting them.

---

## PROBLEM 196 — the Sentry DSN was a literal in tracked source

**Symptom.** PROBLEM 195 shipped `telemetry.rs` with `pub const SENTRY_DSN: &str = "";` — a real string literal, sitting in a tracked `.rs` file, ready for the next step ("paste your DSN here") to turn it into a plaintext secret committed straight into git history. This repo is headed for a public GitHub repo (per `V13_TO_V14_METHOD.md` / release docs), so the moment somebody pasted a working DSN into that literal and committed, the DSN would have been published — permanently, since git history does not forget without a rewrite. A Sentry DSN can only submit events, never read the account, so the blast radius is bounded, but "bounded" is not "acceptable to publish on purpose."

**Root cause.** N/A — preventative. Nothing broke; the shape of PROBLEM 195's fix was simply wrong for what came next.

**Fix.** The DSN moved out of tracked source entirely.

```rust
// src-tauri/src/telemetry.rs
pub const SENTRY_DSN: &str = trim_dsn(include_str!("sentry_dsn.txt"));
```

- `src-tauri/src/sentry_dsn.txt` holds the real DSN. It is listed in `.gitignore` and confirmed absent from `git status --short` and matched by `git check-ignore -v`.
- `src-tauri/src/sentry_dsn.example.txt` is tracked and explains what to put in the real file — so a fresh clone knows the step exists instead of discovering it as a compile error with no context.
- A missing `sentry_dsn.txt` is a **compile error** (`include_str!` fails the build), not a silent fallback to an empty string. That is deliberate: a build that silently ships with telemetry inert because a file was missing would look identical to a build that works, and the whole point of PROBLEM 195 was to stop that kind of invisible gap.

**Generalise.** A value that is safe to SHIP inside a compiled binary is not automatically safe to keep in the SOURCE TREE's git history — those are two different audiences, and a build-time include from a gitignored file separates them without changing what the running app actually contains.

---

## PROBLEM 197 — every scroll, every time: up to 500 unvirtualised app tiles repainted whether they were on screen or not

**Symptom (owner).** Scroll lag "happens EVERY TIME he scrolls, repeatedly, in BOTH the app-picker grid AND the Settings panel itself" — not a one-off stutter, a reliable, repeatable slowdown on every single scroll gesture.

**Root cause.** `app-grid.ts`'s `renderAppGrid()` draws up to `RENDER_CAP` (500) `.ed-tile` elements into the DOM SIMULTANEOUSLY — one `<div>` per detected app, each holding a 34px disc and (for most apps) a base64 `data:` PNG `<img>` — with no virtualisation, no windowing, and no `content-visibility` anywhere in the stylesheet. Every one of those nodes, on-screen or not, sits in the live render tree, so every scroll frame makes the browser consider all 500 for layout/paint, not just the handful actually visible through the 212px-tall `#ed-grid-scroll`/`.ed-grid-scroll` viewport.

This grid is SHARED (by design — see `app-grid.ts`'s own header comment) between the key editor's picker and the Settings panel's "Add an app" exceptions picker. In the Settings panel, that picker is not behind its own isolated scroller in the way the key editor's is — it renders straight into `#set-app-exceptions`, itself straight inside `#settings-panel`'s own `overflow-y: auto` region — so the up-to-500-tile cost is also fully present the instant that picker is open and the OWNER scrolls the Settings panel itself, not just the picker's own inner scrollbar. That is why the owner reported lag in "the Settings panel itself" as a *second*, seemingly separate symptom: it is the SAME unvirtualised list, just experienced through the outer scroller instead of the inner one.

`.exc-tile` (the exceptions list itself, rendered by `settings-panel.ts`'s `renderAppExceptions()`) is the identical shape of problem on a smaller, but structurally uncapped, list — it has no `RENDER_CAP`, grows by exactly as many apps as the user has added as exceptions, and — unlike `.ed-tile` — sits directly in `#settings-panel`'s own scroll flow with no isolating child scroller at all.

Investigated and RULED OUT as a contributor: `#settings-panel`'s own eleven `.set-row` toggle rows and the `.conflict-row` list (bounded to however many known conflicts are actually running — typically 0–3). Neither is a large or unbounded repeat, and neither carries an `<img>`. The one scroll listener in the whole app (`settings-panel.ts:708`, the app-exceptions picker's idle-timer reset) is cheap and unrelated, per the prior investigation this task built on.

**Fix.** `content-visibility: auto` on the two large-repeat tile classes, paired with `contain-intrinsic-block-size` (not the full `contain-intrinsic-size`, and not on `.ed-tile`'s inline/width axis — see reasoning below) so a skipped tile's collapse to zero height cannot jump the scrollbar or the scroll position before it has painted once.

*`src/styles.css`* — `.ed-tile`:

```css
.ed-tile {
  ...
  animation: st-pop-in 380ms var(--ease-spring) backwards;
  content-visibility: auto;
  contain-intrinsic-block-size: auto 80px;
}
```

*`src/styles.css`* — `.exc-tile`:

```css
.exc-tile {
  ...
  background: var(--st-card);
  content-visibility: auto;
  contain-intrinsic-block-size: auto 64px;
}
```

**The reasoning behind the two numbers, and behind touching only `contain-intrinsic-BLOCK-size`.** `content-visibility: auto` turns on size containment for a skipped (off-screen, not-yet-rendered) element, which means its size stops being derived from its content unless `contain-intrinsic-size` says otherwise — an unset axis collapses to 0. `.ed-tile`'s WIDTH comes from its grid column (`#ed-grid, .ed-grid { grid-template-columns: repeat(4, 1fr) }`), which is an explicit track size the grid algorithm computes independently of any one cell's content — size containment only ever substitutes for a dimension that would otherwise be intrinsic (content-derived), so the column width cannot collapse regardless, and constraining it would be pure guesswork for zero benefit. `.exc-tile`'s width is simply `width: 58px`, an explicit value, same reasoning. Only the HEIGHT of both is content-derived (`height: auto`, i.e. grow to fit the disc + name), so only it needs a stand-in:
  - `.ed-tile` ≈ 1.5px border + 10px padding-top + 34px disc + 6px gap + one line of 11px text at the global 1.5 line-height (16.5px) + 10px padding-bottom + 1.5px border ≈ **79.5px → 80px**.
  - `.exc-tile` ≈ 1px border + 7px padding-top + 30px disc + 4px gap + one line of 10.5px text at 1.5 line-height (15.75px) + 5px padding-bottom + 1px border ≈ **63.75px → 64px**.

Both use the `auto <estimate>` form, not a bare estimate — the standard `content-visibility: auto` + `contain-intrinsic-size: auto …` pairing, where `auto` means "once this element has actually rendered once, remember and use its REAL measured size instead of the estimate below." A slightly-off manual measurement therefore self-corrects after first paint rather than staying wrong for the life of the session.

**What was deliberately NOT done.** `contain: layout style paint` on the scroll containers themselves (`#ed-grid-scroll`/`.ed-grid-scroll`, `#settings-panel`) was considered, per the brief's suggestion that it could further isolate repaint cost. Not applied: `contain: layout` makes an element the containing block for any `position: fixed`/`absolute` descendant, and this codebase is popover-heavy enough (`.conflict-prompt`, `.confirm-back`, the exceptions picker's own dismissable wrapper) that verifying no such descendant anywhere under either container silently relies on document- or viewport-relative positioning was not something this pass could fully rule out by reading alone. `content-visibility: auto` on the tiles already captures the dominant cost (hundreds of image-bearing nodes) without that risk, so the container-level `contain` was left out rather than guessed at. `.set-row` and `.conflict-row` were deliberately left untouched — both are small, bounded lists with no `<img>` per row, exactly the case the brief warns `content-visibility` has no benefit for.

**How it was verified.** Reasoned through the CSS cascade by hand (grid-track vs. content-derived sizing, as above) rather than measured, because this is a native Tauri/WebView2 app with no way to drive real scroll input or read paint timing from this shell. `npx tsc --noEmit`: 0 errors (this is a pure-CSS change; nothing here touches typed code). `npm run build`: 0 errors. `cargo test --lib`: 28 passed, 0 failed, 0 warnings (this change touches no Rust). **NOT verified, and cannot be from here: that scrolling actually feels smoother.** This needs an actual hand-test — hold the app-picker grid open with a large app list (or the App-exceptions picker) and scroll it, then scroll the Settings panel itself with that picker open, on the real machine.

**Generalise.** For any large or structurally-unbounded list of repeated elements that carry real paint cost per item (an image, in this codebase's case), `content-visibility: auto` is the standard, low-risk fix — but it is not complete on its own the moment any axis of the item's size is content-derived rather than explicit: pair it with `contain-intrinsic-size` (or the narrower logical longhand for just the axis that needs it) sized from the item's own CSS math, and prefer the `auto <estimate>` form over a bare estimate so a rough hand-calculation self-corrects after the item's first real paint instead of staying a permanent approximation. Scope it to the actual repeat-heavy elements only — applying it to a small, fixed-size list (this codebase's `.set-row`, eleven toggles) adds bookkeeping for no benefit.

---

## PROBLEM 198 — spacedesk showed a letter disc in Conflicts, a real icon everywhere else

**Symptom (owner).** spacedesk appears in Settings → Conflicts with a plain letter-in-a-circle instead of its actual icon, while the exact same program shows a proper icon when searched for in the app-picker grid (key editor or App exceptions).

**Investigation — hypothesis confirmed, not assumed.** Read `hook/conflicts.rs`: `detect()` matches any running process whose name starts with `"spacedesk"` (a deliberate PREFIX match — see its own comment: "spacedesk ships several executables; match by prefix for that one entry, exact otherwise") and records the exact `process` name from the live Toolhelp snapshot, e.g. `spacedeskservice.exe`. `findAppByStem()` (`app-grid.ts`) only ever searches `cachedApps()`, which `list_start_menu_apps` (`commands.rs`) builds ENTIRELY from a `.lnk` scan of the two Start-Menu folders — so a stem match can exist only for an exe that has a Start-Menu shortcut.

Confirmed directly on the dev machine (spacedesk is installed here, though not running during this session) rather than assumed from the vendor's general reputation:

```
Start Menu:  C:\ProgramData\...\Start Menu\Programs\spacedesk DRIVER Console.lnk
               → target: C:\Program Files\datronicsoft\spacedesk\spacedeskConsole.exe
Program Files\datronicsoft\spacedesk\  contains:
   spacedeskConsole.exe        (HAS the one and only spacedesk .lnk — the GUI console)
   spacedeskService.exe        (NO shortcut anywhere in either Start Menu tree — the background service)
   spacedeskServiceTray.exe    (NO shortcut anywhere in either Start Menu tree — the tray helper)
```

The hypothesis is confirmed exactly: spacedesk ships one Start-Menu-visible GUI (`spacedeskConsole.exe`, under a DIFFERENT stem) and at least two background exes with no shortcut at all. Whichever of the shortcut-less two is what is actually running and flagged as the conflict (the service is the one whose job matches the conflict's own `detail` text — "forwards input to a second display and can intercept keys" — so it, not the Console GUI, is what stays resident), `findAppByStem` is structurally unable to match it, no matter how the Start-Menu scan is tuned. This is not a stem-normalisation bug or a caching timing issue; the two sides of the lookup are searching disjoint sets by construction.

**Fix.** A second, orthogonal resolution path: extract the icon straight from the running process's own exe file on disk, using the ALREADY-EXISTING, already-registered `extract_icon_cmd` (`commands.rs`, registered in `lib.rs`'s `invoke_handler!`) — confirmed via `grep` to already be called from `key-detail-panel.ts` and `keyboard-matrix.ts` for manually-typed/dropped paths, so this reuses its icon cache (`IconCacheState`) rather than adding a second one.

That command needs a real exe path, and the frontend never had one for a `Conflict` — only the bare process name. So `hook::conflicts::Conflict` gained a `path` field, resolved LIVE while `detect()` already has the process's PID from the same Toolhelp snapshot entry, using the exact `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` + `QueryFullProcessImageNameW` pattern already used by `hook::exclusions::foreground_stem` and the PID resolvers in `engine::actions::smart_cascade` and `hook::fullscreen` — copied, not re-derived, per this codebase's own rule against two independent resolvers for the same thing.

*`src-tauri/src/hook/conflicts.rs`*:

```rust
pub struct Conflict {
    pub process: String,
    pub product: String,
    pub detail: String,
    /// Full path to the running exe, resolved live (empty if that failed —
    /// a protected process, or it exited between the snapshot and the query).
    pub path: String,
}
...
if hit && !found.iter().any(|c| c.product == *product) {
    found.push(Conflict {
        process: name.clone(),
        product: (*product).to_string(),
        detail: (*detail).to_string(),
        path: full_path_for_pid(entry.th32ProcessID),
    });
}
...
#[cfg(windows)]
fn full_path_for_pid(pid: u32) -> String {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 260];
        let mut size = buf.len() as u32;
        let pwstr = PWSTR(buf.as_mut_ptr());
        let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_FORMAT(0), pwstr, &mut size);
        let _ = CloseHandle(handle);
        if ok.is_err() { return String::new(); }
        String::from_utf16_lossy(&buf[..size as usize])
    }
}
```

`get_conflicts` (`commands.rs`) returns `Vec<Conflict>` directly with no intermediate shape, so the new field reaches the frontend with no further wiring.

*`src/main.ts`* — the mirrored TS type gained the same field:

```ts
export interface Conflict {
  process: string;
  product: string;
  detail: string;
  path: string;
}
```

*`src/components/settings-panel.ts`*'s `renderConflicts()` — two-tier lookup, letter disc as the synchronous starting state either way:

```ts
const known = findAppByStem(exeStem(c.process));
if (known) {
  paintAppDisc(disc, known.icon_base64, known.name, i);
} else {
  paintAppDisc(disc, null, c.product, i);   // letter disc now, synchronously
  if (c.path) {
    invoke<string | null>("extract_icon_cmd", { exePath: c.path })
      .then((icon) => { if (icon) paintAppDisc(disc, icon, c.product, i); })
      .catch(() => { /* letter disc already painted — nothing to do */ });
  }
}
```

This degrades exactly like every other icon in this app: the row never waits on the async call to render, `paintAppDisc`'s own `img.onerror` still covers a corrupt icon payload, an empty `c.path` (process already exited, or protected) skips straight to keeping the letter disc, and a rejected/failed `extract_icon_cmd` call is swallowed rather than surfaced.

**How it was verified.** Confirmed the hypothesis directly against this machine's real spacedesk install (Start-Menu scan + `Program Files` listing, above) rather than trusting the vendor's known architecture from memory. `npx tsc --noEmit`: 0 errors. `npm run build`: 0 errors; `grep -c extract_icon_cmd dist2/assets/main-*.js` rose from 2 (the two pre-existing call sites) to 3, confirming the new call actually reached the built bundle. `cargo test --lib`: 28 passed, 0 failed, 0 warnings — unchanged count, because `full_path_for_pid` is a live Win32 call keyed to a currently-running, externally-owned process and is not the kind of pure, always-reachable logic this codebase's testing law asks for a unit test for (same category as `foreground_stem`, which also has none). **NOT verified, and stated plainly: the icon was never observed actually rendering for a live spacedesk conflict.** spacedesk was not running anywhere during this session — `tasklist` found no `spacedesk*` process, and the installed app's own `debug.log` logged "conflicts: no known keyboard-remapping software running" throughout — so only the code path, the Rust struct wiring, and the Start-Menu evidence above were confirmed; the actual paint was not. This needs an actual hand-test with spacedesk running.

**Generalise.** When an icon/identity lookup only ever consults ONE index built from ONE enumeration method (here, a Start-Menu `.lnk` scan), any legitimate target that predates or bypasses that method — a Windows service, a headless companion exe, anything installed without a shortcut — is a STRUCTURAL non-match, not a bug in the matching logic itself, and no amount of tuning the stem comparison can fix it. The fix is a second, orthogonal resolution path keyed to something the first method cannot see (here: the process is currently running, so its real exe path is directly queryable), not a smarter version of the same index.

---

**Numbering note (2026-08-26).** The unrestricted-profile-names work shipping in 1.0.85 is referenced in its own code comments as "PROBLEM 197" (`commands.rs`'s `regex_lite`, `profile-editor.ts`'s `PROFILE_NAME_RE`, doc comments in `types.ts` and `config/schema.rs`) — but this file had already assigned 197 to the unvirtualised-tile scroll problem above. The code comments are left as they are (they document that fix correctly; only the number collides, and an append-only record never renumbers). This note exists so nobody hunts this file for a second 197: the profile-names record lives in those doc comments and in `all-versions/WHAT-CHANGED.md`'s 1.0.85 row.

---

## PROBLEM 199 — pasting a URL and clicking "Done" silently discarded it

**Symptom (owner).** *"pressing done instead of assigning after pasting a URL doesn't assign the website to the key, fix it."* Paste a URL into the key editor's paste row, press the big primary **Done** button, and the panel closes having saved nothing.

**Root cause.** `assignFromPath` only ever fired from Enter inside the paste field or from the separate small **Assign** button — `#ed-done` did nothing but close. So the flow silently required two deliberate actions (assign, then close) when everything about the panel's layout — one text field, one big primary button right beside it — reads as one. A pasted value sitting in the field when Done is pressed is unambiguous intent; there is no reading of "I typed a URL and clicked the button that closes this panel" other than "save it, then close".

**Fix.** `src/components/key-detail-panel.ts`, inside `renderPanel` — wired at the one place where `path` and `assignFromPath` are both in scope, so exactly ONE listener exists on `#ed-done` (its old attachment point predated both and would have stacked a second):

```ts
_panel.querySelector("#ed-done")!.addEventListener("click", () => {
  void (async () => {
    const pending = path.value.trim();
    if (pending) await assignFromPath(pending);
    closePanel();
  })();
});
```

The `await` is load-bearing, not style. `assignFromPath`'s FILE-PATH branch `await`s `check_app_path` (the PROBLEM 96 installer guard) before it ever calls `commit()`, and `closePanel()` nulls the module-level `_currentKey` SYNCHRONOUSLY. Fire-and-forget plus an immediate close would null `_currentKey` while that check was still in flight, so `commit()` would land on a null key and silently no-op — reproducing the exact bug this fixes, just for pasted file paths instead of URLs. (Only the URL branch happens to have no `await` before its own `commit()`, which is why the bug as REPORTED showed up on URLs.)

**How it was verified.** `npx tsc --noEmit` 0 errors; `npm run build` 0 errors; ships in 1.0.85 and the built bundle carries the change (the 1.0.85 frontend marker checks cover this file). **NOT hand-tested end-to-end:** nobody has yet pasted a URL and pressed Done in the installed build.

**Generalise.** When a panel has both an explicit commit control and a primary close control, any value sitting in an input when the close control is pressed must be committed by it — otherwise the UI quietly demands a second action that its own layout promises is one. And when the commit path contains an `await`, the close path must await it too: a synchronous teardown that nulls shared state turns the race into a silent no-op, which is strictly worse than an error.

---

## PROBLEM 200 — "open this key in a SPECIFIC browser profile": the feature, and the two assumptions the real machine corrected

**What this is.** Chromium browsers accept `--profile-directory="Profile 1"` and open straight into that profile. 1.0.85 lets a URL binding (or a binding to a detected browser's exe) be pinned to one: a "Browser profile" row in the key editor hosts a chip ("Opens normally" until a deliberate pick), the chip opens a picker listing every Chromium browser detected on the PC with its profiles, and the Guide HUD shows "Brave — Studies" for a pinned key. New module `src-tauri/src/browser_profiles.rs` (scan + resolution + every launch-route decision), new leaf module `src/components/browser-profile-picker.ts` (chip + picker), three new optional `KeyBinding` fields (`browser_exe`, `browser_profile_dir`, `browser_profile_name` — `config/schema.rs` + `types.ts`), dispatch in `engine/actions/smart_cascade.rs` (`open_binding_url`, `app_launch_params`), HUD label via `browser_profiles::hud_label` from `engine/mod.rs`.

**The hard requirement, and where it lives.** The owner, twice: *"MAKE SURE THE DEFAULT BROWSER LAUNCHES FROM URL IF NOT EXPLICITLY SET TO SPECIFIC."* One guard (`should_use_specific_browser`) asked in one place (`route_for`, consulted by `open_binding_url` BEFORE anything else), `Default` arm calling `run_browser(url, app_handle)` verbatim. Two tests in `browser_profiles.rs` read the `smart_cascade.rs` SOURCE (`include_str!`) to pin the call-site structure itself: the route decision must precede the first `run_browser` call, and `smart_cascade` must have no unguarded route to a browser. A guard nobody calls passes every value-level test and still ships the regression; only a source-reading test can see the call site.

**Empirical correction (i): `<product>\Application\<exe>` is the WRONG universal assumption.** The brief that commissioned the feature assumed the exe sits at `<product folder>\Application\*.exe`. Measured on this machine (2026-08-26), that holds for approximately nobody:

- **Chrome/Edge**: user data under `%LOCALAPPDATA%\<vendor>\<product>\User Data`, exe in `Program Files` — the LOCALAPPDATA product folder has NO `Application` dir at all.
- **Brave**: `%LOCALAPPDATA%\BraveSoftware\Brave-Browser\Application` EXISTS but contains only a stale versioned folder (`147.1.89.132`) and no exe.
- **Samsung**: exe in `Program Files\Samsung\Internet\Application`, with a `samsunginternet_proxy.exe` sibling that must not win (hence the helper-exe blacklist).
- **Arc**: MSIX-packaged; data at depth SIX under `Packages\…\LocalCache\Local\Arc\User Data`, and the only launchable entry point is the app-execution alias `%LOCALAPPDATA%\Microsoft\WindowsApps\Arc.exe` — a 0-byte reparse point on which `Path::exists()` can FAIL with os error 1920 because `fs::metadata` follows the `IO_REPARSE_TAG_APPEXECLINK` tag. `path_exists()` uses `symlink_metadata` (which does not follow) for exactly this reason.

So resolution is THREE ordered sources — `Application\*.exe` direct children only (versioned subfolders are the PROBLEM 116 trap) → `Clients\StartMenuInternet` registry (married to the data dir by `product_paths_match`, since nothing inside `Local State` records where its browser is installed) → exact-name MSIX alias — and a candidate matching none is DROPPED, never guessed at: a browser missing from the picker is a far smaller failure than a picker entry that launches the wrong program.

**Empirical correction (ii): `profile.info_cache` is NOT a self-validating browser signature.** The brief expected the `Local State` + `profile.info_cache` shape to identify a browser on its own. Measured: **66 files named `Local State` exist under this machine's AppData, and about FORTY of them are WebView2 data folders (`…\EBWebView\Local State`) — including Spaceadom's own — every single one carrying a `profile.info_cache` with a "Profile 1" in it.** Spotify's CEF data dir has the shape too (`%LOCALAPPDATA%\Spotify\Local State`, info_cache listing `Browser` and `Default`, both folders present). Requiring a RESOLVABLE EXECUTABLE is what actually separates a browser from an embedded webview, and it is why resolution failure means "skip", not "fall back to something".

**Fallback polarities, all owner-approved and all tested:** a pinned browser that was uninstalled falls back to the default browser as a NAMED route variant (`PinnedBrowserMissing`) so the log says what happened; a pinned profile whose folder was deleted inside the browser launches WITHOUT `--profile-directory` and warns; a `User Data` dir that cannot be located at all KEEPS the profile argument — "could not verify" must never be treated as "verified absent" (Arc's alias has no derivable data path and launches fine).

**Deliberately deferred, owner's instruction (2026-08-26):** the focus/minimize leg matches windows by title/process only and cannot tell which PROFILE's window it grabs when two profiles of one browser are open; launching into the correct profile always works. Marked as a KNOWN GAP comment in `smart_cascade.rs`; fix after the feature has been used for a while, not before.

**How it was verified.** `cargo test --lib` green (the suite grew from 13 to 57 during the feature work; the entire decision layer is pure functions taking an injected `exists: &dyn Fn(&Path) -> bool`), 0 warnings; the live diagnostic scan (`cargo test --lib -- --ignored --nocapture live_browser_scan`) finds exactly the 5 real browsers on this machine (Arc, Brave, Chrome, Edge, Samsung) with correct exes and profiles. **NOT verified, stated plainly: nobody has yet SEEN the chip or the picker render (the preview harness's editor is a static mockup, and no dashboard session has opened the real editor since the feature landed), and no end-to-end profile launch has been performed — the exe-path→`--profile-directory=` command lines are pinned by unit tests, not by an observed browser window opening in the right profile.**

**Generalise.** A JSON/file signature that looks unique to a technology is usually shared by everything embedding that technology — validate on a property only the real thing has (here: being launchable), not on the shape of its data.

---

## PROBLEM 201 — Opera-style layouts, and "log clearly when a browser is skipped"

**Symptom.** A friend: Opera doesn't show up in the browser-profile picker. And from the log, that report was UNDIAGNOSABLE: a real browser skipped because no exe could be resolved was byte-for-byte indistinguishable from the ~40 WebView2 data folders skipped for the same reason — every skip was silent. Owner's decision: *"Add it, but log clearly when a browser is skipped."*

**Investigation — "does the registry source already cover Opera?" No, three separate ways.** The obvious hope was that `Clients\StartMenuInternet` (source 2) already resolves Opera, since Opera does register there. It does register — and the resolver still could not have detected it, for three independent reasons found by reading the code against Opera's documented layout (Opera 114+; `%APPDATA%\Opera Software\Opera Stable` holding `Local State` and the `Default` profile folder DIRECTLY — no `User Data` level — with the exe at `%LOCALAPPDATA%\Programs\Opera\opera.exe` or `%PROGRAMFILES%\Opera\opera.exe`):

1. `registered_browsers()` DROPPED every entry whose exe was not inside a folder named `Application` — the filter that excluded Internet Explorer. Opera's exe sits directly in `…\Programs\Opera\`, so the one browser actually reported missing was being filtered out by the guard against a browser nobody has used in years.
2. The product folder was derived as `user_data.parent()` — correct only for the `<product>\User Data` nesting. For Opera it yields the VENDOR folder (`Opera Software`), which no resolution source could ever match. Opera was undetectable by construction.
3. `meaningful_parts` kept `users\<name>`, so an unnested per-user install (`…\AppData\Local\Programs\Opera` → `["users","<name>","opera"]`) had ≥2 components, forced the two-component vendor comparison, and compared the USERNAME against a real vendor — every unnested per-user install failed the marriage. (And separately, Opera's data folder carries a channel suffix its install folder lacks: "Opera Stable"/"Opera GX Stable" vs "Opera"/"Opera GX".)

**Fix — extend the existing sources; no fourth source.** All in `src-tauri/src/browser_profiles.rs`:

- `product_dir_for(user_data) -> Option<(PathBuf, DataLayout)>`: parent when the dir is literally named `User Data` (`NestedUserData`), the dir ITSELF otherwise (`SelfContained` — the Opera shape, and also the shape of most embedded CEF data dirs).
- `registered_browsers()` keeps non-`Application` entries, product = the exe's own directory. IE (and Firefox, and anything non-Chromium that registers) is still excluded by the mechanism that was doing the real work all along: a registered browser only reaches the picker by MARRYING a Chromium-shaped data dir via `product_paths_match`, and non-Chromium browsers have none to marry. Display names now come from the key's DEFAULT VALUE with the key name as fallback — measured on this machine, the key name is not always human ("IEXPLORE.EXE" → "Internet Explorer", "Comet.GNZHVZYZL3BMQQFOFFCFNEZIYI" → "Comet"), while all four browsers that resolve through this source today have default value == key name, so nothing currently shown changes.
- `meaningful_parts` drops a leading `users\<name>` pair before the generic filter — the username is container identity, not product identity.
- `strip_channel_suffix` / `leaves_match`: the leaf comparison tolerates a trailing `" stable"` on either side, and nothing else ("Opera GX Stable" ↔ "Opera GX"; Chrome's own channels decorate both sides identically and never need it; "unstable" is not touched).
- **The MSIX-alias source is deliberately NOT consulted for `SelfContained` dirs.** For those the product folder IS the data dir, so any embedded-CEF app whose data folder shares a name with an unrelated alias would marry it — Spotify's `%LOCALAPPDATA%\Spotify` passes the shape check (measured), and on a machine with the Store Spotify's `Spotify.exe` alias it would have walked straight into the picker.
- **Vivaldi: ZERO code, on purpose.** Its documented layout is the standard nesting (`%LOCALAPPDATA%\Vivaldi\User Data` + `%LOCALAPPDATA%\Vivaldi\Application\vivaldi.exe`): source 1 resolves the per-user install, and a per-machine `C:\Program Files\Vivaldi` marries `%LOCALAPPDATA%\Vivaldi` through the one-component leaf fallback that has been in `product_paths_match` since it was written — its doc comment used exactly that path as the worked example. A test now pins that claim.

**Fix — the skip log.** `profiles_from_local_state` returns `Result<Vec<BrowserProfile>, &'static str>` so every shape failure carries its reason ("no profile.info_cache", "info_cache is empty", "none of the listed profile folders exist", …). In `scan_browsers`, every dropped candidate now logs its `Local State` path plus why:

- Shape failures → `debug!` always. They are the ordinary outcome (Electron apps, half-empty CEF dirs), and the shipped log filters debug out, so ~40 lines of WebView2 noise cannot spam it.
- Shape passed but no exe resolved → `info!` when the candidate **looked like a plausible browser**, `debug!` otherwise. Plausible is conservative: at least one live profile in its info_cache AND no path component naming a known embedded-webview dir (`EBWebView`, `WebView2`, `CefCache`, `htmlcache` — belt-and-braces, since `SKIP_DIRS` already prunes those from the walk; the guard survives a future SKIP_DIRS edit). The info line ends "If a real browser is missing from the profile picker, this line is why." — the one findable line the friend's report needed. On this machine, Spotify is the live example that will produce it.
- An exe RESOLVED whose path does not exist → `info!` always (a stale registration is rarer and stranger than an embedded webview).

**REASONED, NOT MEASURED — labelled in the code and here.** Neither Opera, Opera GX nor Vivaldi is installed on this machine. Their layouts above come from current public documentation (Opera 114+ help/forums; Vivaldi's docs), fetched and cross-checked this session rather than trusted from training data — Opera's layout has changed across versions. The new tests pin the DECISIONS made from that documentation; they cannot prove the documentation right, and no Opera install was ever actually detected by this code. The launch-side `user_data_dir_for` was deliberately NOT extended for Opera: it cannot locate an Opera data dir from the exe path, so `profile_arg_for` returns `Use` (the safe polarity — "could not verify" keeps the user's choice), meaning stale-profile DETECTION does not work for Opera-shaped browsers; launching still does.

**How it was verified.** `cargo test --lib`: **64 passed** (57 before this work; +7 covering the layout split, the Opera marriages both per-user and per-machine, the Opera↔GX cross-negatives, the per-machine Vivaldi marriage, the username-not-a-vendor rule with the Samsung guard retained, the plausibility gate, and the narrowness of the suffix rule), 0 warnings. The live scan re-run after the change is **byte-identical** to the baseline captured before it — same 5 browsers, same names, same exes, same profiles — so none of the relaxations changed anything for the browsers that already worked, and Spotify did not appear. The `StartMenuInternet` key names/default values/command shapes were enumerated from this machine's real registry. **NOT verified: no Opera-family or Vivaldi install existed here to detect end-to-end.** The skip log itself, though, IS verified in the installed 1.0.85: the dashboard's `warmBrowsers` ran the scan at 13:25:00 and `%APPDATA%\Spaceadom\debug.log` shows exactly two info-level skip lines (Spotify's `%LOCALAPPDATA%\Spotify\Local State`, "2 usable profile(s), but no launcher exe could be resolved… If a real browser is missing from the profile picker, this line is why", plus a `ReadyFor\data\VaultPlugin` CEF dir) followed by "found 5 Chromium browser(s) in 564ms" — the ~40 WebView2 folders produced nothing at info level, exactly as designed.

**Generalise.** Two things. (1) A filter added to exclude one bad case (IE) can silently exclude a good case met years later (Opera); before retiring such a filter, name the mechanism that REALLY does the excluding — here, the data-dir marriage — and prove it still holds without the filter. (2) When a pipeline drops candidates at several gates, every drop needs a log line carrying the path and the reason, at a level matched to how ordinary that reason is — a silent skip makes a missing real thing indistinguishable from routine noise, which turns a one-line diagnosis into an undiagnosable field report.

---

## PROBLEM 202 — rebinding a key silently kept the OLD binding's browser-profile pin

**Symptom.** Found during the PROBLEM 200 implementation, not reported from the field: drop a new URL onto a key that was pinned to "Brave — Studies" and the NEW site keeps opening in that profile; clear the key entirely and the pin survives in `config.json`, waiting to be resurrected by the next partial update.

**Root cause.** `keyboard-matrix.ts`'s `updateBinding` MERGES — `profile.bindings[key] = { ...(old ?? {}), ...partial }` — so any field a caller omits keeps its old value. Its three callers (`assignAppBinding`, `assignUrlBinding`, `clearBinding`) replace what the key POINTS AT and understandably never mentioned the three new browser-profile fields, so the merge carried them over — but those fields DESCRIBE the target being replaced, so carrying them across a re-bind is semantically wrong, always. The trap is asymmetric: `key-detail-panel.ts`'s `commit` path REPLACES the stored binding outright (`profile.bindings[key] = binding`), so ITS callers were safe leaving the fields off — two update paths with opposite semantics for an omitted field, and the bug only on the merging one.

**Fix.** `src/components/keyboard-matrix.ts` — one named constant, spread into every replacing/clearing call site, plus doc comments on BOTH update functions naming the trap:

```ts
const BINDING_RESET = {
  browser_exe: null,
  browser_profile_dir: null,
  browser_profile_name: null,
} as const;
// … in assignAppBinding / assignUrlBinding / clearBinding:
updateBinding(key, { app: exePath, web_url: null, label, icon_override: iconB64 ?? undefined, ...BINDING_RESET });
```

One named constant rather than three literals per call site, for the same reason `is_known_process` is shared between the two conflict modules: a rule that must be remembered at each of several sites is a rule that gets missed at the site added next.

**How it was verified.** `npx tsc --noEmit` 0 errors; the constant is spread at all three call sites (grep). **NOT hand-tested in the installed build:** the pin-then-rebind sequence has not been performed end-to-end, because nobody has performed ANY pin end-to-end yet (see PROBLEM 200's unverified list).

**Generalise.** A merge-semantics update function turns every omitted field into "keep the old value" — right for edits, wrong for replacements — and when both kinds of caller exist, either split the function or make the reset an explicit named constant no call site can forget. Wider class: when the same record can be written through TWO paths with different update semantics (merge here, replace in `commit`), any rule discovered on one path must be re-checked against the other, because each path's callers have learned opposite habits about what omitting a field means.

---

## PROBLEM 203 — a PiP'd window taken to TRUE fullscreen kept hopping corners from behind everything; and two browser-profile failures said the same vague nothing

Three amendments in one Rust-only pass (2026-08-26, Claude Opus 5). The
frontend half of the browser-profile work was in flight in a separate agent at
the same time; nothing under `src/` was touched here.

### 203a — the F11 half-release state (`src-tauri/src/engine/actions/pip.rs`)

**Symptom.** Take a PiP'd window to true borderless fullscreen (F11 in any
Chromium app). Always-on-top is correctly dropped. Then tap the PiP key: the
window flies to the NEXT corner instead of entering fresh — while sitting
behind every other window, because only ENTRY asserts `HWND_TOPMOST` and
corner-cycling deliberately never does.

**Root cause.** §7's half-release kept the cache entry (right — it is the only
surviving copy of the pre-PiP bounds, and `rcNormalPosition` cannot be
rewritten on a non-maximised window without visibly moving it) but kept it
UNCHANGED, flagged only with a `topmost_released: bool` that nothing except the
watcher read. To the engine an entry is an entry, so `toggle_pip` bumped
`position_index` and cycled.

**Fix.** A named state, not a flag, plus a tap decision that honours it.

```rust
pub enum PipState { Active, Released }

// PipEntry: `pub topmost_released: bool` -> `pub state: PipState`

/// Pure: what does this tap mean, given what the cache holds?
fn tap_for(map: &mut HashMap<isize, PipEntry>, key: isize, pid: u32) -> Tap {
    let Some(existing) = map.get(&key).cloned() else { return Tap::Enter(None) };
    if existing.state == PipState::Released {
        let recycled = pid != 0 && existing.pid != 0 && pid != existing.pid;
        if !recycled { return Tap::Enter(Some(existing)); }   // FRESH entry, preserved bounds
        map.remove(&key);
        return Tap::Enter(None);                              // stranger's window: measure it
    }
    /* ...unchanged cycle / 5th-tap restore... */
}
```

**The part that matters most: re-entry must NOT measure the window.** At that
moment the window shows the FULLSCREEN rect — or, if the user has since left
fullscreen, the corner tile PiP itself left behind, because Windows restores a
Chromium window to its pre-fullscreen bounds and those *were* the tile. Storing
either as `original_*` destroys the last copy of the real frame, and the 5th
tap then "restores" the window to a quarter-screen tile with nothing left
anywhere that knows better — the exact loss the whole design exists to prevent.
So the entry arm branches:

```rust
let (ox, oy, ow, oh, maximized) = if let Some(prev) = &preserved {
    (prev.original_x, prev.original_y, prev.original_w, prev.original_h, prev.was_maximized)
} else {
    match measure_original_frame(hwnd, hwnd_key) { Some(m) => m, None => return String::new() }
};
```

`measure_original_frame` is the old inline measurement, extracted verbatim, so
the one call site that may measure is visible instead of buried in a 60-line
arm. The re-entry publishes a NEW serial (a stale watcher claim must not be
able to drop the topmost this tap just re-asserted), `position_index: 0` and
`PipState::Active`.

The pid check exists ONLY on the released arm, and only a POSITIVE
disagreement between two known pids counts: a released entry is long-lived by
construction and is the one path that trusts stored numbers instead of
measuring, so a recycled handle would tile a stranger's window (NATIVE_SAFETY
rule 3) — but a zero pid means "could not tell", and "could not verify" must
never be read as "verified different" when the wrong answer throws the bounds
away.

`restore_all()` is unchanged and deliberately so: it drains EVERY entry,
released ones included, because a released window's true frame lives nowhere
else. `release_disposition(zoomed, state)` returns `None` for
`(false, Released)`, which is what stops the 500 ms watcher re-dropping topmost
and re-toasting twice a second.

**Verified.** 7 new unit tests, including a full lifecycle
(enter → half-release → re-enter → 4 taps → restore) asserting the 5th tap
still returns the ORIGINAL frame, plus a source-level test that the re-entry
branch reuses all four bounds and never calls `measure_original_frame` — the
arm itself needs a real foreground window to run, so no value-level test can
see which branch it took. The source-level test was itself validated by
printing the slice it inspects and confirming each `prev.original_*` name
appears exactly once there, in the tuple that stores it (an earlier draft would
have passed vacuously on the log line's copies of those names, so the log line
was trimmed).

**NOT verified.** No window has been taken to fullscreen in a built app; no
build or install was performed this pass.

**Generalise.** When two independent readers share one record, a boolean named
after what ONE of them did (`topmost_released`) will be ignored by the other.
Name the STATE, not the action, and the second reader's obligation becomes
visible at the type level.

### 203b — two failures, two sentences (`browser_profiles.rs`, `smart_cascade.rs`)

**Symptom.** A pinned browser that has been uninstalled and a pinned profile
folder that has been deleted both fell back correctly and both told the user
nothing. The design handoff, §5: *"Two different failures get two different
sentences. Something went wrong leaves the user guessing which half to fix."*

**Root cause.** Both reasons were already modelled
(`BrowserRoute::PinnedBrowserMissing`, `ProfileArg::DroppedStale`) and both
were already logged — to `debug.log`, which the user does not read.

**Fix.** Two pure message builders in `browser_profiles.rs`, emitted from
`smart_cascade` only after the launch has actually succeeded:

```rust
pinned_browser_missing_toast(BRAVE, Some("msedge")) == "⚠️ Brave is gone — opened in Edge"
stale_profile_toast(Some("STUDIES"), "Profile 3", BRAVE) == "⚠️ STUDIES is gone — Brave opened"
pinned_browser_missing_toast(BRAVE, None) == "⚠️ Brave is gone — opened in your default browser"
```

`toast.ts` peels the leading glyph off as the icon disc and renders the rest,
so what the user reads is the handoff's sentence exactly. The names come from
the exe STEM (`display_name_for_exe`) because that is genuinely all that is
left: an uninstalled browser is invisible to `scan_browsers()` (every
resolution source it has requires a live exe), its `StartMenuInternet` name
went with the uninstall, and `KeyBinding` stores no browser name — its three
browser fields are the exe path, the profile FOLDER and the profile NAME. The
lookup table names only the stems where title-casing gives a WRONG answer
(`msedge` → Edge, `iexplore`, `samsunginternet`, `opera_gx`, `librewolf`);
everything else title-cases correctly and a longer table would just be a list
to forget to update. The default browser is never guessed at — unresolved says
"your default browser".

Both go through one `notify()` that reads the AppHandle from the existing
`guide_hud` OnceLock (the PiP release path's precedent), not a second copy
threaded through the cascade. The app-binding path (a browser+profile pinned
with NO url) hits the same stale-profile failure, so `app_launch_params` became
`app_launch_plan` returning `AppLaunch { params, stale_profile }`, and the four
`launch_app(app, app_launch_params(b).as_deref(), h)` call sites in
`smart_cascade` became `launch_binding_app(b, app, h)` — one place that
launches and then reports, so the primary and Founders arms cannot drift.

**Preserved absolutely.** `should_use_specific_browser` is untouched and is
still the only thing that diverts a URL; blank is still treated as unset; the
`BrowserRoute::Default` arm still reads `=> return run_browser(url, app_handle)`
verbatim. The existing source-reading tests that pin all of that still pass,
and two new ones pin that both reasons are actually EMITTED (a message nobody
raises passes every value-level test and still ships "something went wrong").

**NOT verified.** No browser has been uninstalled and no profile folder
deleted to see either toast render.

### 203c — `get_default_browser` (`commands.rs`, `lib.rs`)

The key editor's paste row needs the OS default browser's real icon. Nothing
exposed it: `find_browser_cmd` is a different question with a similar name (it
walks four hardcoded Brave/Chrome install paths and answers "is there a
Chromium browser lying around?"). `smart_cascade::browser_stem` had the answer
but threw the path away, so it was split:

```rust
pub fn default_browser_exe() -> Option<String>   // smart_cascade — the resolver
fn browser_stem() -> Option<String>              // now just its file_stem
```

```rust
#[tauri::command]
pub fn get_default_browser(cache: State<'_, IconCacheState>) -> Option<DefaultBrowserInfo>
// DefaultBrowserInfo { exe: String, name: String, icon_base64: Option<String> }
```

One resolver, so the disc in the editor cannot show a different browser than
the key actually opens. The icon comes from the SAME `IconCacheState` keyed by
exe path as `list_start_menu_apps` and `list_browser_profiles`, so a browser
already drawn anywhere in the app costs nothing to draw again. The
`"cascade: default browser resolved"` log marker is unchanged — the path is
appended to it, never substituted.

**Verified live on this machine** (temporary probe test, run then removed):
`HKCU\...\UrlAssociations\https\UserChoice` → ProgId `BraveHTML` →
`"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe"
--single-argument %1` → path parsed, file exists, name "Brave", icon extracted
(3364 base64 chars). No capability entry is needed:
`capabilities/default.json` carries plugin permissions and window names, never
app-defined commands.

**Generalise.** When a second consumer needs a richer form of something an
existing function already computes, split the resolver rather than writing a
second lookup: two registry walks that can disagree about "the default
browser" is PROBLEM 60's entire failure class.

---

## PROBLEM 204 — the browser-profile pin never once reached `config.json`, and the picker that was supposed to set it overflowed the panel it lived in

The FRONTEND half of the 1.0.86 pass (2026-08-26). PROBLEM 203 is the Rust half
of the same release and was written by a different agent at the same time;
nothing under `src-tauri/` was touched here, and nothing in 203 is restated.
Two sub-problems: a pin that saved nothing (204a) and the surface that sets it
(204b).

### 204a — the pin never saved, and the toast said it had

**Symptom.** The owner's verdict on the feature as it shipped in 1.0.85:
browser profiles are *"so bad, non-functional"*.

**The evidence, measured rather than inferred.** His real `config.json`, copied
out and read from OUTSIDE the MSIX container (the PROBLEM 143 rule — a
verification performed by the sandboxed process cannot detect the sandbox):

| Measured in the owner's live config | Count |
| --- | --- |
| Profiles | 5 |
| Bindings total | 130 |
| Bindings holding a URL | 20 |
| Bindings carrying a `browser_exe` field | 130 |
| …of which `browser_exe` is `null` | **130** |
| Bindings with a non-null `browser_profile_dir` | **0** |

Every binding had the field. Not one had ever held a value. This was not a
feature that worked badly; it was a feature whose output had never once reached
disk.

**Root cause — three, and they interlock. Fixing any one alone leaves the
feature broken.**

**(a) Delete-by-omission across a full replace, invisible to TypeScript.**
`src/main.ts`'s `onSave` handler REPLACES the stored binding outright:

```ts
// src/main.ts — unchanged by this fix, and correct
if (profile) profile.bindings[key] = binding;
await persistConfig();
```

while `commit()` in `src/components/key-detail-panel.ts` handed it whatever
partial object the call site happened to build:

```ts
// BEFORE — key-detail-panel.ts
async function commit(binding: KeyBinding, skipConflict = false): Promise<void> {
  const key = _currentKey;
  if (!key || !_onSave) return;
  ...
  _onSave(key, binding);          // ← the caller's four fields, passed straight through
```

and every call site built exactly four:

```ts
// BEFORE — renderGrid()'s onPick, and the same shape in assignFromPath/handleRemove
onPick: (app) =>
  commit({
    app: app.path,
    web_url: null,
    label: app.name,
    icon_override: app.icon_base64 ?? null,
  }),
```

Replace + omit = delete. `browser_exe`, `browser_profile_dir` and
`browser_profile_name` were therefore erased on every save that did not
explicitly mention them, which was every save. **TypeScript could not warn about
it, and this is the part worth internalising: the three fields are OPTIONAL
(`browser_exe?: string | null`), and an object that omits an optional field is a
perfectly valid `KeyBinding`.** The type system agreed with every one of those
call sites. It was right about the type and wrong about the record.

Fixed by normalising to a COMPLETE seven-field `KeyBinding` once, inside
`commit()`, instead of asking six call sites to remember three fields:

```ts
// AFTER — key-detail-panel.ts, inside commit()
// The COMPLETE binding. Every optional field is stated, so a caller that
// leaves one off can no longer delete it by omission.
const full: KeyBinding = {
  app: binding.app ?? null,
  web_url: binding.web_url ?? null,
  label: binding.label ?? null,
  icon_override: binding.icon_override ?? null,
  // Never `""` — the owner's hard requirement is that a URL with no specific
  // browser opens in the OS default, and null is the only value that says so.
  browser_exe: binding.browser_exe ?? null,
  browser_profile_dir: binding.browser_profile_dir ?? null,
  browser_profile_name: binding.browser_profile_name ?? null,
};
_onSave(key, full);
```

**It stays a REPLACE, deliberately.** Re-pointing a key at a new target must
CLEAR the pin — the pin described the target being replaced — so a merge here
would resurrect it and a newly-dropped URL would silently keep opening in the
old profile. That is exactly what `BINDING_RESET` exists to prevent on the OTHER
save path (PROBLEM 202: `keyboard-matrix.ts`'s `updateBinding` DOES merge). The
two paths keep opposite semantics on purpose — **matrix = merge + explicit
reset, panel = replace + explicit normalise** — and both now say so in a doc
comment naming the trap. What changed is only that the panel's clearing is
stated instead of accidental.

**(b) `commit()` ended in `closePanel()`, so the chip could never appear.**
Pressing a browser tile in the app grid bound the key and destroyed the editor
in the same tick; `wireProfileChip` never got a chance to render anything. The
owner, before the cause was known: *"pressing any browser to a letter just
assigns it, i expected something to change in the app choosing dialogue after
detecting it is a browser to let me choose a profile."*

Fixed with an options object — `commit(b, true)` was already a positional
boolean, and a second independent switch would have made every call site a coin
flip about which is which:

```ts
interface CommitOptions {
  /** Skip the Space+<key> conflict prompt. */
  skipConflict?: boolean;
  /** Save WITHOUT collapsing the editor. Only the profile chip does. */
  keepOpen?: boolean;
  /** Override the confirmation text. Only the profile chip does. */
  toast?: string;
  /** Fired AFTER `_onSave` has actually run — never when `commit` bails out. */
  onSaved?: () => void;
}

// …at the end of commit(), after the toast:
opts.onSaved?.();
// A one-property edit leaves the editor where it was.
if (opts.keepOpen) return;
closePanel();
```

`onSaved` exists because *"did this save?"* was previously unanswerable from a
call site: `commit` returns `Promise<void>` and both of its early exits (no key,
and a conflict the user has yet to answer) look identical to success from
outside. The app-grid branch has to know, or a CANCELLED conflict prompt would
still turn the page onto a binding that was never written. It is carried through
`showConflict` too, so "Bind anyway" reaches it and "Cancel" does not.

**(c) The early return read state that `closePanel()` nulls synchronously.**

```ts
// BEFORE
const key = _currentKey;
if (!key || !_onSave) return;      // no save, no toast, NO LOG LINE
```

`closePanel()` sets `_currentKey = null` synchronously, so any caller that
awaited anything before reaching `commit()` could arrive to find the key gone —
and the function then did nothing, silently. This is the same trap as PROBLEM
199's `#ed-done`. A frontend-only failure leaves no trace in `debug.log` either,
so it reports through all three channels now:

```ts
// AFTER
const why = !key ? "no key is open (_currentKey is null)" : "no onSave handler";
const msg = `key-editor: commit ABORTED — ${why}; nothing was saved`;
console.error(msg, binding);
showToast("⚠️ Not saved — the key editor lost track of which key this was");
void invoke("frontend_log", { msg }).catch(() => {});
return;
```

**Exact files.**
- `D:\Claude-Projects\SpaceToggle-V14\src\components\key-detail-panel.ts` —
  `commit()` normalisation, `CommitOptions`, the instrumented early return,
  `commitProfile`, `refreshProfileRow`, `clearPin`, `offerPinUndo`.
- `D:\Claude-Projects\SpaceToggle-V14\src\main.ts` — no behaviour change; the
  full-replace line now carries the comment explaining why it is only safe
  BECAUSE `commit()` normalises first, and what must change if it ever merges.

**How it was verified.** `npx tsc --noEmit`: **0 errors** (re-run 2026-08-26
while writing this entry, not quoted from earlier in the session). The config
measurement above was taken from the copy pulled out through `explorer.exe`. The
whole pin path is now instrumented end to end at one line per step — `bp: chip
rendered` → `bp: chip opened page` → `bp: profile picked` → `bp: commit reached`
→ `key-editor: onSave key=… browser_exe=… profile_dir=…` — deliberately, because
*"I clicked it and nothing happened"* is not a diagnosis and this feature
shipped broken AND silent. **NOT verified: nobody has performed a pin end to
end in an installed build, so no `browser_exe` has yet been observed arriving in
`config.json` with a value in it.** That is the one measurement that would close
this, and it needs a human at the machine.

**Generalise — and the real lesson is not any of the three bugs.**

Each bug alone is ordinary. What made this feature ship broken and STAY broken
is that they formed a loop with no exit:

1. the chip only ever appears on a key that is ALREADY bound;
2. every `commit()` closed the panel, so the moment a browser was bound the
   editor vanished before the chip could be drawn;
3. and the success toast read `✅ Space+Y → Youtube` — from the template
   `✅ Space+${key} → ${label}`, which is **byte-identical** for "I pinned a
   profile" and "I re-bound this key to the same thing".

So a save that WORKED was indistinguishable from a save that did not. The owner
did the only rational thing available to him — re-did the binding to make sure
it had taken — and root cause (a) wiped the pin on that re-assign. **The
feature's own confirmation taught the user the gesture that destroyed its
result.**

Two rules come out of that, and they are the reusable part:

- **A confirmation that does not name what changed is not a confirmation.** It
  only proves that something ran. If two different operations can produce the
  same success message, that message cannot be used to tell them apart — and
  the user will do the thing that message invites, which here was to repeat the
  operation that erased the work. The fix is one line of copy, and it is now
  `🌐 Space+Y opens in ARPON'S STUDIES` (from `opts.toast`), which is a sentence
  a re-bind cannot produce.
- **A save path that can fail silently will.** Every early return on a write
  path needs a channel the user can see AND a channel the log keeps, or "it
  didn't work" is indistinguishable from "it worked and looked the same".

And the narrow corollary, which is the one another AI will hit first: **on a
full-replace write path, an optional field is a field that will eventually be
deleted by omission, and the type checker will not say so.** Normalise to the
complete record at ONE choke point rather than trusting N call sites to remember
N fields — and when the same record is ALSO written through a merging path, make
each path state its semantics in a comment at the point of the write, because
the two sets of callers learn opposite habits about what leaving a field off
means.

### 204b — the picker overflowed the 460px panel by 19px; page 2, and the decisions that are now load-bearing

**Symptom.** The chip's popover (`.bp-pop`) hung off the chip inside the key
editor and overflowed it — **measured: panel right edge 870, popover right edge
888, so ~19px** — which gave the editor a horizontal scrollbar. And on the path
people actually take it was not merely ugly but unreachable, for the reason in
204a(b): the panel was destroyed before the chip existed.

**Root cause.** A floating surface anchored to a chip inside a fixed-width,
absolutely-positioned panel is a layout with two owners of one width and no
arbiter. The popover's width is its content's; the panel's is 460px; nothing
reconciles them, so the popover wins and the panel grows a scrollbar.

**Fix — a second PAGE inside the panel**, per the owner's Claude Design handoff
(`design_handoff_browser_profile_pinning`), which settles it in one line: *"A
second page inside the panel. Not a popover, not a growing panel."*

**Exact files.**
- `D:\Claude-Projects\SpaceToggle-V14\src\components\browser-profile-picker.ts`
  — `renderProfilePage()` replaces the old `drawPicker()` popover;
  `renderProfileChip()` no longer takes an `onChange` (the body opens the page,
  the ✕ clears); the localStorage caches; `loadDefaultBrowser()`.
- `D:\Claude-Projects\SpaceToggle-V14\src\components\key-detail-panel.ts` —
  `openProfilePage()` / `closeProfilePage()`, the `#ed-bp-row` markup, the 4b
  disc (`wirePathDisc`, `syncPathDisc`).
- `D:\Claude-Projects\SpaceToggle-V14\src\styles.css` — `.bp-page` and its
  slide, `.bp-filter`, `.bp-count`, `.bp-checking`, the scroller fade, the warn
  chip states.

The geometry is the whole point, so it is worth one paste:

```css
/* `inset: 0` against the panel — which is already `position: absolute`, so it
   is the containing block — means the page is EXACTLY the panel's padding box.
   Page 1 stays in the DOM underneath, so the panel's height never changes and
   the keyboard behind it never re-layouts. */
#key-detail-panel.bp-paged { overflow: hidden; }
.bp-page {
  position: absolute; inset: 0; z-index: 12;
  display: flex; flex-direction: column;
  padding: 20px;
  border-radius: 22px;                    /* matches the panel it covers */
  background: rgba(var(--srf-rgb), .99);
  overflow: hidden;
  animation: bp-page-in 300ms cubic-bezier(.2, .8, .2, 1) both;
}
.bp-page.bp-page-out { animation: bp-page-out 195ms var(--ease-in) both; }
```

195ms is ~65% of the 300ms entrance — the app's standing exit ratio, not a
number picked here. `:root.reduced-motion` renders final states for the page,
the cross-fade and the checking dot.

**The design decisions that are now LOAD-BEARING.** Recorded so nobody
re-derives them, and so nobody "fixes" one of them back:

- **The filter appears only above 6 profiles, and matches BOTH the display name
  and the folder name.** Six is not arbitrary: six tiles at 3 columns is two
  rows, and the scroller is capped at three rows (246px), so six is the largest
  count that needs no scrolling at all — below it the field is furniture.
  ```ts
  const profiles = q
    ? b.profiles.filter(
        (p) =>
          p.display_name.toLowerCase().includes(q) ||
          p.directory.toLowerCase().includes(q),
      )
    : b.profiles;
  ```
  Matching the folder too is what lets someone type "Profile 9" and find the
  profile whose owner never named it. While filtering, the per-browser count
  reads `3 of 15` — **the filter must never hide how much it hid.**
- **A browser with exactly ONE profile never opens the page — but the profile IS
  still written.** There is no choice to make, so the page would be a stop for
  nothing; writing it anyway (`only?.directory`) makes the launch explicit
  instead of leaning on Chromium's last-used profile.
- **First run binds and closes.** `findBrowserByExe` returning null means NOT
  YET KNOWN, not "no" — and on a true first run that resolves to bind-and-close,
  because nobody waits up to ~2.2s (a ~1.5s AppData scan plus IPC) to be offered
  something optional. Reopening the key once the scan has landed shows the chip.
- **Cache-first paint from the second run onward.** `knownBrowsers()` returns
  this session's scan `??` last session's, so page 2 paints in one frame; the
  fresh scan then repaints IN PLACE over a 300ms cross-fade behind a 5px sage
  dot and the word "checking". No spinner — a spinner over usable content reads
  as "wait". Two localStorage keys, and they are different on purpose:
  `st-bp-browsers-v1` is REPLACED by each scan, while `st-bp-names-v1` (exe →
  friendly name) is MERGED by each scan and never pruned, because
  "Brave (missing)" needs a name from a scan where Brave still existed.
  **Stated stand-in, not a preference:** the handoff asks for this beside the
  config; there is no command for that today, so it uses the WebView's
  localStorage. If a Rust-side `last_known_browsers` ever lands, move it — the
  config is backed up and undoable, localStorage is not. On quota the write
  drops the base64 icons rather than the whole cache, because the icon is the
  one part that can be re-extracted for free next run.
- **Clearing a pin writes three nulls, offers Undo for 6s, and shows no confirm
  dialog.** It is one press to redo. **Never `""` — null is the only value that
  means "the OS default browser opens this"**, which is the owner's hard
  requirement and is re-checked in Rust by `should_use_specific_browser`.
  **Deviation from the handoff, recorded so nobody re-derives it:** the handoff
  puts the Undo in the toast. It cannot go there — `#toast-container` is
  `pointer-events: none`, and `toast.ts` is the overlay's VERBATIM drop-in, so
  the same component renders into the transparent click-through overlay window
  where a button is unreachable by definition. The Undo therefore sits on the
  row where the ✕ that caused it was, for the same 6 seconds; the panel is still
  open (`keepOpen`), so it is if anything closer to hand.
- **An uninstalled pinned browser does NOT rewrite the binding.** Reinstalling
  must simply work again. The chip goes warn-coloured and reads
  `Brave (missing) → default`, and the name comes from the additive name map
  because the binding stores no browser name at all — without it the chip read
  `brave.exe (missing)`, which is true and is not what the user calls it. The
  chip only claims "missing" once a list actually exists (`scanned`): a
  warning-coloured chip on a machine where the scan has merely not landed yet is
  a lie.
- **A deleted profile folder is a different state and says so:**
  `ARPON'S STUDIES missing`, drawn from the profile name STORED IN THE BINDING —
  the browser's `Local State` no longer lists it, so that string is the only
  place the human name survives. Falling back to the folder would read
  `Default missing`, which names the wrong thing.
- **Profile avatars are deliberately NOT extracted.** They live in a versioned
  internal Chromium cache format that changes between releases, and a
  wrong-looking avatar is worse than none; each profile gets `paintLetterDisc`
  like every other missing icon in this app. The BROWSER's icon is real,
  because that one comes from the shell extractor the app grid already uses.
- **The browser header is drawn even when only ONE browser was found** (owner's
  decision): the page has the same structure on every machine, so nobody has to
  learn two layouts.
- **4b, the leading disc in the paste row**, is the second way in — for a URL
  that has not been committed yet. It appears only once the field holds a link
  (`/^https?:\/\//i`), arriving together with its 38px of padding on a 90ms
  tween, because a permanently-present disc would be a control that can do
  nothing on the common path and a permanently-reserved gutter is the same
  problem wearing a different hat. It paints the REAL default browser's icon
  from `get_default_browser` (PROBLEM 203c) — the same resolver `run_browser`
  launches through, so the disc physically cannot name a browser other than the
  one the key would open — and falls back to a dashed ring and `◍` when Rust
  resolves nothing, **never to a guess**: showing Brave's icon because Brave
  happens to be installed would be a lie on exactly the machine that matters,
  one whose default is Firefox. Pressing it commits the URL first and turns the
  page from `onSaved`, so a conflict the user cancels cannot leave a page open
  over a binding that was never written. **The agreed fallback is recorded in
  the source**: if the disc proves undiscoverable, build 4a instead (commit, then
  turn the page automatically); `renderProfilePage` already takes the
  multi-browser shape 4a needs, so only the trigger differs.

**The mid-run defect, found and fixed during this work.** The chip drew an EMPTY
browser name on APP bindings, with the raw folder name "Default" as the profile.
Not a typo — a consequence of a correct decision one layer down: `browser_exe`
is null on an app binding BY DESIGN, because the exe already IS `binding.app`
and storing it twice is how the two get to disagree. The chip's lookup was
handed that null, found no browser, and had nothing to read a display name from.
Fixed by resolving the EFFECTIVE exe at paint time:

```ts
// key-detail-panel.ts — wireProfileChip()
renderProfileChip(host, {
  browserExe: binding.browser_exe ?? boundApp,   // ← which browser to NAME
  profileDir: binding.browser_profile_dir ?? null,
  profileName: binding.browser_profile_name ?? null,
  exePinned: !!binding.browser_exe,              // ← whether anything is PINNED
}, { … });
```

Two fields rather than one, because they answer different questions: which
browser to name, versus whether the binding actually STORES a browser choice.
The second is what separates "this key opens Brave because it is bound to Brave"
from "somebody pinned this to Brave", and therefore whether the chip is filled
and carries a ✕ that has something to clear — a ✕ that writes three nulls over
three nulls is a control that can do nothing, which this codebase treats as
worse than a missing control.

**How it was verified.** `npx tsc --noEmit`: 0 errors (re-run while writing
this). The 19px overflow figure and the 870/888 edges are measurements taken
against the real CSS before the change. The `#ed-bp-row` width arithmetic is
recorded in the markup comment: the 10px/700/.1em uppercase label measures
~110px, plus an 8px gap, plus the chip's 260px cap = ~378px against the row's
416px usable width (460 − 2×20 padding − 2×2 margin), so no CSS change was
needed for the "Browser profile" label.

**NOT verified, stated plainly.** Geometry and motion were measured as SETTLED
GEOMETRY WITH ANIMATIONS DISABLED, because the harness tab composited no frames
— so the 300ms slide, the 195ms exit and the 300ms cross-fade are transcribed
from the handoff's motion table and have never been WATCHED. No human has seen
page 2, the filter, the warn states or the 4b disc render in the real dashboard.
Only the default (Earthy) theme was rendered, while the handoff calls for Earthy,
Warcry and Starry. Nobody has held Space and looked at the HUD. And no pin has
been driven end to end into `config.json` (204a).

**Generalise.** Two things.

1. **A floating surface anchored inside a fixed-width panel has two owners of
   one width and no arbiter.** When the container already owns a box, a PAGE —
   `inset: 0` against that container's own containing block, with page 1 left in
   the DOM underneath so the height never changes — cannot overflow by
   construction. It costs one back button, and it buys a surface that is
   correct on every screen size without a single measurement. Reach for it
   before reaching for collision detection or a popover library.
2. **When one piece of data has two legitimate homes, resolve the effective
   value at ONE point at paint time, and keep "is it set deliberately" as a
   SEPARATE field.** Collapsing the two is what produced the empty browser name
   here, and it is the general shape of every "the label went blank for one kind
   of record" bug: the reader was handed the field that happens to be null for
   that kind, and had no way to ask which kind it was looking at.

---

## PROBLEM 205 — 15.8 seconds of nothing at startup: one non-`async` command holding the main thread, and 85% of the startup with no telemetry at all

**Not a 1.0.86 regression.** 175 logged sessions over 15 days: median manual
launch **7443ms**, minimum ever **5432ms**. `git log -S "void loadApps()"` dates
the warm call to 1.0.27. This has always been true; it simply landed on the
wrong side of the 10s show-fallback on the night it was diagnosed.

**Two of the four changes below are code. The other two are logging, and they
are not decoration** — the reason a ~12s block survived roughly 60 versions is
that it emitted nothing, and the one line that DID fire recorded an intention
rather than an event and actively misdirected the search.

### Symptom

The app is launched. A tray icon appears. **No window appears for ~15.8
seconds** — no splash, no frame, no "(Not Responding)" ghost, nothing at all.
Then the whole dashboard arrives at once, fully painted.

### Root cause

`list_start_menu_apps` in `src-tauri/src/commands.rs` is declared `pub fn`, not
`pub async fn`. **In Tauri v2 a `#[tauri::command]` without `async` executes on
the MAIN THREAD.** That command shells out to PowerShell to walk both Start Menu
trees recursively, resolve every `.lnk` through `WScript.Shell` COM, and
enumerate `shell:AppsFolder` — ~12 seconds — and then extracts an icon for each
result. While it runs, the main thread is held, and **every IPC call from BOTH
webviews queues behind it**, including the `dashboard_ready` call that is the
only thing that shows the window (PROBLEM 74).

The frontend fired it during `bootstrap()`, on the critical path to first paint.

**The evidence, from the live `debug.log`, decisive rather than suggestive:**

| Observation | What it proves |
| --- | --- |
| `get_conflicts` issued at +1.2s, result landed at **+15.772s**, and the scan itself costs **16ms** (two scans logged 16ms apart) | A **14.6-second queue delay** on a 16ms operation. The work was not slow; it was waiting. |
| The overlay is a SEPARATE webview, and its commands drained in the **same 10ms window** | Two independent webviews draining simultaneously cannot be a per-webview queue. It is **one shared serialisation point**. |
| `list_browser_profiles` (2509ms) did not START until **+13.245s** | Queued, not slow. |

Ruled out by direct measurement, all of them: config parsing (logs at +0.001s),
Sentry (+0.000s, no network I/O), icon extraction as a startup path (it is not
on one), the starry sky (completes before +1.137s), software compositing.

**And the reason nobody suspected it for ~60 versions is written down in the
source itself.** `commands.rs` carried this comment above `dashboard_ready`:

```rust
// BEFORE — src-tauri/src/commands.rs, and it is BACKWARDS
// Window ops belong on the main thread; a command handler is not on it.
```

That is true only of `async` commands. Held as a general rule, it makes every
synchronous command in the file look harmless.

### Exact files

| File | Change |
| --- | --- |
| `src-tauri/src/commands.rs` | `list_start_menu_apps` — timing log + a doc comment recording the async attempt; `dashboard_ready` — the backwards comment corrected |
| `src-tauri/src/lib.rs` | `spawn_show_fallback` — the log line that lied |
| `src/components/key-detail-panel.ts` | the three warm-ups moved off bootstrap into `warmPickerData()` |
| `src/components/settings-panel.ts` | **the second bootstrap trigger, missed by the original diagnosis** |
| `src/main.ts` | `mark()` + three startup marks |

---

### 205a — the async conversion was ATTEMPTED, COMPILED, and DELIBERATELY NOT SHIPPED

The obvious fix is `pub fn` → `pub async fn`. It was written and tested. **It is
not shipped, and the reasons are worth more than the fix would have been.**

**It is not a one-line change. It does not compile.** Measured 2026-08-27:

```text
error[E0277]: async commands that contain references as inputs must return a `Result`
   --> src\commands.rs:210:72
    | pub async fn list_start_menu_apps(cache: State<'_, IconCacheState>) -> Vec<AppInfo> {
    |                                                                        ^^^ the trait
    |     `AsyncCommandMustReturnResult` is not implemented for `Vec<AppInfo>`
error[E0597]: `__tauri_message__` does not live long enough
   --> src\commands.rs:209:1
    |   argument requires that `__tauri_message__` is borrowed for `'static`
```

Carrying the `'_` lifetime on `State<'_, IconCacheState>` is **necessary but not
sufficient**: Tauri additionally forces a `Result` return on any async command
holding a borrowed input. The shape that DOES compile — verified clean, then
reverted — is:

```rust
pub async fn list_start_menu_apps(
    cache: State<'_, IconCacheState>,
) -> Result<Vec<AppInfo>, String> {
    ...
    Ok(apps)
}
```

and it needs **no frontend change at all**, because Tauri resolves the JS
promise with the `Ok` value, so `invoke<AppInfo[]>("list_start_menu_apps")` is
unaffected.

**Why it was still not shipped: the body calls in-process COM.** The premise
under which the conversion was authorised was *"the command uses NO COM in
Rust — the COM lives inside PowerShell's own process; it only uses
`std::process::Command`."* **That is false.** The per-app loop does this:

```rust
// src-tauri/src/commands.rs, inside list_start_menu_apps' result loop
if let Some(b64) = crate::icon_extractor::extract_icon(&exe_path) {
```

and `icon_extractor::shell_icon_rgba` does:

```rust
// src-tauri/src/icon_extractor.rs
let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
let factory: IShellItemImageFactory =
    SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
```

That is `CoInitializeEx` + `IShellItemImageFactory` — **apartment-threaded COM,
in this process, on the calling thread** — run once per detected app (210+ on
this machine).

**The check that settled it.** All four call sites of `extract_icon` in the
crate — `commands.rs:162` (`extract_icon_cmd`), `commands.rs:278`
(`list_start_menu_apps`), `commands.rs:2011` (`get_default_browser`) and
`browser_profiles.rs:1423` (`list_browser_profiles`) — sit inside **non-`async`**
commands. **This COM has therefore never once executed off the main thread in
this app.** There is no evidence it survives an STA created on a pooled async
worker with no message pump, and its failure modes are the two worst kinds: a
silent `None` (letter discs instead of icons, which reads as cosmetic and gets
misdiagnosed for days) or a hang. `CoUninitialize` is never called either, so a
converted command would leave a permanent STA on a reusable runtime worker.

**The cheap, safe version for whoever takes this on properly: SPLIT the
command.** PowerShell owns most of the 12s and its COM lives in its own process,
so moving *only* the `std::process::Command` half off the main thread and
leaving the `extract_icon` loop where it is gets most of the win with none of
the apartment question. That needs a real build, a real launch and a look at the
icons — none of which the diagnosing session was permitted to do.

**What WAS shipped for this command is the telemetry it never had:**

```rust
// AFTER — src-tauri/src/commands.rs, end of list_start_menu_apps
log::info!(
    "start_menu_scan: found {} app(s) in {}ms on the MAIN THREAD \
     (powershell {}ms, icons {}ms)",
    apps.len(),
    t_start.elapsed().as_millis(),
    t_powershell.as_millis(),
    t_start.elapsed().saturating_sub(t_powershell).as_millis(),
);
```

The split is the point. PowerShell and the in-process icon pass are two
different costs with two different fixes, and one total cannot tell them apart.

### 205b — the warm-ups came off the bootstrap path, and there were TWO of them

`initKeyDetailPanel` ran three fire-and-forget scans, all of them non-`async`
commands, all of them therefore main-thread:

```ts
// BEFORE — src/components/key-detail-panel.ts, inside initKeyDetailPanel()
void loadApps();          // list_start_menu_apps   ~12s   MAIN THREAD
warmBrowsers();           // list_browser_profiles  ~2.5s  MAIN THREAD
warmDefaultBrowser();     // get_default_browser           MAIN THREAD
```

`initKeyDetailPanel` is called from `bootstrap()`, whose LAST act is
`dashboard_ready` — the only thing that shows the window. They now live in a
first-open warm:

```ts
// AFTER — src/components/key-detail-panel.ts
let _pickerWarmed = false;
function warmPickerData(): void {
  if (_pickerWarmed) return;
  _pickerWarmed = true;
  void loadApps();
  warmBrowsers();
  warmDefaultBrowser();
}

export function openPanel(key: string, config: AppConfig, origin?: HTMLElement): void {
  ...
  if (!_panel) return;
  // PROBLEM 205 — first open pays for the picker scans, not bootstrap.
  warmPickerData();
```

**Nothing regresses, because every consumer already treated "not landed yet" as
its own state rather than as an answer** — that was checked one by one, not
assumed: `drawAppGrid` renders `"Scanning this device…"`; `wireProfileChip`
re-checks after `loadBrowsers()` resolves; `knownBrowsers()` falls back to last
session's list from `localStorage`; the default-browser disc repaints when
`loadDefaultBrowser()` lands.

**THE PART THE DIAGNOSIS MISSED, and the reason to always follow a call graph
rather than a line number.** Deferring the key editor's warm alone would have
changed *nothing measurable*, because the settings panel was a **second**
bootstrap trigger for the very same scan. `initSettingsPanel` → `render()`
reaches BOTH `renderAppExceptions()` and `renderConflicts()`, and each fired it:

```ts
// BEFORE — src/components/settings-panel.ts, renderAppExceptions()
void loadApps();

// BEFORE — src/components/settings-panel.ts, renderConflicts()
void loadApps().then(() => { if (panelEl && !panelEl.hidden) draw(); });
```

Both are now gated on the panel having actually been opened:

```ts
// AFTER — src/components/settings-panel.ts
let _settingsEverOpened = false;
function warmAppsIfOpened(): void {
  if (_settingsEverOpened) void loadApps();
}

export function openSettingsPanel(): void {
  if (!panelEl) return;
  _settingsEverOpened = true;   // PROBLEM 205 — before render(), which reads it
```

The ordering matters and is easy to get wrong: `openSettingsPanel` calls
`render()` **before** setting `panelEl.hidden = false`, so a guard written as
`if (!panelEl.hidden)` would suppress the warm on open too. The flag is set
first, deliberately.

**Honest accounting: this RELOCATES the ~12s, it does not remove it.** Until
205a is done properly, the first key-editor open pays for it. That is a
deliberate trade — a wait the user asked for, with a visible "Scanning this
device…" note, beats the same wait before any window exists — but it is a trade,
not a win, and calling it a win is how the next person concludes the fix failed.

### 205c — a log line that recorded an INTENTION as if it were an EVENT

`spawn_show_fallback` is the 10s safety net that shows the window when
`dashboard_ready` never arrives. It logged this **before** `run_on_main_thread`:

```rust
// BEFORE — src-tauri/src/lib.rs
log::warn!(
    "setup: dashboard_ready never arrived after 10s — showing the \
     window anyway (frontend wedged or webview dead; PROBLEM 74)"
);
let _ = app2.run_on_main_thread(move || { ... let _ = w.show(); ... });
```

**The one case it gets wrong is the exact case the fallback exists for.** When
the main thread is blocked, the closure does not run, `w.show()` never happens,
the window never appears — and the log says it did. Two hours of this
investigation were spent trusting that line and looking for a window that had
been shown and was somehow invisible.

```rust
// AFTER — src-tauri/src/lib.rs
log::warn!(
    "setup: dashboard_ready never arrived after 10s — ASKING the \
     main thread to show the window (frontend wedged or webview \
     dead; PROBLEM 74). If no 'show-fallback: window shown' line \
     follows, the main thread is blocked and it never happened."
);
let app3 = app2.clone();
let _ = app2.run_on_main_thread(move || {
    use tauri::Manager;
    if let Some(w) = app3.get_webview_window("settings") {
        ensure_on_screen(&w); // PROBLEM 83
        let _ = w.show();
        let _ = w.set_focus();
        log::warn!(
            "show-fallback: window shown by the 10s fallback \
             (main thread reached it)"
        );
    } else {
        log::warn!(
            "show-fallback: reached the main thread but there is \
             no 'settings' window to show"
        );
    }
});
```

Both halves are kept on purpose: the ASK proves the fallback fired, the EVENT
proves it landed, and **their absence relative to each other is now itself the
diagnosis.** This is the same rule the window section of `CLAUDE.md` already
states for `hide()`/`show()` — it simply had not been applied to a log line that
sits outside a closure.

### 205d — three marks into a 14.66-second hole

Between `dashboard-js: motion:` (+1.138s) and `dashboard-js: frontend ready`
(+15.797s) there was **not one line of telemetry**. In a median session that
window is ~6.3 of 7.4 seconds — **roughly 85% of every startup this app has ever
performed was unmeasured.** That is the real reason this survived ~60 versions.

```ts
// AFTER — src/main.ts
function mark(what: string): void {
  void invoke("frontend_log", {
    msg: `${what} (+${Math.round(performance.now())}ms)`,
  }).catch(() => {});
}
```

fired at three points: `mark("boot: keyboard matrix wired")` after
`initKeyboardMatrix`, `mark("boot: key editor wired")` after
`initKeyDetailPanel`, and
`mark("boot: bootstrap complete, calling dashboard_ready")` immediately before
the `dashboard_ready` invoke. `grep "boot:" debug.log` is now the whole
bootstrap timeline, and the delta between the third mark and Rust's
`dashboard-js: frontend ready` is **pure IPC queue time** — if those two are
seconds apart, something non-`async` is holding the main thread and the frontend
is not the suspect.

### 205e — the comment that caused all of it

```rust
// BEFORE — src-tauri/src/commands.rs, above dashboard_ready's run_on_main_thread
// Window ops belong on the main thread; a command handler is not on it.
```

Replaced with the rule stated precisely enough that it cannot be misread:

```rust
// AFTER
//   · `#[tauri::command] fn foo(..)`        → runs ON THE MAIN THREAD.
//   · `#[tauri::command] async fn foo(..)`  → runs on the async runtime,
//                                             i.e. OFF the main thread.
//
// This comment used to read "a command handler is not on it", which is
// true only for `async` commands. ... If you add a command that does more
// than a few milliseconds of work, it must be `async` — or its cost must
// be kept off any path the user waits on, and it must LOG its duration.
```

### How it was verified

| Check | Result |
| --- | --- |
| `npx tsc --noEmit` | 0 errors |
| `npm run build` | 0 errors (only the pre-existing `key-wake.ts` dynamic-import notice) |
| `cargo check --lib` | **0 errors, 0 warnings** |
| `cargo test --lib` | **124 passed, 0 failed, 3 ignored** at the time of this pass (baseline was 90+). Re-run minutes later: **127 passed, 0 failed, 3 ignored** — a concurrent agent landed 3 more `smart_cascade` tests while this work was in flight. Neither run has a failure. |
| The async conversion | Compiled in BOTH forms — the naive one **fails** (`AsyncCommandMustReturnResult`, E0597), the `Result<Vec<AppInfo>, String>` one **succeeds**. Reverted, not shipped; see 205a. |
| Startup time | **NOT MEASURED AND NOT CLAIMED.** Measuring it requires a version bump, `npm run tauri build`, an install and a launch — all four explicitly out of scope for the session that made these changes. No speed-up is asserted here. |

The call-graph claims in 205b were verified by reading every caller, not by
assuming: `grep -n "renderAppExceptions()"` returns five sites, and the one
inside `render()` is the bootstrap path.

### Generalise this

**An unlogged operation on a shared thread is invisible twice over: you cannot
see its cost, and you cannot see what it is blocking.** 85% of this app's
startup had no telemetry, and the ~12s inside it was not merely slow — it was
serialising an entire IPC bus that carried the one call responsible for showing
the window. Neither fact was observable, so for ~60 versions the only available
hypotheses were about the things that DID log.

Four rules fall out of it, in descending order of how much they would have
saved:

1. **Log the EVENT, never the INTENTION.** A line that says "showing the window"
   before the code that shows it is not a log, it is a comment that survived into
   production, and it is worse than silence: silence prompts investigation, while
   a confident false statement ends it. If an action is queued to another thread,
   log the ask and the landing separately — and say in the ask what a missing
   landing means.
2. **Know which thread your framework runs your handler on, and write the rule
   down where someone will hit it.** For Tauri v2 it is `async` or not, and the
   difference is the main thread. One backwards comment made ~60 versions of
   readers stop looking.
3. **Time every operation that shells out, touches the shell namespace, or walks
   a directory tree, and log the total with its parts split.** A single number
   cannot tell a 12s subprocess apart from 12s of icon COM, and those have
   different fixes.
4. **Follow the call graph, not the line number, before declaring a hot path
   moved.** The diagnosis named one `void loadApps()` and there were three, two
   of them reached through `initSettingsPanel` → `render()`. Deferring one would
   have produced a fix that measured as no change and got blamed on the wrong
   hypothesis.

And the one that governs the whole entry: **when the justification for a change
lists the reasons it is safe, verify each reason before trusting the
conclusion.** Four were given here for the async conversion. Point 2 was
incomplete (the lifetime is necessary, not sufficient — Tauri also demands a
`Result`) and **point 1 was simply false** — the command does call in-process
COM. Points 3 and 4 held. The conversion may well still be correct; it has just
not been shown to be, and the difference between those two is the entire value
of checking.

---

## PROBLEM 206 — pointer activation on the Guide HUD: point at a chip, release Space (or click), and it launches — built without spending a microsecond the hook does not have

**A feature entry, and NOT SHIPPED with this session** — implement-and-test
only by instruction: no version bump, no `npm run tauri build`, no install.
The gesture itself is untestable without a build, an install and a hand on the
mouse; nothing below claims otherwise.

### What the owner asked for

> "if someone moves the cursor towards the listed [letter] and then leaves the
> cursor, then that app opens up... if they are using the touchpad of a laptop,
> they can use clicking — while holding the space, if they move the cursor to
> click on that app name, then that app opens up. So either click or leave
> space after moving cursor to that app. Make a toggle for this option on off
> in the settings too."

Two gestures, both while Space is physically held: (A) cursor resting on a
chip, then RELEASE Space → launch it; (B) cursor on a chip, then LEFT-CLICK →
launch it. Plus a Settings toggle, **default OFF**.

### The constraints that shaped the whole design (do not re-derive)

1. **The overlay cannot receive mouse input and must not be made to.**
   `configure_overlay_window` (lib.rs) calls `set_ignore_cursor_events(true)`
   and FAILS CLOSED (`OVERLAY_DISABLED`) — a HUD that eats clicks is worse
   than no HUD. The window is `WS_EX_TRANSPARENT`, which hit-testing skips
   entirely: no mousemove, no click, no `:hover` ever reaches the page. All
   cursor knowledge therefore comes from `WH_MOUSE_LL`.
2. **Both LL hooks run serialized on one thread, and this machine's hook
   budget is already failing** — one 6h54m log window held 32 DEAF events and
   65 watchdog re-hooks, and `LowLevelHooksTimeout` is wall-clock. So the
   callback does lock-free atomics and NOTHING else: no geometry, no chip
   scan, no logging (PROBLEM 58), no win32k calls (PROBLEM 134/184).
3. **There is no route from the overlay page to a launch** — `handle_alpha`
   is private and `EngineState` is never `.manage()`d, so no
   `#[tauri::command]` can reach it. Activation travels as a new `HookEvent`
   variant on the existing crossbeam channel and lands in the engine's
   `dispatch()`, inheriting the PROBLEM 135 handover choreography for free.
4. **Coordinate spaces:** `MSLLHOOKSTRUCT.pt` and `GetCursorPos` are PHYSICAL
   px (PerMonitorV2); chip boxes from the page are CSS px == logical inside
   the window. Conversion follows `apply_region`'s exact convention
   (`(x * dpr).floor()` / `.ceil()` on the far edge), against the overlay
   window's ACTUAL read-back position — the log records asked (202,247) vs
   GOT (203,247), a 1px rounding artefact that would shear every rect.

### Architecture as built

**`src-tauri/src/hook/pointer.rs` (new)** — everything lives here:

* **Hook side (µs budget):** `note_cursor(x, y)` — three relaxed stores
  (x, y, GetTickCount64 stamp) on `WM_MOUSEMOVE` while `MODIFIER_ACTIVE`. The
  stamp is load-bearing: cursor data is valid for a hold iff
  `CURSOR_STAMP >= SPACE_DOWN_TS`, which is what stops a STALE position from
  a previous hold dwell-arming a chip under a mouse nobody touched.
* **Chip snapshot:** `publish_keys()` (called by `show_guide_hud` with the
  same `apps` list the page builds chips from) + `publish_chips()` (the new
  `publish_hud_chips` command in commands.rs, payload `{chips:[{x,y,w,h}],
  dpr}` in CSS px — `overlay_shape`'s exact convention). Stored in
  fixed-size static atomic arrays (`[AtomicI32; 40*4]` + counts) — the shared
  snapshot is heap-free. Geometry count is zeroed before any rewrite so a
  poller tick reads old or new, never a mix; arming is bounded by the SMALLER
  of the two counts. `clear_chips()` runs on every hide path.
* **Poller `st-hud-pointer`:** ~60Hz while Space is held (16ms), 40ms idle,
  250ms with the setting off; same shape as `exclusions.rs` (named thread,
  `catch_unwind` around the Win32 probe, non-panicking spawn — PROBLEM 124).
  It reads the atomics, runs the pure `HoldTracker::tick`, applies the
  verdict, and emits the global `hud-pointer` event `{ index }` **on change
  only** — a typical hold produces a handful of emits. Global `emit` + the
  single listener in `initToastListener` (toast.ts), the only arrangement
  that has ever delivered here.
* **Activation:** `HookEvent::PointerActivate(char)`. Gesture A: the Space-up
  path consumes the armed index (`take_armed_key`, atomics only) and sends it
  INSTEAD of `SpaceUp`. Gesture B: `WM_LBUTTONDOWN` while armed consumes,
  latches `CLICK_EATEN`, sends the same event, returns `LRESULT(1)`. The
  engine's new arm does `cancel_hud(true)` (toast coming — PROBLEM 135
  handover) then `handle_alpha` → `smart_cascade` → toast, byte-identical to
  a keyboard combo.

### The paired click suppression

Suppressing `WM_LBUTTONDOWN` but letting `WM_LBUTTONUP` through leaves the
app underneath with unbalanced button state — stuck drag, phantom selection.
`CLICK_EATEN` is latched at the suppressed down and consumed at the next
`WM_LBUTTONUP`, **checked before every other gate in `ms_hook_proc`** —
deliberately ahead of `EXCLUDED_ACTIVE` and `MODIFIER_ACTIVE`, because the
matching up can arrive after Space was released or after the launched app put
an excluded window in front. Same rule as `SPACE_INTERCEPTED`: whoever eats
the down owes the up. The watchdog's eviction reset clears it
(`reset_on_eviction`) alongside the Space latches, so a lost up cannot leave
a future click half-eaten.

### The `SPACE_ABORTED` mechanic, and the ordering that matters

Arming does NOT add a branch to the release path. It **sets
`SPACE_ABORTED = true`** — the wheel's exact mechanic — so armed+release
injects no space through today's unchanged code, and not-armed+release is
untouched. The structural "a tap always types a space" guarantee is
preserved rather than re-argued.

**Disarming must clear it back — but only when the disarm is the user
drifting out.** The tracker distinguishes `DisarmClear` (cursor left every
halo while the hold was live, visible, unblocked → clear the flag, release
types a space again) from `DisarmKeep` (HUD hid because a combo fired, the
wheel blocked the hold, the hold ended → the abort belongs to THAT gesture;
clearing it would type a space behind a launched action). `apply_to()` in
pointer.rs owns both orderings, each chosen so a racing Space-up sees the
QUIET failure:

```rust
// Arm: claim SPACE_ABORTED FIRST (CAS false→true — an already-aborted hold
// REFUSES to arm), then publish ARMED_INDEX. A release between the two sees
// aborted-but-not-armed: no space, no launch. The other order would launch
// AND type a space.
// DisarmClear: retract ARMED_INDEX FIRST, then clear SPACE_ABORTED. A
// release in between: no space, no launch. The other order would do both.
```

### The six guards (all required — "hold Space, move mouse, release" must stay a typed space)

| # | Guard | Value | Why this value |
|---|---|---|---|
| 1 | HUD visible (`is_visible()`) | — | `guide_hud_delay_ms` >= 300ms means nothing can arm that the user cannot SEE; also comfortably past the 200ms rollover window |
| 2 | Minimum travel from Space-down position | 24 physical px | resting-hand jitter is a few px, a deliberate reach is tens-hundreds; start latched by the poller via GetCursorPos at hold start |
| 3 | Dwell on one chip | 150ms (~9 ticks) | middle of the 120-200ms band; a fly-through crosses a chip in far less |
| 4 | **Containment + halo, never nearest-neighbour** | 12 CSS px halo | the owner's decision, overriding his own "towards" wording — nearest-by-angle always has an answer, so every release would launch; overlapping halos tie-break by nearest CENTRE among containing chips only |
| 5 | Visible arming | `hud-pointer` emit | the page paints `.armed`; emit failure logs a WARN naming the consequence |
| 6 | Disarm on wheel | `block_for_hold()` | Space+scroll already aborts and changes opacity; it must not ALSO launch |

### The setting — both ends, per PROBLEM 180

`pointer_hud_activation`, bare `#[serde(default)]` (absent → **OFF**) +
`false` in `Default` — brand-new behaviour is opt-in. Published into
`hook::POINTER_HUD_ACTIVATION` from **`config/mod.rs::save()`** (the single
funnel — `reset_config` included) AND seeded at the **lib.rs startup load**
(the atomic starts false; skipping the seed is the works-all-session,
dead-next-launch failure). `first_install_tests` asserts OFF on BOTH the
`Default` path and the field-removal path. Frontend reads `=== true`, never
`!== false` (types.ts documents why); Settings row "Point to launch"
(id `hudpointer`, TOGGLE_CHAR `rng`, DESC entry present so the label is
clickable and included in show_me_around); `preview.ts` harness carries it.

### Exact files

* `src-tauri/src/hook/pointer.rs` — NEW: statics, hook-path helpers, pure
  `hit_chip`/`HoldTracker`/`apply_to`, poller, 14 tests.
* `src-tauri/src/hook/mod.rs` — `pub mod pointer`; `PointerActivate(char)`;
  WM_MOUSEMOVE/LBUTTONDOWN/LBUTTONUP consts; `POINTER_HUD_ACTIVATION` +
  `publish_pointer_hud_activation` (beside `publish_bound_specials`);
  Space-down `pointer::on_space_down()`; Space-up armed-consume; watchdog
  `reset_on_eviction()`; `ms_hook_proc` new branches.
* `src-tauri/src/engine/mod.rs` — `PointerActivate` dispatch arm.
* `src-tauri/src/commands.rs` — `publish_hud_chips`.
* `src-tauri/src/guide_hud/mod_impl.rs` — `publish_keys` on show,
  `clear_chips` on hide.
* `src-tauri/src/config/schema.rs` — field + Default + both-path tests.
* `src-tauri/src/config/mod.rs` — publish in `save()`.
* `src-tauri/src/lib.rs` — startup seed, poller start (setup 8c), command
  registration.
* `src/types.ts`, `src/components/controls.ts`,
  `src/components/settings-panel.ts`, `src/preview.ts` — the toggle.
* NOT TOUCHED (another agent's in-flight work, by instruction):
  `src/components/toast.ts`, `src/styles/overlay-earthy.css` — the frontend
  half (chip publish, `hud-pointer` listener, `.armed` highlight) lands
  there separately; `publish_hud_chips` matches its already-written contract.

### How it was verified

* `cargo test --lib`: **153 passed, 0 failed, 3 ignored** (14 new tests:
  halo-boundary containment incl. one-px-past misses, outside-every-halo →
  none, nearest-centre tie-break, CSS↔physical round-trip within 1 physical
  px at dpr 1.5, arm-sets-abort + drift-out-clears, keep-leaves-foreign-abort,
  refuse-arming-into-aborted-hold, travel boundary at 23/24px, dwell boundary
  at 147/150ms, fly-through non-event, hud-hide/wheel keep, new-hold reset,
  stale-cursor never arms, publish bounding).
* `cargo check --lib`: 0 errors, 0 warnings.
* `npx tsc --noEmit`: 0 errors (repo-wide, toast.ts included, at the time of
  this run).
* **The gesture itself is UNVERIFIED** — it needs a build, an install and a
  hand. Untested on real hardware: arming feel, thresholds, multi-monitor
  physical-px correctness, click suppression against a real app underneath.

### Generalise this

1. **When a flag has multiple writers, a "clear" must know whose value it is
   clearing.** `SPACE_ABORTED` is written by rollover, combos, the wheel and
   now arming; the disarm path may only clear the abort IT set, which is why
   the tracker carries a disarm REASON instead of a boolean.
2. **When two threads share a two-part state (flag + index), choose each
   write order so the observable race resolves to the quiet failure** — and
   write the chosen order down at the write site, or the next refactor will
   "simplify" it into the loud one.
3. **A poller that owns the thinking keeps the callback honest.** Any time a
   hook callback "just needs a little geometry", the answer is the
   exclusions.rs shape: atomics out, named poller, decisions off-thread.

---

## PROBLEM 207 — two bindings on one browser were ONE binding: window matching had no notion of a profile

**Symptom.** The owner, 2026-08-26, in his words:

> "space b launching ONE PROFILE, BUT I FIXED SPACE N ANOTHER PROFILE, BUT
> SPACE N MINIMIZED THE PROFILE OF SPACE B"

His live `%APPDATA%\Spaceadom\debug.log` caught it exactly, and this is the
best evidence in the entry:

```
23:44:07.905  Target: ...\brave.exe | HWND: HWND(0x40966) | Action: Restore (Enum)
23:44:10.136  Target: ...\brave.exe | HWND: HWND(0x40966) | Action: Minimize
23:44:11.897  Target: ...\brave.exe | HWND: HWND(0x40966) | Action: Restore
```

Identical target string. **IDENTICAL HWND.** Two different keys, bound to two
different Brave profiles, taking turns on one window. And Space+N never
launched at all: `try_focus_or_minimize` returned `true` before
`launch_binding_app` was ever reached, so Space+N's profile could not get a
window of its own for as long as Space+B's window existed. There was no state
in which the bug could self-correct.

Read the middle line again — `Action: Minimize` with **no `(Enum)` suffix**.
That is a CACHE HIT: no enumeration, no matcher, no opportunity for any window
filter to have an opinion. It matters for the fix.

### Root cause

`smart_cascade.rs`'s entire notion of a binding's identity was **the exe file
stem**. Two bindings on `brave.exe` are the same string, so:

* they produced the same HWND cache key, and
* on a cache miss, `EnumWindows` returned whichever visible titled Brave window
  it reached first.

Everything downstream was correct. The identity at the top was wrong.

### What was measured and RULED OUT — do not retry these

This is the part that stops the next person losing a day. Both were tested on
this machine, 2026-08-26/27, and both are dead ends. The record lives in the
source at `src-tauri/src/browser_profiles.rs:340-412`.

* **Command-line inspection is dead.** Brave runs exactly ONE browser process
  (PID 30744) whose command line contains **no `--profile-directory` at all**,
  yet it owns a Profile 1 window. Chrome the same (PID 46752). One browser
  process per user-data-dir hosts EVERY profile. Corroborated independently:
  the Chromium singleton lockfile is at `…\User Data\lockfile`, **not**
  per-profile. `Win32_Process` matching cannot distinguish profiles at all.
* **Window titles are dead.** Measured on two NON-DEFAULT profiles:
  `"Toxic: A Fairy Tale… - Brave"` and `"Best VPN Online… - Google Chrome"`.
  Plain `<tab title> - <Browser>`. No profile marker anywhere.
* **Class names are dead.** `Chrome_WidgetWin_1` for both. No suffix.

### What DOES work

Chromium stamps the profile on **every browser window individually**, in that
window's own property store (fmtid `{9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}`,
pid 2 = `PKEY_AppUserModel_RelaunchCommand`, pid 5 = `PKEY_AppUserModel_ID`):

```
HWND 0x40966  AUMID    Brave.UserData.Profile1
              Relaunch "...\brave.exe" --profile-directory="Profile 1"
HWND 0x170978 AUMID    Chrome.UserData.Profile6
              Relaunch "...\chrome.exe" --profile-directory="Profile 6"
```

### NEWLY MEASURED DATA THAT CHANGED THE CODE

This is the most valuable part of the entry. Measured 2026-08-27 by
`live_profile_probe` — i.e. by THIS crate, natively, not through PowerShell —
against live windows:

```
chrome  ->  Named("Profile 6")
   AUMID    Chrome.UserData.Profile6
   Relaunch "…\chrome.exe" --profile-directory="Profile 6"     <- QUOTED

msedge  ->  Named("Default")
   AUMID    MSEdge                                              <- NO profile suffix at all
   Relaunch "…\msedge.exe" --profile-directory=Default          <- UNQUOTED
```

Two things nobody could have guessed, and either one would have failed
**silently**, on the common case, forever:

1. **A Default-profile window's AUMID carries no profile component at all.**
   `MSEdge`, bare. So AUMID corroboration DOES NOT EXIST for the profile most
   people use — the relaunch command is the only evidence there is. A design
   that required both properties to agree would refuse every single-profile
   browser window.
2. **Chrome QUOTES the folder and Edge does NOT.** Chromium quotes anything
   containing a space (`"Profile 6"`) and leaves a bare token alone
   (`Default`). A parser that only understood the quoted form would return
   nothing on every Edge window, and every Edge binding would relaunch instead
   of toggling, with no error and no log line.

There is a third trap in the same data: **the two properties disagree on
spelling.** The RelaunchCommand carries the literal folder name WITH its space
(`Profile 1`); the AUMID STRIPS it (`Profile1`). Comparing one against the
other verbatim matches nothing, forever, silently. That is the only reason
`same_profile_dir` exists.

Measured cost: 0.026 ms per property read (200 reads in 5.2 ms through
PowerShell COM interop, so an upper bound — the native path is faster), i.e.
~0.05 ms per candidate window, and only for bindings that actually pin
something.

### The fix, in one sentence

**Identity became (exe stem + pinned profile), and a window may only be
touched when it PROVES it belongs.**

### Exact file: `src-tauri/src/engine/actions/smart_cascade.rs`

The rule the whole design hangs on, stated in-source at lines 49-56:

```rust
// THE ONE RULE THAT MATTERS MOST, and it is the reason NATIVE_SAFETY.md exists:
// when we cannot PROVE a window belongs to this binding, we do not match it.
// Falling through to `launch_binding_app` launches with `--profile-directory=`
// and Chromium itself turns that into a focus of the correct window, so the
// cost of being wrong in that direction is nil. The cost of being wrong in the
// other direction is minimising a window the user did not aim at - which is the
// class of bug that once broke this owner's touchpad.
```

**`ProfileRule` — what a binding will accept as "its" window (line 60).**

```rust
enum ProfileRule {
    /// No profile discrimination at all.
    Any,
    /// This binding pins a Chromium profile folder. Only a window that PROVES
    /// it belongs to that profile may be touched.
    Pinned(String),
    /// This binding pins nothing, but other bindings the user can reach right
    /// now pin profiles of THIS browser.
    Unpinned(Vec<String>),
}
```

`Any` is load-bearing and is what makes this safe to ship: every binding that
existed before browser profiles, every non-browser binding, and every browser
binding on a machine where nothing pins a profile of that browser lands there —
and on that arm the matcher does exactly what it did before, **down to reading
no window properties and initialising no COM apartment.** The new code is not
on the path at all unless the user has actually pinned something.

**`WindowProfile` — what a WINDOW says about itself (line 88).**

```rust
enum WindowProfile {
    /// A profile folder was read off this window. The ONLY variant that can
    /// ever authorise a minimise.
    Named(String),
    /// The window carries a Chromium relaunch command that contains no
    /// `--profile-directory` switch at all.  STILL NEVER OBSERVED.
    NoProfileNamed,
    /// Nothing usable: no property store, no relaunch command, an unreadable
    /// `--profile-directory`, or two properties that contradict each other.
    Unknown,
}
```

`NoProfileNamed` is the variant that exists **because the Default case turned
out not to look like it**. It is produced only when the switch is genuinely
absent; a switch that is present but unparseable produces `Unknown`. That
separation is what stops a parser miss from ever being mistaken for a
default-profile window.

**`evidence_satisfies` — the decision (line 116).** Pure, and tested, because
every branch is one a user only reaches after something has already gone
ambiguous, and a wrong answer does not crash — it silently minimises the wrong
window.

```rust
fn evidence_satisfies(evidence: &WindowProfile, rule: &ProfileRule) -> bool {
    use crate::browser_profiles::same_profile_dir;
    match rule {
        ProfileRule::Any => true,

        // FAIL SAFE. Missing, unreadable, or a different profile - all three
        // mean "not proven mine", and all three fall through to launch.
        ProfileRule::Pinned(want) => match evidence {
            WindowProfile::Named(got) => same_profile_dir(got, want),
            WindowProfile::NoProfileNamed | WindowProfile::Unknown => false,
        },

        ProfileRule::Unpinned(claimed) => match evidence {
            // The owner's rule, literally: anything not claimed by someone else.
            WindowProfile::Named(got) => {
                !claimed.iter().any(|c| same_profile_dir(got, c))
            }
            WindowProfile::NoProfileNamed => {
                !claimed.iter().any(|c| same_profile_dir(c, "Default"))
            }
            WindowProfile::Unknown => false,
        },
    }
}
```

The `NoProfileNamed` arm is **the one reasoned step in the matcher** rather
than a measured one, and it is deliberately the least load-bearing: every
window measured so far, default profile included, arrives as `Named`, so the
arm has never been taken. It exists so a browser that DOES omit the switch
degrades to "toggles normally" instead of "never toggles".

**`cache_key` — the branch the reported bug actually fired on (line 168).**

```rust
fn cache_key(exe_stem: &str, rule: &ProfileRule) -> String {
    match rule {
        ProfileRule::Pinned(dir) => format!(
            "{exe_stem}|{}",
            crate::browser_profiles::normalise_profile_dir(dir)
        ),
        ProfileRule::Any | ProfileRule::Unpinned(_) => format!("{exe_stem}|*"),
    }
}
```

So `"brave|profile1"` and `"brave|*"` — and `|` cannot appear in a Windows
file name, so an unpinned key can never collide with a pinned one or with a
bare stem. The pinned arm normalises through the SAME function the window
comparison uses, so "Profile 1" and "Profile1" are one cache entry rather than
two pointing at one window. ONE function, called from the read site and the
write site both (`try_focus_or_minimize:2188`), because two sites deriving the
same key separately is how the original bug walks back in with no visible
cause.

**The cache re-validation, `try_focus_or_minimize:2224-2237`** — this is the
line the owner's 23:44:10.136 log entry needed:

```rust
            // NATIVE_SAFETY.md rule 3 — "never trust a cached HWND" — extended
            // from the class check to the profile.
            let wrong_profile = alive
                && !matches!(rule, ProfileRule::Any)
                && !evidence_satisfies(&window_profile_evidence(hwnd), rule);

            if cached_unsafe || wrong_profile {
                if wrong_profile {
                    log::info!(
                        "cascade: cached {hwnd:?} for {key:?} can no longer prove it belongs \
                         to this binding — dropping it and enumerating again"
                    );
                }
                cache.remove(&key);
            } else if alive {
```

A perfect window matcher alone would still have been defeated by this bug,
because the cache-hit branch never re-enumerates. Re-proving the profile on
every hit is what makes a recycled HWND, a window closed and reopened in
another profile, or a profile switched inside the browser fail SAFE.

**The enumeration gate, `enum_callback:2453-2462`:**

```rust
        if !matches!(payload.rule, ProfileRule::Any) {
            let evidence = window_profile_evidence(hwnd);
            if !evidence_satisfies(&evidence, &payload.rule) {
                payload.declined.push(format!("{hwnd:?} {evidence:?}"));
                return BOOL(1);
            }
        }

        payload.found = Some(hwnd);
        return BOOL(0); // stop enumeration
```

Note the `BOOL(1)`: a window that is not this binding's **continues** the
enumeration instead of ending it. Stopping there would mean the right window
sitting behind the wrong one is never reached — which is exactly what the
owner's unpinned rule requires (B takes whichever Brave window is not N's).

`StemSearch` (line 2333) is shared by `try_focus_or_minimize` (which toggles)
and `find_window_by_exe_stem` (which only ever finds), so the two cannot
disagree about which window belongs to a binding. **They did disagree, in a
way that was hard to see:** a CORRECT launch into Profile 2 could be followed
by the post-launch watcher raising Profile 1's window, because the raise went
through the same profile-blind callback.

**`window_string_property` — factored out, line 380.**

```rust
unsafe fn window_string_property(
    hwnd: windows::Win32::Foundation::HWND,
    pkey: &windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY,
) -> Option<String> {
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
    use windows::Win32::UI::Shell::PropertiesSystem::{
        IPropertyStore, SHGetPropertyStoreForWindow,
    };

    let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
    let pv = store.GetValue(pkey).ok()?;
    let pwstr = PropVariantToStringAlloc(&pv).ok()?;
    let value = pwstr.to_string().unwrap_or_default();
    windows::Win32::System::Com::CoTaskMemFree(Some(pwstr.0 as *mut _));
    Some(value)
}
```

This was pulled out of `aumid_focus_or_minimize`'s inline callback sequence
(now calling it at line 1676) rather than written a second time, so **there is
exactly ONE `SHGetPropertyStoreForWindow` / `GetValue` /
`PropVariantToStringAlloc` / `CoTaskMemFree` sequence in the app**, and both
the Store/UWP matcher and the browser-profile matcher go through it. It
returns the string in its ORIGINAL case — every comparison normalises, and
lowercasing here would throw away the literal folder name the launch leg needs
to stay diff-able against. A failure at any step is `None`: one window's
unreadable property must never abort an enumeration (PROBLEM 79's rule). The
`PROPVARIANT` frees itself on drop (windows-core 0.58 implements `Drop`); only
the `PropVariantToStringAlloc` buffer is ours to free.

**`window_profile_evidence` — reading the two properties (line 417).** The
shape that matters:

```rust
    // The switch is THERE but no value could be read out of it. Refuse, rather
    // than fall through to NoProfileNamed.
    if from_relaunch.is_none() && relaunch.to_ascii_lowercase().contains("--profile-directory")
    {
        return WindowProfile::Unknown;
    }
    ...
    match (from_relaunch, from_aumid) {
        // Two properties of the same window disagreeing has never been seen.
        // If it ever happens, contradictory evidence is not evidence.
        (Some(rc), Some(au)) if !same_profile_dir(&rc, &au) => {
            log::warn!(
                "profile_match: {hwnd:?} contradicts itself - RelaunchCommand says {rc:?}, \
                 AUMID says {au:?}. Treating it as unidentified and leaving it alone."
            );
            WindowProfile::Unknown
        }
        (Some(rc), _) => WindowProfile::Named(rc),
        (None, Some(au)) => WindowProfile::Named(au),
        (None, None) if !relaunch.trim().is_empty() => WindowProfile::NoProfileNamed,
        _ => WindowProfile::Unknown,
    }
```

The `(None, Some(au))` arm is what rescues a window whose relaunch command
omits the switch but whose AUMID still names the profile. The
`(Some(rc), Some(au))` disagreement arm is where "contradictory evidence is not
evidence" is implemented.

**COM is paid for only when it is used (`ComGuard::for_rule`, line 347):**

```rust
    unsafe fn for_rule(rule: &ProfileRule) -> Option<Self> {
        match rule {
            ProfileRule::Any => None,
            _ => Some(ComGuard::new()),
        }
    }
```

Uninitialised only when OUR init was the one that took: an `RPC_E_CHANGED_MODE`
means another apartment already owns the thread and `CoUninitialize` would
decrement somebody else's count.

### The parsers (`src-tauri/src/browser_profiles.rs`)

`profile_dir_from_relaunch_command` (line 437) is the PRIMARY evidence reader,
and the unquoted branch is not defensive padding — it is the Edge measurement
above:

```rust
pub fn profile_dir_from_relaunch_command(cmd: &str) -> Option<String> {
    const FLAG: &str = "--profile-directory=";
    // ASCII-lowercase, never `to_lowercase()`: a browser installed under a
    // path containing non-ASCII characters can change BYTE LENGTH under a
    // Unicode lowercase, and every index taken from the lowered copy would
    // then point at the wrong place in the original.
    let hay = cmd.to_ascii_lowercase();
    let at = hay.find(FLAG)? + FLAG.len();
    let rest = &cmd[at..];
    let value = if let Some(inner) = rest.strip_prefix('"') {
        // Measured shape: --profile-directory="Profile 1"
        inner.split('"').next().unwrap_or("")
    } else {
        // Unquoted. Chromium quotes anything containing a space, so an
        // unquoted value is a single whitespace-delimited token by
        // construction.
        rest.split_whitespace().next().unwrap_or("")
    };
    let value = value.trim();
    if value.is_empty() { None } else { Some(value.to_string()) }
}
```

`None` means "this command line NAMES no profile", which is emphatically not
"this window has no profile". Callers must treat `None` as *unproven*, never as
*proven absent*.

`profile_token_from_aumid` (line 478) parses only the measured
`<browser>.UserData.<profile>` shape, and `None` is an ordinary answer, not a
failure — that is the bare `MSEdge` case.

`same_profile_dir` / `normalise_profile_dir` (lines 506 / 516) are the whole
answer to the spelling trap. An empty name never matches anything, including
another empty one: "we read nothing" must not compare equal to "we read
nothing".

### The claims plumbing (the owner's unpinned rule)

`browser_profiles::profile_claims` (line 538) collects the
`(browser exe stem, profile folder)` pairs that bindings have PINNED;
`active_profile_claims` (line 618) walks the ACTIVE profile's bindings plus the
special keys. `engine/mod.rs:377` and `:455` read it **inside the read guard
the binding lookup already holds open**, so it costs no extra lock and no I/O
on the Space-hold latency path, and it is handed to `smart_cascade(binding,
fallback, claims, app_handle)` (`smart_cascade.rs:486`).

For everyone who pins nothing it visits ~26 bindings, allocates nothing, and
returns an empty Vec — and an empty claim list means the cascade takes the
byte-for-byte pre-2026-08-27 path. A cached claim set was considered and
rejected: a stale cache here means acting on a claim that no longer exists,
which is the same class of bug as the one being fixed.

**The one boundary, stated so nobody rediscovers it:** the FOUNDERS profile's
bindings do NOT claim. A key left unassigned in the active profile falls
through to its Founders binding, so a Founders pin is technically reachable.
The case is narrow and the owner's rule says "the active profile"; if it ever
bites, one more `.chain()` in `active_profile_claims` is the whole fix.

### THE INVARIANT the review settled on

> **The profile the match leg demands is exactly the `--profile-directory` the
> launch leg is about to pass, and nothing when the launch will pass none.**

It is not a comment — it is a named pure function per leg so it can be
asserted in a test rather than trusted, and a test that derives the launch
side independently and compares (`the_url_match_leg_demands_exactly_what_the_launch_leg_passes`,
line 3481, six cases from "no browser, no profile" to "uninstalled browser").

**It caught three defects.** All three had shipped or been about to ship, and
all three looked reasonable in isolation.

**Defect 1 — a stale pin left the key permanently broken**
(`rule_for_binding`, line 234; the note is at 208-234). The match leg read
`browser_profile_dir` raw and handed back `Pinned(dir)` whatever the filesystem
said, while the launch leg ran the same value through `profile_arg_for` and
DROPPED it when the folder was gone. So a user who deletes a pinned profile
from inside the browser got a key that could never match again: press 1
launched Brave without the switch (correctly), presses 2, 3, 4 … launched
ANOTHER one, plus a toast each time, because the match leg was still demanding
a profile no living window could ever have. That is CORE_AIM's "Smart Cascade"
and "Cyclic Reliability" broken permanently for that binding. The fix is that
the single decision function both legs already had is now asked by both legs:

```rust
        ProfileArg::Use(dir) => ProfileRule::Pinned(dir),
        // Nothing was pinned in the first place.
        ProfileArg::None => claims_rule(&stem, claims),
        // Pinned, and the folder is gone. The launch drops the switch; the
        // match must drop the demand, or the key never toggles again.
        ProfileArg::DroppedStale { .. } => claims_rule(&stem, claims),
```

A stale pin degrades the binding to exactly what it now is — an UNPINNED
binding on that browser, which still declines windows another binding has
claimed. Note that `profile_arg_for`'s asymmetric polarity carries straight
through, which is the point of reusing it: a profile that could not be
VERIFIED (Arc's MSIX alias has no derivable `User Data` path) is still `Use`,
so it is still `Pinned` here. Only a positively located directory that
positively lacks the folder is evidence.

**Defect 2 — the match leg declining the very window its own launch would land
in** (`url_rule_for_route`, line 293; the note is at 1970-1990). This is the
violation the brief calls out, and it is the worst of the three. The first
draft applied `claims_rule` on every arm of the URL leg. Two of those three
arms launch through `run_browser`, which passes NO profile switch and
deliberately raises with `ProfileRule::Any` — so a claim could make the match
leg DECLINE the very window the launch would land in, and match and launch
could then never agree. **Unbounded duplicate tabs, on every press, forever,
with no minimise half.** The owner's machine is the worst case for it: Brave is
his default browser AND the browser he pins profiles of, so a plain
`youtube.com` key that pins nothing at all would have declined his own YouTube
window. That is the exact bug `url_focus_or_minimize` was written on
2026-08-11 to prevent.

```rust
        BrowserRoute::Specific { exe } => match browser_profiles::profile_arg_for(...) {
            // The launch will pass `--profile-directory=d`, so only that
            // profile's window is this key's.
            ProfileArg::Use(d) => ProfileRule::Pinned(d),
            ProfileArg::None | ProfileArg::DroppedStale { .. } => ProfileRule::Any,
        },
        BrowserRoute::Default | BrowserRoute::PinnedBrowserMissing { .. } => ProfileRule::Any,
```

**Defect 3 — the post-launch RAISE asking the match question**
(`raise_rule_for_launch`, line 636; the note is at 609-635). Conflating two
questions that look identical:

* the MATCH rule asks *may this key minimise this window?* — a wrong **yes** is
  the reported bug, so it must be provable-only;
* the RAISE rule asks *which window did the launch I just performed create?* —
  a wrong **no** means refusing to raise the window we just opened, and the
  user watches their browser come up behind whatever they were looking at
  (PROBLEM 170's symptom, back again).

A stale-dropped pin went through `claims_rule`, which can decline the window
the launch just made because a DIFFERENT binding pins that profile — and then
`raise_after_launch` polls for `GIVE_UP_MS = 8_000` and raises nothing.

```rust
fn raise_rule_for_launch(
    passed_profile: Option<&str>,
    pin_dropped_as_stale: bool,
    unpinned: &ProfileRule,
) -> ProfileRule {
    match (passed_profile, pin_dropped_as_stale) {
        (Some(d), _) => ProfileRule::Pinned(d.to_string()),
        (None, true) => ProfileRule::Any,
        (None, false) => unpinned.clone(),
    }
}
```

One function for both legs, for the same reason `cache_key` is one function.
`raise_after_launch`'s give-up line now names the rule, so "nothing was raised
on purpose" is distinguishable from "it was slower than 8s".

### A DECISION, not a bug: the unpinned rule is APP-LEG ONLY

The owner's rule — *an unpinned binding must not steal a pinned binding's
window* — is applied to the APP leg and NOT the URL leg. This will read as
drift the first time someone diffs the two, so:

* The URL leg launches via `run_browser` with **no `--profile-directory`** and
  therefore CANNOT honour the exclusion. Enforcing it there could only ever
  refuse the right window and open another tab.
* The two legs also have different candidate sets. The URL matcher has a
  second, per-binding discriminator the app leg does not: **the SITE**. A
  candidate must already carry this binding's own keyword or host in its title,
  so the worst `Any` can do there is toggle a window showing this very key's own
  website — which is the behaviour that shipped in 1.0.86 and which the owner
  uses daily. `try_focus_or_minimize` has no such filter: EVERY window of the
  executable is a candidate, which is precisely the gap the unpinned rule was
  decided to close.

### The live probe — how to run it

`#[ignore]`d, read-only, and it puts the **production** matcher against live
windows. It touches nothing: no `ShowWindow`, no `SetForegroundWindow`, no
config — it enumerates and reads properties only, so it is safe to run against
the owner's live session while he is working.

```
cargo test --lib -- --ignored --nocapture live_window_profiles
```

(from `src-tauri`, with the `CARGO_HOME`/`RUSTUP_HOME`/`PATH` exports in
CLAUDE.md §Build.) It lives at `smart_cascade.rs:3829` in
`mod live_profile_probe`. It prints, per window, the stem, the title, the RAW
AUMID, the RAW RelaunchCommand and what the matcher made of them — the raw pair
is printed because a future reader needs the SHAPE, not only our reading of it.
That is how the Default-profile question got answered.

It then asserts four things against those same live windows:

```rust
            assert!(
                find_window_by_exe_stem(stem, &ProfileRule::Pinned(dir.clone())).is_some(),
                "pinning {dir:?} found no {stem} window, but one is open right now"
            );

            // The AUMID spelling of the SAME profile ("Profile1" for
            // "Profile 1"). If normalisation is wrong this finds nothing, and
            // nothing else in the app would ever say so.
            let squashed = dir.replace(' ', "");
            assert!(
                find_window_by_exe_stem(stem, &ProfileRule::Pinned(squashed.clone())).is_some(),
                "the space-stripped spelling {squashed:?} matched nothing — the AUMID trap is live"
            );

            // And the fail-safe: a profile nobody has must match nothing at all.
            assert!(
                find_window_by_exe_stem(stem, &ProfileRule::Pinned("Profile 4242".into()))
                    .is_none(),
                "a profile that does not exist matched a {stem} window — the fail-safe is broken"
            );
```

**Why it exists.** Everything the matcher knows was measured through PowerShell
COM interop. That proves the PROPERTIES are there; it does not prove this crate
reads them correctly, and "the property store returned nothing" is
indistinguishable from "no window matched" in every log line we have. This
closes that gap **without an install**.

### Exact files

* `src-tauri/src/engine/actions/smart_cascade.rs` — `ProfileRule` (60),
  `WindowProfile` (88), `evidence_satisfies` (116), `cache_key` (168),
  `claims_rule` (188), `rule_for_binding` (234), `url_rule_for_route` (293),
  `ComGuard::for_rule` (347), `window_string_property` (380),
  `PKEY_APPUSERMODEL_FMTID` (404), `window_profile_evidence` (417),
  `smart_cascade` app leg (486-520), `raise_rule_for_launch` (636),
  `launch_binding_app` (708), `raise_after_launch` (1191),
  `find_window_by_exe_stem` (1294), `aumid_focus_or_minimize` now calling the
  shared reader (1676), `try_focus_or_minimize` (2177) with the cache
  re-validation (2224) and the declined-window log (2302), `StemSearch` (2333),
  `enum_callback`'s profile gate (2453), `run_browser`'s deliberate
  `ProfileRule::Any` raise (2715).
* `src-tauri/src/browser_profiles.rs` — the measurement record (340-412),
  `profile_dir_from_relaunch_command` (437), `profile_token_from_aumid` (478),
  `same_profile_dir` (506), `normalise_profile_dir` (516), `profile_claims`
  (538), `active_profile_claims` (618).
* `src-tauri/src/engine/mod.rs` — `active_profile_claims` read inside the
  existing config read guard (377, 455) and passed to `smart_cascade`
  (410, 517).

### How it was verified

* **The live probe above**, run natively against the owner's open browsers on
  2026-08-27 — that is where the Chrome/Edge table came from, and it is the only
  reason the unquoted-`Default` and bare-`MSEdge` cases are handled at all.
* **Unit tests in `smart_cascade.rs`**, including
  `the_reported_incident_no_longer_reproduces` (line 3783), which reconstructs
  the owner's two Brave bindings and asserts both halves:

```rust
        // Half 1: they are no longer the same cache entry.
        assert_ne!(cache_key("brave", &b_rule), cache_key("brave", &n_rule));

        // Half 2: they no longer both accept the same window.
        let profile_1_window = WindowProfile::Named("Profile 1".into());
        assert!(evidence_satisfies(&profile_1_window, &n_rule));
        assert!(
            !evidence_satisfies(&profile_1_window, &b_rule),
            "Space+B must not take Space+N's window — it falls through to launch, \
             and Brave itself puts the right window in front"
        );
```

  plus `a_pin_whose_folder_was_deleted_stops_demanding_it` (3309),
  `a_stale_pin_degrades_to_unpinned_not_to_anything_goes` (3327),
  `an_unverifiable_profile_stays_pinned` (3348),
  `the_url_match_leg_demands_exactly_what_the_launch_leg_passes` (3481),
  `the_url_leg_and_the_app_leg_disagree_on_purpose` (3665),
  `a_pinned_binding_matches_its_own_profile_in_either_spelling` (3699),
  `a_window_naming_no_profile_is_free_unless_default_itself_is_claimed` (3755).
* **NOT VERIFIED, and this is the important half.** Nobody has pressed Space+B
  and Space+N on two Brave profiles since the fix. Nobody has deleted a real
  profile folder to exercise the stale-pin path. Neither can be reached by an
  automated test — both need a build, an install, and a hand on the keyboard.
  Treat the fix as *implemented and unit-tested*, not as *working*, until the
  owner has pressed the two keys.

### Generalise this

1. **Identity for a matcher must be the thing the ACTION will target, not the
   thing that is easy to compare.** An exe stem was easy. What the action
   actually targets is a window belonging to a specific profile, and the whole
   bug is the distance between those two.
2. **Contradictory evidence is not evidence.** When two sources about the same
   object disagree, the matcher returns `Unknown` and declines rather than
   picking a side. The cost of declining is one relaunch; the cost of picking
   wrong is acting on something the user did not aim at.
3. **When two legs of one feature answer the same-looking question, name the
   invariant and make it a function.** All three defects here came from a
   correct-looking inline arm. A pure function per leg lets the invariant be
   asserted by a test instead of maintained by attention.
4. **Nothing about this bug is browser-specific** — deliberately NOT
   generalised in this pass, and the reason is recorded at
   `browser_profiles.rs:405-412`. Two `.lnk` shortcuts with different
   arguments, two Windows Terminal profiles, two VS Code workspaces and two
   Store apps sharing a package family all collide the same way, because they
   all reduce to one exe stem. The general fix is these same three pieces — a
   binding-identity cache key, per-window evidence, and "no proof means do not
   touch it" — with a different evidence reader per family. Widening the scope
   inside the change that fixes the browser case is what would put the cascade
   at risk.
5. **A read-only `#[ignore]`d live probe is worth its weight** when the thing
   under test is the OS. It converts "the property store returned nothing" and
   "no window matched" — which every log line conflates — into two different
   assertion failures, without an install.

---

## PROBLEM 208 — the Guide HUD ring did not grow with its contents: 34 chips distributed perfectly around an 8-chip ellipse

**Symptom.** The owner, on his own config:

> "ensure proper spacing among the 26 letters, make sure the spacings
> automatically adapt to fulfil the ellipse and the names to look good."

Chips overlapping each other on the outer ring while he holds Space.

### Root cause

In `src/components/toast.ts`, the ring's SIZE was decoupled from the item
COUNT:

* `rin`/`rout` were derived from the **single widest chip's half-width**
  (`Math.max(...)` over the measured widths), and
* `ryi = 118` / `ryo = 196` were **hard constants**.

Nothing anywhere grew the ellipse as more keys were bound. The ring for 8 apps
and the ring for 26 apps was the same ring.

### RECORD EXPLICITLY, so nobody re-solves it: the DISTRIBUTION was already correct

`arcAngles()` (line 1042, labelled **PROBLEM 77** in-source) numerically
integrates the ellipse and places chips proportional to their MEASURED DOM
widths — that half has been right since PROBLEM 77 and must not be touched:

```js
function arcAngles(
  ws: number[], rx: number, ry: number, gap: number, off: number,
): number[] {
  const N = ARC_N;
  const cum = arcTable(rx, ry);
  const total = cum[N];
  const shares = ws.map((w) => w + gap);
  const tot = shares.reduce((s, w) => s + w, 0) || 1;
  // arc-length position of each chip centre → invert to the parameter angle
  let acc = 0;
  return shares.map((w) => { ... });
}
```

Given an impossible budget, proportional distribution is the only honest thing
it can do: **every chip got a share SMALLER than its own width, so neighbours
were placed closer together than they are wide.** That is the overlap he saw.
The bug was SIZING, not distribution.

### Measured, on his real config (all 26 letters bound + 8 fixed specials = 34 chips)

| | before | after |
|---|---|---|
| outer rim vs required | 2061 vs 3014 px (146% over) | 3066 vs 3066 |
| overlapping pairs | 5 | 0 |
| window asked of `overlay_fit_hud` | 1200x572 | 1604x733 |

The five real collisions, by label pair:

```
illustrator x intellij idea
reddit      x spotify
utorrent    x vlc
vlc         x whatsapp
whatsapp    x x
```

About 50% more content than rim, which is why the failure was so consistent
along one arc of the ring rather than random.

### The fix — a four-rung ladder, in the owner's stated order of preference

Each rung is only reached if the one before it could not make the ring fit,
and **a rung is accepted only when the MEASURED overlap count is zero.**

```js
  const LADDER: { step: HudFit["step"]; gap: number; dense: boolean; tight: boolean }[] = [
    { step: "a", gap: RING_GAP_PREF, dense: false, tight: false }, // grow only
    { step: "b", gap: RING_GAP_MIN,  dense: false, tight: false }, // tighten the gap
    { step: "c", gap: RING_GAP_MIN,  dense: true,  tight: false }, // shrink the chips
    { step: "d", gap: RING_GAP_MIN,  dense: true,  tight: true  }, // truncate harder
  ];
  // A fresh build always starts from the top of the ladder — otherwise a HUD
  // that once needed rung (d) would wear its shrunken chips forever.
  _hudEl.classList.remove("dense-chips", "tight-chips");
  let out = layout("a", RING_GAP_PREF);
  for (let i = 1; i < LADDER.length && out.fit.overlaps > 0; i++) {
    const rung = LADDER[i];
    _hudEl.classList.toggle("dense-chips", rung.dense);
    _hudEl.classList.toggle("tight-chips", rung.tight);
    out = layout(rung.step, rung.gap);
  }
```

* **(a) GROW** the ring until the rim can hold the content, bounded by the
  screen budget (`SCREEN_BUDGET = 0.94`, which MUST match Rust's own clamp in
  `overlay_fit_hud` — if the request exceeds it Rust silently shrinks the
  window and the outermost chips are cropped).
* **(b) TIGHTEN the gap** from `RING_GAP_PREF = 16` toward `RING_GAP_MIN = 10`.
  Below 10 chips start touching in the diagonal quadrants, where a chip's
  HEIGHT stops protecting it.
* **(c) SHRINK the chips** — `.dense-chips`, ONE step, ~1px of padding and
  ~0.75px of font (`overlay-earthy.css:337-341`).
* **(d) TIGHTEN the labels** last — `.tight-chips`, the truncation cap 118px →
  92px and the chip cap 212 → 168 (`overlay-earthy.css:344-345`).

**At 34 chips only rung (a) triggered** on his 1707x1067 panel, with the gap
still at its preferred 16px. Rungs (b)-(d) are for smaller displays.

Keep (c) and (d) small deliberately: a HUD you read at a glance from the
middle of the screen stops working long before the text becomes technically
unreadable. If a config ever needs more than this, the right answer is a
SECOND PAGE of the ring, not smaller type.

**The growth itself (`growRing`, line 946)** is two phases and the order is
the point:

```js
function growRing(
  ws: number[], rx0: number, ry0: number, gap: number,
  maxRx: number, maxRy: number,
): { rx: number; ry: number; need: number; have: number } {
  const have0 = ellipsePerimeter(rx0, ry0);
  const need = ws.reduce((s, w) => s + w, 0) + ws.length * gap;
  let k = ws.length > 1 && need > have0 ? need / have0 : 1;
  k = Math.min(k, maxRx / rx0, maxRy / ry0);
  if (!(k > 1) || !isFinite(k)) k = 1;
  ...
```

*Phase 1 is UNIFORM.* The perimeter of an ellipse is linear in a uniform
scale, so the factor is solved in ONE SHOT (`k = required / available`) rather
than iterated. Uniform scaling preserves the ring's existing eccentricity
exactly, which is why **an 8-chip HUD comes out byte-identical to what shipped
before this change** — it needs no growth, so it gets none. It also leaves
`arcAngles`' parameter mapping unchanged, so the final screen clamp can still
be applied AFTER the angles are solved.

*Phase 2 runs only when phase 1 hit a bound and the rim is still short.* It
raises `ry` alone toward `RING_ASPECT_MAX * rx` (0.55, the ratio the design
brief names) and no further, buying rim length out of vertical budget phase 1
could not reach — a screen is usually wider than it is tall, so `maxRx` binds
first and leaves height on the table. Perimeter is monotone in `ry`, so a
20-iteration bisection (~1e-4 relative) is exact and cheap. It only ever makes
the ellipse ROUNDER, and never rounder than 0.55, so it can never become a
circle.

`growRing` **never shrinks**. The base radii (`RIN_CLEAR = 115` clearing the
230x60 SPACE pill, `RING_CLEAR = 26`, `RYI_BASE = 118`, `RYO_BASE = 196`) are
already the collision-safe minimum; shrinking below them trades one overlap for
a worse one.

`arcTable` is shared by `ellipsePerimeter` and `arcAngles` (line 905) **so the
length the ring is SIZED against and the length chips are PLACED along can
never disagree** — which is the same one-function discipline PROBLEM 207's
`cache_key` needed, for the same reason.

One more thing the growth forced: `ryo` used to be the constant 196 no matter
what `ryi` was. Once the INNER ring can grow, the outer one has to clear it
**vertically** as well as horizontally, or the two rings meet at the top and
bottom:

```js
    const rout0 = gi.rx + innerHalf + outerHalf + RING_CLEAR;
    const ryo0 = Math.max(RYO_BASE, gi.ry + (innerTall + outerTall) / 2 + RING_CLEAR);
```

Chip HEIGHT is measured for the same reason: on the LEFT and RIGHT flanks of
the ellipse neighbours stack vertically, and there it is the height, not the
width, that keeps them apart.

### The result is MEASURED, not reasoned

`overlapCount()` (line 997) runs the real rectangles against each other after
every rung. Overlap is the failure this exists to fix, so nothing is taken on
trust — 34 axis-aligned rects is 561 comparisons, once per HUD show,
microseconds.

```js
function overlapCount(boxes: ChipBox[]): number {
  let n = 0;
  for (let i = 0; i < boxes.length; i++) {
    for (let j = i + 1; j < boxes.length; j++) {
      const a = boxes[i], b = boxes[j];
      if (Math.abs(a.x - b.x) < (a.w + b.w) / 2 + 0.5 &&
          Math.abs(a.y - b.y) < (a.h + b.h) / 2 + 0.5) n++;
    }
  }
  return n;
}
```

The **0.5px slack leans TOWARD reporting an overlap, not away from it**:
widths come from `offsetWidth`, which rounds to whole pixels and can therefore
under-report a chip by up to half a pixel. Measured: against a strict
fractional-rect test in the browser it was the difference between "2 pairs" and
"4 pairs" on a deliberately-undersized screen.

The chosen geometry is exported as `lastHudFit(): HudFit | null` (line 1025) —
step, gap, all four radii, `outerRim`/`outerNeed`, `innerRim`/`innerNeed`,
overlap count, window size, screen size. Worth having in a bug report: "step d
at 34 chips" says the ring ran out of screen, not that the maths is wrong.

### HONEST LIMIT — logged with numbers rather than shipped silently

Below ~1152px wide with 26 letters the ladder EXHAUSTS and one pair still
intersects. 26 readable chips need ~2.6k px of rim; a 1152x648 display offers
~2.0-2.2k at rung (d). No arrangement avoids a collision at that point — the
only remaining answers are unreadable type or a second ring, and neither was
asked for.

```js
  if (geo.overlaps > 0 && _isOverlay) {
    invoke("overlay_log", {
      msg: `buildHud: ring EXHAUSTED at step ${geo.step} — ${geo.overlaps} chip pair(s) ` +
        `still intersect. ${apps.length} apps + ${specials.length} specials need ` +
        `${Math.round(geo.outerNeed)}px of outer rim; the largest ellipse this ` +
        `${geo.screen.w}x${geo.screen.h} screen allows gives ${Math.round(geo.outerRim)}px ` +
        `(rout=${Math.round(geo.rout)} ryo=${Math.round(geo.ryo)}). Not a layout fault: ` +
        `the content does not physically fit at a readable size.`,
    }).catch(() => {});
  }
```

This follows the same rule the window commands follow (CLAUDE.md §Window
rules): **anything that changes what the user can see must be visible in the
log.** A silently-overlapping ring is precisely the bug this work exists to
end. His own 1707x1067 panel clears it at rung (a), so this is a small-display
case, not his.

### The other half: the exit choreography was one curve for both directions

`#st-hud` carried a SINGLE `transition: … 220ms cubic-bezier(.4,0,1,1)` applied
to both directions. So the HUD **arrived** on an ease-IN curve — accelerating
away from rest, which reads as the ring being yanked on rather than blooming —
and left over exactly as long as it took to arrive. CLAUDE.md's design rules
are explicit: *exits run at ~65% of entrance time with `--ease-in`*.

`src/components/toast.ts:153-154`:

```js
const HUD_IN_MS  = 220;
const HUD_OUT_MS = 143;   // 65% of HUD_IN_MS — the design language's exit ratio
```

`src/styles/overlay-earthy.css:228-233`:

```css
#st-hud { position: fixed; inset: 0; z-index: 25; pointer-events: none;
  transition: opacity   var(--hud-in, 220ms) cubic-bezier(.2,.8,.2,1),
              transform var(--hud-in, 220ms) cubic-bezier(.2,.8,.2,1); }
#st-hud.hidden { opacity: 0; transform: scale(.93);
  transition: opacity   var(--hud-out, 143ms) cubic-bezier(.4,0,1,1),
              transform var(--hud-out, 143ms) cubic-bezier(.4,0,1,1); }
```

220ms ease-OUT in (`cubic-bezier(.2,.8,.2,1)`, the same curve `.pulse` already
uses), 143ms ease-IN out (`cubic-bezier(.4,0,1,1)`) = **65.0%, the house
ratio**. The two durations are HANDED IN from `showGuideHud` as custom
properties (`toast.ts:2227-2228`) rather than duplicated, because a comment
saying "MUST match" is not a mechanism:

```js
  _hudEl.style.setProperty("--hud-in", `${HUD_IN_MS}ms`);
  _hudEl.style.setProperty("--hud-out", `${HUD_OUT_MS}ms`);
```

The CSS literals are first-paint fallbacks only. **Change the constants, not
the CSS.**

**Why shortening the exit by 77ms is safe.** `HUD_OUT_MS` is depended on by
exactly two teardown timers, both in `hideGuideHud`, and both are "wait for the
fade, then tear down":

* the WARP branch: `HUD_OUT_MS + SLING_DOWN_MS + STAGGER * back.length` (2426)
* the plain branch: `HUD_OUT_MS + 20` (2573)

Neither is the handover grace. PROBLEM 135's grace is `SLING_HANDOVER_MS`
(1200ms) and `SPACE_GRACE_MS` (420ms), and both SLING branches `return` before
reaching these timers — so this shortens only the tail AFTER every deferral has
already run. `overlay_toasts_done` (the single terminal path that hides the
window) fires 77ms earlier on a plain release; nothing waits on it.

### A per-chip staggered exit was DECLINED, deliberately

Arithmetic, not taste. The exit window is 143ms. Spread over 26 outer chips
that is a **~1.3ms step — invisible.** Making it visible means running the exit
for 300ms+, which means raising `HUD_OUT_MS`, and `HUD_OUT_MS` is what BOTH
teardown timers count with — the same choreography PROBLEM 135 was fought over.
Lengthening the exit to win a stagger nobody asked for is not worth touching
that.

What DOES fit is **two beats**, at `overlay-earthy.css:435-440`:

```css
#st-hud.collapsing .st-chip.ap,
#st-hud.collapsing .st-chip.sp {
  opacity: 0; transform: translate(-50%, -50%) scale(.72);
  transition: opacity   110ms cubic-bezier(.4,0,1,1),
              transform 110ms cubic-bezier(.4,0,1,1); }
#st-hud.collapsing .st-chip.sp { transition-delay: 33ms; }
```

Outer ring folds first, inner ring follows 33ms behind, last chip gone at
`33 + 110 = 143ms` exactly. Reads as a collapse toward SPACE rather than a
block fade, costs nothing, leaves every teardown timer where it was.

`.collapsing` and not `#st-hud.hidden .st-chip`, on purpose: `.hidden` is ALSO
on the element during the BUILD phase of a show (deliberate — the window move
must happen while nothing is visible), so chips would be born collapsed and
then transition into place, fighting `st-bloom-in` for the entrance. `toast.ts`
adds `collapsing` only on the four real exit paths.

### Exact files

* `src/components/toast.ts` — the timing block and constants (126-154), the
  ring-sizing design note (829-869), the tuning constants (872-900),
  `arcTable`/`ellipsePerimeter` (905-921), `growRing` (946-980), `ChipBox` +
  `overlapCount` (982-1010), `HudFit` + `lastHudFit` (1012-1025), `arcAngles`
  (1042), `layout()` (1150) and the ladder (1247-1265), the exhaustion log
  (1268-1288),
  the CSS-variable handoff (2227-2228), the two teardown timers (2426, 2573).
* `src/styles/overlay-earthy.css` — the entrance/exit split (212-233),
  `.dense-chips` (337-341), `.tight-chips` (344-345), the declined-stagger note
  and the two-beat collapse (415-440), reduced-motion overrides (481-489).

### How it was verified

* **`overlapCount()` is the acceptance test, and it runs in production**, not
  only in a harness: every rung of the ladder is measured against the real
  rendered rectangles and only accepted at zero intersections.
* The before/after table above was produced by driving the real `layout()`
  against the owner's own 34-label set. **That harness is not in the tree** —
  the numbers are recorded here because they are the only surviving record of
  it. `lastHudFit()` is the supported way to get them again.
* **NOT VERIFIED: nobody has held Space and LOOKED at the new HUD.** CLAUDE.md
  is explicit that the overlay cannot be validated in a browser harness —
  *its failure mode lives in the OS compositor, not the page* — and PROBLEM 135
  is what that costs: three builds of animation work played out inside an
  invisible window while every in-page measurement reported perfect health,
  because a page cannot observe that its own window is hidden. Geometry that
  measures correctly is NOT evidence that anything was drawn. This needs a
  build, an install, and the owner holding Space.

### Generalise this

1. **A layout that distributes correctly can still be wrong if the container
   never grows.** Check the container against the CONTENT, not against the
   largest single item. `Math.max(...)` over the items is the tell: it answers
   "how wide is the widest?" when the question was "how much is there?".
2. **When the correct half and the broken half sit one layer apart, say which
   is which in the source.** PROBLEM 77's arc-length distribution was right and
   looked like the obvious suspect. Without the note at `toast.ts:833-838` the
   next reader re-solves the solved half.
3. **Measure the failure you are fixing, in production, every time.** A ladder
   whose rungs were chosen by reasoning would be untrustworthy the first time a
   label got longer. `overlapCount` makes the geometry self-checking, and its
   0.5px slack is biased toward the false POSITIVE because the cost of missing
   a real overlap is the reported bug.
4. **A timing constant that two other systems count with is not free to
   change** — and the way to keep that honest is to hand it to them
   (`--hud-out`) rather than to write "MUST match" in a comment.
5. **When a design ambition does not survive the arithmetic, write the
   arithmetic down and decline it.** "~1.3ms per chip is invisible" is a
   decision anyone can re-check; "we decided against a staggered exit" is not.

---

## PROBLEM 209 — "point to launch" made someone drive the cursor onto the app's name; it is 360°, so it should be the DIRECTION. Plus a switch for the specials ring

**⚠️ THIS ENTRY REVERSES PART OF PROBLEM 206. Read both.** PROBLEM 206's
guard 4 was CONTAINMENT (cursor inside a chip's rect + a 12px halo). The owner
rejected it on 2026-08-27 and it is now DIRECTIONAL SECTORS. If you are here
because the code "looks unsafe compared to what PROBLEM 206 documents", that is
the decision, not a regression — do not put containment back.

### Symptom — the owner, 2026-08-27, on the shipped 1.0.87 behaviour

> "A person shouldn't have to physically move on top of the name of the app to
> launch. It's 360 degrees, right? In different degrees there are different
> apps, and depending on which place the cursor is, if the direction from the
> Space to the app is there, it should launch that app."

He also asked for two defaults to change: **point-to-launch ON at first
install**, and a new **"Show special keys"** switch for the HUD's inner ring.

### Root cause — a safety decision taken against the owner's own wording

PROBLEM 206 records it plainly: his original request said *"moves the cursor
towards the listed [letter]"*, and the implementation deliberately did not
honour "towards". The reasoning was sound in isolation — nearest-by-angle
ALWAYS has an answer, so with no region meaning "nothing", every Space release
after a mouse twitch launches something. Containment gave a natural "nothing"
(outside every halo) and the argument stopped there.

What it cost is the feature. On a 26-app ring at 400px radius, containment
means a 60x30px target: the user has to LOOK at the ring, aim, and land. That
is not a Space-hold gesture, it is a menu. The owner has now reaffirmed the
directional model twice.

**The fix is not "remove the guard". It is "move it".** Direction still needs a
region that means nothing armed, and that region is now a **dead zone** — a
circle around the HUD centre, sized to the apps ring's own inner edge. Inside
it nothing arms and `SPACE_ABORTED` is cleared, so releasing Space types a
space exactly as it always has. Containment was one guard; the dead zone is
the same guard relocated from "on the target" to "away from the middle".

### Exact files

| File | Change |
| --- | --- |
| `src-tauri/src/hook/pointer.rs` | `hit_chip` → `sector_pick`/`chip_angle`/`ang_dist`/`dead_zone_radius`; dwell 150→60ms; hysteresis; centre + dead-zone atomics; tests 14 → 20 |
| `src-tauri/src/commands.rs` (`publish_hud_chips`) | also reads `win.inner_size()` — the ring's centre is the client centre |
| `src-tauri/src/config/schema.rs` | `pointer_hud_activation` → `default_true`; new `hud_show_specials`; both first-install tests |
| `src-tauri/src/engine/mod.rs` | `hud_show_specials` gate: an EMPTY `specials` vec |
| `src-tauri/src/hook/mod.rs` | comment only — why the atomic still starts `false` |
| `src/types.ts`, `src/components/settings-panel.ts`, `src/components/controls.ts`, `src/preview.ts` | reads flip to `!== false`; new row; description copy |

### The code

**1. The sector math** (`pointer.rs`). Each chip owns the directions closer to
its own angle than to any other chip's — which IS "midpoints between
neighbouring chips' angles are the boundaries", stated as a nearest-angle
search so that wraparound at ±π costs nothing:

```rust
pub(crate) fn chip_angle(cx: i32, cy: i32, r: (i32, i32, i32, i32)) -> f64 {
    let ccx = (r.0 as f64 + r.2 as f64) / 2.0;
    let ccy = (r.1 as f64 + r.3 as f64) / 2.0;
    (ccy - cy as f64).atan2(ccx - cx as f64)
}

pub(crate) fn ang_dist(a: f64, b: f64) -> f64 {
    let two_pi = std::f64::consts::PI * 2.0;
    let mut d = (a - b).abs() % two_pi;
    if d > std::f64::consts::PI { d = two_pi - d; }
    d
}

pub(crate) fn sector_pick(
    rects: &[(i32, i32, i32, i32)], cx: i32, cy: i32, dead_r: f64,
    px: i32, py: i32, armed: Option<usize>,
) -> Option<usize> {
    if rects.is_empty() { return None; }
    let dx = (px - cx) as f64;
    let dy = (py - cy) as f64;
    if dx * dx + dy * dy < dead_r * dead_r { return None; }   // THE DEAD ZONE
    let theta = dy.atan2(dx);
    let mut best: Option<(usize, f64)> = None;
    for (i, &r) in rects.iter().enumerate() {
        let d = ang_dist(theta, chip_angle(cx, cy, r));
        if best.map_or(true, |(_, bd)| d < bd) { best = Some((i, d)); }
    }
    let (nearest, d_nearest) = best?;
    if let Some(a) = armed {
        if a != nearest && a < rects.len() {
            let d_armed = ang_dist(theta, chip_angle(cx, cy, rects[a]));
            if d_armed - d_nearest < 2.0 * HYSTERESIS_RAD { return Some(a); }
        }
    }
    Some(nearest)
}
```

**Why `2.0 *` and not `HYSTERESIS_RAD` on its own.** Moving `h` past the
midpoint between two chips changes the DIFFERENCE of the two angular distances
by `2h`, not `h`. Comparing the difference against `2 * HYSTERESIS_RAD` is
therefore exactly "3° past the boundary". Writing `< HYSTERESIS_RAD` would
silently be 1.5°, which is the kind of error that never produces a bug report,
only a feel nobody can name.

**2. The dead-zone radius, derived not guessed** (`pointer.rs`). The largest
circle around the centre that touches no app chip — measured to the nearest
chip EDGE, so it can never overlap one:

```rust
pub(crate) fn dead_zone_radius(cx: i32, cy: i32, rects: &[(i32, i32, i32, i32)]) -> f64 {
    let mut best = f64::INFINITY;
    for &(x0, y0, x1, y1) in rects {
        let dx = (x0 - cx).max(cx - x1).max(0) as f64;
        let dy = (y0 - cy).max(cy - y1).max(0) as f64;
        let d = (dx * dx + dy * dy).sqrt();
        if d < best { best = d; }
    }
    if !best.is_finite() { DEAD_ZONE_MIN_PHYS_PX } else { best.max(DEAD_ZONE_MIN_PHYS_PX) }
}
```

Measured on this machine (1.5 scale): the apps ring's nearest edge is ~181 CSS
px vertically → **~271 physical px**, and ~250 physical with the specials ring
hidden. `DEAD_ZONE_MIN_PHYS_PX = 140.0` is both the floor and the
empty-snapshot value, so it never bites on real geometry; it exists for the
degenerate cases, and it errs LARGE because large means "nothing arms, and
releasing Space types a space".

**3. The HUD centre has to be READ BACK, size included** (`commands.rs`).
`overlay_fit_hud` may clamp the requested size to 94% of the monitor, and the
page then centres itself in what it actually GOT (`#st-hud` is
`position: fixed; inset: 0`; every chip is placed with `calc(50% + …)`). Using
the requested size would put the centre off by half the clamp — which rotates
every sector on exactly the small displays where the ring is tightest:

```rust
let Ok(size) = win.inner_size() else {
    crate::hook::pointer::clear_chips();
    log::warn!("publish_hud_chips: overlay size unreadable — chip snapshot cleared");
    return;
};
crate::hook::pointer::publish_chips(pos.x, pos.y, size.width, size.height, &chips, dpr);
```

`HUD_CENTER_OK` is a separate `AtomicBool` rather than a sentinel coordinate,
because **(0,0) is a legal physical screen position** on the primary monitor —
there is no coordinate that can mean "unknown". Published BEFORE
`CHIP_GEOM_COUNT` and cleared with it, so no tick can pair live chips with a
stale centre.

**4. TWO RINGS, ONE ANGULAR SPACE — only the apps ring participates.** This is
a DECISION, and the owner may reverse it. The specials sit on an inner ring, so
a special and an app can point in the SAME direction and a sector would be
ambiguous; his description ("the direction from the space to the app") is about
apps, and the specials have always been reached by their keys. It is enforced
twice over, and the first enforcement was already there:

* `publishHudChips` in `toast.ts` selects `.st-chip.ap` and nothing else, so
  the specials never reach Rust at all.
* Every special sits at an inner-ring radius, which is **inside the dead zone
  by construction** — the dead zone is derived from the apps ring's inner edge.

**5. Dwell 150ms → 60ms.** 150ms was calibrated for a 60x30 rect, where a
fly-through crossed the target in a couple of frames. A sector is tens of
degrees wide, so the same 150ms is spent staring at a direction the user has
already chosen; on a flick it reads as lag. 60ms is still ~4 poller ticks, and
`a_sweep_across_the_ring_is_a_non_event` holds the line: three 16ms ticks per
sector (48ms) arms nothing, and a real sweep is faster than that. Min travel
stays 24 physical px — it measures the same thing it always did.

**6. Default ON — an owner decision overriding this codebase's own convention.**
The convention (spelled out on `hud_toast_flight`) is that brand-new behaviour
ships OFF. He asked for this one ON anyway, knowingly, in the same
conversation. `default = "default_true"` and **not** a bare `#[serde(default)]`,
because a bool's `Default` is `false`: the bare attribute would deliver the
flip only to a brand-new install, i.e. to nobody who already runs the app. The
field-removal test is where the flip actually travels:

```rust
assert!(
    c.pointer_hud_activation,
    "a config predating pointer_hud_activation must now read as ON \
     (owner's decision, 2026-08-27) — this is the path the flip travels"
);
```

**7. `hud_show_specials` — the entire feature is an empty vec** (`engine/mod.rs`):

```rust
let specials: Vec<(String, String)> = if !cfg.hud_show_specials {
    Vec::new()
} else {
    vec![ /* the eight, unchanged */ ]
};
```

The page draws what it is given, so there is no new overlay event and no
`toast.ts` coupling. **The special KEYS are untouched** — Esc still fires the
Boss Key, backtick still PiPs; those live in the hook and the `KeyCombo` arm,
neither of which has ever read this list. A hidden ring must stay a hidden
ring, never a disabled feature. And **no hook atomic**, deliberately: it is
read on the Space-HOLD path inside a config borrow that already exists, on the
engine thread. An atomic would be a second source of truth and one more thing
to forget to publish (PROBLEM 180's failure mode). The schema comment says so,
so nobody adds one later "for consistency".

### TWO DEFAULT CONVENTIONS NOW LIVE IN ONE STRUCT. That is the trap in this diff.

Three neighbouring fields, and copying the neighbour's read is a bug in two of
the three directions:

| Field | Default | Frontend read | Why |
| --- | --- | --- | --- |
| `hud_toast_flight` | OFF | `=== true` | new behaviour, nobody opted in |
| `pointer_hud_activation` | **ON** | `!== false` | new behaviour, owner overrode the convention |
| `hud_show_specials` | **ON** | `!== false` | EXISTING behaviour becoming optional |

The deciding question is always **"what did this config do YESTERDAY?"**, never
"what does the field next to it do?". `hud_show_specials` and
`pointer_hud_activation` reach the same `!== false` by completely different
arguments. Both schema comments point at each other for this reason.

### How it was verified

* `cargo test --lib` — **159 passed, 0 failed** (baseline 153). The pointer
  module went 14 → 20: three containment tests deleted, eight added (sector
  assignment incl. ±π wraparound, hysteresis in both directions, hysteresis vs
  the dead zone, the inscribed-radius derivation, the two-ring exclusion, the
  no-inner-ring case, a settled shift, and "no centre → nothing arms").
  **The arm/disarm ordering tests survived UNCHANGED**, which is the point of
  having had them: the `SPACE_ABORTED` CAS ordering is independent of how the
  chip is chosen, and this rewrite proved it rather than asserting it.
* `cargo check --lib` — clean, 0 warnings.
* `npx tsc --noEmit` — clean.
* **NOT VERIFIED: the gesture itself.** Nobody has held Space, moved a mouse
  and released it on this build. Sector geometry, the dead-zone radius against
  the REAL ring, whether 60ms feels right and whether 3° kills the flicker are
  all hand-test items — the overlay's failure mode lives in the OS compositor
  and the feel lives in a hand, and neither is reachable from a test.

### Generalise this

1. **When you override the user's own wording for a safety reason, the guard
   you add is not the only shape that reason has.** Containment and the dead
   zone answer the SAME objection ("nearest always has an answer"); one of them
   also destroys the feature. Ask what the guard must GUARANTEE, then find the
   cheapest shape that guarantees it — and write the objection down, because
   PROBLEM 206's note is what made this a 200-line amendment instead of a
   rewrite.
2. **A default flip travels through the serde attribute, not through
   `Default`.** `Default` reaches new installs; `#[serde(default = …)]` reaches
   everybody who already has a config. If the field-removal test is not
   updated, the flip reaches nobody and every check still passes.
3. **When two opposite conventions must coexist, make each site name the
   other.** A lone "read this as `!== false`" invites the next reader to
   "harmonise" the block. A comment that says *and the field above it is the
   opposite, here is why* does not.
4. **There is no coordinate that can mean "unknown".** (0,0) is a real screen
   position. A validity flag costs one atomic and removes a class of bug that
   only reproduces on a monitor arrangement you do not have.
5. **Derive a threshold from the geometry it guards, and keep a floor.** The
   dead zone tracks the ring it protects — it follows the apps ring inward when
   the specials are hidden, with no second constant to keep in sync — and the
   floor covers the degenerate snapshot the derivation cannot see.

---

## PROBLEM 210 — the thruster plume from Space to the armed chip, the specials ring pulled back to its tight baseline, and the two acceptance tests that were passing a bad layout

The FRONTEND half of the 1.0.88 pass (2026-08-27). PROBLEM 209 is the Rust
half — it made the hit-test angular. This is what the user actually SEES while
that hit-test is running, plus two defects that only appeared once somebody
measured the ring instead of looking at it.

Everything here is in `src/components/toast.ts` and
`src/styles/overlay-earthy.css`. **No new Tauri event, no new command, no new
payload field.** The `hud-pointer` event still carries `{ index }` and nothing
else; both beam endpoints are read out of DOM this file already owns. That was
a constraint, not a coincidence — see "generalise this" #1.

### 210a — pointing was invisible

**Symptom.** With PROBLEM 209's directional arming shipped, the cursor no
longer has to touch a chip, so there is nothing on screen connecting where the
cursor IS to what will launch. The armed chip does light up, but at 26 apps a
lit chip on the far rim of an ellipse is a small change a long way from the
eye, and the gesture is made near the centre. The owner asked for the link to
be drawn:

> "There should be a line type of thing going from Space to the app. And the
> line should not look like a plain line. It should look like the boost behind
> a spaceship — from the Space to the app — so a person can visually see which
> app is going to be launched depending on their cursor."

**Root cause.** Nothing existed. `_armed` was reflected only into the chip's
own `.armed` class.

**Exact file.** `src/components/toast.ts` (`BEAM_NOM`, `BEAM_GAP`, `_beamOn`,
`_beamAng`, `paintArmedBeam`, the `.st-beam` markup in `buildHud`, and the
`aiming` class toggle at the `hud-pointer` handler);
`src/styles/overlay-earthy.css` (`#st-hud .st-beam` and its `.jet` / `.lick` /
`.core` layers, the `st-beam-lick` / `st-beam-core` keyframes, and the
`:root.reduced-motion` carve-outs).

**The actual code.** ONE element, TWO style writes per index change, ZERO
per-frame JS. The plume is a fixed-size box pinned at the ring centre with its
`transform-origin` on its own left edge; aiming it is `rotate()` and reaching
the chip is `scaleX()`. Both are composited, so no layout runs while the beam
sweeps — which matters more here than usual, because this overlay composites in
SOFTWARE (`--disable-gpu`).

```ts
/** Nominal (unscaled) length of the beam box, in px. THE ONE SOURCE OF TRUTH:
 *  `buildHud` hands it to the stylesheet as `--beam-nom` — the same mechanism
 *  HUD_IN_MS/HUD_OUT_MS use — because this number divides into the scaleX
 *  written below, and a literal repeated in the CSS would be a second copy of
 *  it free to drift. Chosen large enough that the usual beam is a mild
 *  DOWN-scale (crisper gradient) and small enough that the box the compositor
 *  actually rasterises stays well under the toast glow's proven-safe
 *  footprint, at ANY beam length: a chip 1200px out still paints 880x56. */
const BEAM_NOM = 880;
/** How far SHORT of the chip centre the plume stops. The armed chip's own
 *  halo finishes the story; a tip that stabs into the label reads as an arrow
 *  pointing AT a thing rather than as thrust pushing TOWARDS it. */
const BEAM_GAP = 10;
```

Three details in `paintArmedBeam` are load-bearing and each cost a specific
failure if removed:

```ts
  // 1. offsetLeft/offsetTop, NOT getBoundingClientRect. THREE transforms are
  //    live on these elements at the moment this runs: the chip's inner <i> is
  //    mid-`st-bloom-in` (fill: both, so authoritative even before it starts),
  //    `.st-chip.armed` wears a scale(1.1), and #st-hud itself may still be at
  //    `.hidden`'s scale(.93). None of them touches LAYOUT. A client rect would
  //    aim the beam at wherever the chip happened to be mid-bloom, so the beam
  //    would visibly chase the chip through its entrance.
  const dx = chip.offsetLeft - sp.offsetLeft;
  const dy = chip.offsetTop  - sp.offsetTop;
  const len = Math.hypot(dx, dy) - BEAM_GAP;
  if (!(len > 0)) { off(); return; }

  // 2. UNWRAP THE ANGLE. `atan2` returns (-π, π], and CSS interpolates the
  //    NUMBER inside rotate(), not the direction. A chip at 3.05rad followed
  //    by its neighbour at -3.10rad — the two chips either side of the ±π
  //    seam, i.e. the LEFT flank of the ring, about as ordinary a place to
  //    point as exists — animates 6.15rad the LONG way, sweeping the beam
  //    through every other chip in the ring.
  let ang = Math.atan2(dy, dx);
  ang += Math.round((_beamAng - ang) / (Math.PI * 2)) * Math.PI * 2;
  _beamAng = ang;

  // 3. An arming that follows a DISARM must JUMP to its new angle, not sweep
  //    to it: sweeping is meaningful when the user can watch the beam travel,
  //    and meaningless — but still 190ms long — when the beam it travels from
  //    is invisible. The reflow between add and remove is what makes it a real
  //    frame boundary; without it the browser coalesces both class changes and
  //    the transition survives.
  const jump = !_beamOn;
  if (jump) beam.classList.add("st-beam-jump");
  beam.style.setProperty("--beam-ang", `${ang}rad`);
  beam.style.setProperty("--beam-scale", `${len / BEAM_NOM}`);
  if (jump) { void beam.offsetWidth; beam.classList.remove("st-beam-jump"); }
  beam.classList.add("on");
  _beamOn = true;
```

**NO `filter: blur()` ANYWHERE IN THE PLUME.** Every soft edge is a gradient
stop. This is PROBLEM 37 and it is not negotiable: one 560x320 element at
`blur(34px)` made the ENTIRE overlay window compose zero pixels — HUD and
toasts both gone — while Rust reported it correctly sized, centred and
`visible: true`, and the JS ran to completion with no error. A plume is
*exactly* the kind of element somebody reaches for a blur to soften.

The specials ring ghosts out of the way while aiming, because the beam crosses
it on its way to any app chip:

```css
#st-hud.aiming .st-chip.sp { opacity: .12; }
```

Declared on the base `.st-chip.sp` as well as on the aiming state so an
un-aiming collapses to opacity 0 rather than lingering at .12. Under
`prefers-reduced-motion` the plume keeps its `.on` state and loses only its
looping `lick`/`core` animations, and the specials' fade **stays** — it is a
STATE, not a flourish, and the reduced-motion rule names the aiming selector so
the intent survives an edit to either one.

### 210b — the specials ring had drifted off its tight baseline

**Symptom.** The inner ring sat further out than it used to, for no content
reason.

**Root cause.** PROBLEM 208 gave the inner ring a growth ladder. The BASELINE
it grows from is `rx0 = RIN_CLEAR + half + RING_CLEAR` (115 + widest-special
half-width + 26 — about **240.5** on the owner's set) with `ry = RYI_BASE`
(**118**). Rungs s0-s3 must all start from exactly that, and only s4 grows.

**Exact file.** `src/components/toast.ts`, `fitInner`'s `SP_LADDER` loop.

**The actual code.** The baseline is recomputed per rung, from re-measured
widths, and only the last rung is allowed to move it:

```ts
      const rx0 = RIN_CLEAR + half + RING_CLEAR;   // the old tight baseline
      let rx = rx0, ry = RYI_BASE;
      ...
      if (r.grow) { /* s4 only */ }
```

Widths are re-measured on every rung, because `.dense-sp`/`.tight-sp` change
the real rendered widths and a rung judged on the previous rung's numbers is
the PROBLEM 77 estimate bug wearing a new hat.

### 210c — "zero overlaps" accepted a layout with 2px between two chips

**Symptom.** Owner: the specials were "a little bit overlapping". They were
not overlapping. `overlapCount()` returned 0 and the ladder correctly accepted
the first rung it reached.

**Root cause.** The acceptance test encoded the FAILURE bar, not the QUALITY
bar. `overlapCount` answers "do any two touch?", which is the right question
for the OUTER ring — that ring is *grown* to fit its content, so it arrives at
zero overlaps with sensible spacing on the way. The INNER ring starts from a
fixed tight baseline and is allowed to accept whatever fits there. Measured: 12
specials on that baseline pass `overlapCount === 0` with **2px** between two
neighbours. Technically apart. Unreadable.

**Exact file.** `src/components/toast.ts` — new `minClearance()`, and the
acceptance line in `fitInner`.

**The actual code.** Judge the inner ring on DISTANCE, not on a boolean:

```ts
/** The SMALLEST axis-aligned clearance between any pair of boxes, in px.
 *  Negative means an overlap that deep; `Infinity` for fewer than two boxes.
 *  `max(gx, gy)` and not `min`: two rectangles are separated if they clear on
 *  EITHER axis, so the pair's real clearance is the better of the two. */
function minClearance(boxes: ChipBox[]): number {
  let m = Infinity;
  for (let i = 0; i < boxes.length; i++) {
    for (let j = i + 1; j < boxes.length; j++) {
      const a = boxes[i], b = boxes[j];
      const g = Math.max(
        Math.abs(a.x - b.x) - (a.w + b.w) / 2,
        Math.abs(a.y - b.y) - (a.h + b.h) / 2,
      );
      if (g < m) m = g;
    }
  }
  return m;
}
```

```ts
      // BEFORE:  if (last.overlaps === 0) break;
      // AFTER — TWO conditions, and the second is the one the owner's
      // complaint is actually about. RING_GAP_MIN is reused as the bar rather
      // than a fresh number because that constant already carries exactly this
      // meaning: "below this, chips start touching in the diagonal quadrants".
      // s4 is the last rung and is accepted whatever it measures — growing is
      // the concession of last resort, and there is nothing after it.
      if (last.overlaps === 0 && last.clear >= RING_GAP_MIN) break;
```

`RING_GAP_MIN` is **10**. `spClear` (the accepted rung's measured clearance) is
now on the exported `HudFit`, so a bug report can say whether the inner ring is
merely legal or actually readable.

### 210d — with the specials hidden, the ring became a screen-filling circle

**Symptom.** Found while measuring the new `hudspecials` switch (PROBLEM 209's
other half), NOT reported from the field. Turn the specials off with the
owner's 26 apps bound and the app ring came out **rout 477.6 / ryo 438.4 — an
aspect of 0.918** — a circle, in a window 1002px tall against a 1003px budget.
One pixel from the clamp that shrinks every chip uniformly.

**Root cause.** `RYO_BASE` (196) is a vertical FLOOR chosen for a ring that has
an inner ring beneath it; paired with a `rout0` of ~437 it gives the ellipse its
0.45 eccentricity. Take the inner ring away and `rout0` collapses to ~213 while
the floor stays 196, so the BASE ellipse is 213x196 — already a circle. And
`growRing`'s phase 1 is UNIFORM, so it faithfully preserves that circle all the
way up. Not an overlap; the ladder was right to accept it. Just not the design's
ellipse.

**Exact file.** `src/components/toast.ts`, the `ryo0` / `rout0` expressions.

**The actual code.** The empty-specials branch gets its own floor, capped at the
roundest shape the design admits. `RING_ASPECT_MAX` already existed for exactly
this statement — it was simply only enforced inside `growRing`'s phase 2, which
a ring that STARTS round never reaches:

```ts
    const rout0 = hasSp
      ? gi.rx + innerHalf + outerHalf + RING_CLEAR
      : RIN_CLEAR + outerHalf + RING_CLEAR;

    const ryo0 = hasSp
      ? Math.max(RYO_BASE, gi.ry + (innerTall + outerTall) / 2 + RING_CLEAR)
      : Math.max(SPACE_H / 2 + outerTall / 2 + RING_CLEAR,
                 Math.min(RYO_BASE, RING_ASPECT_MAX * rout0));
```

`Math.max` with the collision floor stays OUTERMOST so the cap can never pull
the ring INTO the SPACE pill. `RING_ASPECT_MAX` is **0.55** — "at 0.55 it is
still unmistakably an ellipse".

The `rout0` empty-specials branch is written as an explicit branch rather than
left to fall out of `innerHalf === 0`: that arithmetic would still add the
phantom ring's own `RING_CLEAR` and push every app chip ~26px further out than
it needs to be, for a ring that is not on screen.

Also in `fitInner`, the empty case returns early and takes NO measurements —
`Math.max()` on an empty array is `-Infinity`, and `-Infinity` propagates into
every radius downstream as `NaN`, which lays every chip out at
`calc(50% + NaNpx)`, i.e. nowhere.

### How it was verified

* `npx tsc --noEmit` — 0 errors.
* `cargo test --lib` — 159 passed, 0 failed (unchanged baseline; this half is
  frontend, and nothing it touched had a Rust test to break).
* Built, bundled and INSTALLED as 1.0.88 on the real machine through
  `explorer.exe` (PROBLEM 143). Frontend markers `st-beam`, `aiming` and
  `hudspecials` all present in `dist2/assets/*`; installed exe stamps 1.0.88 and
  its `LastWriteTime` postdates the newest `dist2` file. **Frontend strings are
  NOT searchable in the exe** — Tauri v2 compresses the embedded bundle — so
  that three-link chain is the proof, not an ASCII scan of the binary.
* 0.918, 477.6/438.4, the 1002px-against-1003px window and the 2px inner-ring
  clearance are all MEASURED numbers off the harness, not estimates. That is the
  whole reason 210c and 210d exist: nobody could see either of them.

### NOT VERIFIED — say so out loud

* **The owner HELD SPACE on this build at 10:35 and 10:37 on install day, and
  the arming pipeline is confirmed live** — `hud-pointer: ARMED chip …` walked
  four distinct chips in 1.1s with no oscillation between neighbours as he swept
  the cursor, then `hide with action pending` → `overlay_toasts_done` fired, so
  a release launched something. **But the log records what RUST DECIDED, not
  what the SCREEN SHOWED.** Nobody has confirmed the plume actually rendered.
  The overlay's worst failure mode lives in the OS compositor, not the page
  (PROBLEM 37/135), and a page cannot observe that its own window is hidden — so
  an armed-chip log line is fully compatible with a beam that painted nothing.
* **No animation has been watched playing.** The plume's sweep, its jump-on-
  rearm, the `lick`/`core` loops and the specials' ghosting are all unwatched.
* **The gesture has been performed but not JUDGED.** Whether the beam reads as
  thrust rather than as an arrow, and whether the 190ms sweep is right, are
  opinions and they are the owner's. Ask him what he saw.

### Generalise this

1. **An acceptance test must encode the QUALITY bar, not just the failure bar.**
   `overlapCount === 0` is a true statement about a layout with 2px between two
   chips, and it is the wrong question. When a test is written to catch a
   reported failure, ask what the *good* state is — not merely what the bad one
   was — or the first layout that clears the bad state ships as if it were good.
   The tell is a boolean where the user's complaint was a matter of degree.
2. **A constant that expresses a design LIMIT must be enforced wherever a shape
   is decided, not only where that shape is CHANGED.** `RING_ASPECT_MAX` lived
   inside the growth path, so it governed rings that grew INTO a circle and had
   nothing to say about a ring that STARTED as one. A limit enforced on the
   delta is not enforced on the value.
3. **A uniform transform faithfully preserves a bad baseline.** `growRing`'s
   phase 1 was correct and made things worse, because "preserve the existing
   eccentricity exactly" is only a virtue when the existing eccentricity is
   right. Check the base case of any scaling path with the input that makes the
   base degenerate — here, an empty collection.
4. **Adding a visual should not require adding a contract.** The plume needed
   two endpoints; both were already in the DOM. Reaching for a new event field
   would have created a second source of truth for a position the page already
   knows, plus a Rust/TS pair to keep in sync forever.
5. **`getBoundingClientRect` reads TRANSFORMS; `offsetLeft/offsetTop` read
   LAYOUT.** When several animations are mid-flight, the layout position is the
   stable one — and it is usually the one you meant. A rect read during an
   entrance animation is a measurement of the animation.
6. **Interpolating an angle interpolates the NUMBER, not the direction.**
   Anything that writes `rotate()` from `atan2` across repeated updates needs
   the unwrap, or the ±π seam produces a full-circle sweep at the least
   convenient moment.

---

## PROBLEM 211 — `youtube.com` was not a URL, and fixing that opened a new way to wipe a browser-profile pin

**Symptom.** Two halves of one owner report. Type `youtube.com` into a key's
path field and (a) the 4b disc and the browser-profile chip never appear, and
(b) committing it stores the bare word — so the binding is not recognised as a
link anywhere downstream. Only a string starting `http://` or `https://` counted.

**Root cause.** `looksLikeUrl` was `/^https?:\/\//i`. That is a test for a
SCHEME, and nobody types a scheme. The field's other job is real file paths, so
the test had been kept deliberately narrow — the right instinct, the wrong
implementation: the thing that separates `C:\Tools\thing.exe` from
`youtube.com` is not the presence of `https://`, it is the shape of the string.

**Exact file.** `src/components/key-detail-panel.ts` — `classifyPathInput`
(replacing `looksLikeUrl`), `normaliseUrl`, `isUnchangedPillValue`,
`syncPathDisc`, `submitPathField`.

### The actual code — classification

Ordered rules, path-vetoes FIRST, hostname shape LAST, and no TLD table:

```ts
type PathInputKind = "url" | "path";

/** `.exe` and friends. NOT `.com` — see the trade-off note below. */
const EXECUTABLE_SUFFIX = /\.(?:exe|lnk|bat|cmd)$/i;
/** One hostname label: alphanumeric, inner hyphens allowed. */
const HOST_LABEL = /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/i;

function classifyPathInput(raw: string): PathInputKind {
  const value = raw.trim();
  if (!value) return "path";

  // 1 — an explicit web scheme settles it.
  if (/^https?:\/\//i.test(value)) return "url";

  // 2 — the path vetoes. Any one of these and the hostname test never runs.
  if (value.includes("\\")) return "path";              // C:\… , \\server\…
  if (/^[a-z]:/i.test(value)) return "path";            // C:  C:/  D:\
  if (value.startsWith("%")) return "path";             // %LOCALAPPDATA%\…
  if (/^\.{1,2}[\\/]/.test(value)) return "path";       // ./run   ../bin
  if (value.startsWith("/")) return "path";
  // Any OTHER scheme is somebody else's business, not a website. The character
  // class deliberately excludes `.` so a host with a port (`youtube.com:8080`)
  // cannot be mistaken for a scheme.
  if (/^[a-z][a-z0-9+-]*:/i.test(value)) return "path";

  // 3 — a bare executable filename.
  if (EXECUTABLE_SUFFIX.test(value)) return "path";

  // 4 — hostname SHAPE. Take the authority only: `youtube.com/watch?v=x` is a
  // url, and the `/` after the first dot is its path, not a file separator.
  const authority = value.split(/[/?#]/)[0] ?? "";
  const host = authority.split(":")[0] ?? "";           // drop any :port
  const labels = host.split(".");
  if (labels.length < 2) return "path";                 // "notepad" stays an app lookup
  if (!labels.every((l) => HOST_LABEL.test(l))) return "path";
  if (!/^[a-z]{2,}$/i.test(labels[labels.length - 1]!)) return "path";
  return "url";
}
```

Three decisions worth not re-litigating:

* **No TLD list.** A TLD table would be wrong the week it was written, and is
  not needed once rule 2 exists. `notebooklm.google.com` and `x.com` classify
  correctly from shape alone.
* **A single label stays a PATH.** `notepad` → path, because a lone label is a
  program name, and binding `notepad` and letting the Start Menu resolve it is a
  REAL existing flow. It must not become `https://notepad`.
* **THE `.com` TRADE-OFF, ACCEPTED DELIBERATELY.** `.com` is both the commonest
  TLD and a DOS-era executable extension, and they collide exactly here.
  `C:\Tools\a.com` is a path (rule 2, the backslash). A bare `a.com` typed with
  no path at all is a URL. That is wrong for someone pasting the bare name of a
  `.com` executable, and right approximately every other time the string
  `something.com` is typed into this field. **The owner asked for precisely
  this.** Anyone tempted to "fix" it: the cure is a file-exists check, not a TLD
  list.

`classifyPathInput` is SYNCHRONOUS, so the "does this file exist?" leg of the
veto is deliberately not in it — it runs on every keystroke via `syncPathRow`,
and an IPC round trip per keypress to answer a question that only matters at
commit time is a cost paid constantly for a vanishingly rare case. The commit
path's `check_app_path` still guards the file branch.

### The actual code — normalisation

```ts
function normaliseUrl(value: string): string {
  const v = value.trim();
  return /^https?:\/\//i.test(v) ? v : `https://${v}`;
}
```

**What is STORED is ALWAYS schemed.** Verified against the consumers rather
than assumed:

* `run_browser` (`smart_cascade.rs`) already prepends `https://` itself and
  logs it — so the DEFAULT-browser leg would survive a bare host, but only by
  being repaired downstream on every single press;
* `open_binding_url`'s `BrowserRoute::Specific` leg does **NOT**. It hands the
  stored string to `build_launch_params` as a command-line argument to a browser
  exe, with no scheme repair anywhere on that path — so a URL pinned to a
  browser profile (the very feature this fix restores access to, PROBLEM 204)
  would be launched as a bare word;
* `url_match_keys` tolerates either; and
* `splitUrlForPill` needs `new URL()` to parse, which a bare host does not.

One normalisation here beats three tolerances downstream.

### The hole normalisation opened, and the guard that closes it

**Symptom.** Not reported — found by re-tracing every call site rather than
adding the new one and stopping. Assign `youtube.com` to a key pinned to
"Brave — Studies". The pill reloads as `https://youtube.com` (the stored form).
Open the field to edit, delete the `https://` you never typed, press Assign.
Byte equality calls that an EDIT, re-commits an identical `web_url`, and —
through `assignFromPath`'s deliberate omission of the three browser-profile
fields — **silently wipes the pin.**

That is verbatim the failure `_pathSeed` exists to prevent (PROBLEM 202/204),
arriving through a door this release opened. Normalisation created a NEW way for
two different strings to mean "no change", and the no-change guard had never
been told about it.

**The actual code.**

```ts
function isUnchangedPillValue(value: string): boolean {
  if (_pathSeed === null) return false;
  if (value === _pathSeed) return true;
  return (
    classifyPathInput(value) === "url" &&
    classifyPathInput(_pathSeed) === "url" &&
    normaliseUrl(value) === normaliseUrl(_pathSeed)
  );
}
```

```ts
function submitPathField(value: string): void {
  if (!value) return;
  if (isUnchangedPillValue(value)) { cancelPathEdit(); return; }
  void assignFromPath(value);
}
```

Nothing else is loosened. The comparison is still exact apart from a scheme
*this code added itself*, so `youtube.com` vs `youtu.be` is still a real edit,
and a path is still compared byte for byte.

And the unchanged case is not a no-op — it puts the pill BACK. Pressing Assign
over a value you did not change is a request to finish, and finishing means the
field goes back to showing what is assigned.

### The four call sites

`classifyPathInput` replaced `looksLikeUrl` at all four, and all four were
re-read rather than mechanically substituted:

1. `syncPathDisc` — the 4b disc + the `has-disc` padding. This is the half of
   the owner's report about the disc never appearing.
2. `isUnchangedPillValue` — the one above, which had to be re-traced rather
   than left alone.
3. the 4b disc's own click handler, which opens the browser-profile picker.
   The disc only appears over a url, so this guard and `syncPathDisc`'s have to
   agree BY CONSTRUCTION — they now read the same function rather than two
   copies of the same regex. (It also calls `isUnchangedPillValue`: a value
   loaded out of the pill and not edited is ALREADY committed, so saving it
   would clear the very pin the page is about to set, and leave it cleared if
   the user backs out with the arrow.)
4. `assignFromPath` — the one that decides what the binding IS, and the reason
   `youtube.com` became an APP binding that only worked because the engine's
   Start Menu cascade happened to rescue it. It normalises at COMMIT time, not
   at classify time: the field, the pill and the disc all keep working with
   exactly what the user typed; only the value that reaches `web_url` is
   repaired, and it is repaired once.

### How it was verified

* `npx tsc --noEmit` — 0 errors.
* `cargo test --lib` — 159 passed (Rust untouched by this entry).
* Shipped in 1.0.88 and installed on the real machine. `dist2/assets/main-*.js`
  contains the surviving classifier markers `(?:exe|lnk|bat|cmd)`,
  `[a-z0-9+-]*:` and `4b disc ` — **`classifyPathInput` itself is NOT a usable
  marker, it is minified away**; a function NAME is the frontend equivalent of
  PROBLEM 209's short-literal trap, and picking one would have produced a False
  on a bundle that plainly contains the feature.
* The owner's live `config.json` (98,215 bytes, copied out through
  `explorer.exe` and cross-checked against `debug.log`'s
  `config: saved 98215 bytes`) parses, holds 5 profiles × 26 bindings, 16 of
  them `web_url`, and **all 16 are already schemed** — so this change is purely
  additive for his data and no migration is involved.

### NOT VERIFIED

**Nobody has typed `youtube.com` into the field on this build.** The disc
appearing, the profile chip appearing, the commit storing `https://youtube.com`,
and the delete-the-scheme-and-press-Assign sequence keeping the pin are all
hand-test items. The pin-wipe hole was found by reading, closed by reading, and
has not been exercised end-to-end — consistent with PROBLEM 202/204, where
nobody has performed ANY pin end-to-end yet.

### Generalise this

1. **Every NEW way to express "no change" has to be taught to the no-change
   guard.** Normalisation, trimming, case-folding, unit conversion, ID
   canonicalisation — each one creates a fresh class of "different bytes, same
   meaning", and each one silently converts a no-op into a partial write
   wherever a guard is still comparing bytes. **When you add a canonical form,
   grep for every equality test on that value in the same breath.** The bug is
   never in the normaliser; it is in the comparison three functions away that
   nobody thought was affected.
2. **A partial-update commit path makes every spurious write DESTRUCTIVE.**
   `assignFromPath` omits the browser-profile fields on purpose, so "commit an
   identical value" is not harmless — it is data loss. Where a write drops
   fields by design, the guard that decides whether to write at all is a
   data-integrity control, not an optimisation, and must be documented as one.
3. **Test for the SHAPE, not for the prefix.** `^https?://` tests for something
   the user does not type. The discriminating features were on the other side —
   backslashes, a drive letter, `%VAR%`, a foreign scheme — so the rule became
   "veto on path evidence, then accept on hostname shape", which needs no list
   to maintain.
4. **A deliberate wrong answer, written down with the reason, is finished
   work.** The `.com` collision has no clean resolution; it has an owner's
   decision. Recording that decision *and* the cure a future reader would
   otherwise reach for (a file-exists check, not a TLD table) is what stops it
   being re-opened every six months.
5. **A minified function name is not a shipping marker.** Same family as
   PROBLEM 209's `st-hud-pointer`: pick a string the toolchain has a reason to
   PRESERVE — a regex literal, a log message — and confirm it in the built
   artifact before trusting its absence as evidence.

---

## PROBLEM 212 — the Space-hold guide dropped the special keys once you owned enough apps, and the ring that fixes it had to be switchable

**Symptom.** Two halves, one system.

(a) With enough apps bound — measured threshold **23** — the Space-hold guide
silently stopped drawing the special-key ring. Boss Key, PiP and the other six
vanished from the guide. They still WORKED; nothing said they were gone; and the
user with the most bindings, the one who most needs a reminder of what Esc does,
was the only one who never saw them. Below 23 apps they were there. Nobody could
name the number, because nothing logged it.

(b) The single grown ring that made this happen also read badly at scale: 26
chips at one radius, each carrying a full app name, is a wall of text arranged in
a circle. Nothing to aim at, nothing to skim.

**Root cause.** The classic layout has ONE app ring and grows it until the chips
fit. The specials share the inside of that ring. So the two compete for one
budget, and when the apps won — which they must, they are the point — the code
took the only option its geometry left it and dropped the specials.

That was the actual mistake, and it is not a layout bug. **The ceiling was
treated as real without ever being checked.** A single ring's circumference is a
hard bound only if you insist on a single ring. Nothing about the screen, the
window budget or the readable chip size required one. Once the apps are allowed
to occupy TWO bands, the inner band is free, the specials have a home that the
apps never contend for, and the bound that forced the drop turns out to have been
self-imposed.

**Exact files.**

| file | what it holds |
| --- | --- |
| `src/components/toast.ts` | the whole ring — `buildHud`, `layoutMagnetic` / `layoutClassic`, `packBands`, `bandStepFor`, `restWord`, `paintBloom`, the `EXHAUSTED` log |
| `src/components/hud-layout.ts` | NEW leaf. `_layout` state + the `!== false` normaliser |
| `src/components/hud-band-count.ts` | NEW leaf. `_bandCount` state + the auto/one/two normaliser |
| `src/overlay.ts` | the two `listen()` halves and the two `get_config` seed halves |
| `src/components/settings-panel.ts` | the three-row group, both handlers, both inert painters |
| `src/components/controls.ts` | `paintInert` + `SPECIALS_INERT_NOTE` + `ROWS_INERT_NOTE` |
| `src-tauri/src/engine/mod.rs` | `specials_for_hud` — the deterministic half of the decision |
| `src-tauri/src/config/schema.rs` | `hud_band_count`, `hud_magnetic_layout` and their defaults |
| `src-tauri/src/commands.rs` | the two global `emit`s in `save_config` |
| `src-tauri/src/hook/pointer.rs` | `sector_pick` — aim scoring that survives two bands |

### The actual code — the specials become the innermost band

`toast.ts:2095-2105`. This is the whole fix for (a): the specials are not
squeezed inside the app ring any more, they ARE band 0.

```ts
    /* --- BAND 0: THE SPECIALS, when they are shown ------------------------
       The owner's decision 2 makes this ONE geometry: the specials are simply
       the innermost band and the apps fill outward from the next one. Their
       own s0-s4 ladder is UNTOUCHED (decision 3) — they sit inside the dead
       zone, can never be aimed at and therefore never bloom, so word-clipping
       them would destroy information with no recovery path. `fitInner` starts
       from the tight 1.0.88 baseline the owner asked to keep and spends
       CONTENT before radius. */
    const gi = fitInner(gap, maxRoutBudget, maxRyoBudget, outerHalf, outerTall);
    const spW = gi.w, spH = gi.h;
    const hasSp = spChips.length > 0;
```

The app bands then stack outward from there. `packBands`, `toast.ts:1431-1438`:

```ts
  const bands: Band[] = [];
  for (let b = 0; b < n; b++) {
    const rx = n === 1
      ? fitRx(total, lo, hiR)
      : Math.min(hiR, lo + b * ((hiR - lo) / (n - 1)));
    bands.push({ rx, ry: rx * BAND_RATIO,
                 cap: ellipsePerimeter(rx, rx * BAND_RATIO), items: [] });
  }
```

The band step is NOT the chip height, and that is the one number worth copying
out — `bandStepFor`, `toast.ts:1033-1038`:

```ts
/** A radial step of S in x buys only `BAND_RATIO · S` in y, and bands stack at
 *  the TOP and BOTTOM of the ellipse where it is y that has to clear. A naive
 *  46px step leaves 28.5px vertically and the bands touch. */
function bandStepFor(chipH: number): number {
  return Math.round((chipH + BAND_PAD) / BAND_RATIO);
}
```

### The actual code — the branch back to 1.0.88

ONE line, at the top of the fit ladder rather than sprinkled through it, so both
geometries share every rung and neither can drift out from under the other.
`toast.ts:2283-2287`:

```ts
  /** THE BRANCH. One line, at the top of the ladder rather than sprinkled
   *  through it, so every rung of the (a)-(d) walk below is shared and neither
   *  geometry can drift out from under it. */
  const layout = (step: HudFit["step"], gap: number): LayoutOut =>
    mode === "classic" ? layoutClassic(step, gap) : layoutMagnetic(step, gap);
```

`mode` is read once per show, `toast.ts:1555-1562`:

```ts
function hudLayoutMode(): HudLayoutMode {
  try {
    return (window as unknown as { __stHudLayout?: string }).__stHudLayout === "classic"
      ? "classic" : "magnetic";
  } catch {
    return "magnetic";
  }
}
```

The acceptance bar differs by mode too, deliberately — classic must reproduce
1.0.88 byte-for-byte, so it is not held to the new clearance minimum
(`toast.ts:2322-2323`):

```ts
  const clean = (f: HudFit) =>
    f.overlaps === 0 && (mode === "classic" || f.clear >= RING_GAP_MIN);
```

### The actual code — first-word labels that open on aim

Clipping to the first WORD, not to a character count, is what makes 26 chips
skimmable. `toast.ts:1040-1049`:

```ts
/** The at-rest text of an app label: its FIRST WHOLE WORD.
 *  "Google Chrome" -> "Google"; "Samsung Browser" -> "Samsung";
 *  "WhatsApp" -> "WhatsApp" (already one word, so nothing is lost).
 *  A single long token comes back whole and is bounded by `--chip-cap`
 *  instead, with a CSS ellipsis. */
function restWord(label: string): string {
  const m = /^\S+/.exec(label.trim());
  return m ? m[0] : label;
}
```

Both strings are stashed on the element at build time, so opening one costs no
recomputation (`toast.ts:1700-1706`):

```ts
  const apRest: [string, string][] = apps.map(
    (a) => [a[0], mode === "classic" ? a[1] : restWord(a[1])]);
  apChips.forEach((c, i) => {
    c.dataset.stFull = apps[i][1];
    c.dataset.stRest = apRest[i][1];
    setChipLabel(c, apRest[i][1]);
  });
```

…and the restore is one line inside `paintBloom` (`toast.ts:2628-2634`):

```ts
    if (cell.classList.contains("bloom") !== lit) {
      cell.classList.toggle("bloom", lit);
      setChipLabel(cell, (lit ? cell.dataset.stFull : cell.dataset.stRest) ?? "");
    }
```

**Note the reduced-motion carve-out immediately above it (`toast.ts:2623-2627`):
the outward PUSH is suppressed, the LABEL still opens.** Reduced motion is a
request about movement, not a request to be denied the app's name. Getting that
backwards would have made the feature useless to exactly the users who set the
preference.

### The actual code — the bloom push, by SCREEN angle

The armed chip and its two neighbours push outward. The neighbours are chosen by
`atan2` — the on-screen angle — and not by the ellipse parameter or payload
order, which is the subtle part (`toast.ts:2609-2622`):

```ts
    // The two NEAREST ANGULAR NEIGHBOURS — by `atan2(y, x)`, the on-screen
    // angle, NOT by the ellipse parameter and NOT by payload order. On a 0.62
    // ellipse those disagree by up to ~25°, and with two bands the chip beside
    // you on screen is routinely not the next letter of the alphabet.
    const order = _apRest.map((c, i) => ({ i, g: c.geo })).sort((a, b) => a.g - b.g);
    const k = order.findIndex((o) => o.i === _armed);
    if (k >= 0 && order.length > 1) {
      on.add(order[(k - 1 + order.length) % order.length].i);
      on.add(order[(k + 1) % order.length].i);
    }
```

Push distances are `BLOOM_PUSH_ARMED = 40`, `BLOOM_PUSH_NEIGHBOUR = 24`
(`toast.ts:1005-1008`), doubly clamped — first by the radial ceiling, then by the
window (`toast.ts:2636-2653`) — so a bloom can never push a chip off its own
overlay.

### The actual code — aim scoring that survives a second band

The 1.0.88 hit-test picked the chip at the NEAREST ANGLE. With two bands that is
wrong: an outer chip 2° off the aim ray is 18.0px away from it, while an inner
chip 3° off is only 15.7px away — and the inner one is what the user is pointing
at. `sector_pick`, `src-tauri/src/hook/pointer.rs:621-636`:

```rust
    let mut best: Option<(usize, f64, f64)> = None;
    for (i, &r) in rects.iter().enumerate() {
        let d = ang_dist(theta, chip_angle(cx, cy, r));
        // cos(d) <= 0 — the chip is behind the direction the cursor points.
        // `ang_dist` is already folded into [0, π], so this is the whole test.
        if d >= std::f64::consts::FRAC_PI_2 {
            continue;
        }
        let score = chip_radius(cx, cy, r) * d.sin();
        if best.map_or(true, |(_, bs, _)| score < bs) {
            best = Some((i, score, d));
        }
    }
    let (nearest, _, d_nearest) = best?;
```

`chip_radius * sin(angular distance)` is the perpendicular pixel offset from the
aim ray. **The score uses the CHIP's radius, never the cursor's** — pushing the
cursor further out along the same direction must never change the answer, and
`pushing_the_cursor_further_out_never_changes_the_pick` (`pointer.rs:1404`)
holds it to that.

### The actual code — Rust decides what it CAN decide, and no more

The rows/specials interaction is one system split by *who can know the answer*.
Rust cannot resolve `"auto"` — that depends on measured label widths, which exist
nowhere but in the overlay document. `engine/mod.rs:776-784`:

```rust
pub(crate) fn specials_for_hud(show_specials: bool, band_count: &str) -> Vec<(String, String)> {
    if !show_specials || band_count == "two" {
        return Vec::new();
    }
    HUD_SPECIALS
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}
```

Its doc comment carries the six-row truth table and the sentence that keeps it
honest: *"a NON-EMPTY return means 'show these if one band is enough', not 'show
these'."* Anything that is not `"one"` or `"two"` is treated as `"auto"`, so a
config carrying `""` or a typo behaves like every previous build rather than like
a layout the user never chose. `an_unknown_band_count_falls_back_to_auto_not_to_two`
(`engine/mod.rs:842`) pins that.

The page then performs the half Rust could not, dropping the specials only if the
measurement says the labels need two bands (`toast.ts:2377-2382`):

```ts
  if (mode === "magnetic" && bandMode === "auto" && spChips.length > 0 && !clean(out.fit)) {
    for (const c of spChips) c.remove();
    spChips = [];
    _spDropped = true;
    out = runLadder();
  }
```

### The actual code — the two settings, and the defaults that decide who gets them

`src-tauri/src/config/schema.rs:329-330` and `:373-374`:

```rust
    #[serde(default = "default_band_count")]
    pub hud_band_count: String,
...
    #[serde(default = "default_true")]
    pub hud_magnetic_layout: bool,
```

```rust
fn default_true() -> bool { true }
/// See `hud_band_count`. `"auto"` — the behaviour every build before 1.0.89
/// already had, so an upgrading config changes nothing by acquiring the key.
fn default_band_count() -> String { "auto".into() }
```

**`default = "default_true"`, NEVER a bare `#[serde(default)]`.** A bool's
`Default` is `false`, so the bare attribute would hand the classic layout to
every config already on disk and deliver the new ring to nobody — which is the
exact opposite of a default-ON feature. This deliberately overrides this struct's
own convention that new behaviour ships OFF; the schema says so in place, and
says *"do not harmonise these three fields."*

The same rule has a frontend half. Every reader uses `!== false`, never
`=== true`, because the key is ABSENT from every config written before 1.0.89
(`settings-panel.ts:206`, `hud-layout.ts:92-103`, `overlay.ts:131`):

```ts
  const hudLayout = appConfig.hud_magnetic_layout !== false;
```

`overlay.ts` hands the raw value straight through and lets the normaliser apply
the rule — its comment says why: *"Do not 'help' by writing
`cfg?.hud_magnetic_layout === true` here — that would ship the flip to nobody."*

Two halves are required for BOTH settings, and the second is the one that gets
skipped. `commands.rs` re-emits on change (`emit`, never `emit_to`, which has
never worked in this app), and `overlay.ts` ALSO seeds from `get_config` on load,
because an event that fires only on CHANGE leaves a freshly-created overlay
(first launch, or a display-change rebuild) drawing whatever the module defaulted
to:

```rust
        let _ = app.emit("hud-band-count-changed", new_config.hud_band_count.clone());
        let _ = app.emit("hud-layout-changed", new_config.hud_magnetic_layout);
```

```ts
      applyBandCount(cfg?.hud_band_count);
      applyHudLayout(cfg?.hud_magnetic_layout);
```

### The actual code — the inert dependencies, which are the interesting part

Three rows, in one group, in this order, and the ordering is load-bearing
(`settings-panel.ts:250-257`):

```html
      <!-- ONE GROUP, IN THIS ORDER, AND DO NOT SEPARATE THEM. Each row gates
           the one below it: the layout switch decides whether the rows pill
           can do anything, and the rows pill decides whether the specials
           switch can. Put another row between any two of them and the reason
           a control is greyed out stops being visible from the control. -->
      ${toggleRow("hudlayout", "New ring layout", hudLayout, 11)}
      ${bandRow(band, 12)}
      ${specialsRow(hudSpecials, specialsInert, 13)}
```

The two dependencies, one line each (`settings-panel.ts:213`, `:220`):

```ts
  const specialsInert = band === "two";
  const rowsInert = !hudLayout;
```

Both point at ONE treatment, in the leaf module so `preview.ts` renders the
identical dead control without a backend (`controls.ts:110-124`):

```ts
export function paintInert(
  wrap: HTMLElement | null | undefined,
  note: HTMLElement | null | undefined,
  inert: boolean,
): void {
  if (wrap) {
    wrap.style.opacity = inert ? ".45" : "";
    wrap.style.pointerEvents = inert ? "none" : "";
    wrap
      .querySelectorAll<HTMLInputElement | HTMLButtonElement>("input, button")
      .forEach((el) => { el.disabled = inert; });
    wrap.closest<HTMLElement>(".set-row")?.setAttribute("aria-disabled", String(inert));
  }
  if (note) note.style.display = inert ? "" : "none";
}
```

**`disabled` cannot be dropped in favour of `pointer-events: none`** —
`pointer-events` does nothing for the keyboard, and the half that goes missing in
a hand-rolled second copy is always that one, leaving a "greyed out" control fully
operable from Tab. That is precisely why there is one implementation and three
callers rather than three copies.

The notes, both in `controls.ts` so the panel and the preview harness cannot
drift:

```ts
export const SPECIALS_INERT_NOTE =
  "Two rows are in use, so there is no inner ring left for these to sit in. " +
  "Pick 1 row — or Auto, which uses one ring whenever your apps fit — and they " +
  "come back. What you chose here is remembered either way, and the special " +
  "keys themselves keep working.";

export const ROWS_INERT_NOTE =
  "Rows only apply to the new ring layout. The classic ring uses its own fixed " +
  "shape. Turn the new layout back on to choose how many rows — what you " +
  "picked here is remembered either way.";
```

**NEITHER GREY-OUT WRITES TO CONFIG.** `hud_band_count` and `hud_show_specials`
keep whatever the user picked, so undoing the gate restores their choice exactly.
Writing `"auto"` or `false` "to make the UI honest" would silently destroy a
preference they never asked to change, and they would find out much later. Both
call sites say so in place (`settings-panel.ts:1619-1623`, `:1681-1684`), and both
handlers carry a redundant runtime guard as the belt to that brace
(`settings-panel.ts:391`, `:616`).

### The one asymmetry, marked rather than hidden

`toast.ts` reads the layout mode through the `window.__stHudLayout` mirror
instead of importing `hud-layout.ts`, while band count IS imported normally
(`toast.ts:17`). That is deliberate, marked temporary at `toast.ts:1538-1547`,
and the intended replacement is spelled out in the comment (`return
getHudLayout();`). It is written down here so the next reader knows it is a known
inconsistency with a stated exit, not an accident to preserve.

### How it was verified

**Measured, not eyeballed.** All of this is from the fit ladder's own
instrumentation, run over generated layouts:

- **Specials were never dropped across 156 layout cases.** The classic ring
  dropped them from **23 apps** upward. That is the headline: the threshold did
  not move, it stopped existing.
- **On the owner's real 26-app config:** 0 overlaps, **19.5px** worst-pair
  clearance, **582px** furthest outer edge — against the classic ring's
  **671px** for the same content. The new layout is both cleaner AND 89px
  tighter, which is the arithmetic behind "two bands beat one grown ring".
- **Window budget:** the HUD asks for **1344×869** against a **1604×1002**
  budget on the owner's 1707×1067 logical panel. 260px and 133px of headroom.
- **The one remaining sub-10px clearance** is `rows=one` + specials hidden at 23+
  apps — i.e. the user explicitly overruling the arithmetic. It is not silently
  patched up: the ladder logs `EXHAUSTED` loudly with every number needed to
  replay it (`toast.ts:2393-2408`), including band count, each band's rx, the
  rows mode, and whether the page dropped the specials. The last sentence of that
  log is the important one — *"Not a layout fault: the content does not
  physically fit at a readable size."*
- `cargo test --lib`: **167 passed, 0 failed**, including
  `the_six_rows_and_specials_combinations` (all six rows of the rows×specials
  table), `an_unknown_band_count_falls_back_to_auto_not_to_two`,
  `hud_magnetic_layout_defaults_to_true_on_both_paths` (the `Default` path AND the
  field-removed-from-an-old-config path, which is the one that actually reaches
  existing users), and
  `single_band_scoring_is_exactly_todays_nearest_angle` — which sweeps every whole
  degree for n ∈ {2, 4, 13, 26}, **17,640 cases**, against a copy of the 1.0.88
  algorithm and counts its two documented divergences rather than waving them
  away.
- **Shipped and proven on the real machine**, 1.0.89, 2026-08-27. Rust markers
  `hud-band-count-changed`, `hud-layout-changed` and `hud_magnetic_layout` all
  False in the installed 1.0.88 and True after the install, with five positive
  controls True on both sides. Frontend markers `New ring layout`, `Shortcut
  rows`, `bandRx`, `magnetic`, `ring EXHAUSTED at step` all present in
  `dist2/assets/*`, exe written 13:50:24 against a newest-bundle file of 13:49:06.
  See PROJECT_STATUS.md, 2026-08-27.
- **NOT verified: what it looks like.** The ring, the label opening on aim and
  the bloom push have not been seen on a screen. The overlay's failure mode lives
  in the OS compositor and no harness reaches it (CLAUDE.md, window rules). Hold
  Space and look.

### Generalise this

1. **A ceiling that forces a feature to be DROPPED is worse than a ceiling
   exceeded — check whether the bound is real before honouring it.** The
   specials disappeared because one ring's circumference was treated as a law of
   physics. It was a choice made three functions earlier. When code reaches
   "there isn't room, so remove something the user asked for", the next question
   is never "what do we drop" — it is "who decided there is only one row, and can
   they be overruled?" A drop is the loudest possible symptom of an unexamined
   constraint, and it arrives silently.
2. **A control that silently no-ops on the user's own config is
   indistinguishable from a broken one.** At two rows the specials switch changes
   nothing, whatever it says; at the classic ring the rows pill changes nothing.
   A user who flips one hears the sound, watches the animation, sees no result,
   and has no way to tell "this doesn't apply here" from "this app is broken".
   The fix is not to hide the control and it is certainly not to write the config
   value — it is to render it inert WITH THE REASON VISIBLE, using a note class
   that is on by default rather than one gated behind a help mode.
3. **Split a decision by who can KNOW the answer, not by which layer is
   convenient.** Rust resolves `one` and `two` because those are deterministic;
   it refuses `auto` because band count falls out of measured label widths that
   exist only in the overlay document. A backend that guessed at `auto` would be
   authoritative and wrong, and the page would have no way to disagree.
4. **When a default must ship ON, `#[serde(default)]` is a bug.** A bool's
   `Default` is `false`, so the bare attribute delivers the new behaviour to
   nobody who already runs the app — the only people who have a config. Pair
   `default = "default_true"` with a frontend that reads `!== false`, and test
   BOTH paths: `Default::default()` for a fresh install and
   field-removed-from-an-old-config for everyone else. Nothing forces those two
   to agree except the test.
5. **Reduced motion is a request about MOVEMENT, not a request for less
   information.** Suppressing the bloom push is right; suppressing the label that
   opens with it would deny the app's name to exactly the users who set the
   preference. Whenever a motion carve-out wraps a block, check what non-motion
   work is inside it.
6. **One inert treatment, called three times, beats three hand-rolled ones.**
   Two copies of an opacity/pointer-events/disabled trio drift, and the piece
   that goes missing is always `disabled` — which looks perfect on screen and
   leaves the control fully reachable by keyboard.
7. **When the arithmetic loses to an explicit user choice, SAY SO in the log with
   every number needed to replay it.** The one remaining tight layout is the user
   overruling the fit, not a defect, and the `EXHAUSTED` line ends by saying so in
   words. A diagnostic that reports a number without saying whose decision
   produced it generates false bug reports.

---

## PROBLEM 213 — sky mode stranded people with no gear, and the Escape that rescued them fired two handlers at once

**Symptom.** Two reports from the owner, one day apart, and the second was
CAUSED by the fix for the first.

(a) Turn on sky mode — the dashboard clears away and leaves only the night sky —
and the ONLY way back was a faint 34px arrow in the bottom-right corner, plus
Escape if you happened to know. People got stranded in an empty window. The
settings gear, which is where sky mode was turned on, disappeared along with
everything else.

(b) Once the gear was allowed to stay, its popover could be open while the sky
was up. Pressing Escape then did BOTH things in one keystroke: the settings
popover closed **and** sky mode exited. Whatever the popover was in the middle of
was dropped, and the user who only wanted to back out of a menu found themselves
returned to the whole dashboard.

**Root cause.**

(a) `body.sky-mode` hid every direct child of `#stage` except `#sky-return`. The
gear dock is a direct child of `#stage`. It was never excluded because, until the
owner said so, nobody had noticed that the one control which turns the mode ON is
also the one you want when you cannot find your way out.

(b) **Two independent, non-capturing `keydown` listeners on `document` both acted
on the same Escape.** Neither stopped propagation, both are in the bubble phase,
so a single press ran both:

1. `wireSkyEscape`'s listener (`main.ts:117`) called `leaveSkyMode()`.
2. The general dashboard handler in `bootstrap()` (`main.ts:331-334`) called
   `closeAllPopovers()`, which closes the settings panel.

Registration order cannot fix this. Both listeners are attached to the same node
in the same phase and both are correct in isolation; there is no ordering that
makes one of them decline. The missing thing was not an order, it was a
**guard**.

**Exact files.** `src/styles.css` (sky-mode block, lines 1792 / 1805 / 1834-1851)
and `src/main.ts` (`wireSkyEscape`, lines 105-123). No new JS was needed for (a)
at all — `applySkyMode` (`main.ts:424-440`) was NOT changed.

### The actual code — (a), CSS only

Both hiding rules gained a `:not(#gear-dock)`. `styles.css:1792` and `:1805`:

```css
body.sky-mode #stage > *:not(#sky-return):not(#gear-dock),
body.sky-mode .topbar,
body.sky-mode .dock {
  opacity: 0;
  transform: scale(.985);
  pointer-events: none;
  transition: opacity 420ms var(--ease-out), transform 420ms var(--ease-out);
}
...
body.sky-mode #stage > *:not(#sky-return):not(#gear-dock) * { pointer-events: none !important; }
```

**BOTH are required.** The second rule exists because the starry-night carve-out
in `themes.css` re-enables `pointer-events` on leaf elements, and a child's
`pointer-events: auto` beats its parent's `none` — that is PROBLEM-history from
2026-08-20, when an invisible keyboard was still taking key presses. Exempting
the gear from the first rule and not the second would have left it visible and
dead.

Then the dimming, matching `#sky-return` exactly so two escape hatches do not
read as clutter on an empty sky (`styles.css:1834-1851`):

```css
/* The OTHER way back: the settings gear stays reachable in sky mode too
   (owner, 2026-08-27 — a faint arrow alone still stranded people). It is
   exempted from both hiding rules above by :not(#gear-dock); this block only
   adds the faint-until-approached dimming, matching #sky-return exactly, so
   two escape hatches don't read as clutter on an otherwise empty sky. */
body.sky-mode #gear-dock {
  opacity: .28;
  transition: opacity .22s var(--ease-out);
}
body.sky-mode #gear-dock:hover,
body.sky-mode #gear-dock:focus-within {
  opacity: 1;
}
body.sky-mode #gear-btn:focus-visible {
  outline: 2px solid var(--st-accent-brd);
  outline-offset: 3px;
}
:root.reduced-motion body.sky-mode #gear-dock { transition: none; }
```

`.28` is not a taste call — it is `#sky-return`'s value at `styles.css:1828`,
reused so the two ways out are equally faint. `:focus-within` and
`:focus-visible` are there because an escape hatch that only responds to a mouse
is not an escape hatch.

### The actual code — (b), the guard

BEFORE was this exact function WITHOUT line 119. AFTER, `main.ts:105-123`:

```ts
/**
 * Esc is the guaranteed way out of sky mode — but the settings gear now stays
 * reachable while the sky is up too (owner, 2026-08-27: a faint arrow alone
 * still stranded people), and its popover has its own Escape-driven dismissal
 * lower down in bootstrap(). Escape must peel, not clear — the same rule
 * dismissable.ts already applies to its own stack of surfaces: closing an open
 * settings popover takes priority, and Esc only leaves the sky once nothing is
 * open. So bail out here and let the ordinary Escape handler close the
 * popover instead; firing both on one press would exit the sky AND drop
 * whatever the popover was doing.
 */
function wireSkyEscape(): void {
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape" || !document.body.classList.contains("sky-mode")) return;
    if (isSettingsPanelOpen()) return;
    e.preventDefault();
    void leaveSkyMode();
  });
}
```

The one added line is `if (isSettingsPanelOpen()) return;`. The sky handler
DECLINES while something is open above it, and the ordinary handler
(`main.ts:331-334`) does its job:

```ts
    if (e.key === "Escape") {
      if (getCurrentKey()) closePanel();
      else closeAllPopovers();
    }
```

### Why the existing mechanism did not already cover this

`dismissable.ts:88-99` already owns exactly this rule, and it is worth reading
because it is the pattern the fix follows:

```ts
  // Capture phase: sky mode, the special cards and the conflict prompt all
  // listen for Escape too, and the topmost surface must win. Closing exactly
  // ONE per press is what makes a stack of surfaces feel right — Escape should
  // peel, not clear.
  document.addEventListener(
    "keydown",
    (ev) => {
      if (ev.key !== "Escape" || _open.size === 0) return;
      let top: Entry | null = null;
      for (const entry of _open) if (!top || entry.seq > top.seq) top = entry;
      if (!top) return;
      ev.stopPropagation();
      close(top);
    },
    true,
  );
```

Capture phase, topmost surface wins, `stopPropagation`, exactly one close per
press. **The settings panel is not in that registry** — it is closed by
`closeAllPopovers`, not by `dismissable` — so its Escape never reached the
capture-phase arbiter and nothing stopped the sky handler. That is why an
explicit `isSettingsPanelOpen()` guard was the fix rather than "register the
panel with dismissable", which would have been a larger change to a working
popover on the day of a release.

### How it was verified

- `npx tsc --noEmit`: 0 errors. `cargo test --lib`: 167 passed.
- Shipped in 1.0.89 and proven installed on the real machine, 2026-08-27 — see
  PROJECT_STATUS.md for the marker chain and the differential.
- **NOT verified by observation.** Neither half has been looked at on a screen:
  the gear's faintness in sky mode, and Escape peeling one layer at a time, both
  need a human at the keyboard. Labelled untested until the owner reports.
- The reasoning for (b) is not inference — the failure is structural and readable
  from the source: two bubble-phase `document` listeners, neither calling
  `stopPropagation`, both matching `e.key === "Escape"`.

### Generalise this

1. **Two independent listeners on the same node, in the same phase, both
   "correct", is a bug the moment their conditions can BOTH be true.** Nothing
   fails while the states are mutually exclusive — and then a feature makes them
   overlap and one keystroke does two things. When adding a global key handler,
   the question is never "does mine work"; it is "**what else on `document` also
   answers this key, and can we both be right at once?**"
2. **Escape should PEEL, not clear.** One press closes exactly one surface, the
   topmost. Any handler for a key that means "back out" needs an explicit "is
   something above me?" guard, or an arbiter that owns the whole stack. This
   codebase has the arbiter (`dismissable.ts`) — and the bug happened in the one
   surface that was never registered with it. **A stack arbiter only arbitrates
   what it knows about; a surface outside the registry is a surface outside the
   rule.**
3. **Registration order is not a synchronisation primitive.** If two handlers
   must not both run, one of them has to DECLINE — by a guard or by
   `stopPropagation` from the capture phase. Ordering "works" until someone moves
   an import.
4. **When a mode hides everything, enumerate what it must NOT hide, and include
   the control that turns the mode ON.** A `:not()` list is a specification of
   escape hatches. The gear was missing from it for the most natural reason —
   nobody hides a control they are looking at — and the cost was users stranded in
   an empty window.
5. **A fix that adds a surface adds interactions with every existing global
   handler.** (b) exists only because (a) shipped. Whenever something becomes
   reachable in a state where it previously could not exist, re-walk the global
   key handling for that state; that is where the second bug will be.

---

## PROBLEM 214 — the overlay rebuild raced ITSELF: two rebuilds, one label, and the loser switched off the winner's overlay

**This is the fourth time this symptom has been fixed** (PROBLEMS 37, 92, 117,
118). The owner's words when handing it over: *"this requires separate focused
dealing so that it never comes up ever again."* So this entry also records
**why the previous fixes did not hold**, at the end.

**Symptom.** Plug the second display in (or out) — which this owner does several
times a day — then hold Space. Shortcuts still work: apps launch, focus and
minimise. **No Guide HUD and no sound.** It stays that way until the app is
restarted. Reported as recurring on essentially every monitor plug-in.

**The log, captured live on 1.0.89 while the owner reproduced it**
(`%APPDATA%\Spaceadom\debug.log`, 2026-08-28):

```
18:51:03.226 [WARN]  display_watch — display: configuration CHANGED — was [(0,0,2560,1600,150)],
                     now [(0,0,1920,1080,100),(1920,0,2560,1600,150)]. Rebuilding the overlay
18:51:06.547 [WARN]  display_watch — display: configuration CHANGED — was [2 displays],
                     now [(0,0,2560,1440,100)]. Rebuilding the overlay
18:51:08.216 [INFO]  overlay: configured (on-demand, click-through)
18:51:08.216 [INFO]  display_watch — display: overlay rebuilt for the new display configuration
18:51:08.389 [ERROR] display_watch — display: overlay REBUILD FAILED (a webview with label
                     `overlay` already exists) — the HUD and toasts cannot appear until the
                     next display change or a restart
```

**Root cause — a guard that guarded nothing.**

`display_watch.rs` had a `REBUILDING: AtomicBool`, added by PROBLEM 117 with the
comment *"A burst of events must not start two rebuilds."* It did not work, and
the reason is one line of Tauri semantics:

```rust
// tauri-2.11.5/src/app.rs
pub fn run_on_main_thread<F: FnOnce() + Send + 'static>(&self, f: F) -> crate::Result<()> {
    self.runtime_handle.run_on_main_thread(f).map_err(Into::into)   // POSTS. Does not wait.
}
```

`run_on_main_thread` **posts a closure to the event loop and returns
immediately.** The old rebuild treated a successful post as a completed build:

```rust
// BEFORE — display_watch.rs, the last third of rebuild_overlay()
if let Err(e) = app.run_on_main_thread(move || {
    let built = tauri::WebviewWindowBuilder::new(&a2, "overlay", ...).build();
    match built { Ok(w) => { configure_overlay_window(&w); ... }
                  Err(e) => { log::error!("overlay REBUILD FAILED ({e}) ...");
                              OVERLAY_DISABLED.store(true, Ordering::Relaxed); } }
}) {
    log::error!("could not reach the main thread to build the overlay ({e})");
}
done();          // <-- clears REBUILDING here, with the window NOT YET BUILT
```

Replay the owner's timeline against that code:

| time | what actually happened |
| --- | --- |
| 18:51:03.2 | change #1 seen; after the 1.2 s `SETTLE`, rebuild #1 starts, takes `REBUILDING` |
| ~18:51:04.6 | rebuild #1 destroys the old overlay, polls the label free, **queues** the build, calls `done()` → **`REBUILDING` is false again** |
| 18:51:06.5 | change #2 seen; after `SETTLE`, rebuild #2 walks through the unlocked door |
| ~18:51:07.8 | rebuild #2 finds no `overlay` window (#1's replacement is still only queued), so its destroy is a no-op and its "wait for the label to free" passes on the first try; it queues a **second** build |
| 18:51:08.216 | the main thread runs build #1 → **success** |
| 18:51:08.389 | the main thread runs build #2 → `a webview with label 'overlay' already exists` → **`OVERLAY_DISABLED = true`** |

So the end state was: a **correctly built, correctly configured, perfectly
healthy overlay window**, with the one flag that gates every path into it set to
`true` by the rebuild that lost a 173 ms race.

That is the exact symptom. Shortcuts kept working because they are Rust and
never touch the overlay. The HUD died because `show_guide_hud` opens with
`if !OVERLAY_DISABLED`. **The sound died for the same single reason** — see the
verdict below.

**The sound verdict — fully explained by the dead overlay, no independent cause.**
Evidence, not inference:

1. `grep -rni "playsound|MessageBeep|rodio|PlaySoundW|audio" src-tauri/src` returns
   nothing that plays a sound. The only Core Audio in the whole backend is
   `engine/actions/boss_key.rs`, which **mutes**, and `config/schema.rs`'s
   comment on the setting: *"Whether the optional WebAudio sine-tick sound
   effects are enabled."*
2. The entire kit is WebAudio inside the overlay page:
   `src/components/toast.ts:177 fn beep(f)` and the open/close pitch sweep at
   `:205`, both gated on `_soundOn`, both constructing an `AudioContext` in that
   page.
3. Every call site of `beep()` is a toast or HUD render step
   (`toast.ts:621, 705, 714, 751, 789, 810`). Those only run when Rust emits
   `guide-hud-show` / a toast — and **every one of those paths is gated on
   `OVERLAY_DISABLED`** (`guide_hud/mod_impl.rs:263`, `commands.rs:1291`
   `overlay_fit`, `commands.rs:1365` `overlay_fit_handover`).

With the flag set, the page is never asked to render, so it never plays. There
is no second bug. Fixing the flag fixes the sound.

*(Note the contrast with PROBLEM 117, where sound WORKED and only pixels were
missing. That was the compositor failing under a live page. Here the page was
never invoked at all. Same complaint, opposite mechanism — which is exactly why
"the HUD is dead again" must never be diagnosed from memory.)*

**Exact file.** `src-tauri/src/display_watch.rs` (rewritten),
`src-tauri/src/guide_hud/mod_impl.rs` (the show path now reports and asks for a
heal instead of returning in silence).

### Fix 1 of 4 — SERIALISE. Make the guard cover the whole rebuild.

New helper. Every hop onto the main thread now **waits**:

```rust
/// `AppHandle::run_on_main_thread` posts to the event loop and returns
/// immediately. The old rebuild treated it as if it had run, released the
/// `REBUILDING` guard and returned — which is PROBLEM 214's root cause.
///
/// MUST NOT be called from the main thread: it would deadlock waiting for a
/// queue only the caller can drain. The only caller is `st-overlay-rebuild`.
fn on_main_thread_blocking<F, T>(app: &tauri::AppHandle, what: &str, f: F) -> Option<T>
where F: FnOnce() -> T + Send + 'static, T: Send + 'static {
    let (tx, rx) = std::sync::mpsc::channel();
    if let Err(e) = app.run_on_main_thread(move || { let _ = tx.send(f()); }) {
        log::error!("display: could not reach the main thread to {what} ({e})");
        return None;
    }
    match rx.recv_timeout(MAIN_THREAD_TIMEOUT) {   // 20s
        Ok(v) => Some(v),
        Err(_) => { log::error!("display: the main thread did not {what} within {}s — \
                    carrying on. The closure may still run later; the rebuild is \
                    idempotent so a late arrival is harmless (PROBLEM 214).",
                    MAIN_THREAD_TIMEOUT.as_secs()); None }
    }
}
```

and the guard now queues instead of admitting a second runner:

```rust
pub fn rebuild_overlay(app: &tauri::AppHandle) {
    if REBUILDING.swap(true, Ordering::SeqCst) {
        PENDING.store(true, Ordering::SeqCst);          // queue, never concurrency
        log::info!("display: a rebuild is already in flight — this request is queued \
                    behind it rather than run concurrently (PROBLEM 214)");
        return;
    }
    ...
    std::thread::Builder::new().name("st-overlay-rebuild".into()).spawn(move || {
        loop {
            rebuild_once(&app);
            if !PENDING.swap(false, Ordering::SeqCst) { break; }
            log::info!("display: a display change arrived while the last rebuild was \
                        running — running the queued rebuild now (PROBLEM 214)");
        }
        REBUILDING.store(false, Ordering::SeqCst);
        // Tiny window between that swap and this store: honour a request that
        // landed in it rather than losing it.
        if PENDING.swap(false, Ordering::SeqCst) { rebuild_overlay(&app); }
    })
```

### Fix 2 of 4 — COALESCE. Wait for stability, not for a timer.

The old code slept a fixed `SETTLE = 1.2s` after the FIRST change it saw. The
owner's transitions were **3.3 s apart**, so 1.2 s fired *between* them — a
short debounce cannot coalesce a burst that is slower than the debounce. The
test is now STABILITY:

```rust
/// How many CONSECUTIVE polls must report the same configuration before it is
/// treated as settled. Measured from the owner's 2026-08-28 log: one physical
/// plug-in produced transitions 3.3 s apart. Three polls at 2 s means the
/// configuration must hold still for at least 4 s — comfortably past that gap,
/// so the whole storm collapses into ONE rebuild, while worst-case latency from
/// the last transition to the rebuild stays under 8 s.
const STABLE_POLLS: u32 = 3;

pub struct Coalescer { built_against: Vec<Mon>, last_seen: Vec<Mon>,
                       dirty: bool, agreed: u32, needed: u32 }

pub fn observe(&mut self, now: Vec<Mon>) -> Observation {
    if now.is_empty() { return Observation::Ignored; }   // mid-mode-change; never a reset
    if now != self.last_seen {
        self.last_seen = now; self.dirty = true; self.agreed = 1;
        return Observation::Changed;                     // clock restarts
    }
    if !self.dirty { return Observation::Ignored; }
    self.agreed += 1;
    if self.agreed >= self.needed {
        self.dirty = false; self.agreed = 0;
        self.built_against = self.last_seen.clone();
        return Observation::Settled(self.built_against.clone());
    }
    Observation::Settling(self.agreed)
}
```

Two deliberate properties, both unit-tested: an **empty** reading is ignored
without resetting the run (Windows emits empty lists mid-transition), and a
**round trip back to the original arrangement still rebuilds** — PROBLEM 117 is
about composition established against an arrangement that changed, and it
changed whether or not it changed back.

### Fix 3 of 4 — "ALREADY EXISTS" IS SUCCESS.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildOutcome { Built, Adopted, Failed }

/// Pure, so the branch that cost the owner his HUD can be exercised by a test.
pub fn classify_build(built_ok: bool, existing_present: bool) -> BuildOutcome {
    if built_ok { BuildOutcome::Built }
    else if existing_present { BuildOutcome::Adopted }   // the thing we wanted IS THERE
    else { BuildOutcome::Failed }
}
```

and in the build closure:

```rust
let existing = if built.is_err() { a2.get_webview_window("overlay") } else { None };
match (classify_build(built.is_ok(), existing.is_some()), built, existing) {
    (BuildOutcome::Built,   Ok(w),    _)       => { crate::configure_overlay_window(&w); ... }
    (BuildOutcome::Adopted, _,        Some(w)) => {
        crate::configure_overlay_window(&w);        // <-- also clears OVERLAY_DISABLED
        log::warn!("display: the overlay window already existed ({err}) — ADOPTED and \
                    reconfigured it instead of failing. This is the state that used to \
                    switch the HUD and every sound off until a restart (PROBLEM 214).");
    }
    _ => { log::error!("... and no overlay window exists ..."); OVERLAY_DISABLED.store(true, ...); }
}
```

`OVERLAY_DISABLED` is now set on exactly one condition: **the build failed AND
no window with that label exists** — a state in which nothing could have been
shown either way.

### Fix 4 of 4 — SELF-HEAL. Never "until a restart" again.

The old error text admitted the defect in its own wording: *"the HUD and toasts
cannot appear until the next display change or a restart."* There is now a
healer on the watcher's own poll loop:

```rust
pub fn should_self_heal(overlay_disabled: bool, window_present: bool) -> bool {
    overlay_disabled || !window_present     // either symptom is fatal on its own
}

/// Backoff in POLL ticks: immediate, then 2, 5, 15, 30 (4s, 10s, 30s, 60s at a
/// 2s poll). CAPPED, never abandoned — a machine mid-mode-change, or one whose
/// WebView2 is not serviceable yet, gets better on its own.
pub fn heal_backoff_polls(attempts: u32) -> u32 {
    match attempts { 0 => 0, 1 => 2, 2 => 5, 3 => 15, _ => 30 }
}
```

driven each poll:

```rust
if REBUILDING.load(Ordering::SeqCst) { continue; }
if HEAL_ASAP.swap(false, Ordering::Relaxed) { healer.reset(); }
let disabled = crate::guide_hud::OVERLAY_DISABLED.load(Ordering::Relaxed);
let present  = app.get_webview_window("overlay").is_some();
if healer.poll(should_self_heal(disabled, present)) {
    log::warn!("display: the overlay is unusable (disabled={disabled}, \
                window_present={present}) — self-healing, attempt {}. The user must \
                never have to restart the app for this (PROBLEM 214).", healer.attempts());
    rebuild_overlay(&app);
}
```

And the HUD reports instead of failing silently — `guide_hud/mod_impl.rs`, the
`else` arms that never existed before:

```rust
} else {
    // THIS is the state the owner reported as "shortcuts work, the HUD and the
    // sound are dead". The window itself was healthy; a second, racing rebuild
    // had set this flag on its way out. Do not return silently.
    log::error!("guide_hud: OVERLAY_DISABLED is set, so the HUD and every sound are \
                 suppressed (the sound kit is WebAudio inside the overlay page, so it \
                 dies with it). Asking the display watcher to rebuild and re-enable the \
                 overlay; no restart is required (PROBLEM 214).");
    crate::display_watch::heal_now();     // drops the backoff; heals on the next poll
}
```

Worst case from a broken overlay to a working one: **one 2 s poll.**

### The `OVERLAY_DISABLED` audit that was asked for

Every writer in the tree, after this change:

| file:line | writes | when | who clears it |
| --- | --- | --- | --- |
| `lib.rs:635` (`configure_overlay_window`) | **false** | `set_ignore_cursor_events(true)` succeeded | — (this IS the clear) |
| `lib.rs:643` (`configure_overlay_window`) | **true** | click-through failed — fail closed, a HUD that eats clicks is worse than none | the healer: `should_self_heal(true, _) == true`, so a rebuild + reconfigure is attempted forever on the capped backoff |
| `display_watch.rs:574` (`rebuild_once`) | **true** | build failed **and** no `overlay` window exists | the same healer, and any later successful rebuild (`Built` or `Adopted` both call `configure_overlay_window`) |

Readers: `guide_hud/mod_impl.rs:263`, `commands.rs:1291`, `commands.rs:1365`,
`hook/pointer.rs:995`.

**Conclusion: there is no longer any path that can set it and never clear it.**
Before this change there were two — the racing rebuild (the bug above) and the
startup click-through failure, which nothing ever retried.

One unrelated path deliberately left alone: PROBLEM 122's compositing self-test
(`commands.rs:1751`) stops after 3 rebuilds and says "restart the app". It does
**not** set `OVERLAY_DISABLED`, so the healer will not fight it, and its failure
is a genuinely different one (a compositor that cannot draw, with a healthy
window and a healthy flag).

### Tests — 14 new, all on the pure parts

`cargo test --lib`: **167 → 181 passed, 0 failed, 3 ignored.**

Debounce/coalescing: `steady_configuration_never_rebuilds`,
`an_empty_read_is_ignored_and_does_not_reset_the_run`,
`a_change_needs_the_full_run_before_it_rebuilds`,
`the_owners_plug_in_storm_produces_exactly_one_rebuild` (feeds the real
1 → 2 → 1 sequence from his log and asserts **one** rebuild),
`a_round_trip_back_to_the_original_still_rebuilds`,
`monitor_order_does_not_create_a_phantom_change`.

Adopt branch: `a_successful_build_is_built`,
`already_exists_is_adopted_not_failed`,
`a_build_failure_with_no_window_is_the_only_real_failure`.

Self-heal: `a_healthy_overlay_does_not_heal`,
`either_symptom_alone_triggers_a_heal`,
`the_first_heal_is_immediate_then_backs_off`,
`healing_stops_the_moment_the_overlay_is_well_again`,
`the_backoff_is_capped_and_never_gives_up`.

**How it was verified.** `cargo check --lib` 0 errors 0 warnings;
`cargo test --lib` 181 passed. The root cause is not inferred — it is read
directly off the owner's timestamped log and off `tauri-2.11.5/src/app.rs`.

**NOT verified on hardware, and this is the one thing that matters** — see
PROBLEM 118, whose whole lesson was that a repair path which has never been
executed is a guess with good syntax. The agent that wrote this cannot plug a
monitor in. **The owner must, on the installed build:** plug the second display
in, wait ~10 s, unplug it, wait ~10 s, then hold Space. The HUD and the sound
must both appear, `debug.log` must contain **one** `configuration settled …
rebuilding the overlay ONCE` per plug event and **zero** `REBUILD FAILED` lines.
If an `already existed … ADOPTED` warning appears, that is the fix doing its job,
not a fault.

### WHY THE PREVIOUS FIXES DID NOT HOLD

This is the part worth more than the code.

- **PROBLEM 117 fixed DETECTION.** It answered "how does the app learn the
  displays moved?" and shipped a `REBUILDING` guard as a one-line aside. The
  guard was written against the right idea and the wrong API contract.
- **PROBLEM 118 fixed THE REBUILD.** `close()` → `destroy()`, poll until the
  label is free, and stop disabling a working overlay on a failed teardown. It
  was verified across five real display changes — and every one of those five
  was a **single** rebuild. The five-change test never produced the overlap,
  so the surviving defect was invisible to the very test that certified the fix.
- **Neither ever asked "what if two of these run at once?"** Both treated the
  rebuild as an event handler, not as a shared resource with an owner. The
  guard's existence made the question look already answered.
- **And the failure path was still allowed to be destructive.** PROBLEM 118
  removed the worst instance of that (disabling on a failed teardown) but left
  the same shape one step later: disabling on a failed *build*, without ever
  asking whether the thing it was failing to build was already there.

**Generalise this.**

1. ***A recovery path that can itself fail must be idempotent and self-healing,
   or it becomes the new failure.*** Every branch of a repair has to answer:
   *if I run twice, is that harmless?* and *if I fail, does the app get better
   on its own?* Here the answers were "no" and "no", and the repair for a
   several-times-a-day event became a several-times-a-day outage. "Already
   exists" is the canonical shape of the first question: a create that fails
   because the thing exists has **succeeded**.
2. ***A guard released by a call that only REQUESTS work guards nothing.*** Same
   family as PROBLEM 118's `close()`-versus-`destroy()`, one level up:
   `run_on_main_thread` returns `Ok(())` for *queued*, not for *done*. If a lock
   is meant to span an operation, it must be released by the operation's
   completion, not by its submission.
3. ***A test that only exercises the single-actor case cannot certify a
   concurrency fix.*** PROBLEM 118's five clean rebuilds were five clean
   *sequential* rebuilds. To test a guard, you must overlap.
4. ***When the same symptom is fixed for the fourth time, stop fixing the
   symptom and enumerate the mechanisms.*** PROBLEMS 37, 92, 117, 118 and 214
   all read as "the HUD is gone", and the causes were a blur radius, a reset, a
   compositor, a teardown and a race. The one thing they share is that the
   overlay is a long-lived resource with many owners and no arbiter. That is
   what the serialised, queued, self-healing rebuild is now.

---

## PROBLEM 215 — after a laptop reboot the shortcuts were dead for ten seconds, because the WebView2 wait was in front of the keyboard hook

**Symptom.** The owner: *"When restarting my laptop this app needs a long time
to show up. The main features — app opening, the Space HUD and toast — should
come up as soon as possible. The rest of the app, the dashboard and everything
inside it, can happen a bit later."*

**Root cause.** One line, in the wrong place:

```rust
// BEFORE — lib.rs:446, in run(), BEFORE tauri::Builder::default()
if std::env::args().any(|a| a == "--autostart") {
    log::info!("autostart launch — waiting 10s for the shell to settle (PROBLEM 59/76)");
    std::thread::sleep(std::time::Duration::from_secs(10));
}
```

The sleep itself is correct and stays. PROBLEM 59 is measured: at a cold logon
WebView2 is often not serviceable yet, `CreateCoreWebView2Controller` fails with
`HRESULT(0x80070490) ERROR_NOT_FOUND`, Tauri destroys the host window, and the
user gets an app with no dashboard and no Guide HUD while the log claims
success. PROBLEM 76 measured the real logon timings (boot 14:20:48 → Run key
14:22:31 → hook live 14:23:01) and cut 30 s to 10 s.

But it sat **before `tauri::Builder`**, and the hook thread and the engine are
spawned inside `.setup()`. So a wait that exists to protect **WebView2** was
also delaying **`WH_KEYBOARD_LL`**, which has nothing to do with WebView2.
Ten seconds of dead shortcuts, every reboot, for no reason.

**Exact file.** `src-tauri/src/lib.rs`, `src-tauri/tauri.conf.json`,
`src-tauri/src/tray.rs`.

**The code.** The sleep did not move; the *work* moved out from behind it.

1. Both windows are now declared with `"create": false`, which tells Tauri not
   to build them during its own `setup()` (`tauri-2.11.5/src/app.rs`:
   `for window_config in app.config().app.windows.iter().filter(|w| w.create)` —
   and note that loop runs **before** the user's setup closure, which is why the
   old sleep had to be so early to help at all):

```json
{ "label": "settings", "create": false, "title": "Spaceadom", ... }
{ "label": "overlay",  "create": false, "url": "overlay.html", ... }
```

2. A new `pub fn create_app_windows(app_handle: &tauri::AppHandle)` builds them
   from that same declaration and then runs everything that depends on them.
   `from_config` is used rather than a hand-copied builder because a hand-copied
   builder is exactly how PROBLEM 81 produced *"an opaque, decorated,
   focus-stealing rectangle"*:

```rust
for wc in app_handle.config().app.windows.clone() {
    if app_handle.get_webview_window(&wc.label).is_some() { continue; }
    let label = wc.label.clone();
    match tauri::WebviewWindowBuilder::from_config(&app_handle, &wc).and_then(|b| b.build()) {
        Ok(_)  => log::info!("setup: window '{label}' created from its tauri.conf.json declaration"),
        Err(e) => log::error!("setup: window '{label}' could not be created ({e}) — the \
                               PROBLEM 59 recovery below will try again with an explicit builder"),
    }
}
```

   Moved into it, in their original order and unmodified: step **9b** (overlay
   configuration), step **9c** (dashboard work-area fit + `spawn_show_fallback`),
   the **PROBLEM 86** own-window opacity registration, step **11**
   (`setup_close_to_tray`), the **PROBLEM 59** webview-existence check and
   rebuild, and finally `display_watch::start`.

3. `setup()` now ends with the split:

```rust
if autostart_launch() {
    log::info!("autostart launch — hook and engine are LIVE now; only the window/webview \
                creation waits {}s for the shell to settle (PROBLEM 59/76/215). A Space hold \
                before then still launches, focuses and minimises; it simply draws no HUD.",
               AUTOSTART_SETTLE.as_secs());
    let settle_handle = app_handle.clone();
    std::thread::Builder::new().name("st-window-settle".into()).spawn(move || {
        std::thread::sleep(AUTOSTART_SETTLE);
        let h = settle_handle.clone();
        // ON THE MAIN THREAD: window creation is not thread-safe anywhere in
        // Win32, and this is the same hop the display-watch rebuild uses.
        if let Err(e) = settle_handle.run_on_main_thread(move || { create_app_windows(&h); }) {
            log::error!("setup: could not reach the main thread to create the windows after \
                         the settle wait ({e}) — the app has a tray icon and a working hook \
                         but no UI");
        }
    }) /* ...falls back to creating them inline if the thread cannot spawn... */
} else {
    // A manual launch: the same instant Tauri itself would have built them, so
    // nothing about this path changed.
    create_app_windows(&app_handle);
}
```

**Resulting order at logon:** logger → panic hook → config → hook thread →
engine → guide-HUD wiring → conflict scan → **tray icon** → *(settle 10 s)* →
dashboard + overlay + display watcher.

**What happens if Space is held during the settle window — decided, not
accidental.** The shortcut **works**: launch, focus, minimise, boss key, PiP,
opacity are all Rust. **Nothing is drawn.** There is no half-HUD to look broken,
because the overlay window does not exist at all, so every show path takes its
`if let Some(win)` miss and returns before emitting anything. It is logged
calmly rather than as an error, so the owner is not trained to ignore the line
that means something:

```rust
} else if !crate::windows_created() {
    log::info!("guide_hud: still starting — the overlay webview is not built yet \
                (autostart settle, PROBLEM 59/76/215). The shortcut works; no HUD is \
                drawn for this hold.");
}
```

Silent-but-functional, which the owner accepted as the trade.

**PROBLEM 74 is untouched.** `create_app_windows` never shows a window. The
dashboard still appears only when the frontend calls `dashboard_ready`, or via
the 10 s `spawn_show_fallback`. "Boot, then show" still holds.

**Two paths that must not have to wait out the settle.** Asking for the app IS
asking for its UI, so both create the windows immediately
(`create_app_windows` is idempotent — a `WINDOWS_CREATED.swap(true)` gate plus a
per-label existence check):

```rust
// tray.rs — restore_window(), for "Open Settings" and the tray click
if app.get_webview_window("settings").is_none() {
    log::info!("tray: the dashboard was asked for before the settle wait finished — \
                creating the windows now (PROBLEM 215)");
    crate::create_app_windows(app);
}

// lib.rs — the single-instance handler, for a manual launch during the settle
if app.get_webview_window("settings").is_none() { ... create_app_windows(app); }
```

**How it was verified.** `cargo check --lib` 0 errors 0 warnings;
`cargo test --lib` 181 passed. The Tauri ordering claim is read from
`tauri-2.11.5/src/app.rs` (`fn setup`), not assumed.

**NOT verified on hardware.** The agent that wrote this cannot reboot the
machine. **The owner must, on the installed build:** reboot, and from the moment
the tray icon appears, hold Space + a bound letter. It must launch the app
straight away — well before the dashboard is reachable. `debug.log` should show
`setup: hook thread spawned` and `setup: system tray built` within a second or
two of the `--autostart` line, then
`autostart launch — hook and engine are LIVE now; only the window/webview
creation waits 10s…`, and roughly ten seconds later
`setup: window 'settings' created…` / `setup: windows created and configured`.

**Generalise this.** *A delay added to protect one subsystem must be scoped to
that subsystem.* A blanket `sleep()` at the top of `main` is a delay applied to
everything you have not thought about, including the feature the app exists for.
When adding a wait, name the exact resource that is not ready and put the wait
in front of that resource only — and write the name in the log line, so the next
person can tell what it is guarding without reading the commit.

## PROBLEM 216 — WhatsApp relaunched on every press: the launcher had branches that could START an app but had no way to FIND it

**Symptom.** The owner, 2026-08-28, in his words:

> "WhatsApp is launching but WhatsApp is not minimizing. This bug has been dealt
> with so many times and still appears — it needs a permanent fix."

And, when the scope became clear:

> "This is not only the case of WhatsApp and Discord. Ensure that users all
> around the world who use different types of apps do not have to face this
> type of error again."

His live `%APPDATA%\Spaceadom\debug.log`, four presses over two minutes, every
one identical:

```
00:18:47.655  cascade: launching via URI protocol: whatsapp://
00:18:47.817  cascade: ShellExecute accepted whatsapp:// (hInstApp=42, process_created=true)
00:18:48.335  raise_after_launch: 'whatsapp' — you switched to another window, standing down…
```

**Discord is the control that proves the diagnosis, and it is the best evidence
in this entry.** Same app kind, same intent, bound as an ABSOLUTE PATH instead of
a protocol, and it cycles perfectly:

```
23:56:09.113  cascade: launching absolute path: C:\Users\beamu\AppData\Local\Discord\app-1.0.9255\Discord.exe
00:19:16.463  Event: Space+? | Target: Discord.exe | HWND: HWND(0x80bf8) | Action: Restore (Enum) | Rule: Any
00:19:19.740  Event: Space+? | Target: Discord.exe | HWND: HWND(0x80bf8) | Action: Minimize      | Rule: Any
00:22:01.038  Event: Space+? | Target: Discord.exe | HWND: HWND(0x80bf8) | Action: Restore       | Rule: Any
```

Two bindings with the same intent, two different code paths, only one of which
could cycle. That is the whole bug in two log excerpts.

### Root cause

`smart_cascade`'s app leg was a TWO-arm ladder:

```rust
if is_shell_target(app) {
    if aumid_focus_or_minimize(app, &rule) { … }   // Store apps
    …
} else if try_focus_or_minimize(app, &rule) { … }  // everything else, by exe stem
```

but `launch_app_inner` underneath it was a FOUR-case ladder: Store target,
absolute path, **protocol URI**, and resolve-by-name. The protocol-URI case had
no arm in the match ladder above it. Its binding string is `whatsapp.exe`, so
the `else` arm asked the stem matcher for a process called `whatsapp` — and the
Store build of WhatsApp runs as **`WhatsApp.Root.exe`** (measured on this
machine, PID 16984, HWND 0x4902EA, title "WhatsApp"). The stem never matched,
`try_focus_or_minimize` returned `false` with nothing to log, and the press fell
straight through to a re-launch. Every press. For months.

**WHY EVERY PREVIOUS FIX FAILED, and this is the reusable part.** PROBLEMS 79,
170 and 207 all improved a MATCHER — the AUMID matcher, the post-launch raise,
the profile-aware stem matcher. All three are downstream of a decision this path
never reaches. **A branch that returns early is invisible to every fix
downstream of it**, so each fix was verified against the branches that did reach
it, passed, and shipped, while the broken branch carried on doing exactly what
it always did. The bug was not in any matcher. It was in the CONTROL FLOW that
decided which matcher, if any, got asked.

### The full branch audit — the deliverable that proves the class is closed

Every route from a binding to a running program, before this change:

| # | Branch | file:line (before) | How it resolved the binding | Focus/minimize first? | Identity that attempt used | Verdict |
|---|---|---|---|---|---|---|
| 1 | Store / `shell:` verb | `smart_cascade.rs:510`, launch `:2582` | AUMID as written in the binding | YES — `aumid_focus_or_minimize` | AUMID → package family → Apps-folder target exe | **OK** |
| 2 | Absolute `.exe` path | `smart_cascade.rs:517`, launch `:2543` | the path itself | YES — `try_focus_or_minimize` | stem of the path | **OK** — this is Discord's working path |
| 3 | Absolute `.lnk` path | same as 2 | the shortcut; ShellExecute follows it | YES | stem of the **shortcut**, not of its target | **BROKEN when the shortcut's name differs from its target exe** (`NVIDIA App.lnk` → `NVIDIA Share.exe`) |
| 4 | Self-update repair | `:2557` (PROBLEM 116) | re-resolved versioned folder | YES (via 2) | stem of the dead path | OK in practice — a Squirrel app keeps its exe name |
| 5 | **Protocol URI** | launch `:2587`; **no match arm existed** | `protocol_uri()` table → `whatsapp://` | **NO** | — (the `else` arm asked for the binding's own stem, which is a different program) | **TOTALLY BROKEN — the reported bug** |
| 6 | Bare name → known-paths table / App Paths / PATH | launch `:2596` | `resolve_path()` | YES | stem of the **bare name**, not of the resolved path | OK when they agree; broken when they do not |
| 7 | Bare name → Start-Menu `.lnk` | inside `resolve_path`, `find_shortcut` | prefix-matched `.lnk` | YES | stem of the bare name | **BROKEN whenever the shortcut resolves to a differently-named exe** |
| 8 | `web_url`, default browser | `:530`, `run_browser` | OS default browser | YES — `url_focus_or_minimize` | site keyword from the URL | **OK** |
| 9 | `web_url`, specific browser + profile | `open_binding_url:745` | pinned exe + `--profile-directory` | YES | site keyword + `ProfileRule` (PROBLEM 207) | **OK** |
| 10 | Founders fallback, all of the above | `:552-566` | a SECOND copy of branches 1-2's ladder | partially | duplicated by hand | **OK but duplicated** — the exact pair PROBLEM 207 found drifting |

Three broken rows (5, 7, 3) and one duplicated ladder, all with the same shape:
**the launch resolved the binding one way and the match had resolved it
another.**

### The invariant

> **Every launch branch must first attempt focus/minimize using an identity
> derived from the SAME resolution the launch will perform. A branch that can
> launch but cannot match is a bug by construction.**

This is the general form of PROBLEM 207's rule (*"the profile the match leg
demands is exactly the `--profile-directory` the launch leg is about to
pass"*), one level up.

### Exact file: `src-tauri/src/engine/actions/smart_cascade.rs`

**The spine.** One resolution carrying all three halves — the shape the launch
dispatches on, how to FIND the window, and what the post-launch watcher should
look for:

```rust
enum TargetShape { ShellVerb, AbsolutePath, ProtocolUri(String), BareName }

enum MatchIdentity {
    /// → try_focus_or_minimize (stems whatever it is given)
    ExeStem(String),
    /// → aumid_focus_or_minimize, held in `shell:AppsFolder\…` form so that
    /// matcher's Apps-folder fallback can still parse it
    Aumid(String),
    /// Resolve this binding the way the LAUNCH will (known-paths table → App
    /// Paths → PATH → Start-Menu .lnk → the shortcut's target exe) and match
    /// on the stem that produces. LAZY: resolution can walk two Start Menu
    /// trees and this is the Space-hold path.
    ResolvedExeStem(String),
}

enum Identities {
    Try(Vec<MatchIdentity>),      // non-empty BY CONSTRUCTION
    Unmatchable(String),          // an explicit, named, LOGGED decision
}

enum RaiseIdentity { ExeStems(Vec<String>), Resolved(String), Skip(&'static str) }

struct LaunchPlan { shape: TargetShape, identity: Identities, raise: RaiseIdentity }
```

**THE STRUCTURAL GUARANTEE — this is the part that stops it recurring.**

```rust
impl LaunchPlan {
    /// `first` is by value: a zero-identity plan cannot be SPELLED through
    /// this constructor, which is the point.
    fn matchable(shape: TargetShape, first: MatchIdentity,
                 rest: Vec<MatchIdentity>, raise: RaiseIdentity) -> Self { … }

    fn unmatchable(shape: TargetShape, reason: impl Into<String>) -> Self { … }
}
```

* `resolve_launch_plan` is the ONLY function that builds a `LaunchPlan`, and it
  is an exhaustive `match` over `TargetShape`.
* `launch_app_inner` dispatches on `plan.shape` **and on nothing else** — it no
  longer re-derives anything, so it cannot disagree with the match leg about
  which case applies. They are now the same `match`.
* Adding a fifth branch next year means adding a `TargetShape` variant, which
  makes `resolve_launch_plan` **and** `launch_app_inner` **and** the test
  `shape_coverage_is_exhaustive_by_construction` all fail to compile until the
  author has written a launch action, a match identity and a raise identity for
  it. "I forgot to write the matching code" is no longer expressible. The only
  way to say "this cannot be matched" is `LaunchPlan::unmatchable`, which
  demands a reason string that is then logged at `info!`.

**The one match leg.** Both arms of `smart_cascade` (primary and Founders) now
call this, and there is no other way in:

```rust
fn app_focus_or_minimize(target: &str, rule: &ProfileRule) -> bool {
    let plan = resolve_launch_plan(target);
    let ids = match &plan.identity {
        Identities::Unmatchable(why) => {
            // info!, never debug! — debug is filtered out of the shipped log,
            // which is exactly when this line is needed (PROBLEM 38).
            log::info!("cascade: {target:?} ({:?}) has no window identity — {why}. \
                        Launching unconditionally, which is what this binding did \
                        before.", plan.shape);
            return false;
        }
        Identities::Try(ids) => ids,
    };
    // deduped by STEM, because identity #0 carries the whole binding string
    // and a resolved identity carries a bare stem
    …
}
```

`before`:

```rust
if is_shell_target(app) {
    if aumid_focus_or_minimize(app, &rule) { return CascadeOutcome::Primary; }
    if launch_binding_app(binding, app, &rule, app_handle.clone()) { … }
} else if try_focus_or_minimize(app, &rule) {
    return CascadeOutcome::Primary;
} else if launch_binding_app(binding, app, &rule, app_handle.clone()) { … }
```

`after`:

```rust
if app_focus_or_minimize(app, &rule) { return CascadeOutcome::Primary; }
if launch_binding_app(binding, app, &rule, app_handle.clone()) {
    return CascadeOutcome::Primary;
}
```

**Protocol resolution — and the measurement that decided it.** Measured on this
machine 2026-08-29 by `live_protocol_probe` (a `#[ignore]`d, read-only test in
this crate, so the numbers are the production code's own, not PowerShell's):

```text
whatsapp   HKCR\whatsapp                     URL Protocol, and NO shell\open\command AT ALL
           AssocQueryString ASSOCSTR_APPID   5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App
           its window's process              WhatsApp.Root.exe        <- not "whatsapp"
discord    HKCR\discord\shell\open\command   "…\app-1.0.9255\Discord.exe" --url -- "%1"
           AssocQueryString ASSOCSTR_APPID   0x80070002 (none)
spotify    HKCR\spotify\shell\open\command   "…\Spotify\spotify.exe" --protocol-uri="%1"
steam      HKCR\steam\shell\open\command     "C:\Program Files (x86)\Steam\steam.exe" -- "%1"
```

Two shapes, and **neither route alone covers both**: the Store shape has no
command to parse and the classic shape has no AUMID to read. So both are asked,
AUMID first.

**A registry walk cannot answer for the Store shape, and this was verified as a
real negative rather than an empty read.** `HKCR\whatsapp` has a `URL Protocol`
value and no subkeys at all, and
`HKCR\Extensions\ContractId\Windows.Protocol\PackageId` — 48 packages on this
machine, enumerated and printed, so its silence is meaningful — has no WhatsApp
entry. Modern packaged protocol registration lives in the State Repository, not
the registry. `AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_APPID, …)` reads
it; `winreg` cannot. *(A check that cannot produce a negative result is not a
check — so the enumeration was validated against 48 packages it DOES list before
its silence about WhatsApp was believed.)*

The resulting plans, printed by the probe after the fix:

```text
whatsapp.exe  Try([ExeStem("whatsapp.exe"),
                   Aumid("shell:AppsFolder\5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App")])
              raise: Skip("this scheme is handled by a PACKAGED app…")
discord.exe   Try([ExeStem("discord.exe"), ExeStem("discord")])   raise: ExeStems(["discord"])
spotify.exe   Try([ExeStem("spotify.exe"), ExeStem("spotify")])   raise: ExeStems(["spotify"])
steam.exe     Try([ExeStem("steam.exe"),   ExeStem("steam")])     raise: ExeStems(["steam"])
```

**The binding's own stem stays identity #0 on every non-Store shape.** It is
free, it is what every working binding already uses, and for a classic handler
it is already the right answer — so Discord's working path is byte-identical and
the new work only runs when the cheap answer misses.

**Why the cache, and why process-lifetime.** `scheme_identities` is memoised per
scheme, and `resolved_target_stem` per binding string, because both are on the
Space-hold path. A protocol registration changes only when an app is installed,
uninstalled or updated — a few times a year, never without the user noticing,
and always followed by a restart of the app sooner or later. The failure mode of
a stale entry is bounded and self-correcting: a stale exe simply matches no
window and the binding launches, which is exactly today's behaviour.
`resolved_target_stem` additionally **re-validates on every hit** — if the
remembered path has vanished the entry is dropped and re-resolved, so a
self-updating app (PROBLEM 116's class) cannot pin it to a dead folder.

**`raise_after_launch` now takes a LIST**, derived from the same plan:

```rust
fn raise_after_launch(exe_stems: Vec<String>, rule: ProfileRule)
```

It used to be told one stem, taken from the binding string rather than from the
resolution — which is why the owner's log shows it polling for `'whatsapp'`, a
program that does not exist, and then standing down. For a packaged handler the
raise is now skipped with a named reason (the shell foregrounds a packaged
activation itself, and `AllowSetForegroundWindow` covers the handoff); for a
classic handler it polls for the exe the REGISTRY named; for a `.lnk` it polls
for the shortcut's target.

### What was deliberately NOT touched

* **No third matcher was written.** Both existing matchers are reused, because
  they carry the NATIVE_SAFETY protections — explorer/`CabinetWClass` positive
  filter, captionless-window skip, own-PID skip, cloaked-window skip, HWND
  recycle re-validation — and PROBLEM 207's profile discipline. A new matcher
  would have to re-earn all of it and would drift.
* **PROBLEM 207's machinery stays inert for non-browser bindings.** The `rule`
  is threaded through unchanged; the owner's Discord lines still read
  `Rule: Any`, meaning no window property is read and no COM apartment is
  initialised.
* **The `Event: Space+? | Target: … | HWND: … | Action: … | Rule: …` line shape
  is preserved**, including the `Target:` field. That is why
  `MatchIdentity::ExeStem` holds the whole binding string for identity #0
  instead of a pre-stemmed copy — `try_focus_or_minimize` prints its argument,
  and handing it a stem would have quietly deleted the path from the one line
  the owner reads to see the cascade working.

### FAMILY-NAME COLLISION — the PROBLEM 207 trap, checked

`aumid_focus_or_minimize` falls back to the package FAMILY name, so two entry
points of one package are indistinguishable — the mechanism that made two
browser profiles collide. Verdict for these bindings: **not reachable.**
`whatsapp://` is the only scheme on this machine that resolves to a package, and
`5319275A.WhatsAppDesktop` declares exactly one `<Application Id="App">` and one
`windows.protocol` extension (read from its `AppxManifest.xml`). It becomes
reachable only if two bindings resolve to two entry points of ONE package — for
example two schemes of the same app. If that ever appears, the answer is
PROBLEM 207's, one level over: the identity must carry the entry point, and a
window that cannot PROVE it is that entry point must not be touched. It is
recorded here so the next reader does not have to rediscover the mechanism.

### How it was verified

* `cargo test --lib` — **196 passed, 0 failed, 4 ignored** (baseline 181/3; +15
  tests, +1 ignored probe).
* `cargo check --lib` — `Finished dev profile … in 2.24s`, **0 errors, 0
  warnings**.
* `live_protocol_probe` run against the real machine; its output is the table
  above.
* **NOT verified: the keypress itself.** This was written from an agent shell
  that cannot press the owner's keys, and `SetForegroundWindow` is blocked for
  it. Nothing here is claimed as working on the real machine until he presses
  the key. See PROJECT_STATUS for exactly what to do.

The tests worth naming:

* `every_binding_shape_can_be_matched_or_says_why` — table-driven over all seven
  binding shapes; asserts each yields a non-empty identity list OR a named
  reason. **This is the test that fails when someone adds a tenth branch and
  forgets**, and it does not mention WhatsApp anywhere.
* `shape_coverage_is_exhaustive_by_construction` — a wildcard-free `match`, so a
  new `TargetShape` variant breaks the build here too.
* `the_quoted_shell_open_command_shape_parses` /
  `the_unquoted_shell_open_command_shape_parses_too` — both real registry values
  off this machine, plus the unquoted-with-spaces case that a first-whitespace
  parser would truncate to `C:\Program`.
* `a_plain_absolute_exe_is_unchanged_from_before_the_spine` — the regression
  guard on Discord's working path.
* `a_binding_that_names_no_file_is_unmatchable_with_a_reason` — the fall-through.

### Generalise this

* **Before improving a decision, verify the code actually reaches it — a branch
  that returns early is invisible to every fix downstream of it.** Three
  correct fixes to three matchers all shipped, all verified, and none of them
  touched the reported bug, because the reported bug was one `if` above them.
  When the same symptom has been "fixed" more than twice, stop improving the
  fix and go and prove the fixed code executes.
* **When several paths must each do N things, encode it so a path cannot exist
  without doing them, rather than checking that today's paths do.** The old
  code was correct for the branches somebody remembered to check. The new code
  will not compile for a branch nobody checked.
* **A control-flow gap leaves no log line, which is why it survives.** A wrong
  decision logs something wrong; a missing decision logs nothing at all. Every
  "no identity" and "resolved to a different program" case here logs at `info!`
  — `debug!` is filtered out of the shipped log, which is precisely when the
  line is needed (PROBLEM 38).

---

## PROBLEM 217 — the crash reporter could not see the entire UI layer, nor any of the "still running, half broken" states

**Symptom.** Sentry has been live since PROBLEM 195 and it works — a real
`display: overlay REBUILD FAILED` event arrived from release 1.0.89. But two
whole classes of failure could never produce an event, so they were invisible
however many friends hit them:

1. **Every JavaScript error, in both webviews.** A shipped WebView2 has no
   console. The only bridges out of a webview were `frontend_log`
   (`log::info!`) and `overlay_log` (`log::warn!`), and `SENTRY_MINIMUM_LEVEL`
   is `Error`. So a wedged dashboard or a dead HUD — the exact thing a friend
   reports as "it looks broken" — stayed on their machine. The dashboard did
   not even have a `window.onerror` handler.
2. **The degraded-but-running states.** Hook deafness, the compositing
   self-test's dead verdict, `OVERLAY_DISABLED` — all `warn!`, all invisible.
   These are the ones where the user *cannot tell*: the app is up, the tray
   icon is there, and the shortcuts silently do nothing.

**Root cause.** One constant, chosen for volume rather than for content.
`SENTRY_MINIMUM_LEVEL = Error` was picked because `Warn` would send thousands
of routine lines a session (the spacedesk/PowerToys conflict warnings alone
fire constantly) and bury the signal. That reasoning is correct and the
constant stays where it is — but it was doing two jobs at once: deciding
*volume* and, by accident, deciding *which failures exist*. The frontend was
below the line for no better reason than that `frontend_log` had been written
for boot breadcrumbs.

**Exact files.**

| File | Change |
| --- | --- |
| `src-tauri/src/telemetry.rs` | `DEGRADED_TARGET`, `Degraded`, `report_degraded`, `report_frontend_error`, `RateLimiter`, `scrub`, `cap`; `log_filter` gains the target skip |
| `src-tauri/src/commands.rs` | new `frontend_error` / `overlay_error` commands; three `report_degraded` calls in `compositing_selftest` |
| `src-tauri/src/lib.rs` | both commands registered; the click-through-failure site |
| `src-tauri/src/hook/mod.rs` | the `hook: DEAF` site |
| `src-tauri/src/display_watch.rs` | the `overlay REBUILD FAILED` site |
| `src-tauri/src/guide_hud/mod_impl.rs` | the `OVERLAY_DISABLED is set` site |
| `src/js-error-reporter.ts` | NEW — the shared, leaf, non-throwing, non-recursing, rate-limited reporter |
| `src/main.ts`, `src/overlay.ts` | install it |
| `PRIVACY.md` | what is sent is now three things, not two |

### The actual code

**1. A SIBLING COMMAND, NOT A LEVEL PARAMETER.** `frontend_log` and
`overlay_log` have call sites all over `src/` (key-detail-panel, toast, main,
overlay), every one of them passing a bare `msg`. A level argument means either
touching all of them or an `Option<Level>` that reads as "somebody forgot" at
every existing site. A second command touches none of them:

```rust
// commands.rs — BEFORE (unchanged, still there, still INFO)
#[tauri::command]
pub fn frontend_log(msg: String) {
    log::info!("dashboard-js: {msg}");
}

// commands.rs — AFTER, added beside it
#[tauri::command]
pub fn frontend_error(msg: String) {
    crate::telemetry::report_frontend_error("dashboard-js", &msg);
}
#[tauri::command]
pub fn overlay_error(msg: String) {
    crate::telemetry::report_frontend_error("overlay-js", &msg);
}
```

The `dashboard-js:` / `overlay-js:` prefix is applied on the RUST side, in one
place, so the log convention cannot drift per call site.

**2. AN EXPLICIT LIST, NOT A WIDER THRESHOLD.** The promoted set is an enum
someone can read in one screen, not a level that silently enrols every future
`warn!`:

```rust
pub enum Degraded {
    HookDeaf,                   // hook/mod.rs        — every shortcut is dead
    OverlayCompositingStrike,   // commands.rs        — overlay drew nothing
    OverlayCompositingDead,     // commands.rs        — GPU composition declared dead
    OverlayDisabled,            // lib.rs, guide_hud  — HUD + all sound suppressed
    OverlayRebuildFailed,       // display_watch.rs   — no overlay window exists
}
```

Call site, e.g. `hook/mod.rs` (the existing `warn!` is UNCHANGED — the log
wording someone greps for must not move):

```rust
log::warn!("hook: DEAF for the last {elapsed_s}s — …");     // unchanged
crate::telemetry::report_degraded(                          // added
    crate::telemetry::Degraded::HookDeaf,
    &format!("the primary keyboard hook saw 0 events in {elapsed_s}s while the \
              reference hook fired {ref_silence}ms ago — …"),
);
```

**3. THE DOUBLE-SEND TRAP, AND WHAT SOLVES IT.** Three of the promoted sites
were ALREADY `log::error!`, so they already reached Sentry through the log
bridge — unbounded. `guide_hud`'s is the worst shape possible: it fires on
every hold while `OVERLAY_DISABLED` is set, i.e. one event per key press.
Adding `report_degraded` beside them would have sent each twice.

The fix is a log TARGET, not a level change — the local `debug.log` line keeps
its ERROR severity and its exact wording, and the bridge learns to skip it
because it has already been reported, rate-limited, by hand:

```rust
// telemetry.rs
pub const DEGRADED_TARGET: &str = "spaceadom::degraded";

pub fn log_filter(metadata: &log::Metadata<'_>) -> sentry_log::LogFilter {
    if !SENDING_ENABLED.load(Ordering::Relaxed) { return LogFilter::Ignore; }
    if metadata.target() == DEGRADED_TARGET { return LogFilter::Ignore; }  // added
    if metadata.level() <= SENTRY_MINIMUM_LEVEL { LogFilter::Event } else { LogFilter::Ignore }
}
```

```rust
// guide_hud/mod_impl.rs — BEFORE
log::error!("guide_hud: OVERLAY_DISABLED is set, …");

// AFTER — same level, same words, off the automatic bridge
log::error!(target: crate::telemetry::DEGRADED_TARGET, "guide_hud: OVERLAY_DISABLED is set, …");
crate::telemetry::report_degraded(crate::telemetry::Degraded::OverlayDisabled, "…");
```

**4. THE RATE LIMITER.** Hook deafness fired **38 times in one session** on the
owner's machine. That must be one event. Keyed per condition; the key is also
the Sentry fingerprint, so the 38 collapse into one issue rather than 38:

```rust
const DEGRADED_COOLDOWN: Duration = Duration::from_secs(15 * 60);
const MAX_PER_KEY: u32 = 3;             // per run of the app
const MAX_EVENTS_PER_PROCESS: u32 = 25; // across every key
```

`RateLimiter::check(&mut self, key, now)` takes `now` as a PARAMETER rather
than reading the clock, which is the only reason a fifteen-minute window can be
tested in microseconds. A suppressed occurrence is counted, and the next event
that does get through carries `(+37 further occurrence(s) suppressed)` — one
event must never read as "it happened once".

**A dead guard the test found.** A `MAX_KEYS: usize = 64` was written to bound
the key map. It was unreachable: an entry is only ever created on a SEND, and
sends are capped at `MAX_EVENTS_PER_PROCESS = 25`, so the map can never exceed
25 entries. It was deleted rather than left in — an unreachable safety check is
worse than none, because it reads as protection that was never exercised. The
assertion now states the real property: 500 distinct errors produce 25 map
entries.

**5. THE FRONTEND REPORTER — three rules, because telemetry must never become
a crash source.** `src/js-error-reporter.ts`, a LEAF module (imports only
`invoke`, so the overlay can use it without dragging the dashboard in —
PROBLEM 148):

- **Never throws.** The whole handler body is in a `try`, the `invoke` is
  `.catch()`ed, and `String(value)` is itself guarded (a thrown object whose
  `toString()` throws is a real thing).
- **Never recurses.** A module-level `reporting` flag is held across the whole
  synchronous body. An error raised inside the handler returns immediately
  instead of re-entering — without this, one fault in the reporter is an
  infinite loop and a hung webview.
- **Never spams.** Same signature at most once a minute; 24 distinct
  signatures; 20 reports per page load, after which it says so once and goes
  silent. Rust rate-limits again on its side, so a caller invoking the command
  directly is bounded too.

What it sends: message, source file, line, column, and the stack when there is
one. `overlay.ts` already had BOTH listeners (reporting to `overlay_log`, i.e.
WARN). They were REPLACED by `installJsErrorReporter("overlay_error")`, not
supplemented — there is still exactly one `error` and one `unhandledrejection`
listener on that window. `main.ts` installs at MODULE level, not inside
`bootstrap()`: an exception thrown *during* bootstrap is the most valuable one
there is, and a handler registered at the end of bootstrap misses exactly those.

**6. PII, AND THE LEAK THAT WAS NEARLY SHIPPED.** A JS stack legitimately
contains the app's OWN bundle paths (`http://tauri.localhost/assets/index-….js`)
and those are KEPT — they are identical on every installation and they are the
only thing that makes a minified stack readable. Everything else is scrubbed in
Rust: drive-letter paths, UNC paths, and any URL whose host is not
`tauri.localhost` / `localhost` / `127.0.0.1`.

That was not enough, and grepping the command layer is what showed it:

```rust
// commands.rs, seven of these
return Err(format!("Profile '{name}' not found"));
```

A rejected `invoke` rejects with that STRING. An unhandled rejection would
therefore have carried the user's **profile name** — user-derived data, exactly
what PRIVACY.md promises is never sent. Rust cannot tell that string from a real
JS error; the frontend can, because it has the type:

```ts
const isError = reason instanceof Error;
const text = isError ? clip(reason.message, 500) : redactQuoted(clip(reason, 500));
```

Quoted runs are redacted for non-`Error` reasons only. A real `Error` keeps its
quotes, because `Cannot read properties of undefined (reading 'offsetWidth')`
quotes a PROPERTY name — the entire diagnosis, and nothing about the user.
Nothing in `src/` throws with config-derived text (there is not one
`throw new Error` in the tree), so an `Error`'s message is always the browser's
or the engine's own words.

**7. NO SECOND PANIC HOOK.** Nothing here touches `std::panic::set_hook`. The
one hook in `lib.rs` already routes to `log::error!`, which already reaches
Sentry, and that path is untouched (PROBLEM 131).

### How it was verified

- `cargo test --lib` — **199 passed, 0 failed** (baseline 196; +3).
  - `the_rate_limiter_collapses_a_storm_into_one_event` — 38 occurrences give
    one event, the backlog count is carried, a different condition is
    unaffected, `MAX_PER_KEY` holds across ten cooldowns, and 500 unique keys
    produce 25 events and 25 map entries.
  - `scrub_removes_everything_that_identifies_the_machine` — the user name, the
    drive path, the UNC host and the third-party URL all die; the app's own
    bundle path and the diagnosis itself survive; `src/lib.rs:744` is untouched.
  - `frontend_error_is_error_while_frontend_log_stays_info` — installs a
    capturing `log::Log` and asserts the real records: `frontend_log` is
    `Level::Info`, `frontend_error` and `overlay_error` are `Level::Error`, the
    `dashboard-js:` / `overlay-js:` prefixes are intact, and the error record
    carries `DEGRADED_TARGET`.
  - `the_kill_switch_actually_kills` — extended, not duplicated (adding a second
    test over the same process-global statics is PROBLEM 130). With sending off,
    `report_degraded` and `report_frontend_error` both return `false` and submit
    nothing; a `DEGRADED_TARGET` record is dropped by the bridge.
- `cargo check --lib` — 0 errors, **0 warnings**.
- `npx tsc --noEmit` — clean. `npm run build` — clean.
- **NOT VERIFIED, AND NOT VERIFIABLE FROM HERE: that an event reaches the Sentry
  dashboard.** Nothing was built, bundled or installed. The proof procedure is
  in PROJECT_STATUS.md's entry for this date.

### Generalise this

* **A reporting threshold chosen for volume decides which failures you will
  never hear about — pick it from what users report, not from what is cheap to
  send.** `Error` was chosen because `Warn` was too noisy. Both are true, and
  the answer was neither: an explicit list of conditions, with per-condition
  rate limiting, gets the four failures that matter without the thousands that
  do not.
* **A threshold silently widens; a list has to be edited.** `SENTRY_MINIMUM_LEVEL
  = Warn` would have enrolled every `warn!` anyone adds from now on, without
  anyone deciding to. `enum Degraded` cannot grow by accident.
* **An unbounded reporting path attached to a per-event code path is a flood
  waiting for the right bug.** `guide_hud`'s `OVERLAY_DISABLED` line was already
  `error!` and already reaching Sentry — once per key press, for as long as the
  flag was set. Reporting paths need a rate limit for the same reason retries
  need a backoff.
* **When you add a second reporting path over the same records, something has to
  decide which one owns them.** A log target is a cheap, greppable answer that
  keeps the local log's severity honest.
* **Type information the frontend has and the backend does not is a PII
  boundary.** Rust sees one `String` and cannot tell a browser's error message
  from a Rust command's rejection that quotes a profile name. The webview can,
  so the redaction has to happen there.
* **An unreachable safety check is worse than no safety check.** It reads as
  protection to everyone who comes after, and it has never once been exercised.

---

## PROBLEM 218 — the Guide HUD outlived its hold, and "shortcuts are dead in my own window" was never a guard

**Symptom.** Two reports from the owner on 2026-08-28, filed separately.

(A) *"While holding the space to see the Space HUD, I opened the Spaceadom app.
Then the Space HUD froze — it interacted, but even after I left my hand from the
space it stayed. And it ultimately opened whatever my cursor was towards."*

(B) *"I did want the Space HUD and the keyboard functions to work even while
using my Spaceadom app. At some versions it used to work within the app. Then it
stopped. A user has to minimize Spaceadom in order to use its functions."*

**Root cause — and they are ONE fault seen from two ends, not two faults.**

The fault is that **this app's hooks get evicted, and the Guide HUD's only
teardown path runs inside the hook that just went away.**

`HookEvent::SpaceUp` is the sole route to `engine::cancel_hud(false)`, which is
the sole route to `hide_guide_hud_pending(false)` on an ordinary release. It can
only be sent by a callback that is still installed. So a hold that straddles an
eviction loses its Space-UP outright, and with it every piece of state that
release was going to clear:

* `MODIFIER_ACTIVE` stays latched,
* `HUD_VISIBLE` stays true, so the ring stays on screen,
* the published chips stay published.

Report (A) is the *visible* end of that, and its third clause is what proves the
mechanism. The MOUSE hook is a **separate hook** and often survives the eviction
that took the keyboard hook, so `pointer::note_cursor` keeps running,
`st-hud-pointer` keeps arming chips against the stranded ring, and the next
`WM_LBUTTONDOWN` activates whichever chip the cursor happens to point at —
*"it interacted"*, then *"it ultimately opened whatever my cursor was towards"*.
Both halves of his sentence fall out of one lost Space-UP.

Report (B) is the *invisible* end of the same eviction: while the hook is gone,
`Space + key` reaches nothing. Minimising Spaceadom removes the WebView2 render
load that PROBLEM 134 already identified as what starves the callback past
`LowLevelHooksTimeout`, so the app starts working again and the minimising looks
like the cure.

**What (B) is NOT, checked rather than assumed.** Every candidate for a
deliberate "stand down while our own window is focused" guard was tested and
ruled out:

| Candidate | Verdict | Evidence |
| --- | --- | --- |
| `FG_IS_SELF` (`hook/mod.rs`) | not a gate | its only reader is `KB_EVENTS_OWN_FG.fetch_add` — a diagnostic counter |
| `FULLSCREEN_ACTIVE` | cannot match us | `check_fullscreen` requires `WS_POPUP` + `WS_EX_TOPMOST` + full monitor; the dashboard is decorated and not topmost. `fullscreen-suppressed:0` in 374 of 381 diagnostics lines |
| `EXCLUDED_ACTIVE` | not set | the live process printed `exclusions: 0 app(s) excluded — []` from 2026-08-27 10:45 on, and `excluded-app:0` in **all 381** diagnostics lines |
| a binding-capture mode swallowing Space | does not exist | there is no press-a-key-to-bind mode anywhere in `src/` — bindings are assigned by clicking a key tile and picking an app |
| a commit that introduced a guard | none | `git log -S` on `FG_IS_SELF`, `is_self`, `GetCurrentProcessId`, `std::process::id` finds only the diagnostic counter (`0018e04`) and `pip.rs`'s refusal to PiP itself (`7ae9ba6`) |

So there was **no guard to remove and no narrower case to preserve.** The rule
the owner asked for is therefore stated as an invariant rather than a
carve-out — see "The new rule" below.

**One genuine own-window stand-down path did exist, and it was unguarded in
Rust.** `hook/exclusions.rs::publish_excluded_apps` honoured whatever
`cfg.excluded_apps` contained. The frontend picker refuses `spaceadom` with a
comment naming this exact trap — but `excluded_apps` also arrives from a
hand-edited `config.json`, a restored backup, an import and a schema migration,
none of which pass through that picker. Any of those would have produced report
(B) *verbatim and deterministically*, with no log line naming the cause. It was
not the cause this time (measured above), and it is closed anyway.

**Exact files.**

* `src-tauri/src/hook/mod.rs` — the SPACE-UP block moved above the three
  stand-down gates; `SPACE_TICK_TS` / `SPACE_REPEATS` / `hold_is_stale` /
  `reap_stale_hold` / `STALE_HOLDS_REAPED` added; `previous_worked` made
  per-hook; the hold-off line promoted from `debug!` to a throttled `info!`;
  `FG_SAMPLES` / `FG_SELF_SAMPLES` / `WD_ALARMS` / `WD_ALARMS_OWN_FG` added and
  reported; `stale_hold_tests` (5 tests).
* `src-tauri/src/hook/pointer.rs` — `reap_stale_hold()` called at the top of the
  `st-hud-pointer` tick.
* `src-tauri/src/hook/exclusions.rs` — `own_stem()`, `without_self()`, the
  self-exclusion drop in `publish_excluded_apps`, 3 tests.

### The actual code — part 1: whoever eats the down owes the up

`kb_hook_proc` ran its three stand-down gates BEFORE the SPACE-UP branch, and
all three `return CallNextHookEx` for every event:

```rust
// BEFORE — ordering only, but the ordering IS the bug
if FULLSCREEN_ACTIVE.load(Ordering::Relaxed) { ...; return CallNextHookEx(...); }
if EXCLUDED_ACTIVE.load(Ordering::Relaxed)   { ...; return CallNextHookEx(...); }
if BYPASS_MODE.load(Ordering::Relaxed)       { ...; return CallNextHookEx(...); }
...
if vk == VK_SPACE && is_up {          // <-- unreachable once any gate is TRUE
    if !SPACE_INTERCEPTED.swap(false, Ordering::Relaxed) { ... }
    ...
    send_event(HookEvent::SpaceUp { modifier_fired });   // the ONLY HUD teardown
}
```

Three flags, three 500 ms pollers and one user-facing toggle can each flip
**mid-hold**. When one did, the user lost the space they typed (we ate the down
and never injected the up) *and* the HUD teardown. The block now sits above all
three:

```rust
// AFTER — hook/mod.rs, immediately after the diagnostics counters
if vk == VK_SPACE && is_up {
    // If we never swallowed the matching down-stroke, this up-stroke
    // belongs to the OS. Injecting here would duplicate the space.
    if !SPACE_INTERCEPTED.swap(false, Ordering::Relaxed) {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }
    SPACE_REPEATS.store(0, Ordering::Relaxed);
    // ... body unchanged: inject_space / take_armed_key / send_event ...
}

// --- Fullscreen / App exceptions / Bypass gates now follow ---
```

Moving it up cannot change behaviour for a hold that *started* inside a
stand-down: those never set `SPACE_INTERCEPTED`, so the first line passes them
to the OS byte-identically to before. Cost: two register compares, no syscall.

### The actual code — part 2: the stale-hold reaper

Part 1 cannot help when the callback is not called at all. That needs a liveness
signal read from OUTSIDE the hook.

`GetAsyncKeyState(VK_SPACE)` is forbidden here and always will be: we suppress
Space-down, so Windows never marks it pressed and the API reports a physically
held Space as UP. Building a failsafe on it broke every shortcut in the app
once already.

**Windows auto-repeat is the signal instead.** A physically held Space produces
a fresh `WM_KEYDOWN` every repeat period, and every one of them enters this
callback — which is exactly why the down branch says *"always suppress Space
down to prevent auto-repeat leaking to the OS"*. Auto-repeat stops the instant
the key comes up **or** the hook stops being called, and those mean the same
thing to the HUD.

```rust
// hook/mod.rs — SPACE DOWN branch. One relaxed store; one relaxed add on
// repeats. Space-down branch ONLY, so per-keystroke callback cost is unchanged.
if !MODIFIER_ACTIVE.load(Ordering::Relaxed) {
    // ... existing fresh-hold setup ...
    SPACE_REPEATS.store(0, Ordering::Relaxed);
    send_event(HookEvent::SpaceDown);
} else {
    SPACE_REPEATS.fetch_add(1, Ordering::Relaxed);   // an AUTO-REPEAT
}
SPACE_TICK_TS.store(now, Ordering::Relaxed);
SPACE_INTERCEPTED.store(true, Ordering::Relaxed);
```

```rust
// hook/mod.rs — the decision, pure so it can be tested
const MIN_OBSERVED_REPEATS: u32 = 2;
const STALE_HOLD_GRACE_MS: u64 = 2_000;

pub(crate) fn hold_is_stale(
    modifier_active: bool,
    repeats: u32,
    since_last_tick_ms: u64,
    grace_ms: u64,
) -> bool {
    modifier_active && repeats >= MIN_OBSERVED_REPEATS && since_last_tick_ms > grace_ms
}
```

`repeats >= MIN_OBSERVED_REPEATS` is **the check that can produce a negative**.
On a keyboard, driver or accessibility setting with auto-repeat OFF, a real hold
produces zero repeats, the reaper never arms, and the app falls back to today's
behaviour. Without it, the reaper would tear the HUD down two seconds into every
legitimate hold on such a machine — the fix being worse than the bug.

`reap_stale_hold()` claims the hold with a `swap` (two callers race), then
performs exactly the reset set `watchdog_check` already performs after an
eviction — `SPACE_INTERCEPTED`, `SPACE_ABORTED`, `pointer::reset_on_eviction()`
(which clears the armed chip and the `CLICK_EATEN` latch) — and finally
`guide_hud::hide_guide_hud()`. It is called from **two homes with different
failure modes**:

* `st-hud-pointer` (`hook/pointer.rs`), at the top of the tick and **above** the
  `enabled` bail-out — the stranded HUD is not a pointer-activation feature and
  must be repaired for users who have that setting off. An independent thread,
  so it still runs when the hook thread is the thing that is stuck.
* the hook pump's `WM_TIMER` branch (`watchdog_check`), **above every early
  return in it** — including the `millis_since_last_input() >= 2000` idle
  return, because the person most likely to notice a stranded HUD is the one who
  has stopped touching the keyboard. This second home exists because the pointer
  thread's spawn is allowed to fail (PROBLEM 124) and its failure message says
  *"keyboard shortcuts are unaffected"*, which would otherwise have become a lie.

### The actual code — part 3: a keyboard repair is proven by a keyboard event

```rust
// BEFORE — hook/mod.rs, watchdog_check cooldown
let previous_worked = LAST_KB_EVENT.load(Ordering::Relaxed) > last
    || LAST_MS_EVENT.load(Ordering::Relaxed) > last;
```

In the `kb_only_dead` failure the mouse hook being alive is **the premise** — it
is how we know the keyboard hook alone was evicted. So a mouse event was being
read as evidence against the very condition it helps establish. With the mouse
in the user's hand, `previous_worked` was true on every tick and the watchdog
held off for the full 60 seconds while the keyboard stayed dead.

```rust
// AFTER
let previous_worked = if kb_only_dead {
    LAST_KB_EVENT.load(Ordering::Relaxed) > last
} else {
    LAST_KB_EVENT.load(Ordering::Relaxed) > last
        || LAST_MS_EVENT.load(Ordering::Relaxed) > last
};
```

And the line that reports the hold-off was `log::debug!`, while release builds
run at `Info` (`logger.rs`) — so **the busiest decision in the watchdog has
never once appeared in the owner's log**, and "held off for 60s" and "the
watchdog never noticed" were indistinguishable from outside. Promoted to `info!`
and throttled to one line per cooldown window via `HOLDOFF_LOGGED_FOR`.

### The actual code — part 4: Spaceadom can never exclude itself

```rust
// hook/exclusions.rs
pub fn without_self(list: Vec<String>, own: &str) -> (Vec<String>, Vec<String>) {
    let own = normalize_stem(own);
    let (dropped, kept): (Vec<String>, Vec<String>) =
        list.into_iter().partition(|e| !own.is_empty() && *e == own);
    (kept, dropped)
}
```

called from `publish_excluded_apps`, which now logs at `error!` naming the files
the entry must have come from. `own_stem()` falls back to the literal
`"spaceadom"` when `current_exe()` fails, because failing toward "no name" would
turn the guard into a no-op — and a guard that cannot fire is not a guard.

### The new rule for (B), stated plainly

> **Spaceadom never stands itself down for its own window.** Focus is not, and
> must never become, an input to the decision to act. There is exactly one
> narrower case that would justify one — a mode in which the user is CAPTURING a
> keystroke to assign it, where Space + key must edit the binding rather than
> fire it — and that mode does not exist in this app: bindings are assigned by
> clicking a key tile and picking an app. **If a capture mode is ever added, the
> guard belongs to "a capture is in progress", never to "our window is
> focused".** The two are not the same condition, and conflating them is what
> would make the dashboard a dead zone again.

Corollary, now enforced in code: nothing may put our own exe into a list that
stands the hook down (part 4).

### On the pointer click, with shortcuts live inside our own window

A left-click is eaten **only** when a chip is ARMED, which needs the ring
visible, the cursor to have travelled, and a dwell — all of which require a live
hold. Ordinary clicking while holding Space already passes through untouched
(`hook/mod.rs`, `WM_LBUTTONDOWN`), so clicking our own UI is only ever eaten
during a genuine hold, which is the documented PROBLEM 206 gesture and correct.
What made the app unusable was the arm **outliving** the hold. Parts 1 and 2
close that from both ends, and the reaper explicitly clears `ARMED_INDEX` and
`CLICK_EATEN` (via `pointer::reset_on_eviction`), so a stranded arm can no
longer eat a click after its hold is over. Paired click suppression is
untouched.

### How it was verified

* `cargo test --lib` — **207 passed, 0 failed** (196 baseline + 8 added here + 3
  from the concurrent telemetry work).
* `cargo check --lib` after `touch src/lib.rs` (a FULL recheck, not an
  incremental one) — **0 errors, 0 warnings**.
* `npx tsc --noEmit` — clean, exit 0. No TypeScript was changed.
* Root-cause evidence, measured from the owner's LIVE
  `%APPDATA%\Spaceadom\debug.log` (1.3 MB, last write 2026-08-29 01:05 — that
  log is NOT shadowed by the agent container; `config.json` beside it is, and
  was deliberately not used):
  - **660** watchdog alarms carry a kb/mouse/ref triple; **143** `DEAF` lines.
  - `fullscreen-suppressed` is 0 in 374 of 381 diagnostics lines;
    `excluded-app` is 0 in **all 381**; the live exclusion list printed
    `0 app(s) excluded — []`. Fullscreen and app-exceptions are ruled OUT as the
    cause of (B) by measurement, not by reading.
  - **68,266** key events seen, **29** of them while the Spaceadom window held
    the foreground (0.04%) — while **29 of 360** watchdog alarms (8%) name
    `spaceadom.exe` as the foreground. Suggestive of focus-specificity, and
    deliberately NOT claimed as proof: the numerator is per-keystroke and the
    denominator it needs is per-second-of-focus. That denominator is exactly
    what the new `hook focus exposure` line adds.
* **NOT verified, and it cannot be from here:** neither report can be reproduced
  by this agent. `SendInput` from the containerised agent shell returns success
  and the hook sees nothing (Testing laws), and `SetForegroundWindow` is blocked
  for the agent. Both fixes are UNTESTED ON HARDWARE and are labelled so.
  Nothing was built and nothing was installed.

### Generalise this

1. **A teardown that lives inside the thing that can disappear is not a
   teardown.** The HUD's only hide path ran inside the hook callback. Any
   visible state must have at least one owner that outlives the mechanism which
   normally clears it — here, an independent thread on a bounded clock.
2. **Whoever eats the down owes the up — and the code that discharges the debt
   must run BEFORE every gate that can stand the app down.** This app had
   already written that rule twice (`SPACE_INTERCEPTED`, `CLICK_EATEN`) and
   still placed the discharge below three early returns. A rule is only as good
   as its position in the function.
3. **A liveness signal must come from the mechanism you are trying to check, not
   from one that survives its failure.** A mouse event cannot prove a keyboard
   hook was repaired. Ask the hook that is actually dead.
4. **A check that cannot produce a negative is not a check — so make a reaper
   prove its own premise first.** Requiring two observed auto-repeats before
   arming is what stops a machine with auto-repeat disabled from having its
   legitimate holds destroyed by the fix.
5. **A rule enforced only in the UI is not enforced.** The self-exclusion trap
   was correctly identified and correctly guarded — in TypeScript, at one of the
   five doorways. Put the rule where the value is CONSUMED.
6. **A decision logged at `debug!` in a release build that runs at `Info` is a
   decision that does not exist.** Anything that can silently keep the app
   broken for a minute has to say so at a level the user's log will contain.
7. **Before accepting "there must be a guard", enumerate the candidate guards
   and rule each one out with a measurement.** Four of the five candidates here
   were disproved by counters already in the log; the fifth by `git log -S`. The
   answer was that the guard never existed — which is only a useful finding if
   it is proved rather than assumed.

---

## PROBLEM 219 — a fullscreen video cannot be cornered by taking it out of fullscreen, so Space+Tab moves it *while it stays fullscreen*

**Not a bug report. A feature the existing key could not grow into**, requested
2026-08-29. It is written up here anyway because the mechanism, the
measurement and the two traps it walked into are exactly the things this file
exists to stop the next agent re-deriving.

### Symptom

The owner watches a video fullscreen in Brave and wants it cornered showing
**only the video** — no tabs, no address bar. Space+` cannot do it, by
construction rather than by accident: its first act is to take the window out
of its maximised/fullscreen state, and every piece of browser chrome comes back
with it. What he asked for is the opposite order of operations — keep the
window in fullscreen and shrink it — so the page still believes it is
fullscreen, the video keeps filling the (now small) window, and no browser UI
reappears.

### Root cause — measured, not reasoned

A fullscreen Chromium window **vetoes an ordinary move**. Measured 2026-08-29
against a throwaway Brave launched with its own `--user-data-dir` (the owner's
profile and session were never touched), driven from PowerShell with
`SetProcessDpiAwareness(2)` so the numbers are physical pixels:

| call | flags | result |
|---|---|---|
| `SetWindowPos` to a 1280x800 corner tile | `SWP_NOZORDER\|SWP_NOACTIVATE` | **ret=TRUE, err=0 — and the window is back at 0,0 2560x1600 within 40 ms** |
| `MoveWindow` to the same tile | — | same: reverted to full monitor |
| `SetWindowPos` to the same tile | `… \| SWP_NOSENDCHANGING` | **moved, and STAYED** — held through a full four-corner cycle and a restore, sampled at +150 ms and +1.65 s per corner and +4 s settled |

Throughout the successful case the style stayed `0x160B0000` — no `WS_CAPTION`,
no `WS_THICKFRAME` — i.e. the window never left fullscreen and never re-drew
its chrome. The windowed control in the same harness reported `0x16CF0000`; the
two styles differ in exactly `WS_CAPTION | WS_THICKFRAME`, which is the whole
structural half of the fullscreen test.

**The mechanism**: Chromium reasserts its monitor bounds from
`WM_WINDOWPOSCHANGING`. `SWP_NOSENDCHANGING` suppresses that message, so the
veto is never *asked for* rather than being overruled. `ret=TRUE` from a call
that is about to be undone is why "the API succeeded" was never evidence of
anything here.

**A finding that falls out of the same measurement and is NOT fixed**: today's
Space+` cannot corner a fullscreen Chromium window either, for exactly this
reason — it places with unsuppressed flags. That is pre-existing behaviour and
the owner's instruction was that Space+` is unchanged, so it is recorded, not
altered.

### Exact files

| File | What changed |
|---|---|
| `src-tauri/src/engine/actions/pip.rs` | §9 header, `PipMode`, `PipEntry::fullscreen_pip`, `toggle_fullscreen_pip`, `is_fullscreen_geometry`, `fullscreen_probe`, `placement_held`, `move_flags`, `place_fullscreen_preserving`, `clear_fullscreen_flag`, `shell_safety_refusal`, `fullscreen_corner_label`; `release_disposition` gained a third argument; `restore_window` composes its flags |
| `src-tauri/src/hook/mod.rs` | `KeyCombo::Tab`; `VK_TAB` dispatched unconditionally; `special_bit` no longer maps `"tab"`; `publish_bound_specials` warns about a now-shadowed binding |
| `src-tauri/src/engine/mod.rs` | `KeyCombo::Tab => handle_fullscreen_pip`; `handle_fullscreen_pip`; `HUD_SPECIALS` is 9 entries with `("Tab", "Fullscreen PiP")` |

### The actual code

The whole feature, reduced to the flag set:

```rust
fn move_flags(fullscreen_pip: bool) -> SET_WINDOW_POS_FLAGS {
    if fullscreen_pip {
        SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSENDCHANGING
    } else {
        SWP_NOZORDER | SWP_NOACTIVATE          // Space+`, byte-for-byte unchanged
    }
}
```

The test that decides whether there is a fullscreen state to preserve at all —
all three clauses came off the measurement above:

```rust
fn is_fullscreen_geometry(style: u32, zoomed: bool,
                          win: (i32,i32,i32,i32), mon: (i32,i32,i32,i32)) -> bool {
    if zoomed { return false; }                                   // maximised != fullscreen
    if style & WS_CAPTION.0 != 0 || style & WS_THICKFRAME.0 != 0 { return false; }
    let ((wl,wt,wr,wb), (ml,mt,mr,mb)) = (win, mon);
    if mr <= ml || mb <= mt { return false; }                     // degenerate -> fall back
    wl <= ml + FS_SLACK && wt <= mt + FS_SLACK
        && wr >= mr - FS_SLACK && wb >= mb - FS_SLACK             // FS_SLACK = 4
}
```

`FS_SLACK` is not cosmetic: the probe caught the real fullscreen window
reporting **2560x1599** on a 2560x1600 monitor. An exact-equality test would
have answered "not fullscreen" on the exact case the feature is for.

The verification and the fallback, which is what makes the measurement a
guarantee instead of an assumption about one Chromium build:

```rust
// … SetWindowPos with move_flags(true) …
std::thread::sleep(Duration::from_millis(FS_VERIFY_MS));      // 200ms; revert measured at <40ms
if placement_held((tx,ty,tw,th), actual) && still_fullscreen { return; }   // it worked

// It did not. Never leave the user with a window that is neither fullscreen
// nor cornered: become an ordinary corner PiP and say so.
clear_fullscreen_flag(&cache, hwnd_raw, serial);
animate_to(h(), tx, ty, tw, th);
crate::show_toast(&handle, "📐 Window left fullscreen — corner PiP");
```

The watcher exemption (see "the two traps" below):

```rust
fn release_disposition(zoomed: bool, state: PipState, fullscreen_pip: bool)
    -> Option<Disposition> {
    if zoomed && !fullscreen_pip { Some(Disposition::Full) }
    else if state == PipState::Released { None }
    else { Some(Disposition::HalfKeepEntry) }
}
```

### The two traps, and what each one actually turned out to be

**Trap A — "the fullscreen watcher will immediately undo this."** The fear was
that `release_enlarged` (§7), which releases any PiP whose window exceeds 75%
of the work area or is `IsZoomed`, would tear the feature down on its first
500 ms tick, because the window is still in FULLSCREEN state.

It does not, and the measurement says why: **`should_release` has always
measured actual bounds, never fullscreen state.** After the move the window's
real rect is the corner tile — 1280x800 on a 2560x1600 work area, 25%, with
`IsZoomed` false — because "fullscreen" here is the app's own drawing state and
not a window rect. No change was needed and none was made.

What *did* need an exemption is the thing nobody flagged: **the
`rcNormalPosition` write-back.** A fullscreen entry's `original_*` is the
MONITOR RECT, because that is what a fullscreen window measures as.
`Disposition::Full` hands those bounds to Windows as the window's normal
position — permanently recording "this window's un-maximised size is the whole
screen", a lie the user would meet the next time they un-maximised and which
nothing on the machine could then correct. So a `fullscreen_pip` entry never
gets a `Full` release; it gets the half — topmost dropped, entry kept as the
only copy of the bounds — which is precisely what §7 already does for the case
where the bounds cannot honestly be handed back. The flag is also **sticky
across a re-entry** (`effective == FullscreenPreserving || preserved.fullscreen_pip`),
because a released fullscreen entry re-entered through Space+` would otherwise
have the flag cleared and the monitor rect written back after all.

**Trap B — the restore.** Reused verbatim, no parallel path.
`measure_original_frame` takes its `GetWindowRect` branch for a fullscreen
window (`showCmd` is `SW_SHOWNORMAL`, so `was_maximized` is false) and stores
the monitor rect in SCREEN coordinates; the 5th tap is the same
`restore_window`, which places that rect back — measured to return the window
to (0,0)-(2560,1600) with style unchanged at `0x160B0000`, i.e. still
fullscreen. The only difference is the flag set, and it is read off the entry
rather than off the key that was pressed, so a tile keeps the behaviour it
started with no matter which of the two keys is tapped next.

### Why a separate key, and why not the browser's own PiP

Both are the owner's explicit decisions, recorded so they are not re-opened:

* **Space+` is untouched.** Keeping a window in fullscreen means it has **no
  minimize and no close button** — inherent, since fullscreen is the state in
  which a window draws no chrome. The 5th tap is the way out. His words:
  *"I'm okay with the trade off. It's a new key. If I don't like it, I can just
  not use it."* Imposing that on Space+` would impose it on every app he PiPs.
  **The key being separate IS the opt-in** — that is why there is no setting.
* **The browser's own document Picture-in-Picture was rejected**, not
  overlooked. It would give a proper always-on-top video window with none of
  this Win32 work, and it is unreachable from here: it can only be called by
  code running INSIDE the page. That means an extension the owner installs and
  maintains, a remote debugging port left open on his browser, or UI automation
  clicking each site's own PiP button — different per site and broken by every
  redesign.

### Tab stopped being an optional special

`VK_TAB` was gated on `special_bound(13)`, i.e. Tab was a user-bindable
`special_keys` entry that nothing in the UI has ever been able to write
(PROBLEM 180). It is now a FIXED special like Esc and the backtick, dispatched
unconditionally. `special_bit` no longer maps `"tab"`; **bit 13 is left unused
rather than reassigned**, because the numbering is quoted in the
`BOUND_SPECIALS` log line and in PROBLEM 180's write-up and renumbering the
survivors would make every historical log entry read wrong. An existing
`special_keys["tab"]` binding is **not deleted** — config is never rewritten to
suit a code change — but `publish_bound_specials` now warns once that it can no
longer fire.

### How it was verified

* `cargo test --lib` — **221 passed, 0 failed, 4 ignored** (207 before). Fourteen
  new tests: the fullscreen-vs-not decision including the measured 2560x1599
  one-pixel case and the maximised/windowed negatives, the corner maths for a
  fullscreen source against a work area with a 48px top bar, the flag sets, the
  hold/snap-back verification predicate, the watcher exemption with its
  corner-entry control, the fullscreen restore round trip through `tap_for`,
  the serial-matched demotion, and a structural assertion that the entry arm
  still ORs the preserved flag in.
* `cargo check --lib` — clean, **0 warnings**.
* `npx tsc --noEmit` — exit 0. `npm run build` — exit 0.
* **The Chromium behaviour is MEASURED**, table above, on a throwaway Brave.
* **NOT verified, and it is the one thing a hand test must settle**: every
  measurement used `--kiosk` fullscreen, because `--start-fullscreen` proved
  unreliable across relaunches and keystroke injection does not work from the
  agent shell (Testing laws). Kiosk produces the same *window* state — same
  style, same monitor coverage, same veto — but nobody has watched a
  **page-initiated** fullscreen (an F11'd tab, or a YouTube video's own
  fullscreen button) through this. If a page-initiated fullscreen exits when
  the window shrinks, the verification catches it, the fallback fires and the
  toast says `Window left fullscreen — corner PiP`. **Nothing was built or
  installed**, so none of this is in the owner's running app yet.

### Generalise this

1. **`ret=TRUE` from a window API is not evidence the window did what you
   asked.** `SetWindowPos` returned success and err=0 on a call the target
   reverted 40 ms later. The only proof a window moved is reading its rect back
   afterwards — which is why the verification stayed in the code even after the
   measurement said the flag works.
2. **When a window fights you, look for the message it fights you WITH.** The
   fix here was not a bigger hammer or a retry loop; it was noticing the veto
   arrives as `WM_WINDOWPOSCHANGING` and that Win32 already has a flag to not
   send it. A retry loop against this would have produced a window that
   flickers between two positions forever.
3. **A fallback is only a fallback if it lands somewhere the user already
   accepts.** "Never leave them with a window that is neither fullscreen nor
   cornered" is satisfiable precisely because today's Space+` behaviour exists
   as a floor to fall back to.
4. **A derived value inherits its provenance.** `original_*` measured from a
   fullscreen window is a MONITOR rect, and every downstream consumer that
   treats bounds as "the window's normal size" is wrong about it. The flag on
   the entry is not a mode switch; it is a label on the data saying where it
   came from — which is why it has to be sticky.
5. **Probe with a throwaway instance, never the user's own.** A separate
   `--user-data-dir` made it safe to launch, resize and kill a browser
   repeatedly without touching a single byte of his profile or session — and
   killing only PIDs whose command line carried the probe's tag is what kept
   that true.


---

### AMENDMENT, 2026-08-29 (same day) — PROBLEM 219's RESTORE LEG: the 5th tap gave back a MAXIMISED window, not a fullscreen one

The entry leg above shipped correct: four corners, chrome-less, confirmed by
the owner and by his log. **The 5th tap did not restore true fullscreen**, so
the next Space+Tab correctly probed "not fullscreen" and fell back to ordinary
corner PiP — which he reported as "the 4-corner loop then the 5th fullscreen
logic is broken".

#### Symptom, from `%APPDATA%\Spaceadom\debug.log`

```
working corners:  fs-pip: … fullscreen probe = true  (style 0x160b0000, zoomed=false,
                          window (0,0)-(2560,1600) vs monitor (0,0)-(2560,1600))
after 5th tap:    fs-pip: … fullscreen probe = false (style 0x170b0000, zoomed=true,
                          window (0,0)-(2560,1600) vs monitor (0,0)-(2560,1600))
```

**`zoomed=true` WAS THE TELL.** Same bounds both times — the geometry was
right the whole way through, which is exactly why nothing that checked
geometry could see the failure. `0x170b0000` is `0x160b0000 | WS_MAXIMIZE`.

#### Root cause

Two claims in the original §9 write-up, both stated as fact, both false on the
owner's Brave:

1. *"`measure_original_frame` for a fullscreen window takes its
   `GetWindowRect` branch, because showCmd is `SW_SHOWNORMAL`."* It does not.
   `GetWindowPlacement` reports **`showCmd == SW_SHOWMAXIMIZED`** for a Brave
   window that was maximised *before* it went fullscreen, so the measurement
   takes the `rcNormalPosition` branch and stores the window's
   **pre-fullscreen WINDOWED frame**. His own log, first entry:
   `pip: entering PiP for hwnd 0x10c66 (restored frame 2558x1550 at (1,49), maximized=true)`.
2. *"`was_maximized` is false for a fullscreen entry."* It is `true`, for the
   same reason — and `restore_window` ended with:

```rust
if entry.was_maximized {
    let _ = ShowWindow(hwnd, SW_SHOWMAXIMIZED);
}
```

So the 5th tap placed the right rect and then **re-maximised the window**,
setting `WS_MAXIMIZE`. `is_fullscreen_geometry` refuses any zoomed window, so
the next probe said no and the fallback (correctly) gave corner PiP.

#### The fix — exact file: `src-tauri/src/engine/actions/pip.rs`

**Restore no longer infers the fullscreen state. It replays the one that was
captured.**

*New — the capture, taken at the only instant the window is known to be
fullscreen:*

```rust
pub struct FullscreenState {
    pub style: u32,      // measured 0x160B0000; windowed control is 0x16CF0000
    pub ex_style: u32,   // measured 0x00200000
    pub x: i32, pub y: i32, pub w: i32, pub h: i32,   // SCREEN coords
}

pub struct PipEntry {
    …
    pub fullscreen_pip: bool,
    pub fullscreen_state: Option<FullscreenState>,   // NEW
}
```

`fullscreen_probe` changed from `-> bool` to `-> Option<FullscreenState>` and
ends with `verdict.then_some(FullscreenState { … })` — captured only on a yes.
The entry arm stores it, sticky the same way the flag is:

```rust
fullscreen_state: probed_state
    .or_else(|| preserved.as_ref().and_then(|p| p.fullscreen_state)),
```

`clear_fullscreen_flag` now clears the capture with the flag, so a demoted
entry cannot be restored as a fullscreen one.

*New — the decision, pure and therefore testable (it used to be two `if`s
inside a Win32 function no test could reach, which is why "a fullscreen entry
takes the maximize path" stayed invisible):*

```rust
enum RestorePlan {
    Frame { x: i32, y: i32, w: i32, h: i32, maximize: bool },
    Fullscreen(FullscreenState),
}

fn restore_plan(entry: &PipEntry) -> RestorePlan {
    match (entry.fullscreen_pip, entry.fullscreen_state) {
        (true, Some(fs)) => RestorePlan::Fullscreen(fs),
        _ => RestorePlan::Frame {
            x: entry.original_x, y: entry.original_y,
            w: entry.original_w, h: entry.original_h,
            maximize: entry.was_maximized,
        },
    }
}
```

*BEFORE — `restore_window`, all entries:*

```rust
let mut flags = SWP_NOACTIVATE;
if entry.fullscreen_pip { flags |= SWP_NOSENDCHANGING; }
let _ = SetWindowPos(hwnd, HWND_NOTOPMOST,
    entry.original_x, entry.original_y, entry.original_w, entry.original_h, flags);
if entry.was_maximized { let _ = ShowWindow(hwnd, SW_SHOWMAXIMIZED); }
```

*AFTER — the fullscreen arm (the `Frame` arm is byte-for-byte the code above,
minus the `SWP_NOSENDCHANGING` line, so Space+` is untouched):*

```rust
// A fullscreen window is NEVER zoomed. Clear it the documented way.
if IsZoomed(hwnd).as_bool() { let _ = ShowWindow(hwnd, SW_RESTORE); }

// Style back, ONLY if it moved — SWP_FRAMECHANGED on a Chromium window that
// does not need it is how you make it flicker.
let mut style_changed = false;
if GetWindowLongW(hwnd, GWL_STYLE) as u32 != fs.style {
    SetWindowLongW(hwnd, GWL_STYLE, fs.style as i32); style_changed = true;
}
if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 != fs.ex_style {
    SetWindowLongW(hwnd, GWL_EXSTYLE, fs.ex_style as i32); style_changed = true;
}

let mut flags = SWP_NOACTIVATE | SWP_NOSENDCHANGING;
if style_changed { flags |= SWP_FRAMECHANGED; }
let _ = SetWindowPos(hwnd, HWND_NOTOPMOST, fs.x, fs.y, fs.w, fs.h, flags);

// The verification, logged by the SAME probe entry uses.
if fullscreen_probe(hwnd).is_some() {
    log::info!("fs-pip: hwnd {hwnd_key:#x} restored to TRUE FULLSCREEN — style …");
    return;
}
// Could not: finish today's frame restore and toast the reason. Never leave a
// window that is neither fullscreen nor properly restored.
```

`ShowWindow(SW_SHOWMAXIMIZED)` is now unreachable for a fullscreen entry
except inside that documented fallback.

#### How it was verified

* **MEASURED, on a throwaway Brave with its own `--user-data-dir`** (never the
  owner's profile; only PIDs whose command line carried the probe tag were
  killed, and survivors were counted afterwards). Physical pixels via
  `SetProcessDPIAware`, monitor (0,0)-(2560,1600):

  | step | style | zoomed | showCmd | rect |
  |---|---|---|---|---|
  | fullscreen, as launched | `0x160b0000` | False | 1 | (0,0)-(2560,1600) |
  | cornered with `SWP_NOSENDCHANGING` | `0x160b0000` | False | 1 | (0,0)-(1280,800) |
  | **TODAY'S restore** (`original_*` + `SW_SHOWMAXIMIZED`) | **`0x170b0000`** | **True** | 3 | (0,0)-(2560,1600) |
  | **THE FIX** (un-maximise + style + captured rect, no maximize) | **`0x160b0000`** | **False** | 1 | (0,0)-(2560,1600) |
  | the fix, +2.5 s | `0x160b0000` | False | 1 | (0,0)-(2560,1600) |

  Row 3 **reproduces the owner's failing log exactly**. Row 4 is the fixed
  path, run as the same call sequence the Rust now issues (the window *was*
  zoomed going in, so the `SW_RESTORE` branch was exercised). Row 5 says Brave
  does not fight the return trip.
* `cargo test --lib` — **226 passed, 0 failed** (221 before; 6 new, 1 replaced).
* `cargo check --lib` and the test build — **0 warnings**.
* **NOT verified:** nothing was built or installed, by instruction, so this is
  not in his running app. The keystroke path (five real Space+Tab taps) cannot
  be driven from the agent shell (Testing laws) — his hand test is in
  `PROJECT_STATUS.md`.

#### A SECOND MEASURED CONDITION, recorded so it is not mistaken for this bug

A Brave window that was **maximised and then sent F11** reaches fullscreen
while **keeping `WS_MAXIMIZE`**: style `0x170b0000`, `IsZoomed` true,
`showCmd` 3, covering rcMonitor with no caption. It is genuinely fullscreen and
`is_fullscreen_geometry` says **no**, because it refuses any zoomed window — so
Space+Tab degrades to ordinary corner PiP for that window. That is the *safe*
answer and it was deliberately left alone (the zoomed guard is what keeps a
merely-maximised window off the suppressed-veto path). Written down because
"Space+Tab did not preserve fullscreen on that window" is **not automatically
this bug** — re-test before treating it as one.

#### Generalise this

6. **When a feature preserves a STATE, the restore must replay the state it
   captured — not re-derive it from geometry.** Bounds equality is not state
   equality. The first cut's test asserted the 5th tap handed back the right
   rect, and it passed for a restore that produced the wrong window.
7. **A documented assumption about another program's API is not a
   measurement.** "showCmd is `SW_SHOWNORMAL` for a fullscreen window" was
   written down as fact, was plausible, was tested by nothing, and was false —
   and the owner's own log had contradicted it since the first entry it logged.
   When a comment states a fact about a foreign window, grep the log for it.
8. **Put the decision somewhere a test can reach it.** The defect was two
   `if`s inside an `unsafe fn` that needs a real foreground window to run.
   Lifting them into a pure `restore_plan` did not just make the fix testable —
   it is what made the wrong behaviour legible in the first place.

---

### AMENDMENT 2, 2026-08-29 — PROBLEM 219's 5th TAP LEAKED A REAL Tab INTO THE PAGE, and the cause was **PROBLEM 218's reaper**, not the fullscreen gate

Owner, on 1.0.93: *"The 5th tap, instead of making it fullscreen, is
interacting with the browser — pressing Tab actually does stuff in the browser.
**It has to understand that I am still holding Space.** The 5th tap while I'm
still holding Space should make it fullscreen. And the 6th should equal the
first."*

#### Symptom

Space held, Space+Tab tapped: taps 1-4 corner the fullscreen window correctly,
then the tap that should restore fullscreen instead reaches Brave as a real Tab
and moves focus between page elements.

#### The diagnosis that was WRONG, and how the log killed it in one line

The obvious suspect was `hook/mod.rs`'s fullscreen stand-down gate:

```rust
if FULLSCREEN_ACTIVE.load(Ordering::Relaxed) {
    SUPPRESS_FULLSCREEN.fetch_add(1, Ordering::Relaxed);
    return CallNextHookEx(None, n_code, w_param, l_param);   // everything passes
}
```

It is not this, and the app's own diagnostics say so. **Every** `hook
diagnostics` line across the owner's whole test window reads
`fullscreen-suppressed:0`:

```
03:18:57.617  hook diagnostics - fullscreen-suppressed:0 ... stale-holds-reaped(lost Space-UP):1
03:19:26.551  hook diagnostics - fullscreen-suppressed:0 ... stale-holds-reaped(lost Space-UP):1
03:19:57.620  hook diagnostics - fullscreen-suppressed:0 ... stale-holds-reaped(lost Space-UP):3
```

That gate never fired once. It structurally cannot fire for his case:
`hook/fullscreen.rs`'s `NOT_A_GAME` list contains `brave.exe` (PROBLEM 172), so
a fullscreen browser video never sets `FULLSCREEN_ACTIVE` at all. **The counter
that was zero is the counter that exonerates the suspect** - and the counter
right beside it, non-zero in exactly the same minutes, names the real one.

#### Root cause - Windows moves auto-repeat to the LAST key pressed

PROBLEM 218 added a reaper for a Space-hold whose Space-UP was lost, and chose
Windows auto-repeat as the hold's liveness signal:

> *"A physically-held Space produces a fresh WM_KEYDOWN every repeat period for
> as long as it is held ... So auto-repeat IS the liveness signal for a hold."*

That is true only until a second key goes down. Windows auto-repeats the
**most-recently-pressed** key, and it does **not** hand the slot back when that
key is released. So the first Space+Tab of a hold silences Space's repeat for
the rest of that hold, `SPACE_TICK_TS` freezes, and `STALE_HOLD_GRACE_MS`
(2000 ms) later the reaper tears down a hold whose key is still physically
down.

Measured, from `%APPDATA%\Spaceadom\debug.log`, 2026-08-29 (Space held
throughout):

```
03:19:50.429  fs-pip: hwnd 0x10c66 fullscreen probe = true      <- last Space auto-repeat ~here
03:19:50.651  fs-pip: ... held its 1280x800 tile at (0,0)       <- tap 1
03:19:50.972  pip: ... -> corner 1 at (1280,0)                  <- tap 2
03:19:51.453  pip: ... -> corner 2 at (1280,800)                <- tap 3
03:19:51.939  pip: ... -> corner 3 at (0,800)                   <- tap 4
03:19:52.444  hook: a Space-hold has been latched for 2016ms with no auto-repeat
              after 5 of them - ... Reaping it (HUD was up: false).
              <- MODIFIER_ACTIVE cleared while Space is still physically held
              <- tap 5 now falls past the combo branch to CallNextHookEx:
                 a real Tab into Brave
```

Seven such reaps in that session, at 02:45:04, 02:45:48, 03:18:52, 03:19:00,
03:19:30, 03:19:39 and 03:19:52 - each `2015`-`2016 ms`, i.e. the grace exactly,
each immediately after a burst of taps. `03:19:39.423` reaps after only THREE
corners, which is why he has also seen the 4th tap misbehave.

**The condition, stated so it can be re-tested:** it only bites when the gap
between two taps exceeds 2000 ms. His fast run at `03:19:24.375`-`03:19:25.391`
put all five taps inside one second and the 5th tap restored correctly. *"It
worked that time"* is therefore not evidence against this.

With `MODIFIER_ACTIVE` false, `VK_TAB` never reaches the `combo_opt` match; it
falls to the function's last line, `CallNextHookEx` - the browser gets the Tab.
Nothing about Tab is special here: **every** bound letter and special leaks the
same way after a reap. Same class as PROBLEM 176 (Space+PrintScreen) - a key
escaping the combo branch - but a different door.

#### Exact file - `src-tauri/src/hook/mod.rs`

**BEFORE** (three conditions, all of which a healthy Space+key user satisfies):

```rust
pub(crate) fn hold_is_stale(
    modifier_active: bool,
    repeats: u32,
    since_last_tick_ms: u64,
    grace_ms: u64,
) -> bool {
    modifier_active && repeats >= MIN_OBSERVED_REPEATS && since_last_tick_ms > grace_ms
}
```

**AFTER** - a fourth condition, plus the atomic that feeds it:

```rust
/// PROBLEM 219 - has ANOTHER key gone down during the current Space-hold?
static SPACE_COMBO_SEEN: AtomicBool = AtomicBool::new(false);

pub(crate) fn hold_is_stale(
    modifier_active: bool,
    repeats: u32,
    since_last_tick_ms: u64,
    grace_ms: u64,
    combo_seen: bool,
) -> bool {
    modifier_active
        && !combo_seen
        && repeats >= MIN_OBSERVED_REPEATS
        && since_last_tick_ms > grace_ms
}
```

Set on the ONE branch that can observe it - the combo branch, which already
loaded `MODIFIER_ACTIVE`, so the callback pays one relaxed store and no new
read (PROBLEM 58's budget is intact):

```rust
if MODIFIER_ACTIVE.load(Ordering::Relaxed) && is_down {
    // PROBLEM 219 - FIRST, above every branch that can return. Space-down was
    // handled and returned above, so this is always some OTHER key going down
    // while Space is held: the exact event that hands Windows' auto-repeat
    // slot to that key.
    SPACE_COMBO_SEEN.store(true, Ordering::Relaxed);
```

Cleared wherever a hold ends: fresh Space-down, Space-up, `reap_stale_hold`,
and the watchdog's post-eviction latch reset.

`reap_stale_hold` passes it through:

```rust
if !hold_is_stale(
    MODIFIER_ACTIVE.load(Ordering::Relaxed),
    repeats,
    since,
    STALE_HOLD_GRACE_MS,
    SPACE_COMBO_SEEN.load(Ordering::Relaxed),
) {
    return false;
}
```

#### Why DISARM rather than re-stamp the clock

Re-stamping `SPACE_TICK_TS` on every key-down was considered and rejected on
the numbers: it only moves the deadline to 2 s after the LAST tap, so any pause
for thought between taps still reaps a live hold. After a combo there is **no
liveness signal for a held Space at all** - held-quietly and hook-is-dead are
indistinguishable from inside the callback, and no threshold separates them.
CLAUDE.md: *a check that cannot produce a negative result is not a check.*

What is given up is bounded and named. The reaper keeps full power over the
hold shape PROBLEM 218 was written for - Space held, HUD on screen, pointer
arming chips, no combo pressed - which is every stuck-HUD report in the log. It
stands down only for a hold that has already fired a key, where the HUD was
taken down by `cancel_hud` on that very combo, where `MAX_MODIFIER_HOLD_MS`
(30 s, in the combo branch) still bounds the latch, and where the next Space
press/release clears it outright.

#### What was deliberately NOT changed, and why

The plan for this session was to narrow the fullscreen gate so a bound combo is
intercepted whenever Space is physically held. **That change was not made**, on
two grounds:

1. It fixes nothing here. `fullscreen-suppressed:0` and `brave.exe` in
   `NOT_A_GAME` prove the gate is not on the path of this bug.
2. It would regress the case the gate exists for. In a real game a held Space
   is *jump*, not an intent signal - Space+W is running and jumping, and under
   the narrowed gate it would fire a shortcut. The gate's premise ("holding
   Space addresses Spaceadom") is true at a desk and false in a game, which is
   the one place the gate is active.

The residual is real and is REPORTED, not silently closed: inside a genuine
exclusive-fullscreen app that is not on `NOT_A_GAME` or the user's allowlist,
Space+Tab and every other combo do pass through. That is the stand-down working
as designed; whether it should carve out an exception is the owner's call.
`SUPPRESS_FULLSCREEN` and its `fullscreen-suppressed:` log text are untouched
and still count exactly what they counted before.

#### Tap-types-a-space, traced

Unchanged, and it never depended on the reaper. In a stand-down
(`FULLSCREEN_ACTIVE`) the gate sits ABOVE the Space-DOWN branch, so Space is
never intercepted, `SPACE_INTERCEPTED` is never set and the OS types the space
itself. Outside a stand-down, Space-down sets `SPACE_INTERCEPTED` and Space-up
injects - and PROBLEM 218 deliberately put that Space-UP block ABOVE the gates
so a gate flipping mid-hold cannot eat the space. The reaper only ever ran when
the UP was already gone, and it clears `SPACE_INTERCEPTED` so the next release
cannot double-space. Reaping less often cannot cost a space.

#### The 6th tap - verified, not assumed

It does behave as the 1st. `tap_for` (`engine/actions/pip.rs`) does
`map.remove(&key)` on the `Tap::Restore` arm, so the 5th tap deletes the entry
outright - it does not leave a `Released` one. The 6th tap finds nothing,
returns `Tap::Enter(None)`, and takes the genuine-first-entry path: fresh
`fullscreen_probe`, fresh `fullscreen_state` capture, corner 0, topmost
re-asserted, new serial. The sticky `fullscreen_state` that survives re-entry
only applies to `Tap::Enter(Some(preserved))`, which is the RELEASED path, and
a restore does not produce one. Confirmed in his log at `03:19:25.391`
(`pip: restoring hwnd 0x10c66 ...`) followed by `03:19:28.636`
(`pip: entering PiP ... reentry=false`).

#### How it was verified

`cargo test --lib`: **229 passed, 0 failed** (baseline 226 - three new tests).
`cargo check --lib`: clean, 0 warnings. New tests in
`hook::stale_hold_tests`:

* `a_hold_that_fired_a_combo_is_never_reaped` - his measured inputs
  (`repeats=5`, `since=2016 ms`) must NOT reap, at any pause length.
* `the_combo_flag_is_the_only_difference` - identical inputs, flag flipped:
  reaps / does not reap. Proves nothing else moved.
* `a_silent_hold_with_no_combo_is_still_reaped` - PROBLEM 218's own shape is
  untouched.

**NOT hand-verified: an agent cannot press the owner's keys** (`SendInput` from
this containerised shell never reaches the hook - Testing laws). His
confirmation, on a build that contains this: fullscreen a video, hold Space and
tap Tab five times *with a pause between taps* - the 5th returns real
fullscreen and **no Tab reaches the page** - then a 6th starts the corner cycle
again. Watch `stale-holds-reaped(lost Space-UP):` in the 60 s diagnostics line:
it must stay 0 across that run.

#### Generalise this

9. **A liveness signal borrowed from the OS can be taken away by the feature
   that uses it.** Auto-repeat belongs to the last key pressed, so any app that
   reads repeat as "this modifier is still held" breaks the moment its own
   modifier+key combo fires. Before trusting a signal as liveness, ask what the
   user doing the intended thing does to it.
10. **Read the diagnostics counters as a SET, not one at a time.** The suspect
    counter reading zero and the neighbouring counter reading non-zero in the
    same minute is a complete diagnosis, available before opening any source
    file. `fullscreen-suppressed:0` next to `stale-holds-reaped:3` named both
    the innocent and the guilty in one line.
11. **A fix that fires on a timer needs a re-test CONDITION written down, not
    just a symptom.** This one only bites when taps are more than 2000 ms
    apart; the owner's own fast run passed. "It worked that time" is not
    evidence against a timing bug unless the timing was reproduced.

---

### AMENDMENT 3, 2026-08-29 — PROBLEM 219's TILE COLLAPSED THE MOMENT IT WAS CLICKED, because `SWP_NOSENDCHANGING` only protects OUR move

Owner, on 1.0.93: *"interacting with the tab pip just made it full screen at
the first tap."*

#### Symptom

Space+Tab corners a fullscreen video correctly. Click anywhere in the cornered
window — to scrub, to pause, to do anything at all — and it fills the screen
again. A cornered video you cannot click is close to useless.

#### Root cause

`SWP_NOSENDCHANGING` (§9) suppresses the `WM_WINDOWPOSCHANGING` **that our own
`SetWindowPos` would have sent**, so Chromium never gets asked to veto our
move. It does nothing whatsoever about Chromium moving the window *later, on
its own initiative*. A window kept in fullscreen is still, from the browser's
point of view, genuinely fullscreen — so whenever it re-runs its fullscreen
layout it reasserts the monitor rect, and it is correct to do so.

**The trigger is not activation. It is every click.** That distinction is the
whole fix, and it was measured rather than reasoned:

```
H1  tiled while NOT foreground, left alone   -> holds indefinitely
H2  click to ACTIVATE                        -> back at (0,0) 2560x1600
H3  one re-assert, then left alone           -> holds 3s (being foreground is fine)
H4  click again while ALREADY foreground     -> SNAPS BACK AGAIN   <-- the decider
H5  6 clicks, re-assert on each drift        -> 6 corrections, ends tiled, no fight
H6  corner cycle while foreground            -> holds
```

`EVENT_SYSTEM_FOREGROUND` would have fixed H2 and nothing else — it would have
fixed the *sentence* the owner wrote ("at the first tap") and left the problem
he actually has. A finer run timed the drift at **+16 ms**, which also rules
out the existing 500 ms fullscreen watcher: it would leave the window filling
the screen for up to half a second per click.

Measured on a throwaway Brave with its own `--user-data-dir`, never the owner's
profile. **Both F11 browser-fullscreen and a real `<video>` in ELEMENT
fullscreen — the owner's actual case — behave identically** (style `0x160B0000`,
`IsZoomed` false, unchanged throughout; the window never leaves fullscreen, it
only resizes itself).

#### The signal, also measured

`EVENT_OBJECT_LOCATIONCHANGE` via `SetWinEventHook`, scoped to the target
window's process (`WINEVENT_OUTOFCONTEXT`, so no DLL injection and the callback
lands on our own thread):

```
P1 observe only : LOCATIONCHANGE DOES fire for Chromium's snap-back (+31ms)
P2 callback re-asserts, 7 clicks : 7 corrections, each 15-31ms after the drift,
                                   every drift followed by exactly ONE non-drift
                                   event (our placement). It CONVERGES.
P3 idle 4s with the guard armed  : 0 events, 0 corrections   <-- costs nothing
P4 corner cycle, guard retargeted: 0 spurious corrections    <-- no self-fighting
```

#### Exact file

`src-tauri/src/engine/actions/pip.rs` — new §10 block, `arm_click_guard` /
`disarm_click_guard` / `click_guard_proc` / `run_click_guard`.

#### The code

The callback is atomics, plus one `GetWindowRect` and one `SetWindowPos` only
once a drift is real. It re-asserts with the SAME suppressed-veto flags the
tile was placed with, because the window is still fullscreen:

```rust
let target = GUARD_HWND.load(Ordering::SeqCst);
if target == 0 || hwnd.0 as isize != target { return; }
// ... load GUARD_X/Y/W/H, GetWindowRect ...
if placement_held((x, y, w, h), actual) { return; }
// ... runaway accounting ...
let _ = SetWindowPos(hwnd, HWND(std::ptr::null_mut()), x, y, w, h, move_flags(true));
```

Armed **before** the placement, not after the verification:

```rust
arm_click_guard(hwnd_raw, tx, ty, tw, th);
unsafe { let _ = SetWindowPos(h(), .., move_flags(true)); }
std::thread::sleep(std::time::Duration::from_millis(FS_VERIFY_MS));
```

Otherwise a user who taps Space+Tab and immediately clicks the video has the
snap-back read by the 200 ms verification as *"this window refused the tile"*
and is dropped to corner PiP — losing the fullscreen the feature exists to keep.

#### The sharpest edge: the 5th tap must DISARM first

`restore_window` puts a fullscreen entry back to the **whole monitor**. A guard
still armed reads that as drift and hauls the window straight back into the
corner, so the 5th tap would appear to do nothing. The disarm is the first
thing `restore_window` does, before any placement, and a test asserts that
ordering from the source:

```rust
assert!(disarm < first_place, "the disarm must come BEFORE any placement ...");
```

Disarmed on every exit route: the 5th tap, the placement fallback, the
maximised-not-fullscreen demotion, the §7 release, a dead window, and a runaway.

#### Not fighting the user

The guard is armed only for `fullscreen_pip` entries, and such a window has
style `0x160B0000` — **no `WS_CAPTION`, no `WS_THICKFRAME`** (measured,
unchanged across every run). There is no title bar to drag it by and no edge to
resize it from, so there is no user gesture to overrule. Space+`` ` ``'s tiles are
ordinary windows the user *can* drag, and they are never guarded.

#### The runaway stop

An app that re-asserts in a loop would have us loop with it. More than 60
corrections in 3 s disarms the guard, toasts, and leaves 1.0.93's behaviour
(snaps back on click; the 5th tap still restores). Calibration: Brave costs
exactly ONE correction per click and no human clicks 60 times in 3 seconds, so
the two cases are separated by more than an order of magnitude.

#### How it was verified

`cargo test --lib` 239 passed / 0 failed (baseline 229 + 10 new).
`cargo check --lib` 0 warnings. The behaviour itself is measured in the tables
above; **the owner still has to confirm on his own keys — an agent cannot press
them.**

#### GENERALISE

**A flag that suppresses a message suppresses it for YOUR call only.** It buys
nothing against the same program doing the same thing on its own schedule
afterwards. When you win an argument with another process by not letting it
speak, expect it to act later anyway, and decide then whether you need a
standing correction or just a louder one-off.

**And: fix the behaviour, not the sentence.** The report said "at the first
tap". Measuring found it happens on *every* tap, and the mechanism that would
have satisfied the sentence (`EVENT_SYSTEM_FOREGROUND`) would have shipped a
bug the owner would have re-reported within a minute.

---

## PROBLEM 220 — Space+`` ` `` and Space+Tab shared ONE cache entry per window, so the two features fought over one position index, one set of bounds and one sticky flag

Owner, on 1.0.93, after being warned the two keys shared a map and asked to
test it: *"it did behave oddly."* His instruction: *"make separate pip cache,
is it possible?"*

#### Symptom

A window cornered by one key and then tapped with the other behaved
incoherently — jumping to the wrong corner, restoring to the wrong frame, or
acquiring fullscreen-preserving placement it never asked for.

#### Root cause

`PipCache` was `HashMap<isize, PipEntry>` — keyed by the bare HWND. **One
window, one entry, no matter which key created it.** So the two features shared,
per window:

- `position_index` — a tap of either key advanced the same corner counter;
- `original_x/y/w/h` and `was_maximized` — the frame the 5th tap restores;
- `fullscreen_pip` and `fullscreen_state`, which are **deliberately sticky
  across re-entry** (§8/§9, so a monitor rect is never handed to
  `rcNormalPosition`). Sticky is right *within one feature*; across two keys it
  meant Space+`` ` `` could inherit Space+Tab's suppressed-veto placement and its
  captured fullscreen state.

#### Exact file

`src-tauri/src/engine/actions/pip.rs`, plus one doc comment in
`src-tauri/src/engine/mod.rs`.

#### The code

Before:

```rust
pub type PipCache = Arc<Mutex<HashMap<isize, PipEntry>>>;
```

After:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PipKey {
    pub hwnd: isize,
    pub mode: PipMode,
}

pub type PipCache = Arc<Mutex<HashMap<PipKey, PipEntry>>>;
```

**ONE map keyed by `(hwnd, mode)`, not two maps — and the reason is the one
that matters.** Every consumer of this cache has to see ALL of it, and with two
containers each is one forgotten line away from a silent half-failure that
nothing would report:

- `restore_all()` draining only one map leaves windows pinned topmost at
  quarter size with the app gone — PROBLEM 167's orphan, exactly;
- `release_enlarged()` scanning only one leaves the other feature's windows
  stuck on top forever;
- `prune_dead` pruning only one leaves dead HWNDs for a recycled handle to
  inherit (NATIVE_SAFETY rule 3).

With the mode in the key there is one map, so a drain is a drain, a scan is a
scan and a prune is a prune — none can be taught about half the state, and a
third PiP mode would inherit all three correctly for free.

**The mode in the key is the key that was PRESSED, not the behaviour that
resulted.** A Space+Tab tap on a non-fullscreen window degrades to corner
behaviour (`fullscreen_pip: false`) but still lives in Space+Tab's namespace,
because "which key owns this window" is what the user experiences. Behaviour is
still decided by `PipEntry::fullscreen_pip`, exactly as before.

#### THE TAKEOVER RULE — stated, because the owner asked for it explicitly

**A window may be held by only ONE of the two keys at a time.** A tap in mode M
on a window the other key holds claims that entry, RESTORES the window the way
that feature found it, and only then enters PiP in mode M:

```rust
fn takeover_victim(
    map: &mut HashMap<PipKey, PipEntry>,
    hwnd: isize,
    mode: PipMode,
) -> Option<PipEntry> {
    map.remove(&PipKey { hwnd, mode: mode.other() })
}
```

Executed with the lock released (TASK 4 — `restore_window` is SendMessage-class
against a foreign app), and **before** the `Tap::Enter` arm measures anything:
without the restore, the new entry's `original_*` would be a measurement of the
*other feature's corner tile*, and the 5th tap would "restore" the user's
window to a quarter of the screen with nothing left that knows better.

The claim uses the **strict** recycled-handle polarity — `pid != 0 && victim.pid
!= 0 && pid == victim.pid` — because this branch ACTS on a window. (`tap_for`'s
released arm keeps the softer "both known and different" rule, where a wrong
answer only costs a re-measure; here it would move a stranger's window.)

Never two live entries for one HWND. That is what produced "odd".

#### How it was verified

`cargo test --lib` — 239 passed, 0 failed (baseline 229). Ten new tests, eight
of them for this problem:

- `one_window_can_hold_a_separate_entry_under_each_key` — same HWND, two
  namespaces; bounds, flag and fullscreen capture all independent;
- `cycling_one_key_does_not_advance_the_other_keys_corner`;
- `the_fifth_tap_of_one_key_leaves_the_other_keys_entry_alone`;
- `the_second_key_takes_the_window_over_and_hands_back_what_to_restore`;
- `a_takeover_claims_only_the_other_key_and_only_for_that_window` — a same-key
  tap is a CYCLE, not a takeover, and no other window is disturbed;
- `a_takeover_carries_no_state_from_the_key_it_replaced`;
- `prune_dead_prunes_both_namespaces` — with a REAL live window
  (`GetDesktopWindow`, read-only, never acted on) as the control, so the test
  can produce a negative;
- `restore_all_drains_both_namespaces` — dead HWNDs, so `IsWindow` is false and
  no window is touched; this exercises the DRAIN, which is the part that can be
  written wrong.

The two tests that use the process-wide cache are serialised behind
`GLOBAL_CACHE_TESTS`, because `restore_all` drains it and a flaky test is worse
than no test.

#### A BUILD-SYSTEM TRAP FOUND ON THE WAY, and it is not this bug

`cargo test --lib` **cannot run a freshly-linked harness in this tree**, for a
reason that has nothing to do with any of the above:

```
process didn't exit successfully: ... (exit code: 0xc0000139,
STATUS_ENTRYPOINT_NOT_FOUND)
```

`tauri-build` links the app manifest with `cargo:rustc-link-arg-bins=` —
**bins only**. A `cargo test` harness exe therefore has no manifest, loads
comctl32 **v5**, and dies on `tauri-plugin-dialog`'s missing
`TaskDialogIndirect`. This is the exact failure `windows-app-manifest.xml`
already documents for the app itself.

Diagnosed by resolving every one of the 388 imports against its DLL with
`LoadLibrary` + `GetProcAddress` — one miss, named. Worked around **without
touching the build** by dropping an EXTERNAL manifest beside the harness:

```
target/debug/deps/space_toggle_os_lib-<hash>.exe.manifest
```

containing the `Microsoft.Windows.Common-Controls 6.0.0.0` dependency. With it
the harness loads and all 239 tests run.

**Do not read a `0xC0000139` from `cargo test` as "the code is broken".** Note
also the trap inside the trap: a `cargo test --lib` that passes may be running
a *pre-existing* harness exe that cargo did not relink. That is almost certainly
why the 229-test baseline passed at the start of this session and the first
rebuild did not.

Whether to fix this properly — a `cargo:rustc-link-arg-tests=` line in
`build.rs` would do it — is the owner's call; it was left alone because the
brief was PiP, and a build.rs change is a shipping change.

#### GENERALISE

**Shared mutable state between two features is a feature of neither.** When one
map is reached by two entry points with different rules, every field in it
becomes an unwritten contract between them — and the sticky ones
(`fullscreen_pip` here) are the dangerous ones, because "sticky within a
feature" silently becomes "leaks across features".

**When separating state, put the discriminator in the KEY, not in a second
container** — unless every consumer is guaranteed to be updated. One map cannot
be half-drained, half-scanned or half-pruned.

## PROBLEM 222 — rename profiles had a working backend and frontend but zero test coverage, so "active profile follows a rename" and "bindings survive a rename" were promises nobody had checked

#### Symptom

None reported by the owner — this is the "add rename" feature request landing
on a codebase where `rename_profile` (Rust), the frontend's inline-rename
popover row, and the shared `regex_lite`/`PROFILE_NAME_RE` validation already
existed and were wired end to end (`lib.rs` registers `commands::rename_profile`,
`profile-editor.ts`'s `startInlineRename` calls it). What did not exist was any
test asserting the two guarantees that make a rename safe rather than merely
present:

- renaming the ACTIVE profile must move `active_profile` to the new name in the
  same save — not on some later reconcile — or every name-keyed lookup
  (`browser_profiles::active_profile_claims`, `engine::cycle_profile`, the
  dashboard's own `p.name === active_profile` checks) briefly or permanently
  disagrees about which profile is active;
- the bindings living inside the renamed `Profile` must be untouched — a
  rename is not a create-then-delete, and nothing should be able to make it
  behave like one.

Traced every consumer of `active_profile` and `profile.name` before concluding
this (`browser_profiles.rs`, `engine/mod.rs`, `main.ts`, `keyboard-matrix.ts`,
`key-detail-panel.ts`, `settings-panel.ts`): all of them compare against
`cfg.active_profile` / `_config.active_profile` LIVE, at call time, off the
same shared `AppConfig` (`Arc<RwLock<AppConfig>>` on the Rust side, the same
object reference passed to every frontend module's `init*()` on the TS side —
none of them cache a copy). So a rename that updates both fields atomically is
sound everywhere else in the app for free; the missing piece was proving that
one command actually does it, and keeping that proof compiled in against
whoever edits `rename_profile` next.

#### Root cause

Not a bug — a coverage gap. `rename_profile` (`src-tauri/src/commands.rs`) took
`State<'_, ConfigState>`, so its logic could not be unit-tested without a live
Tauri app instance (the `tauri::test` mock-app feature is not enabled in this
crate's `Cargo.toml`), and nobody had pulled the pure part out.

#### Exact file

`src-tauri/src/commands.rs` (`apply_profile_rename` extracted + tests),
`src-tauri/src/config/schema.rs` (a `Profile` serde round-trip test),
`src/components/profile-editor.ts` (a discoverability tooltip only — the
rename UI itself was already complete).

#### The code

`rename_profile`'s body used to do the guard-then-mutate work directly against
`state.0.write()`. Extracted into a plain function that needs no `State`:

```rust
fn apply_profile_rename(
    cfg: &mut AppConfig,
    old_name: &str,
    new_name: &str,
) -> Result<(), String> {
    if new_name != old_name && cfg.profiles.iter().any(|p| p.name == new_name) {
        return Err(format!("Profile '{new_name}' already exists"));
    }
    let profile = cfg
        .profiles
        .iter_mut()
        .find(|p| p.name == old_name)
        .ok_or_else(|| format!("Profile '{old_name}' not found"))?;
    profile.name = new_name.to_string();

    // Same `cfg`, same pass as the rename above — no reader can ever observe
    // a profile renamed but `active_profile` still pointing at the old name.
    if cfg.active_profile == old_name {
        cfg.active_profile = new_name.to_string();
    }
    Ok(())
}

#[tauri::command]
pub fn rename_profile(
    old_name: String,
    new_name: String,
    state: State<'_, ConfigState>,
) -> Result<(), String> {
    if !regex_lite(&new_name) {
        return Err("Profile name must be 1–24 characters, not blank".into());
    }
    let new_name = new_name.trim().to_string();
    let mut cfg = state.0.write().unwrap_or_else(|p| p.into_inner());
    apply_profile_rename(&mut cfg, &old_name, &new_name)?;
    let snapshot = cfg.clone();
    drop(cfg);
    config::save(&snapshot)
}
```

`commands.rs`'s new `profile_rename_tests` module (built on a plain in-memory
`AppConfig`, no `State` needed) covers: `regex_lite` rejecting empty/whitespace/
control-character names and accepting 24 chars while rejecting 25; a rename
updating the profile's name; renaming the ACTIVE profile moving
`active_profile` atomically; renaming an INACTIVE profile leaving
`active_profile` alone; a duplicate target name rejected with the profile list
unchanged; a same-name rename succeeding as a no-op (PROBLEM 85's guard);
renaming an unknown profile erroring; and bindings surviving the rename
untouched.

`schema.rs`'s `a_profile_round_trip_carries_every_binding_across_a_rename`
serialises a `Profile` with populated bindings, deserialises it, mutates
`.name` (the exact thing a rename does to the in-memory struct), and
round-trips through JSON again — proving a rename cannot lose a binding
through the actual save/load path, not just through `apply_profile_rename`'s
in-memory mutation.

`profile-editor.ts` gained one line: a `title` tooltip on the row's name span
(`Double-click to rename …`), because the existing dblclick-to-edit-in-place
gesture — matching the URL-pill's click-to-edit precedent rather than a
dialog — had no visible affordance at all; delete's ✕ button is discoverable
by sight, rename previously was not.

#### How it was verified

`npx tsc --noEmit`: clean. **`cargo test --lib` could NOT be run this pass** —
every invocation (`cargo check --lib` included) currently fails with
`error: invalid instruction 'cargo:rustc-link-arg-tests' … does not have a test
target`, from `build.rs`'s in-flight PROBLEM 221 fix (a different, concurrently
running agent's file, out of this pass's scope per the session's file-division
rule — not touched here). Confirmed non-transient: three retries a few minutes
apart, same error, `build.rs`'s own mtime moving between retries. The new
Rust tests above are therefore reviewed by hand (types, borrow shapes, and
literal values checked against `regex_lite`'s and `apply_profile_rename`'s
actual logic) but **not run-verified** — flag this file's tests for a rerun
once `build.rs` is fixed, before trusting them as green.

#### GENERALISE

**A feature can be end-to-end wired and still be unverified** — "the button
calls the command and the command has a guard" is not the same claim as "the
guard does what it says", and the gap only shows up once someone tries to
write the test. Extracting the guard into a plain function of the data it
guards, rather than of the framework handle carrying that data, is what makes
the second claim checkable at all when the framework's test-state
construction (`tauri::test`) is not part of the build.

## PROBLEM 223 — the browser-profile picker had no way to tell two identically-named Chromium profiles apart, because `BrowserProfile` never carried the signed-in account

#### Symptom

Two Chromium profiles on the owner's machine can share a display name (e.g.
two profiles both named "Nur", measured, see below). In the key-editor's
browser-profile picker and the bound-key chip, both read as the same label —
nothing distinguished them.

#### Root cause

`browser_profiles.rs` already parsed `Local State`'s `profile.info_cache`, but
`BrowserProfile` only carried `directory` and `display_name` (`info_cache[dir]
.name`) — the signed-in account was never extracted, so there was nothing for
the frontend to show even if it wanted to.

#### MEASURED before writing any code (2026-08-31), not assumed

Read the owner's real `Local State` files — Chrome, Edge, Brave, Samsung
Internet — and inspected every profile's `info_cache` entry directly:

- **Chrome** — every one of its 14 profiles is signed in. `user_name` holds a
  real, email-shaped string on all 14. `gaia_name` is ALSO populated on all
  14, but it is the Google account's **display name** ("Nur Arpon", "I am
  Nur", ...) — not an email. At least one profile's `user_name` and
  `gaia_name` disagree in shape, which is the case that actually proves the
  extraction reads the right field rather than either one working by luck.
- **Edge** (`Profile 1`, not signed in) — `user_name` is `""`: the key is
  PRESENT in the JSON but EMPTY, not absent. So is `gaia_name`.
- **Brave** (`Default` and the owner's `"ARPON'S STUDIES"` profile, neither
  signed in) — `user_name` is `""` again, but `gaia_name` is **not in the
  JSON at all** for Brave's shape. Same symptom (not signed in), two
  different causes (empty vs. absent) — the extraction has to treat both the
  same way regardless.
- **Samsung Internet** (`Default`, not signed in) — `user_name` is `""`,
  `gaia_name` is `""`.

**Conclusion acted on:** `user_name` is the only field ever actually shaped
like an email; `gaia_name` is a display name and must never be used as an
email fallback (it would put a person's name where an address belongs, and it
isn't even reliably present). Both "present but blank" and "key entirely
absent" collapse to the same `None`.

#### Exact files

`src-tauri/src/browser_profiles.rs` (struct field + extraction + tests),
`src/types.ts` (mirror), `src/components/browser-profile-picker.ts` (tile
tooltip, tile second line, chip tooltip), `src/styles.css` (`.bp-tile-email`).

#### The code

`browser_profiles.rs` — the field, and the extraction added to the existing
`display_name` map closure in `profiles_from_local_state`:

```rust
pub struct BrowserProfile {
    pub directory: String,
    pub display_name: String,
    /// The signed-in account email, from `info_cache[dir].user_name`. `None`
    /// when the profile is not signed in — never rendered as an empty line.
    pub email: Option<String>,
}

// ...inside profiles_from_local_state's .map(|(dir, v)| { ... }):
let email = v
    .get("user_name")
    .and_then(|n| n.as_str())
    .map(str::trim)
    .filter(|s| !s.is_empty())
    .map(str::to_string);
BrowserProfile { directory: dir.clone(), display_name: display, email }
```

`types.ts` mirrors it as `email: string | null` (not optional — matches this
file's existing convention for every other Rust `Option<String>`, e.g.
`AppInfo.icon_base64`).

`browser-profile-picker.ts` — the tile tooltip now includes the email when
known, and a NEW element only exists in the DOM when there is something to
show it (this is what keeps a no-email tile's height exactly what it was
before this feature):

```ts
tile.title = p.email
  ? `${b.browser_name} — ${p.display_name} — ${p.email}  (${p.directory})`
  : `${b.browser_name} — ${p.display_name}  (${p.directory})`;
...
tile.append(disc, name);
if (p.email) {
  const email = document.createElement("span");
  email.className = "bp-tile-email";
  email.textContent = p.email;          // textContent — user data
  tile.appendChild(email);
}
```

The SAME chip that shows a bound key's stored `browser_profile_name`
(`renderProfileChip`'s `paintChip`) gets the email in its tooltip too, when
the profile is still live enough to look up (a GONE profile has no
`info_cache` entry left to read one from):

```ts
chip.title = gone
  ? `${browserLabel} still opens, but the profile folder "${sel.profileDir}" is gone`
  : profile?.email
    ? `Opens in ${browserLabel}, profile "${prof.textContent}" (${profile.email})`
    : `Opens in ${browserLabel}, profile "${prof.textContent}"`;
```

`styles.css` — a dimmed, truncating second line, added without touching
`.bp-tile`'s `min-height`:

```css
.bp-tile-email {
  max-width: 100%;
  font-size: 9px;
  color: var(--st-text-dim);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
```

#### Privacy — verified, not assumed

The brief's hard requirement: the email must never leave the machine and must
never appear in a log line, even at `info` level. Grepped every
`console.info` call in `browser-profile-picker.ts` (six of them) and every
`log::info!` / `log::debug!` / `println!` in `browser_profiles.rs`: none of
them reference `.email` — the existing "profile picked" line still logs only
`exe`/`dir`/`display_name`, and the production `log::info!` summary line
(`browser_profiles: found N Chromium browser(s)...`) logs only browser names
and profile COUNTS, never a name or an email. The one place `email` is
printed at all is `live_scan`, an `#[ignore]`d, manually-run
(`--ignored --nocapture`) diagnostic test that never ships and never leaves
the machine it is run on.

#### How it was verified

`npx tsc --noEmit`: clean.

Six new Rust unit tests (`local_state_email_tests`, real files on a scratch
temp dir per PROBLEM 130's parallel-test rule, not a hand-built
`serde_json::Value`): a signed-in Chrome-shaped profile yields its
`user_name` as the email and NOT `gaia_name` even though both are populated
and disagree; an Edge/Samsung-shaped empty-string `user_name` reads as no
email; a Brave-shaped entry with the `user_name` key missing entirely also
reads as no email; a whitespace-only `user_name` is stripped the same as an
empty one; a real value is trimmed of padding; and a mixed file with one
signed-in and one signed-out profile proves neither leaks into the other.

The DOM/CSS side was verified visually rather than assumed: built a
standalone HTML page reproducing the exact `.ed-tile` / `.bp-tile` /
`.bp-tile-email` rules from `styles.css` and rendered it in the browser tool.
Measured, not eyeballed: a row of three no-email tiles all report the
IDENTICAL `getBoundingClientRect().height` (98.44px); a short email fits with
`scrollWidth === clientWidth` (not truncated); a long email truncates
(`scrollWidth 274 > clientWidth 122`); a tile with both a two-line wrapped
name AND an email grows past the 98.44px floor exactly as `.bp-tile`'s
`min-height` (not `height`) was always meant to allow. Fixture emails were
kept synthetic (`test.user@example.com`, never the owner's real measured
addresses) — the measurement above proves the extraction logic against real
files without baking real personal Gmail addresses into committed source.

`cargo test --lib` — **initially blocked by the same shared-tree issue
PROBLEM 222 hit** (`error: invalid instruction 'cargo:rustc-link-arg-tests'
... does not have a test target`, from `build.rs`'s in-flight PROBLEM 221 fix,
a different concurrently-running agent's file, out of scope here per
`buildrs=build.rs only`). Rather than working around it or giving up, a
monitor was left polling `build.rs` for a change and re-ran `cargo test --lib`
the moment it did (376s later, once that agent's fix landed). Result: **274
passed, 0 failed, 4 ignored** — every new test in `local_state_email_tests`
green (`a_signed_in_chrome_profile_yields_its_user_name_as_email`,
`an_empty_string_user_name_is_not_signed_in`,
`a_missing_user_name_key_is_also_not_signed_in`,
`a_whitespace_only_user_name_is_not_signed_in`, `a_padded_user_name_is_trimmed`,
`signed_in_and_signed_out_profiles_coexist_in_one_file`), and nothing in the
rest of the shared tree broken by this pass's changes.

#### GENERALISE

**Two fields can describe "the same person" and still not be interchangeable
for a given purpose.** `user_name` and `gaia_name` are both populated,
together, on every signed-in Chrome profile measured here — a shape check
that only asked "is SOMETHING here" would have picked either one and passed
every test that didn't specifically set them to disagree. The fixture that
matters is the one where the two candidate fields diverge, not the common
case where either would have worked by accident.

---

## PROBLEM 225 — the post-launch raise stood down on the owner's own keypress: it asked "did a key move?" of a static that answers "were we called at all?"

**Symptom.** The owner's launched apps do not come to the front, and when one
does its taskbar button flashes first. Reported as a foreground/taskbar-flash
bug; the log says it is mostly not a foreground bug at all.

**The measurement, before any code was touched** (`%APPDATA%\Spaceadom\debug.log`,
2026-08-25 → 08-31, 79 `raise_after_launch` outcomes):

| Outcome | Count |
|---|---|
| `— you started typing, standing down.` | 29 |
| `— you switched to another window, standing down…` | 28 |
| `raising '<stem>' HWND(…)` (an actual raise attempted) | ~20 |
| `force_foreground: all 4 steps failed` (a real denial) | **1** (brave, 08-31 08:55:54) |

**57 of 79 launches never attempted a raise at all.** The one confirmed
foreground denial is 1 in 20. So the bug the owner sees is overwhelmingly the
watcher standing itself down, not Windows refusing us.

Two representative launches, timed off the log:

```
08-31 08:33:54.971  cascade: launching …\brave.exe --profile-directory="Profile 1"
08-31 08:33:55.805  raise_after_launch: 'brave' — you started typing, standing down.   <- 834 ms
08-31 08:55:37.407  cascade: launching …\chrome.exe --profile-directory="Profile 1"
08-31 08:55:38.071  raise_after_launch: 'chrome' — you started typing, standing down.  <- 664 ms
```

The owner was not typing. He was still holding Space, reading the Guide HUD
(`guide_hud: overlay window shown` precedes each combo by 500–900 ms), and the
"key" that stood the watcher down was **the Space-UP of the very combo that
fired the launch**.

---

### Root cause 1 — `last_keyboard_event_tick()` is a LIVENESS signal, borrowed as a TYPING signal

`hook/mod.rs`'s `LAST_KB_EVENT` exists for the eviction watchdog (PROBLEM
65/66). Its question is *"was our callback called at all?"*, so it is stamped
as widely as possible — and four different things that are **not a person
typing** move it:

| # | What stamps it | Where |
|---|---|---|
| A1 | Every key **UP** — including the combo's own Space-up and letter-up | `kb_hook_proc`, before any filtering |
| A2 | `install_hooks()` | `hook/mod.rs` ~line 1196, off the hook path entirely |
| A3 | `watchdog_check()` | `hook/mod.rs` ~line 1386, on the 1 s pump timer |
| A4 | **Our own injected input** — `inject_space()` and `force_foreground`'s synthetic tap | the store sits ABOVE the `dwExtraInfo == 0x7A7A7A7A` early-return |

A1 alone explains the entire measured 0.62–1.00 s cluster. And it could never
have been fixed by lengthening `SETTLE_MS`: the owner reads the HUD, so his
Space-up lands *after* any settle worth having.

**Generalise this: a signal deliberately stamped as widely as possible cannot
also be a signal about intent.** Liveness ("did anything happen?") and intent
("did the user do this?") are opposite requirements — the first wants every
event, the second wants almost none. Borrowing one for the other reads as free
in review, because the call site is one line and the static's name is plausible.

### Root cause 2 — "you switched to another window" was a bare HWND comparison

`fg != started_fg` is true for the launched app's own splash screen, for the
File Explorer window a folder binding just asked for (stems seen in the log:
`lc-hurdle-electrical`, `claude-projects` — folder names, which never match
`explorer`), and for any transient. It asserted the user had moved on with no
evidence that the new window belonged to anyone else.

### Root cause 3 — the ladder was unmeasurable, and half-implemented

`force_foreground`'s four step-outcome logs were all `log::debug!`, so **a clean
step-2 success and a step-4 `SwitchToThisWindow` success — which this file's own
comment says "may flash the taskbar button once", i.e. the owner's exact
symptom — were indistinguishable in every report he has ever sent.** Only the
total-failure branch was `warn!`.

And step 1 attached this thread to the **outgoing foreground** thread only.
There was no `GetWindowThreadProcessId(hwnd, …)` anywhere in the function. The
standard recipe attaches the caller to the outgoing foreground thread **and the
target's**, so all three share one input queue across the `BringWindowToTop` +
`SetForegroundWindow` pair.

### Root cause 4 — step 3 injected `VK_MENU`

A bare Alt down+up is delivered to the **current foreground** window's queue and
opens its menu bar / KeyTips. The owner's launch targets are Word, File
Explorer, Chrome and Brave — all four react to it. Its stated justification was
also wrong: injected input makes the OLD app the last-input recipient, never us,
so it never satisfied the `SetForegroundWindow` carve-out it was cited for.

---

### Exact files

* `src-tauri/src/hook/mod.rs` — new `LAST_USER_TYPING` / `LAST_USER_TYPING_VK`
  statics, their accessors, and one stamp inside `kb_hook_proc`.
* `src-tauri/src/engine/actions/smart_cascade.rs` — `raise_after_launch`,
  `force_foreground`, `shell_launch`, `run_browser`, plus new pure helpers and
  their tests.

---

### The actual code

**1a — a clean signal (`hook/mod.rs`).** Added beside `last_keyboard_event_tick`:

```rust
static LAST_USER_TYPING: AtomicU64 = AtomicU64::new(0);
static LAST_USER_TYPING_VK: AtomicU32 = AtomicU32::new(0);

pub fn last_user_typing_tick() -> u64 { LAST_USER_TYPING.load(Ordering::Relaxed) }
pub fn last_user_typing_vk() -> u32 { LAST_USER_TYPING_VK.load(Ordering::Relaxed) }
```

and, in `kb_hook_proc`, immediately after `track_modifier(vk, is_down)` — which
is **below** the `dwExtraInfo == MAGIC_INJECTED` early-return, and that position
is what kills A4:

```rust
if is_down && vk != VK_SPACE && !MODIFIER_ACTIVE.load(Ordering::Relaxed) {
    LAST_USER_TYPING.store(now, Ordering::Relaxed);
    LAST_USER_TYPING_VK.store(vk as u32, Ordering::Relaxed);
}
```

Each condition kills one false signal by construction, not by timing:

* `is_down` -> kills A1's whole up-stroke class (the measured 0.62–1.00 s cluster).
* `vk != VK_SPACE` -> kills the combo's own Space release directly.
* `!MODIFIER_ACTIVE` -> kills the combo's letter (a key pressed while Space is
  held is a COMMAND, not prose).
* the cookie -> satisfied by POSITION, since the injected-input early-return is
  above. **If this block is ever moved up, the cookie test must move with it.**
* A2/A3 -> impossible: those run on the pump thread and never enter this
  function.

Hook-law compliance (PROBLEM 58 / NATIVE_SAFETY): one relaxed load and two
relaxed stores on the key-down path. No allocation, no logging, no Win32, no
lock — the same envelope `SPACE_TICK_TS` (PROBLEM 218) and `record_margin`
(PROBLEM 95) already pay. **`GetAsyncKeyState` is deliberately NOT used for the
"is Space held" test** — it reports a key we SUPPRESS as UP, which is the exact
lie that once broke every shortcut in the app. `MODIFIER_ACTIVE` is our own
bookkeeping and the only honest answer.

**1b/1c — the decision, extracted as PURE functions (`smart_cascade.rs`).** The
old decision was three lines welded to `GetForegroundWindow` and a static in
another module, which is why no test could have caught it:

```rust
const RAISE_GRACE_MS: u64 = 1_500;

fn typing_stand_down(since_launch_ms: u64, baseline_tick: u64, latest_tick: u64) -> bool {
    since_launch_ms >= RAISE_GRACE_MS && latest_tick > baseline_tick
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ForeignVerdict { KeepPolling, StandDown, Unidentified }

fn foreign_foreground_verdict(
    since_launch_ms: u64,
    fg_changed: bool,
    fg: Option<(u32, &str)>,
    launched_pid: u32,
    stems: &[String],
) -> ForeignVerdict {
    if !fg_changed || since_launch_ms < RAISE_GRACE_MS {
        return ForeignVerdict::KeepPolling;
    }
    let Some((fg_pid, fg_stem)) = fg else {
        return ForeignVerdict::Unidentified;
    };
    if launched_pid != 0 && fg_pid == launched_pid {
        return ForeignVerdict::KeepPolling;
    }
    if stems.iter().any(|s| s.eq_ignore_ascii_case(fg_stem)) {
        return ForeignVerdict::KeepPolling;
    }
    ForeignVerdict::StandDown
}
```

The watcher loop now only gathers facts; these two decide. `Unidentified` is a
third variant and not a `bool` on purpose: **a foreground window we cannot name
is not evidence about the user.** The watcher keeps polling and lets the 8 s
deadline expire raising nothing, which this file already argues is the safe
failure. Collapsing it into `StandDown` would re-create the bug for every window
we lack rights to query.

**Where 1500 comes from, stated honestly.** It is **a proposal, not a measured
constant.** It is inside this file's own recorded cold-start range (Brave
~500 ms, VLC ~1 s), it covers the first ~8 polls on the 500 + n×120 grid, and it
is the number **22 of the 29** false "you started typing" stand-downs fall
under. Moving it is the owner's call.

**Positive process identity (1c).** `shell_launch` already receives
`sei.hProcess` back from `ShellExecuteExW` under `SEE_MASK_NOCLOSEPROCESS` and
hands it straight to a waiter thread that closes it. The PID is now read out
first:

```rust
// entry of shell_launch — clear FIRST, on every path
LAST_LAUNCH_PID.store(0, Ordering::Relaxed);
…
// after ShellExecuteExW succeeds, before the waiter thread takes the handle
let created_pid = windows::Win32::System::Threading::GetProcessId(hproc);
LAST_LAUNCH_PID.store(created_pid, Ordering::Relaxed);
```

`raise_after_launch` **takes** it once (`swap(0, …)`) on the calling thread,
before spawning the watcher — the launch is the statement immediately above
every call site, so on that thread the pairing is deterministic. `run_browser`
is the one launch entry point that does not go through `shell_launch`
(`ShellExecuteW`, no handle), so it stores 0 explicitly. Every path clears
before it can set, so the worst case is **losing** an identity, never inventing
one — and losing it only means we keep polling.

This is what the stem matcher can never work out: a `.lnk`, a `whatsapp://`
handler (PROBLEM 216: binding says `whatsapp`, process is `WhatsApp.Root`) and a
Squirrel `Update.exe` stub all run under a different name than the binding.

**NATIVE_SAFETY, explicitly:** this identity NEVER authorises touching a window.
Its only outcomes are "keep polling" and "stop". The decision to
`ShowWindow`/`SetForegroundWindow` still belongs entirely to
`find_window_by_exe_stem` -> `enum_callback`, which keeps the explorer.exe
`CabinetWClass` **positive** filter and the empty-title rule. Proving a process
is not a way past the class rule.

**1d — the messages state the observation, not an assertion about the user.**
"you started typing" and "you switched to another window" were false ~57 times
in six days, and each was a sentence blaming the owner for a bug. Now:

```
raise_after_launch: 'brave' — STANDING DOWN. Observed: a key went DOWN (vk 0x4a)
about 2317 ms after the launch, past the 1500 ms grace, with Space not held. …

raise_after_launch: 'brave' — STANDING DOWN. Observed: winword.exe (pid 31337)
has held the foreground since about 4512 ms after the launch, past the 1500 ms
grace. It is neither this launch's process (pid 7788, 0 = the shell created
none) nor any of its exe stems, so nothing here belongs to us. Nothing was
raised.
```

The `vk` in the first line is the permanent version of the brief's temporary
probe: with it, *"the watcher stood down on the combo's own Space"* and *"the
owner really did start typing"* stop being the same log entry.

**Commit 0 — four `log::debug!` -> `log::info!`,** plus two more lines that are
also raise decisions (`came up in front by itself`, `AllowSetForegroundWindow
refused`). Cost at ~20 launches/day: one extra line per launch. Without it
nobody can tell a clean step-2 success from a step-4 `SwitchToThisWindow`
success, and step 4 is the one that flashes the taskbar. Step 4's line now says
so in the line itself.

**Part 2 — the target-side attach (`force_foreground`).**

```rust
// BELOW the IsHungAppWindow(hwnd) early-return, deliberately — the new attach
// inherits that guard for free (PROBLEM 133). Do not move either one.
let target_thread = GetWindowThreadProcessId(hwnd, None);

let attached_fg = fg_thread != my_thread && fg_thread != 0 && !fg_hung;
let attached_target =
    target_thread != 0 && target_thread != my_thread && target_thread != fg_thread;

if attached_fg     { let _ = AttachThreadInput(my_thread, fg_thread, true); }
if attached_target { let _ = AttachThreadInput(my_thread, target_thread, true); }

let _ = BringWindowToTop(hwnd);
let _ = SetForegroundWindow(hwnd);

if attached_target { let _ = AttachThreadInput(my_thread, target_thread, false); }
if attached_fg     { let _ = AttachThreadInput(my_thread, fg_thread, false); }
```

Two **independent** bools, each detached on exactly the condition it attached
on. PROBLEM 121's entire cost was a leaked attachment, and re-deriving either
expression at detach time is how that happens.

**Part 2 — `VK_MENU` -> `VK_NONAME` (0xFC).** Reserved by Windows, does nothing
in any application, no menu bar, no KeyTips. The `0x7A7A7A7A` cookie stays — it
is load-bearing twice now: our own hook ignores it (never `LLKHF_INJECTED`,
NATIVE_SAFETY row 4) and `LAST_USER_TYPING` excludes it by the same test, so the
tap can never stand the watcher down against itself. Both keys stay in ONE
`SendInput` batch and the pair is symmetric, so no modifier is ever left down.
Its comment no longer claims to satisfy the input-recency rule, because it never
did.

**Rejected, and why they must stay rejected:**

* **Minimize/restore to force a raise** — NATIVE_SAFETY DO-NOT-TOUCH row 1. The
  owner's most frequent launch targets in this very log are **folders**, whose
  windows are explorer.exe, and the rule there is a POSITIVE filter
  (`CabinetWClass` only). It also discards restore bounds (the open PiP hazard),
  and a launch that minimises then restores looks *worse* than one that opens
  behind.
* **`SPI_SETFOREGROUNDLOCKTIMEOUT = 0`** — the next trick anyone reaches for. It
  writes a persistent HKCU user preference, changes global OS state with no safe
  undo for a process that can be killed (NATIVE_SAFETY rule 4), and is a
  system-settings modification.

---

### How it was verified

* **`cargo test --lib` in the repo tree: 274 passed, 0 failed, 4 ignored.
  0 warnings** on a full recompile of both edited files. (274 is 239 baseline
  plus this pass's 9 and other concurrent agents' additions on the same shared
  tree — the number is not this change's alone.)
* **Nine new pure tests** (`raise_decision_tests` in `smart_cascade.rs`), every
  case taken from a line in the owner's log: the 834 ms Brave stand-down is now
  `KeepPolling`; typing at 2.3 s still stands down; an unmoved tick never stands
  down at any age; the grace boundary is exercised on both sides; the launched
  PID beats the stem matcher; `pid 0` never matches a real PID; stem matching is
  case-insensitive; a genuine third application after the grace stands down;
  **an unidentifiable foreground is `Unidentified`, not `StandDown`.**
* No TypeScript was touched, so `tsc` was not re-run by this lane.
* **NOT verified on the real machine. Not built, not installed, not observed.**
  This is a keyboard-hook + foreground change on the owner's live input path;
  per this repo's rules it is UNTESTED until he presses Space+key on an
  installed build and the log shows which ladder step won.

**A measurement trap this pass hit, worth keeping.** For most of it, EVERY cargo
invocation in the repo — including a bare `cargo check --lib` — died with
`error: invalid instruction 'cargo:rustc-link-arg-tests' … does not have a test
target`, from `build.rs`, which a different concurrently-running agent owned
under the session's file-division rule. Cargo rejects `rustc-link-arg-tests`
unless the package has a real test *target* (files under `tests/`); unit tests
inside the lib do not create one. Rather than edit another lane's file, this
change was first verified against a byte-identical copy of the crate staged in
scratch with that one `println!` neutralised — a linking-only substitution that
cannot change how any Rust source compiles. The numbers above are from the real
tree after that agent landed its own fix (`/DELAYLOAD:comctl32.dll`), and they
agreed with the scratch run exactly. **The general rule: when a shared-tree
blocker is in someone else's file, reproduce the check somewhere you own rather
than editing theirs or reporting "could not verify".**

### What to grep for in the next log

```
raise_after_launch: … STANDING DOWN. Observed:      <- should be RARE now
force_foreground: step-2 … succeeded — clean raise, no taskbar flash
force_foreground: step-3 … succeeded — clean raise, no taskbar flash
force_foreground: step-4 SwitchToThisWindow succeeded — … TASKBAR BUTTON MAY HAVE FLASHED
```

If step 4 still wins after this change, Part 2's target attach did not help and
the remaining work is genuinely the foreground lock. If steps 2/3 win, it did.
**Land Part 1 and measure before concluding anything about Part 2** — Part 1
touches 57 of 79 observed outcomes and Part 2 touches 1 confirmed denial out of
20 attempts, so shipping them together makes the improvement unattributable.
They are separable here: Part 1 is `hook/mod.rs` + `raise_after_launch`, Part 2
is `force_foreground` alone.

### Generalise this

1. **A signal stamped as widely as possible cannot also be a signal about
   intent.** Liveness wants every event; intent wants almost none. Check what a
   static is stamped *for* before reading it for something else — the call site
   is one line and the name is always plausible.
2. **"The user did X" in a log line is a claim that can be wrong.** Log the
   observation (which vk, how long after what, which process) and let the reader
   conclude. 57 log lines blamed the owner for a bug in this file.
3. **Unknown is not the same as no.** An identity check that cannot resolve must
   return a third answer, not fall into the negative branch — otherwise every
   permission failure becomes a false positive.
4. **A branch whose outcome is invisible cannot be judged.** Four `debug!`s made
   "clean raise" and "raise that flashed the taskbar" the same observation for
   the entire life of the feature.

### Numbering note

Filed as 225, not the naive "highest + 1". At write time the highest FILED
heading was 222; 221, 223 and 224 were all already claimed in code comments by
other concurrently-running agents in this same multi-agent pass, and 223 was
filed as a heading by another lane in the minutes between this entry's number
check and its write. This section was renumbered from 223 to 225 in place —
its code comments (`hook/mod.rs`, `smart_cascade.rs`) say 225 and agree.
Same collision class as the PROBLEM 197 note. Append-only files never
renumber: if 225 also collides, that is the record.

---

## PROBLEM 226 — `cargo test --lib` crashed at process startup (`0xC0000139`/`0xC0000138`) or hung behind an Entry-Point-Not-Found modal, because the manifest that grants comctl32 v6 only ever links into `bins`

PROJECT_STATUS.md's 2026-08-29 PiP entry found this and named the exact fix
it thought was needed, then explicitly deferred it: *"Fixing it properly is
one `cargo:rustc-link-arg-tests=` line in `build.rs`; I left it alone because
it is a build change and the brief was PiP. Your call."* This entry is that
call — and the proposed one-liner turned out not to work. Both the dead end
and the fix that does work are recorded here so the dead end is never
re-tried.

#### Symptom

`cargo test --lib` either crashed immediately —

```
exit code: 0xc0000139, STATUS_ENTRYPOINT_NOT_FOUND
```

— or, depending on the machine, popped a native "Entry Point Not Found"
modal dialog that **hangs `cargo test` forever** because nothing is present
to click it.

#### Root cause

`build.rs` builds the app's manifest through
`tauri_build::try_build(Attributes::new().windows_attributes(windows))`. That
call chain is `tauri_build::try_build` -> `tauri_winres::WindowsResource::
compile()` -> `embed_resource::compile()` (traced through all three crates'
source in the local registry cache, not guessed). `embed_resource::compile()`
**unconditionally** compiles `windows-app-manifest.xml` into
`$OUT_DIR/resource.lib` and **unconditionally** prints
`cargo:rustc-link-arg-bins=<path>` — bins only, no override, no companion
call to link it anywhere else. `windows-app-manifest.xml` carries the
Common-Controls-v6 dependency (its own "MUST NOT BE OMITTED" comment) that
`tauri-plugin-dialog`'s `TaskDialogIndirect` needs. A test harness built from
`--lib` never receives that manifest, so it activates plain `comctl32.dll`
v5, which does not export `TaskDialogIndirect` — and Windows resolves the
WHOLE import table before `main` runs, so the crash happens at process
startup regardless of whether any test actually calls the dialog.

**The proposed fix (`cargo:rustc-link-arg-tests=`) does not exist for this
crate — proved two ways, not assumed:**

1. Adding `println!("cargo:rustc-link-arg-tests={resource_lib}")` to
   `build.rs` made **every** cargo invocation die immediately:
   ```
   error: invalid instruction `cargo:rustc-link-arg-tests` from build script of `spaceadom v1.0.94 (...)`
   The package spaceadom v1.0.94 (...) does not have a test target.
   ```
   Reproduced from scratch with a minimal `lib.rs` + `#[test]` and no
   `tests/` directory: identical error. Cargo's `-tests` scoping targets only
   `Test`-kind targets — files under `tests/`, or explicit `[[test]]`
   entries — **not** the lib's own `--lib`/`--test` unit-test harness, no
   matter how many `#[cfg(test)]` blocks it contains. `spaceadom` has zero
   `tests/*.rs` files and zero `[[test]]` entries, so the directive is
   flatly invalid, every time, for any cargo command.
2. Even with a real `Test`-kind target added (to make the directive valid),
   `-vv` output showed the resource `.lib` reaching the link line of the
   NEW `tests/*.rs` binary, but **never** the `src\lib.rs --test` line that
   `cargo test --lib` actually runs. Wrong target scope either way — this
   would not have fixed the real problem even if it had compiled.

**The next thing tried, and why it also had to be rejected:** the bare,
un-suffixed `cargo:rustc-link-arg=<resource_lib>` DOES reach the lib's own
`--test` harness (confirmed with `-vv`: the flag appears on that link line).
But it also reaches `bins` — on top of the `-bins` copy `embed_resource`
already supplies there, unavoidably. Two copies of the same manifest
resource on one link line is not redundant, it is fatal:
```
CVTRES : fatal error CVT1100: duplicate resource.  type:MANIFEST, name:1, language:0x0409
LINK : fatal error LNK1123: failure during conversion to COFF: file invalid or corrupt
```
That is `spaceadom.exe` itself failing to link — the actual shippable binary,
not a test artifact. Reproduced on a from-scratch crate shaped like this one
(a `[lib]` + a `[[bin]]`, one manifest resource via `embed_resource::compile`
+ `-bins`, a bare `rustc-link-arg=` pointed at the same file): `cargo build`
died with the exact same `CVT1100`/`LNK1123` pair. Unshippable — ruled out.

#### The actual fix

Don't put a second manifest anywhere. **Delay-load `comctl32.dll`** so the
loader never resolves `TaskDialogIndirect` (or anything else from that DLL)
at process startup — only on the first real call to it. None of the
existing tests make that call, so the import is simply never touched and the
harness starts clean, every time, with zero risk of a hang. `/DELAYLOAD` is a
linker flag, not a resource, so — unlike the manifest — applying it to
`bins` too is harmless: there is nothing for it to collide with.

#### Exact file

`src-tauri/build.rs`, inside the existing `#[cfg(target_os = "windows")]`
block that already calls `tauri_build::try_build`. No other file touched.

#### The code

Added immediately after the existing `tauri_build::try_build(...)
.expect(...)` call:

```rust
println!("cargo:rustc-link-arg=/DELAYLOAD:comctl32.dll");
println!("cargo:rustc-link-arg=delayimp.lib");
```

That's the whole fix. `comctl32.lib` itself is already linked (transitively,
via the `windows` crate's own `#[link(...)]` metadata on the FFI it
generates for `TaskDialogIndirect` and friends) — nothing to add there.

#### How it was verified

Proved the failure mode first, faithfully, before trusting any fix for it —
a from-scratch crate (`[lib]` + `[[bin]]`, no `tests/` dir, matching
`spaceadom`'s shape) with a **real, compiled-but-never-executed** call to
`TaskDialogIndirect` behind `#[test] #[ignore]` (so the import survives
`/OPT:REF` dead-stripping but never actually runs) reproduced the exact crash
class on demand:
```
process didn't exit successfully: `...delaytest-....exe` (exit code: 0xc0000138, STATUS_ORDINAL_NOT_FOUND)
```
Adding the two `/DELAYLOAD` lines to that same crate's `build.rs` made the
identical binary pass clean:
```
test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out
```
and even running the `#[ignore]`d test explicitly — forcing the delay-loaded
call to actually execute, the one case that should still fail — exited
immediately and cleanly (`0xc06d007f`, the delay-load helper's own failure
exit), never a hang, never a modal.

Then on the real tree, with the two leftover `space_toggle_os_lib-*.exe.
manifest` files from PROJECT_STATUS's 2026-08-29 external-manifest
workaround **deleted first** (so a pass here could not be it silently
still doing the work):

- `cargo test --lib` — **274 passed, 0 failed, 4 ignored** (this session's
  instructed baseline was 239; the tree is shared with concurrently-running
  agents on other files per this session's file-scope split, so the higher
  count is their tests landing too, not a discrepancy in this fix). No
  crash, no hang, no dialog, finished in 1.14s.
- `cargo check --lib` — 0 warnings, 0 errors.
- `cargo build` (the real bin, not shipped — no version bump, no
  `npm run tauri build`, no install, per instruction) — succeeds clean, no
  `LNK1123`, no duplicate resource.
- Grepped the freshly-built `target/debug/spaceadom.exe` for ASCII markers
  from the manifest to confirm PROBLEM 61/62 are still intact (this change
  never touches the manifest mechanism, only adds to it, but a load-bearing
  area gets checked, not assumed): `Microsoft.Windows.Common-Controls`,
  `asInvoker`, and `PerMonitorV2` all still present.

#### Generalise this

**A build-script link-arg directive's `-bins`/`-tests`/`-examples`/
`-benches` suffix is scoped by Cargo's `TargetKind`, and a library's own
`#[cfg(test)]` unit-test harness (built via `--lib`/`--test`) is
`TargetKind::Lib`, never `TargetKind::Test` — `-tests` cannot reach it no
matter how many unit tests exist, only files under `tests/` (or `[[test]]`
entries) qualify.** If a crate has none of those (this one doesn't), `-tests`
is not merely a no-op, it is a hard `error: invalid instruction` that kills
every cargo invocation, not just `cargo test`.

**A Windows manifest resource (`RT_MANIFEST`, id 1) cannot be linked into
one target twice, even from the literal same file** — MSVC's `CVTRES`
rejects it as a duplicate regardless of content identity. Any fix that adds
a second source of the app manifest to a target that already has one (via
`-bins`, unavoidable once `tauri_build`'s `app_manifest` is set) will break
that target's link, full stop. When you need a Win32-loader effect (like
comctl32 v6 activation) in ONE extra target without duplicating a resource
that's already elsewhere, prefer a **linker behaviour flag** (`/DELAYLOAD`)
over a **linker resource** — flags compose safely across targets; embedded
resources with fixed IDs do not.

---

## PROBLEM 224 — every shutdown, sign-out and .msi-over-a-running-app ended in tao's `cannot move state from Destroyed`, because tao bets the process dies inside `WM_ENDSESSION` and we survived the bet

*(Numbering note: 222 and 223 were filed by other lanes between this entry's
number check and its write, and 225 was filed while this one was being written.
This section is 224 and its code comments say 224. Append-only files never
renumber.)*

### Symptom

**30 recorded crashes across `debug.log` and `debug.log.0`, all one line:**

```
2026-08-30 15:09:27.084 [ERROR] space_toggle_os_lib — PANIC on thread 'main' at
  D:\RUST-DOWNLOADED-HERE\cargo\registry\src\index.crates.io-1949cf8c6b5b557f\
  tao-0.35.3\src\platform_impl\windows\event_loop\runner.rs:371:25:
  cannot move state from Destroyed.
```

Main-thread panic, so the app dies. The user sees Spaceadom vanish — no window,
no tray icon, and (because a GUI binary has no console) nothing on screen. The
05:00:02 and 14:20 timestamps line up with reboots and scheduled maintenance,
and on 2026-08-30 an `.msi` was upgrading the app while it was running:

```
15:09:25.078 [WARN] hook: WATCHDOG — ... Foreground: msiexec.exe (pid 69488) ...
15:09:27.084 [ERROR] PANIC on thread 'main' ... cannot move state from Destroyed
```

### Root cause

`tao-0.35.3/src/platform_impl/windows/event_loop/runner.rs:371` is the last arm
of tao's state machine:

```rust
(Destroyed, _) => panic!("cannot move state from Destroyed"),
```

The only thing that sets `Destroyed` outside the normal loop exit is tao's own
`WM_ENDSESSION` handler, in `thread_event_target_callback`
(`event_loop.rs:2384`) — i.e. on tao's hidden `Tao Thread Event Target` window:

```rust
// We don't process `WM_QUERYENDSESSION` yet ...
win32wm::WM_ENDSESSION => {
  if wparam.0 == TRUE.0 as usize {
    subclass_input.event_loop_runner.loop_destroyed();   // -> Destroyed
  }
  // Note: after we return 0 here, Windows will shut us down
  LRESULT(0)
}
```

**Read tao's comment again: "after we return 0 here, Windows will shut us
down."** That is not a mistake, it is a BET — that no further message reaches a
window procedure before the process dies. Windows does not kill a process the
instant it returns from `WM_ENDSESSION`; it kills it when the whole session-end
sequence finishes, and for a Restart Manager close (`ENDSESSION_CLOSEAPP` — an
installer that only wants the exe closed) that may never happen at all. In that
gap one more message reaches a Tauri window, and the runner is asked to change
state while it is already `Destroyed`.

**The recorded backtrace shows the whole path, and it is also why the obvious
fix does not work:**

```
 24: GetMessageW                 <- parked in the message pump
 23: NtUserGetMessage
 22: KiUserCallbackDispatcher    <- the kernel dispatches a SENT message
 20: SendMessageW
 19: CallWindowProcW
 18: GetWindowSubclass
 17: DefSubclassProc
 16: <wry::webview2::InnerWebView>::parent_subclass_proc
 15: DefSubclassProc
  9: core::panicking::panic_fmt
```

So the fix is not to catch the panic; it is to **make tao's assumption true** —
take `WM_ENDSESSION` first and exit from inside the handler, so tao's runner is
never told the session ended and can never be asked to move out of `Destroyed`.

### Exact files

| File | Change |
| --- | --- |
| `src-tauri/src/session_end.rs` | **NEW.** The guard: subclass every main-thread window, own `WM_QUERYENDSESSION` / `WM_ENDSESSION`, tear down and `exit(0)`. |
| `src-tauri/src/lib.rs` | `mod session_end;`; `install()` from `setup()` and from the end of `create_app_windows`; startup version line; the panic hook's three `log::error!`s moved onto `DEGRADED_TARGET`. |
| `src-tauri/src/display_watch.rs` | `install()` again at the end of `rebuild_once`, **via `on_main_thread_blocking`**. |
| `scripts/verify-session-end.ps1` | **NEW.** The harness that reproduces the crash on demand. |

**No `Cargo.toml` change.** `Win32_UI_Shell` (SetWindowSubclass / DefSubclassProc),
`Win32_UI_WindowsAndMessaging` (EnumThreadWindows / GetClassNameW) and
`Win32_System_Threading` (GetCurrentThreadId) are already enabled features.

### The actual code

The classifier and the reason decoder are pure and unit-tested:

```rust
pub const WM_QUERYENDSESSION: u32 = 0x0011;
pub const WM_ENDSESSION: u32 = 0x0016;
pub const ENDSESSION_CLOSEAPP: usize = 0x0000_0001;
pub const ENDSESSION_CRITICAL: usize = 0x4000_0000;
pub const ENDSESSION_LOGOFF: usize   = 0x8000_0000;

pub fn classify(msg: u32, wparam: usize) -> SessionMsg {
    if msg == WM_QUERYENDSESSION { return SessionMsg::Query; }
    if msg == WM_ENDSESSION {
        // A Win32 BOOL is only guaranteed NON-ZERO for true. `wparam == 1`
        // would read a TRUE of 2 as "cancelled" and let the panic back in.
        return if wparam != 0 { SessionMsg::Ending } else { SessionMsg::Cancelled };
    }
    SessionMsg::Other
}
```

Installation — **`EnumThreadWindows`, never `EnumWindows`** (NATIVE_SAFETY.md:
widening a window enumeration to the whole desktop is what once minimised
explorer.exe's shell windows):

```rust
pub fn install() {
    let tid = unsafe { GetCurrentThreadId() };
    let mut tally = Tally::default();
    unsafe {
        let _ = EnumThreadWindows(tid, Some(enum_proc),
                                  LPARAM(&mut tally as *mut Tally as isize));
    }
    // ... logs ERROR when tally.seen == 0 (wrong thread / too early) and when
    // tally.tao_target == false (the one window that matters was missed).
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // dwRefData is deliberately 0: no heap data, so nothing to free when a
    // window is destroyed and no way to leave a dangling pointer behind.
    let ok = SetWindowSubclass(hwnd, Some(session_end_proc), SUBCLASS_ID, 0).as_bool();
    /* ... tally ... */
    BOOL(1)
}
```

The guard itself. `SetWindowSubclass` puts the newest subclass at the HEAD of
the chain, so this runs ahead of wry's `parent_subclass_proc` and tao's
`thread_event_target_callback`:

```rust
unsafe extern "system" fn session_end_proc(
    hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
    _uid: usize, _ref: usize,
) -> LRESULT {
    match classify(msg, wparam.0) {
        SessionMsg::Other => DefSubclassProc(hwnd, msg, wparam, lparam),

        SessionMsg::Query => {
            if !SESSION_ENDING.swap(true, Ordering::SeqCst) { log::warn!(/* once */); }
            LRESULT(1)          // TRUE. NOT exit: a query can still be cancelled.
        }

        SessionMsg::Cancelled => {
            if SESSION_ENDING.swap(false, Ordering::SeqCst) { log::warn!(/* ... */); }
            DefSubclassProc(hwnd, msg, wparam, lparam)   // the process lives on
        }

        SessionMsg::Ending => {
            if ENDING_HANDLED.swap(true, Ordering::SeqCst) { return LRESULT(0); }
            log::warn!(/* reason, decoded from lParam */);
            teardown();
            log::warn!("session: teardown finished — exiting with code 0 ...");
            std::process::exit(0);     // never chains, never returns
        }
    }
}

fn teardown() {
    crate::engine::actions::pip::restore_all();   // PROBLEM 167
    crate::hook::stop_hook();
    log::info!("session: PiP windows restored and the keyboard hook told to stop");
}
```

`teardown` is deliberately two calls. `restore_all` returns immediately when
nothing is cornered and otherwise makes a handful of `SetWindowPos`/`ShowWindow`
calls on windows it has already `IsWindow`-checked; `stop_hook` posts one thread
message. **Config is deliberately absent**: `config::save` runs synchronously at
every mutation and the file is never held open, so there is nothing pending to
flush — and writing the owner's config from a shutdown path is a way to corrupt
it, not a way to protect it. The whole handler shares one `HungAppTimeout`
(5 s default) budget with the log writes around it.

Call sites — **all three must be on the main thread**, because
`EnumThreadWindows` is thread-scoped:

```rust
// lib.rs, setup(), immediately before PROBLEM 215's split. NOT redundant with
// the one below: tao's event target exists from event-loop construction, and
// create_app_windows is delayed 10s on an autostart launch, so without this the
// crash is live for the first ten seconds of every logon.
session_end::install();

// lib.rs, end of create_app_windows() — adds `settings` and `overlay`.
session_end::install();

// display_watch.rs, end of rebuild_once(). THE HOP IS THE POINT: rebuild_once
// runs on the st-display-watch thread, where EnumThreadWindows(GetCurrentThreadId())
// enumerates NOTHING. A rebuilt overlay is a new HWND with none of the old
// window's subclasses, and this owner's display changes daily.
if on_main_thread_blocking(app, "re-arm the WM_ENDSESSION guard",
                           || crate::session_end::install()).is_none() {
    log::error!("display: the WM_ENDSESSION guard could not be re-armed ...");
}
```

PROBLEM 214's serialisation/adopt/self-heal is untouched: the re-arm runs inside
the same `REBUILDING` hold, adds no window operation, and cannot fail a rebuild.

### Two smaller changes in the same commit

**1. One panic is now ONE Sentry event.** All three `log::error!` lines in the
panic hook carry `target: telemetry::DEGRADED_TARGET`:

```rust
log::error!(
    target: telemetry::DEGRADED_TARGET,
    "PANIC on thread '{thread}' at {loc}: {msg}. ..."
);
```

`logger.rs` bridges every record to Sentry via `telemetry::log_filter`, which
sends at ERROR and above; the hook then ALSO called `capture_panic`. One panic
therefore cost **four events across two Sentry issues**, so a single crash read
as two unrelated bugs — which is exactly what happened here. `log_filter` drops
`DEGRADED_TARGET` records (PROBLEM 217 added that rule for the same reason), so
the only thing that leaves the machine now is `capture_panic`'s single Fatal
exception — the one event of the four that carries a stack trace. **debug.log is
unchanged in severity and wording**; only the `{t}` column reads
`spaceadom::degraded` instead of `space_toggle_os_lib`, so grep for `PANIC`, not
for the module path. The alternative (dropping `capture_panic`, keeping the log
route) was rejected: three events instead of one, and no stacktrace.

**2. The log now says which build it is.** Beside "Spaceadom starting":

```rust
log::info!(
    "Spaceadom build — version {} ({} bytes at {})",
    env!("CARGO_PKG_VERSION"), /* exe size */, /* exe path */
);
```

Every crash investigation in this project has had to answer "which build?" from
outside the log — an installer timestamp, a byte size, an ASCII marker hunt.
With two installers, a Store build and a repo build all able to be the running
exe, that is a real ambiguity. It doubles as a long ASCII marker for the
installed-exe check.

### How it was verified

**Verified:**

1. **Against tao's own source, not from memory.** `runner.rs:371` is
   `(Destroyed, _) => panic!(...)`; `event_loop.rs:2384` is the `WM_ENDSESSION`
   arm inside `thread_event_target_callback`; `event_loop.rs:703` installs that
   callback with `SetWindowSubclass`, and `create_event_target_window` gives it
   `WS_POPUP | WS_VISIBLE` — a real top-level window, therefore in the
   `WM_ENDSESSION` broadcast set and therefore enumerable.
2. **The premise, measured against the LIVE 1.0.94 app, with a positive
   control.** `EnumThreadWindows` over every thread of `spaceadom.exe`
   (pid 30712) — after first proving the probe works by enumerating
   explorer.exe's windows:

   ```
   UI thread 36640 owns 7 top-level window(s):
       0x20B34  class='Tauri Window'  title='Spaceadom Overlay'
       0x408BC  class='Tauri Window'  title='Spaceadom'
       0x30718  class='tray_icon_app'
       0x30716  class='com.spaceadom.app-sic'
       0x406F4  class='Tao Thread Event Target'    <-- the one that matters
       0x10B1C  class='MSCTFIME UI'
       0x40706  class='IME'
   ```

   ONE thread owns all seven, tao's event target among them. That is the entire
   premise of the fix: a single `EnumThreadWindows(GetCurrentThreadId())` from
   the main thread reaches every window that can receive `WM_ENDSESSION`.
3. `cargo check --lib` — 0 errors, 0 warnings.
4. `cargo test --lib` — 274 passed, 0 failed, 4 ignored, including 8 new
   `session_end::tests`. (The tree was shared with other concurrent lanes that
   day; 274 is the whole-tree count at this pass, not this change's
   contribution. This change adds 8.)

**NOT verified — say so, do not imply otherwise:** the runtime behaviour. No
build and no install was made (instructed: no version bump, no `tauri build`,
no install), so the fixed code has never executed. `scripts/verify-session-end.ps1`
exists to close that gap in one pass and was DRY-RUN only (it found the app, the
UI thread and the tao event target, and stopped before sending anything).
**Run it against an UNFIXED build first**: if it does not produce
`cannot move state from Destroyed` there, the harness is reproducing nothing and
any later clean run is VOID. End to end, the real case to re-test is the one
that produced this crash — the `.msi` upgrading a running app
(`src-tauri/wix/main.wxs`, `util:CloseApplication`).

### Generalise this

* **A dependency's comment can tell you its assumption; the fix is often to make
  the assumption TRUE rather than to patch the dependency.** tao says "Windows
  will shut us down". It is right about the normal case and wrong about
  `ENDSESSION_CLOSEAPP`. Owning the message and exiting is smaller, testable
  here, and does not move a pinned graph (`tao 0.35.3` is pinned through
  `tauri-runtime-wry 2.11.4` / `wry 0.55.1`).
* **`WM_QUERYENDSESSION` and `WM_ENDSESSION` are SENT, not posted.** They arrive
  through `KiUserCallbackDispatcher` straight into the window procedure, so
  anything that filters `GetMessageW` — tao's `with_msg_hook`, any message-pump
  filter — will never see them. That attempt compiles, keeps the tests green,
  and changes nothing: **a fix that cannot fail loudly is the most expensive
  kind.**
* **A thread-scoped Win32 call made off its thread returns success and does
  nothing.** `EnumThreadWindows(GetCurrentThreadId())` from a worker enumerates
  zero windows and installs zero subclasses. That is why `install()` logs an
  ERROR on a zero count and on a missing tao target — same family as CLAUDE.md's
  *a check that cannot produce a negative result is not a check.*
* **Two reporting paths for one event is two bugs in the tracker.** If a local
  log line is bridged to a reporter AND a dedicated call reports the same thing,
  pick one. Here it turned one crash into two issues and delayed this diagnosis.

---

## PROBLEM 227 — the cache-hit branch could minimise or force-foreground a RECYCLED HWND, and the comment beside it said that could not happen

*(Numbering note: 221 is claimed by another lane's in-flight `build.rs` work and
is referenced in this file's prose, and 222-226 are filed. This entry is 227 and
its code comments say 227. Append-only files never renumber.)*

This entry settles an adversarial review of the five-lane 2026-08-31 diff. Seven
findings, F1-F7. **Six were real and are fixed here; one (F2) was a real
DOCUMENTATION defect whose code is correct and was deliberately left alone.**
Each verdict is recorded, including the reasoning for the one that was not a
code change, because "the reviewer was wrong / the reviewer was right about the
words only" is exactly the kind of thing that gets re-litigated at token cost
six weeks later.

### F1 (HIGH) — Symptom

`smart_cascade.rs`'s cache-hit branch re-validated a cached HWND on exactly two
facts:

```rust
// BEFORE
let cached_unsafe = exe_lower == "explorer" && alive && { /* class != CabinetWClass */ };
let wrong_profile = alive
    && !matches!(rule, ProfileRule::Any)
    && !evidence_satisfies(&window_profile_evidence(hwnd), rule);
if cached_unsafe || wrong_profile { cache.remove(&key); }
else if alive { /* SW_MINIMIZE, or SW_RESTORE + force_foreground */ }
```

For **the overwhelmingly common case — any non-browser binding, which is
`ProfileRule::Any`** — both guards short-circuit:

* the class filter is gated on `exe_lower == "explorer"`, i.e. on what the USER
  BOUND, not on what the handle points at now;
* the profile filter is gated on `!matches!(rule, ProfileRule::Any)`.

So the only surviving check was bare `IsWindow(hwnd)`.

**The concrete failure.** Bind Space+N to `notepad`. Press it: HWND `0x000A1234`
is cached. Close Notepad. Windows recycles that handle value onto another
process's top-level window — the session handle table is shared, which is the
entire reason NATIVE_SAFETY.md rule 3 exists. Next Space+N: `alive` is true, the
class filter never runs (the binding is not explorer), the profile filter never
runs (`Any`), and the branch calls `SW_RESTORE` + `force_foreground` — or
`SW_MINIMIZE` — on a window that has nothing to do with the binding. If the
recycled handle belongs to explorer.exe shell infrastructure (`Shell_TrayWnd`,
`WorkerW`, `XamlExplorerHostIslandWindow`), **that is the 2026-08-10 touchpad
incident reproduced through a path that no longer looks at classes at all.**

Worse than the hole: the diff had just added a comment claiming the opposite —
*"Re-proving the profile on every hit is what makes a recycled handle ... fail
SAFE"*. True for `Pinned`/`Unpinned`, false for the majority of bindings, and it
reads as covered in review. PROBLEM 220's own note three lines above says the
reported bug fired on this exact branch ("Action: Minimize" with no "(Enum)"
suffix — a cache hit, no enumeration).

### F1 — Root cause

The cache path and the enumeration path had drifted apart. `enum_callback`
re-derives pid -> exe stem -> class -> title for every window it considers; the
cache path asserted none of it. The rule that keeps them honest was never
written down: **the cache-hit branch may act only on a window the fresh
enumeration would also have selected.**

### F1 — Exact file and the actual code

`D:\Claude-Projects\SpaceToggle-V14\src-tauri\src\engine\actions\smart_cascade.rs`

A pure decision function, one arm per `return BOOL(1)` in `enum_callback`, in
the same order (safety first, acceptance last):

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
enum CacheEvict { Dead, UnprovableIdentity, WrongExecutable, ShellWindow, NoTitle, WrongProfile }

fn cached_window_verdict(
    alive: bool,
    live_stem: Option<&str>,        // window_process_identity(hwnd), None = unprovable
    class: &str,                    // read off the LIVE window
    has_title: bool,                // GetWindowTextLengthW != 0
    binding_stem: &str,
    profile_proven: impl FnOnce() -> bool,   // LAZY: the only expensive fact
) -> Result<(), CacheEvict> {
    if !alive { return Err(CacheEvict::Dead); }
    let Some(live_stem) = live_stem else { return Err(CacheEvict::UnprovableIdentity); };
    if live_stem != binding_stem { return Err(CacheEvict::WrongExecutable); }
    if live_stem == "explorer" {
        if class != "CabinetWClass" { return Err(CacheEvict::ShellWindow); }
    } else if !has_title {
        return Err(CacheEvict::NoTitle);
    }
    if !profile_proven() { return Err(CacheEvict::WrongProfile); }
    Ok(())
}
```

and the branch now reads every fact from the live window before acting:

```rust
let live = if alive { window_process_identity(hwnd) } else { None };
let (class, has_title) = /* GetClassNameW + GetWindowTextLengthW, only when alive */;
match cached_window_verdict(alive, live.as_ref().map(|(_, s)| s.as_str()),
                            &class, has_title, &exe_lower,
                            || matches!(rule, ProfileRule::Any)
                               || evidence_satisfies(&window_profile_evidence(hwnd), rule)) {
    Err(why) => { /* one info! line naming what the handle points at NOW */ cache.remove(&key); }
    Ok(()) => { /* the original minimise / restore + force_foreground */ return true; }
}
```

Two details that are load-bearing:

* **The class rule is keyed on the LIVE stem, not the binding.** Keyed on the
  binding it asks "did the user bind explorer?", which is not a safety check.
* **`profile_proven` is a closure.** The Chromium property-store read is COM on
  the Space-hold dispatch path; a window that already failed a cheaper filter
  must not pay for it, and `ProfileRule::Any` must not pay for it at all. That
  preserves the "`Any` costs nothing new" property the arm's doc promises — the
  added cost on `Any` is `GetWindowThreadProcessId` + `OpenProcess` +
  `QueryFullProcessImageNameW` + `CloseHandle` + `GetClassNameW` /
  `GetWindowTextLengthW`, the same quartet `enum_callback` runs per window and
  measured at ~0.05 ms, against a `ShowWindow` it is guarding.

### F2 (MEDIUM-HIGH) — REAL, but the defect is the WORDS. Code deliberately unchanged.

`ForeignVerdict::Unidentified`'s doc, `raise_after_launch`'s doc and the
user-visible log line all asserted: *"we keep polling and let the deadline expire
having raised nothing."* The loop runs step 2 (find the target and raise it)
BEFORE step 3 (the verdict) and `return`s from step 2 — so `Unidentified` means
**keep polling AND still raise the moment the target appears.** The claim holds
only in the sub-case where the target never appears.

**Not changed, and why.** Gating the raise on "no `Unidentified` seen" would
suppress the feature's main job — the Space+key press IS the instruction, and
the only window the watcher can ever raise is the one this launch just started —
on the strength of a window we could not even name (a protected/AV process, a
process that exited between `GetWindowThreadProcessId` and `OpenProcess`, pid 0).
The behaviour is the intended trade; the sentences were wrong. All three now say
what the code does, and the enum doc carries a "READ THIS BEFORE 'RESTORING'
ANYTHING" note, because a contract the code does not keep is an invitation to
"fix" the code to match it.

### F3 (MEDIUM) — the target-thread attach: a guard that could not fire, and a missing `!fg_hung`

**(a)** PROBLEM 225 added `AttachThreadInput(my_thread, target_thread, true)` and
its comment said the new attach "inherits that guard for free" from the
`IsHungAppWindow(hwnd)` early return above it. `IsHungAppWindow` only reports
true after ~5 s of a thread not pumping, and the target this attach was added for
is `raise_after_launch`'s — a window younger than `GIVE_UP_MS` (8 s) and usually
younger than 2 s. **The detector's threshold is longer than the window's entire
life**, so on the launch path that guard cannot produce a negative result, and we
joined our input queue to a process that is mid-startup and may not be pumping
yet.

Fixed with a question a 600 ms old window CAN fail:

```rust
const PUMP_PROBE_MS: u32 = 100;
let target_pumping = target_thread == my_thread || {
    SendMessageTimeoutW(hwnd, WM_NULL, WPARAM(0), LPARAM(0),
                        SMTO_ABORTIFHUNG, PUMP_PROBE_MS, None).0 != 0
};
```

A thread that is pumping answers `WM_NULL` in microseconds. Failure skips only
the ATTACH — `BringWindowToTop`/`SetForegroundWindow` and steps 3 and 4 still
run, so the worst case is a raise that falls through to `SwitchToThisWindow`.
Bounded on purpose: this runs on the ENGINE thread, so an unbounded wait would
stall every shortcut.

**(b)** `attached_fg` carried `&& !fg_hung`; `attached_target` carried no
`fg_hung` term. Brave wedged in the foreground, Space+D for a healthy Discord: we
correctly skipped the fg attach, logged *"Focus may not switch this time"* — then
attached to Discord's UI thread anyway and called `SetForegroundWindow`, whose
activation work still has to reach the wedged outgoing window. If that blocks,
the caller stalls **while sharing an input queue with Discord**, which is PROBLEM
121's "two applications go unresponsive instead of one", one window over — and
from `try_focus_or_minimize` the caller is the ENGINE thread, so every shortcut
dies with it.

```rust
// AFTER
let attached_target = target_thread != 0
    && target_thread != my_thread
    && target_thread != fg_thread
    && !fg_hung           // F3(b)
    && target_pumping;    // F3(a)
```

The attach exists only to defeat the foreground LOCK, and that requires the
FOREGROUND thread's queue. Once that half is skipped, attaching to the target
alone buys nothing and keeps all of the risk. The detach pairing (same bools,
reverse order) was already correct and is untouched.

### F4 (LOW) — the one step-3 outcome release builds could not see

`log::debug!("force_foreground: SendInput sent {sent} events ...")` — and
`logger.rs` filters at Info in release, so `sent == 0` (injection blocked by UIPI
or another thread's `BlockInput`) was unreportable. PROBLEM 225's COMMIT 0
promoted every other step outcome for exactly this reason and missed this line.
Now `info!` on a full insert and `warn!` on a short one.

### F5 (MEDIUM) — seven `SendInput` batches discarded their return value; NATIVE_SAFETY section 3's recovery existed only in prose

`SendInput` inserts events **one at a time** and stops at the first one another
thread blocks, returning a short count. A partial insert of `Win-down
Shift-down M-down` leaves LWIN and LSHIFT physically latched in the OS with no
corrective KEYUP anywhere in the tree. NATIVE_SAFETY.md section 3 names the cure
— *"send corrective KEYUPs"* — and nothing implemented it. Sites:
`boss_key.rs:117,168`, `focus_engine.rs:196,210,221,254`, `engine/mod.rs:656,724`,
plus the hook's own `inject_space` / `inject_space_then_key`, where a partial
insert latches SPACE ITSELF.

One function now owns the rule, in `hook/mod.rs`:

```rust
/// Which virtual keys a SHORT insert left physically DOWN, newest-first.
/// NO ALLOCATION: two callers are inside the hook callback, where a `Vec` is
/// not allowed (PROBLEM 58). `get(i)` yields `(vk, is_keyup)` so the caller can
/// decode `INPUT`s in place instead of building a list first.
pub(crate) fn unreleased_keys_into(
    len: usize, inserted: usize, get: impl Fn(usize) -> (u16, bool), out: &mut [u16],
) -> usize {
    let mut n = 0usize;
    for i in 0..inserted.min(len) {
        let (vk, is_up) = get(i);
        if vk == 0 { continue; }                      // KEYEVENTF_UNICODE latches nothing
        if is_up {
            if let Some(pos) = out[..n].iter().position(|&h| h == vk) {
                out.copy_within(pos + 1..n, pos);
                n -= 1;
            }
        } else if !out[..n].contains(&vk) && n < out.len() {
            out[n] = vk;
            n += 1;
        }
    }
    out[..n].reverse();                               // release newest-first
    n
}

/// `Vec` convenience for tests and readers. DELEGATES — two copies of this
/// ordering rule that could disagree is the failure mode being fixed.
pub(crate) fn unreleased_keys(batch: &[(u16, bool)], inserted: usize) -> Vec<u16>;

pub(crate) unsafe fn send_keys_raw(inputs: &[INPUT]) -> bool;      // NEVER logs - callback-safe
pub(crate) unsafe fn send_keys_checked(inputs: &[INPUT], what: &str) -> bool;  // + a WARN
```

`send_keys_raw` decodes the batch in place out of the `INPUT` union rather than
making the caller describe it twice, builds the corrective KEYUPs in a stack
`[INPUT; 16]`, sends them as one cookie-tagged batch, and counts
`PARTIAL_INJECTIONS` / `CORRECTIVE_KEYUPS`. **The two hook
callback sites use `send_keys_raw`, which never logs** — PROBLEM 58 — and the
counters are drained on the engine thread by `drain_hook_diagnostics`.
`force_foreground`'s VK_NONAME tap keeps its own call: it is immune by
construction (a reserved key, not a modifier), and that reasoning is precisely
what had never been carried to the sites that send real modifiers.

### F6 (NIT) — a stale comment whose "correction" would have introduced a bug

`hook/mod.rs`'s PROBLEM 184 comment said `track_modifier` runs "BEFORE every
early return below (fullscreen, bypass, **the cookie check**)". The cookie
early-return is ~30 lines ABOVE it. The code is right — our own injected Alt
(from `handle_force_close`) must not enter the physical-modifier mask, or the
mask that decides whether Space is passed through would read our injection as a
held Alt. Comment corrected, with the reason, so nobody "fixes" the order.

### F7 (MEDIUM) — `capture_panic` was the one submit path that never scrubbed

`telemetry.rs`. `report_degraded` scrubs (`scrub(&cap(detail, MAX_DETAIL))`) and
`report_frontend_error` scrubs. `capture_panic` passed `info.payload()` straight
through as the exception value, at Fatal, from the one path guaranteed to fire on
the crashes that matter most — while PRIVACY.md promised the user every report is
scrubbed.

```rust
// BEFORE                          // AFTER
value: Some(message.to_owned()),   value: Some(scrub(&cap(message, MAX_DETAIL))),
```

`send_default_pii = false` does not cover this: it governs the crate's automatic
user/server context, not an exception value the app supplies. Today's production
panics are dependency strings with no PII, so this was a contract gap rather than
a live leak — one `expect(&format!("... {}", path.display()))` away from being
real, and invisible because the surrounding hook looks handled. PRIVACY.md's
paragraph was also corrected: the `<redacted>` pass is the FRONTEND reporter's
(`js-error-reporter.ts`), there is no quoted-string pass in Rust's `scrub` at all,
so "scrubbed twice" was true of one route and false of every other.

### How it was verified

* `cargo test --lib` — **294 passed, 0 failed** (274 before this work; +8 for the
  cache-hit verdict, +8 for `unreleased_keys`, +4 for PROBLEM 228's instrument).
  New: `smart_cascade::cached_hwnd_tests` (the recycled-handle case is
  `a_recycled_handle_now_pointing_at_the_shell_is_never_acted_on`, plus a test
  that the profile read is never paid for by a window that already failed) and
  `hook::partial_injection_tests`.
* `cargo check --lib` — clean. **The check itself was validated against a known
  bad case first** (a deliberate type error was appended to `hook/mod.rs`, the
  check failed with E0308 at the right line, then the probe was removed) — note
  `lib.rs:1` carries `#![allow(dead_code, unused_must_use, unused_imports)]`, so
  "no warnings" here is partly that allow, not proof that nothing is unused.
* `npx tsc --noEmit` — clean, exit 0. No frontend file was touched.
* **NOT verified on the real machine.** Nothing was built, bundled or installed,
  by instruction. F1's recycled-handle path, F3's `WM_NULL` probe and F5's
  corrective KEYUPs all describe runtime behaviour that only hardware testing can
  confirm.

### Generalise this

* **A cache is a second implementation of the matcher.** Any branch that acts on
  a remembered handle must assert everything the fresh path asserts, or the two
  will drift and the drift will be invisible — the cache path is the one that
  never enumerates, so nothing else can catch it.
* **A safety filter keyed on the request is not a safety filter.** "Did the user
  bind explorer?" and "is this the taskbar?" are different questions; only the
  second one protects anything.
* **A check whose threshold outlives its subject cannot fire.** `IsHungAppWindow`
  needs 5 s; the window it was guarding lives 600 ms. Same family as CLAUDE.md's
  *a check that cannot produce a negative result is not a check* — and the comment
  claiming the guard was inherited "for free" is what made it invisible.
* **A comment that asserts a safety property is a claim, and claims get audited.**
  Two of these seven findings are comments that were more confident than the code.
  Write what the code does; when the code is deliberately weaker than the ideal,
  say so in the same breath.
* **Every syscall with a return value is a branch you have written or a branch you
  are ignoring.** `SendInput`'s short count went unread at nine sites for months,
  in an app whose worst failure mode is a latched key.

---

## PROBLEM 228 — the "hook is DEAF" alarm cited its own repair as proof: 281 of 282 reports were the instrument measuring itself

### Symptom

`SPACEADOM-2` is the app's oldest recurring Sentry issue: 17 grouped events,
*"the primary keyboard hook saw 0 events in Ns while the reference hook fired Nms
ago — keys are reaching the chain and this app is not seeing them, so every
shortcut is dead."* Every previous session treated it as the app's central
unsolved fault.

Measured against `%APPDATA%\Spaceadom\debug.log` + `.0` — 41,675 parsed lines,
2026-08-11 to 2026-08-31 — **281 of the 282 `hook: DEAF` lines have their implied
"reference hook fired" instant within 10 ms of a watchdog re-hook.** The cleanest
sample:

```
09:52:39.420 [WARN] hook: WATCHDOG — ... Re-hooking. reinstall ok: true
09:52:39.435 [WARN] hook: DEAF for the last 60s — the reference hook fired 16ms ago
                          (keys ARE reaching the chain) but the primary hook saw 0 of them.
```

The reference hook did not fire 16 ms earlier. `install_hooks()` stamped its clock
15 ms earlier. Strip that clause and the line reduces to "the primary saw 0 keys
in 60 s", which the surrounding minutes explain as a typing pause: `09:47:39 saw
29 keys / 09:48:39 saw 37 / 09:49:39 DEAF / 09:50:39 saw 31 / 09:51:39 DEAF /
09:54:09 saw 49`. That is the ambiguity PROBLEM 101 deleted a whole detector over,
reinstated at Sentry level by PROBLEM 217.

### Root cause

`LAST_REF_KB_EVENT` had TWO writers: `ref_kb_hook_proc` (the callback) and
**`install_hooks()`**, which runs on every watchdog re-hook. The DEAF test read
that clock:

```rust
// BEFORE - hook/mod.rs, drain_hook_diagnostics
let ref_silence = now.saturating_sub(LAST_REF_KB_EVENT.load(Ordering::Relaxed));
...
} else if ref_silence < 60_000 {
    log::warn!("hook: DEAF ... the reference hook fired {ref_silence}ms ago ...");
    crate::telemetry::report_degraded(Degraded::HookDeaf, ...);
}
```

**The repair was writing the evidence, and the alarm was reading it back.** On a
machine that re-hooks 15-40 times a day, essentially every quiet minute that
contained a re-hook came out "deaf".

The same seeding pollutes two more instruments, recorded here for the next
session: `LAST_KB_EVENT` has THREE writers (callback, `install_hooks`, and the
watchdog's own idle re-stamp), which is why **54% of watchdog alarms print
`kb_silence == ms_silence` exactly** — two hooks do not fall silent in the same
millisecond; one non-hook writer setting both does. And `kb_only_dead`, the one
honest detector PROBLEM 181 built, **has never fired once in 20 days**: all 1,925
alarms are `both_dead`. There is no measured instance of the failure SPACEADOM-2
names.

### Exact file and the actual code

`D:\Claude-Projects\SpaceToggle-V14\src-tauri\src\hook\mod.rs`

**1. A counter only the callback can move.** A timestamp can be forged by anything
holding a `store`; a counter incremented in one place cannot.

```rust
static REF_KB_EVENTS: AtomicU32 = AtomicU32::new(0);          // callback only
static REF_HOOK_INSTALLED_AT: AtomicU64 = AtomicU64::new(0);  // what the install may say
static LAST_KB_CALLBACK: AtomicU64 = AtomicU64::new(0);       // primary hook, callback only

unsafe extern "system" fn ref_kb_hook_proc(...) -> LRESULT {
    if n_code >= 0 {
        LAST_REF_KB_EVENT.store(tick_count(), Ordering::Relaxed);
        REF_KB_EVENTS.fetch_add(1, Ordering::Relaxed);   // the only addition this fn may accept
    }
    CallNextHookEx(None, n_code, w_param, l_param)
}
```

`install_hooks()` loses one line and gains one:

```rust
// BEFORE                                   // AFTER
LAST_REF_KB_EVENT.store(now, Relaxed);      REF_HOOK_INSTALLED_AT.store(now, Relaxed);
```

`LAST_KB_EVENT` / `LAST_MS_EVENT` keep their re-stamps: those ARE the alarm's
baseline and removing them re-opens PROBLEM 101's 260 false alarms. The rule is
now written into their doc comments — **seeded clocks for the alarm,
callback-only clocks for the sentences.**

**2. The verdict is arithmetic on counters, and it is pure:**

```rust
pub(crate) enum HookWindow { Working, Quiet, Deaf }

pub(crate) fn classify_hook_window(primary_seen: u32, genuine_ref_events: u32) -> HookWindow {
    if primary_seen > 0 { HookWindow::Working }
    else if genuine_ref_events > 0 { HookWindow::Deaf }
    else { HookWindow::Quiet }
}
```

`drain_hook_diagnostics` keeps a `LAST_REF_COUNT` baseline advanced in lockstep
with `LAST_SEEN_REPORT` (two halves of one test must describe one window), takes
the delta with `wrapping_sub`, and reports:

```
hook: DEAF for the last 60s — the reference hook was genuinely CALLED 12 time(s)
in that window (the last genuine key reached it 340ms ago) but the primary hook
saw 0 of them. Counted, not inferred from a timestamp the re-hook could have
stamped.
```

**The Sentry `report_degraded(Degraded::HookDeaf, ...)` call is inside that arm
and nowhere else**, so a window whose only "evidence" is a re-hook can no longer
produce an event.

**3. The watchdog's sentences, made honest without touching its repairs.**
`ref_silence` is genuine now, and `kb_only_dead` gained `ref_seen > 0` (the
reference clock is 0 until it has genuinely fired, and `now - 0` is machine
uptime, which is under `BLIND_MS` for the first three seconds after a boot — a
test satisfiable by a fresh boot clock is not a test). The 60-second hold-off line
used to say *"the last repair DID deliver events"* while its test read the seeded
`LAST_KB_EVENT` — satisfied by the idle re-stamp two seconds after any re-hook,
100 such lines in the current log. **The decision is unchanged on purpose**; the
line now prints the callback-only clock beside it:

```
... but the cooldown test says the last repair delivered events — genuine keyboard
callback: NONE since the repair (the test above was satisfied by the idle
re-stamp, not by a key). Holding off for the rest of the 60s cooldown.
```

Grep the next fortnight for that phrase: it is the hold-off happening on no
evidence, and it is the measurement that decides whether the decision should
change.

### What was deliberately NOT done, and why

* **The escalation regression (fix brief part B) is untouched.**
  `BLIND_REINSTALLS.store(0)` sits in the "hooks look fine" early return, and
  after a re-hook `install_hooks()` stamps all the clocks, so ticks at +1/+2/+3 s
  are healthy by construction and zero the streak: `streak >= 2` is unreachable,
  the escalation ERROR last fired 2026-08-25 and has not fired in 1,925 alarms
  since. **Making it reachable while the alarms may be phantoms would convert
  4-second noise into a permanently deaf app** — `lib.rs`'s supervisor gives up
  FOREVER after 5 rebuilds in 10 minutes. That is the fix being worse than the
  bug. Do it only after a fortnight of the honest instrument says the alarms are
  real.
* **The `previous_worked` DECISION and the idle re-stamp (part C) are unchanged.**
  Both change how often the app re-hooks. This pass was scoped to what gets
  REPORTED.
* **The user-facing copy was NOT changed, and it is currently wrong.**
  `get_hook_health` (`commands.rs:646`) feeds `drawHookHealth`
  (`src/components/settings-panel.ts:1240`), which tells the user *"The likely
  cause is PowerToys and spacedesk, which watch the keyboard too"* and explains
  everything through `LowLevelHooksTimeout` eviction. The owner's own data refutes
  the first — three windows totalling ~17 h with both closed give **21.0
  DEAF-minutes per 100 active minutes versus 9.9 with them running** (confounded,
  but there is no window in 20 days where closing them stopped it) — and the
  reference hook has never once confirmed the second. **Owner's call**, because it
  is user-visible copy: the recommendation is to keep the eviction count and drop
  the causal sentence until the honest instrument can name a cause.
* **Section 9 of the investigation is unresolved.** On 63% of alarms `user active`
  reads 0/15/16 ms — faster than `GetTickCount`'s resolution, i.e. continuous
  pointer motion — while the MOUSE hook has been silent 3-4 s. Either the hooks
  really are bypassed for seconds, or something keeps `GetLastInputInfo` fresh
  without producing keyboard/mouse messages (touch/pen, a non-mouse HID, a
  jiggler). The settling measurement is two `GetCursorPos` calls ~200 ms apart plus
  the raw `LASTINPUTINFO.dwTime`, logged at alarm time on the `WM_TIMER` branch
  where Win32 is already legal. Not added here.

### How it was verified

* `cargo test --lib` — 294 passed, 0 failed. New module
  `hook::deaf_instrument_tests`; the load-bearing one is
  `a_re_hook_alone_is_not_evidence_that_keys_are_reaching_the_chain`, which asserts
  `classify_hook_window(0, 0) == Quiet` — the 281 lines.
* `cargo check --lib` clean (validated against a deliberate error first);
  `npx tsc --noEmit` clean.
* **NOT verified on the real machine, and the next confirmation is a LOG, not a
  test.** After this ships, `hook: DEAF` should become rare, and every one that
  does print names a counted number of reference calls. If DEAF lines keep
  arriving at the old rate, the alarms were real all along and part B becomes
  urgent. Either way the next fortnight's data is trustworthy for the first time —
  which is the point of this entry.

### Generalise this

* **An instrument the measured system can write to is not an instrument.**
  `install_hooks()` stamping `LAST_REF_KB_EVENT` is the whole bug: the repair
  wrote the evidence and the alarm read it back. Ask "who else can move this
  value?" of every static a log line quotes.
* **Prefer a counter to a clock for "did X happen?".** A timestamp answers "when
  was this value last written", which is not the same question, and any writer can
  answer it. A counter incremented at exactly one site cannot be forged.
* **Seeded baselines and observations must be different variables.** When a clock
  has to be re-stamped to suppress false alarms, that clock has stopped being an
  observation. Keep both, name them differently, and let the alarm read the seeded
  one and the sentences read the honest one.
* **A report that is wrong 281 times out of 282 is worse than no report**, because
  it trains its reader to ignore the tracker — and it sends a session chasing a
  fault the app has never once observed.
* **Fix the measurement before the mechanism.** Restoring escalation on top of a
  phantom alarm would have converted 4-second noise into a permanently dead app.

---

## PROBLEM 229 — the browser-profile picker led with a name the browser invented ("Person 3") and put the ONE distinguishing value, the account, on a dimmer second line

**Shipped in 1.0.95, 2026-08-31. This REVERSES the design PROBLEM 223 shipped**
— it does not replace the feature, it re-ranks the two values that feature
already had.

### Symptom

Owner's verdict on the 1.0.94 tree: the picker tile, the key-editor chip and
the Guide HUD chip all headline `display_name`, the browser's own label for a
profile. On his machine Chrome has **14 profiles**, and Chrome names them
"Person 1", "Person 3", … — the very thing PROBLEM 223 set out to fix was
still the first thing the eye landed on, with the value that actually
distinguishes them (the signed-in account) rendered smaller and greyer
underneath. The HUD chip was worse: it caps at 118px, so a full address could
never fit there at all, and only `display_name` was ever passed to it.

### Root cause

Not a bug — a ranking decision made when the email field was added. PROBLEM 223
treated the address as *confirmation* for a name the user was assumed to know.
On a machine where the user never named the profiles, there is no such name:
the browser made one up, and the address is the only human-meaningful value in
the record.

### The fix, in one sentence

Derive ONE label in Rust — the email's local part, falling back to the display
name — hand it to the frontend on `BrowserProfile`, and render THAT everywhere
a profile is named. The full address moves to the hover tooltip only.

### Exact files

| File | What changed |
| --- | --- |
| `src-tauri/src/browser_profiles.rs` | `email_local_part` + `account_label` (new, pure, tested); `account_label` field on `BrowserProfile`, computed in `profiles_from_local_state`; the sort key moved to it; a new log line reporting the split by COUNT; the live-scan diagnostic no longer prints addresses |
| `src-tauri/src/telemetry.rs` | `redact_emails` — `scrub()` now replaces anything address-shaped with `<email>` before its path/URL walk |
| `src/types.ts` | `account_label?: string` on `BrowserProfile`; `browser_profile_name`'s contract restated |
| `src/components/browser-profile-picker.ts` | `labelOf()`; tile headline, letter disc, chip, filter and `onPick` all read it; `bp-tile-email` → `bp-tile-sub` now carries `display_name`; localStorage key bumped to `v2`; the picked label removed from the console line |
| `src/components/key-detail-panel.ts` | the one-profile auto-pin stores `labelOf(only)`; the commit log line no longer echoes the label |
| `src/styles.css` | `.bp-tile-email` → `.bp-tile-sub`, comment rewritten |

### The actual code

```rust
// browser_profiles.rs — the whole rule.
pub fn email_local_part(email: &str) -> Option<String> {
    let e = email.trim();
    let (local, domain) = e.split_once('@')?;
    if domain.contains('@') { return None; }      // two @ — refuse, do not guess
    let local = local.trim();
    if local.is_empty() || domain.trim().is_empty() { return None; }
    Some(local.to_string())
}

pub fn account_label(display_name: &str, email: Option<&str>) -> String {
    email.and_then(email_local_part)
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| display_name.to_string())
}
```

`user_name` is free text in someone else's JSON, so the validation is
deliberately strict: the fallback (`display_name`) is always a sane thing to
show, which is exactly when a doubtful value should be refused rather than
half-rendered. **Generalise: validate hard when you have a good fallback; it is
the paths with NO fallback that have to accept whatever they get.**

```ts
// browser-profile-picker.ts — the single READ. Every surface goes through it.
export function labelOf(p: BrowserProfile): string {
  return p.account_label?.trim() || p.display_name;
}
```

The `?.trim() ||` is not defensive noise. `readLastKnown()` deserialises a
browser list that a build of **1.0.94 or earlier** wrote to localStorage, and
that shape has no `account_label` at all. Two guards, both wanted: the key was
bumped to `st-bp-browsers-v2` so a stale entry reads as "nothing cached", and
this fallback covers anything that still slips through.

### Why the field is computed in Rust and not in the picker

Four surfaces name a profile — picker tile, HUD ring chip, key-editor chip,
toast — and only ONE of them can read `Local State`. The HUD runs on the
Space-hold latency path and must never open a ~96 KB JSON file (that is the
entire reason `browser_profile_name` exists, PROBLEM 223). So the label is
resolved once, at pick time, and stored. Deriving it in the picker would have
put the rule in TypeScript where `cargo test` cannot reach it, and where the
one-profile auto-pin in `key-detail-panel.ts` had already drifted once.

### Emails never reach a log or a crash report

Three separate places, because "no call site does it today" is a promise about
the present:

1. `console.info("bp: profile picked — …")` and `"bp: commit reached — …"` no
   longer echo the label. `dir` identifies the tile just as precisely and
   identifies nobody; the commit line reports `named=yes|no`.
2. The one NEW Rust log line reports **counts only** — "N of M profile(s) are
   signed in and are labelled by the local part of their account". That is what
   is diagnosable (a `user_name` read coming back empty on a machine where it
   should not) without naming anybody.
3. `telemetry::scrub` now redacts addresses structurally, so it does not matter
   who drops one into a panic message or a JS error.

```rust
// telemetry.rs — anchored on the '@' and expanded outwards, because an
// address has no prefix to key off. Runs BEFORE the path/URL walk: both of
// those consume to the next space, and a path CAN contain spaces, so running
// them first would let an address survive in the tail of one.
let redacted = redact_emails(input);
```

A domain must end in a real TLD (a dot, then two or more letters), which keeps
`a@b`, a bare `@` and Rust's own `#[cfg(…)]`-shaped noise out of it.

### The picker tile, before and after

```
BEFORE (1.0.94)              AFTER (1.0.95)
+------------------+         +------------------+
| (P)  Person 3    |         | (S)  studies     |   <- account, the headline
|      studies@... |  dim    |      Person 3    |   <- browser's name, dim
+------------------+         +------------------+
   tooltip: Chrome - Person 3 - studies@example.com  (Profile 6)
                              \- the FULL address, here and nowhere else
```

The second line is drawn **only** when the profile is signed in AND the label
differs from the display name — a profile that is not signed in is already
headlined by its name, and repeating it is the "Brave — Brave" mistake
`hud_label` has guarded against since PROBLEM 223.

### How it was verified

- `cargo test --lib`: **302 passed, 0 failed** (294 before; 8 new — five on the
  pure rule, two on the parse path, one on the scrubber).
- `cargo check --lib`: 0 warnings. `npx tsc --noEmit`: clean.
- Installed on the real machine as 1.0.95 through
  `explorer.exe → scripts/install-real.cmd` (PROBLEM 143). The sandbox
  differential was re-measured first and still holds: the SAME path string
  `C:\Users\beamu\AppData\Local\Spaceadom\spaceadom.exe` returned **1.0.53 /
  14,109,184 bytes** from the agent shell and **1.0.94 / 19,200,512 bytes**
  through `explorer.exe`.
- Marker, both halves as CLAUDE.md requires. Two pieces of the new log format
  string — "signed in and are labelled by the local part of their account" and
  "keep the browser's own display name (no address is ever logged)" — were
  confirmed **True in the freshly-built exe** (11:24:13) BEFORE the baseline
  ran, measured **False in the installed 1.0.94** (11:24:39, seven controls
  True in the same scan), and **True in the installed 1.0.95** (11:24:49).
  Both are `format_args!` pieces, so `.rodata` is guaranteed and the
  short-literal immediate-store trap cannot reach them.
- Frontend chain: `bp-tile-sub`, `st-bp-browsers-v2` and `account_label` all
  present in `dist2/assets`; exe `LastWriteTime` 11:23:50, later than the
  newest `dist2` file at 11:22:31.
- Live: PID 45384 from `%LOCALAPPDATA%\Spaceadom\spaceadom.exe`, startup
  **1129 ms** (band 889–1474), overlay verdict **alive** — one
  `overlay: configured`, zero `REBUILD FAILED`, zero `OVERLAY_DISABLED`.
- Config untouched and read from outside the container: 77,830 bytes, matching
  `config: saved 77830 bytes` in `debug.log` — NOT the 47,754-byte shadow.
  Parses; 6 profiles; zero `"browser_exe": ""`.

**Untested by hand, and labelled so:** the tooltip is the only place the full
address now appears, and a tooltip cannot be triggered from this shell. The
rendering path is the same `tile.title` assignment that shipped in 1.0.94 —
what changed is which field feeds the visible spans, not whether the tooltip
exists.

### Old pins are NOT migrated

A binding pinned before 1.0.95 holds the display name in
`browser_profile_name`, and it keeps it. There is no migration because there is
nothing to migrate FROM: the config stores no address, and re-deriving the
label would mean reading every browser's `Local State` at startup — the one
piece of I/O this field exists to avoid. Re-picking the profile rewrites it in
one press, and the chip is unaffected in the meantime because it prefers the
LIVE profile and only falls back to the stored string for an uninstalled
browser.
