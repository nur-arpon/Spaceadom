# V14 — every problem, and the exact code that solved it

**Written 2026-08-11 by Claude Opus 5 (Claude Code).**

Purpose: an AI picking this up should not have to re-diagnose anything below,
and should not have to hunt for where the fix lives. Each entry is
**symptom → root cause → the exact file, the exact code, how it was verified.**

Companion docs: `PROJECT_STATUS.md` (chronological log), `WHAT_HAPPENED.md`
(plain English), `V13_TO_V14_METHOD.md` (why two earlier attempts failed),
`OVERLAY_ACHIEVED.md` + `OVERLAY_RUST_HTML_CHANGES.md` (the overlay).

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
The BEHAVIOUR — a silent setup.exe upgrade over a running app landing
correctly — is being tested by the owner with 1.0.39 as of this writing and is
NOT yet confirmed.

**KNOWN GAP.** This fixes the NSIS `setup.exe` only. Tauri v2 has no
`installerHooks` equivalent for WiX, so the `.msi` still defers silently over a
running app. The setup.exe is what users download and what 10.2.9 accepts; if
the MSI ever becomes the shipped artifact, it needs a WiX
`util:CloseApplication` custom action.

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

**How it was verified.** Arithmetic verified against the four display cases
above; `tsc` clean; shipped in 1.0.39. NOT yet judged visually — that verdict
belongs to the owner on his own screens.

**Generalise this.** *When a user says "proportionate", the margins are part of
the proportion.* A layout that pins its padding while scaling its content
reads as cramped at exactly the sizes where it was meant to shine. And a fix
that inverts a complaint (too small → too big) usually means the constraint
was moved to the opposite extreme instead of being related to the thing it
should track.
