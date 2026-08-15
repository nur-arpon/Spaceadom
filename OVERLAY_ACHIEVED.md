# The overlay: radial Guide HUD + island toasts — WHAT WORKS, AND EXACTLY HOW

**Written 2026-08-10 by Claude Fable 5 (Claude Code).**
Scope: the **overlay window only** — the Guide HUD and the toast system.
The user confirmed the radial Guide HUD as correct ("brilliant"). This file
records precisely how it was achieved so it never has to be re-derived.

**This file deliberately says NOTHING about the dashboard.** That work was
rejected by the user and must not be treated as a reference.

Source of truth for the visuals:
`D:\Claude-Projects\design-overhaul\Design system overhaul project\design-upgrade-using-claude\handoff\overlay-reference.html`
Port from that file. It is a working vanilla implementation.

---

## 1. Files that make it work

| File | Role |
| --- | --- |
| `src/components/toast.ts` | ALL overlay logic: toast manager + HUD builder + listeners |
| `src/styles/overlay-earthy.css` | Every overlay visual + keyframe, both palettes |
| `overlay.html` | Loads both; body transparent; links the css |
| `src-tauri/src/commands.rs` | `overlay_fit`, `overlay_fit_hud`, `overlay_toasts_done` |
| `src-tauri/src/guide_hud/mod_impl.rs` | Emits the payload, shows/hides the window |

---

## 2. Window mechanics — the non-obvious part (get these wrong = invisible UI)

1. **The overlay window is `transparent: true` and small/on-demand.**
   A *fullscreen* transparent window composes ZERO pixels on this machine
   (V13 PROBLEM 14). A small one works. Never make the HUD full work-area —
   size it to the computed bloom box instead (§3.4).
2. **Windows 11 draws a 1px DWM border on undecorated windows**, which reads
   as a rectangle around the overlay. Killed in `lib.rs` overlay setup with
   `DWMWA_BORDER_COLOR = 0xFFFFFFFE (COLOR_NONE)` + `DWMWCP_DONOTROUND`.
3. **Show the window BEFORE rendering content** — hidden webviews don't paint.
4. **This page must be the ONLY listener registrant** (`overlay.ts`), and the
   window MUST be listed in `capabilities/default.json` or every `listen()`
   rejects silently and the HUD is an empty box.
5. **Global `emit` + a single listener** is the only arrangement that
   delivers here; `emit_to` never worked.
6. `backdrop-filter` is banned on this window (white-box bug).

---

## 3. The radial Guide HUD — the part that came out right

Shown only while Space is held. Centred on screen. Chips are built from the
LIVE payload, so only assigned letters appear (0–26) and they update as
bindings change.

### 3.1 Geometry (no fixed radii — names and screens both vary)

```js
estW(label, special) = min(label.length * 6.8, 118) + (special ? 64 : 56)
innerHalf = max(estW of specials) / 2
outerHalf = max(estW of apps) / 2
rxInner = 115 + innerHalf + 26        // 115 clears the SPACE pill
rxOuter = rxInner + innerHalf + outerHalf + 26
ryInner = 118 ;  ryOuter = 196        // ellipses, not circles
```

### 3.2 The viewport clamp — this is what makes it fit ANY screen

```js
const vw = screen.availWidth, vh = screen.availHeight;   // SCREEN, not window
const scale = Math.min(1, (vw/2 - outerHalf - 24) / rxOuter,
                          (vh/2 - 40) / ryOuter);
rxInner *= scale; rxOuter *= scale; ryInner *= scale; ryOuter *= scale;
```
Measure against the **screen**, because the window is sized *from* these
numbers afterwards. Rebuild on `resize`.

### 3.3 Angles — width-weighted so chips never overlap

```js
angles(items, special, offset):
  ws  = items.map(estW)
  tot = sum(ws)
  acc = 0
  → for each w: ((acc + w/2)/tot) * 2π - π/2 + offset ; acc += w
```
Inner ring offset `0`; **outer ring offset `π / apps.length`** — a half-step
rotation so the two rings never line up.

### 3.4 Window size — with padding, or the circle looks "cut off at the back"

```js
PAD = 180;                                   // clears the 340px ring-pulse + glow
w = max(2*(rxOuter + outerHalf) + PAD, 360 + PAD)
h = max(2*ryOuter + PAD, 360 + PAD)
invoke("overlay_fit_hud", { width: w, height: h })
```
`overlay_fit_hud` centres on BOTH axes and clamps to 94% of the monitor
(never fullscreen — see §2.1). `guide_hud/mod_impl.rs` also places the window
centred before showing, so there is no jump from a bottom-anchored box.

### 3.5 Motion

- Centre: terracotta **SPACE pill 230×60 radius 999**, `st-space-pop` 560ms
  spring. Text is `SPACE` only.
- One-shot `st-ring-pulse` ring (340px) behind it, 900ms, on **every** show.
- Chips bloom OUTWARD from centre: per chip set
  `--fx = -x`, `--fy = -y` (vector back to centre), animation
  `st-bloom-in` 620ms spring, delay **specials `120 + i*26`ms**, **apps
  `300 + i*26`ms**.
- Hide: whole HUD `opacity 0` + `scale(.93)`, 220ms `--ease-in`, then unmount.

### 3.6 Chip styling
Specials = terracotta tint `#f6dfc9` / border `#dfaa7c` / text `#7c3f16`,
kbd chip `#c67139`. Apps = sage `#e9ecdd` / `#b9c2a2` / `#47523a`, kbd
`#7a8a5e`. Radius 110px. Label `max-width:118px` + ellipsis (hard bound —
arbitrary app names must never break layout). Nocturne swaps all of these
via `body.nocturne` (indigo `#6b8cd6` family on navy).

---

## 4. The toast system — island pill

### 4.1 Phases (per toast)
```
dot   (0–200ms) : max-width 42px, opacity .95, translateY(18px) scale(.45), NO transition
open  (200ms+)  : max-width per depth, transform none, all 560ms spring
                  inner fade-ins: msg 300ms@140ms, dot @180ms, ring @200ms
leave           : max-width 42px, opacity 0, translateY(-8px) scale(.5), 340ms ease-in
```
Lifetime 2800ms; removal at `200 + LIFE + 380`.

### 4.2 THE STACK RULE (this is the one that is easy to get wrong)

> A pill's depth = **the number of NEWER toasts currently in the "open"
> phase** — never its array index.

Index-based depth breaks while another toast is mid-enter or mid-exit;
counting open successors keeps sizes monotonic newest→oldest at all times.

```js
for i in list:
  if list[i].phase !== "open": continue
  depth = list.slice(i+1).filter(t => t.phase === "open").length
  el.dataset.depth = min(depth, 2)
```
Depth drives all three: **scale 1 / .9 / .8**, **opacity 1 / .7 / .45**,
**max-width 560 / 300 / 128px**. Newest = biggest and longest; older ones
step shorter toward circular; the oldest collapses to a dot and exits.
Hard cap 3.

### 4.3 Contents
24px round icon disc · message · 7px status dot · 20px SVG progress ring
(`r=8`, `stroke-dasharray 50.27`, `rotate(-90 10 10)`, animation
`st-ring-drain` over the toast lifetime, linear forwards).

### 4.4 The glow, and why it looked "cut"
A 340×150 blurred ellipse breathes behind the stack (`st-toast-glow` 4s).
At `bottom:-34px` with a 22px blur it bled past the window frame and
rendered as a straight cut. Fix = keep it **inside** the window and pad the
window:
```
container bottom: 74px ; glow bottom: 8px
window w = max(content + 420, 520) ; window h = content + 240
```
(Same principle as the HUD: the window box must be bigger than the glow.)

### 4.5 HUD → toast transition
A toast arriving during the HUD's 220ms fade used to resize the window
mid-fade — a violent jump. Guard with `_hudBusy`: set true in `hideGuideHud`,
cleared after 240ms; **all** window fitting is skipped while
`_hudActive || _hudBusy`, then one clean fit runs.

---

## 5. Traps that cost real time here

1. `SetWindowRgn` must be called on the **window's own thread**
   (`run_on_main_thread`) or it silently does nothing — and GDI regions have
   no antialiasing, so they are a poor fit for rounded UI. The region path is
   kept but DISABLED behind `USE_WINDOW_REGION = false`; transparency is the
   better answer.
2. Diagnose visual artifacts by **sampling pixels** from a screenshot
   (`Bitmap.GetPixel`), not by eyeballing. That is what proved the overlay
   interior was truly transparent (R31,G31,B30 = the desktop behind) and the
   leftover "box" was a 1px R27 DWM border.
3. Never `innerHTML` an app name — arbitrary user data. Use `textContent`.
4. `prefers-reduced-motion`: render final states, skip entrances.

---

## 6. Status

- **Radial Guide HUD: confirmed correct by the user.** Centred, blooming,
  viewport-aware, live from the active profile.
- **Toast island pill:** implemented to the spec above. The stack rule,
  phases, ring and glow padding are in place; the glow-clipping and
  HUD→toast transition fixes were built but **not re-confirmed on screen** —
  verify before claiming them.
- Everything in this file refers to the overlay only.
