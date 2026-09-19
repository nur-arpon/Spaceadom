# Touchpad brief T2 — the feature + the page (1.0.120)

**T1/T1b RESULT (owner's runs, 2026-09-19 afternoon, `%APPDATA%\Spaceadom	ouchpad-probe.log`):**
the pad is a Goodix Precision Touchpad (119×74 mm, 100+ reports/s, 5
contacts). **Two-finger edge slides CANNOT be used**: the LL hook ate every
WM_MOUSEWHEEL (536/0) and Brave still panned — Chromium, Store apps and
Win11 Explorer scroll through Direct Manipulation, which no hook sees.
Three fingers fire Windows' own gestures. **ONE finger works**: arm on the
landing report inside a band and eat WM_MOUSEMOVE while live → cursor
moved 0 px over a 1.2 s hold with 53 moves eaten, page did not scroll.
So the feature is **ONE finger, starting inside the band**. `raw.rs` and
the example's `LandingTracker` (N=1 path) are the proven code — promote
that, not the two-finger `BandTracker`. Write the T1b outcome as its own
PROJECT_STATUS entry (the agent asked). This replaces every "two fingers"
in this brief and in the design's copy/demo: the demo animation shows ONE
finger landing at the edge and sliding (the "one finger just moves the
pointer" beat becomes "a finger that starts in the middle just moves the
pointer — start at the edge and it changes something"); the invitation
card title becomes "Start at the edge"; the hint "One finger, inside the
band. The pointer holds still while you slide."

The design is **`D:\Claude-Projects\design\spaceadom-touchpad_1.html`** —
six screens: (1) page, default/first-run with the two-finger demo; (2) page,
right edge selected mid-drag; (3) live test; (4) not a Precision Touchpad;
(5a/5b) corner ownership undecided/decided; (6) home thumbnail, two states.
**Transcribe it literally** (CLAUDE.md: the design is a specification —
copy values, never paraphrase), dropping only §10 "THIS SHEET ONLY" and
the sheet's theme switcher. Rules: no installer, no install, no commit,
never write `%APPDATA%\Spaceadom\config.json`, never delete/rename files
you did not create, no elevation, no injection except the feature's own
scrub taps through `send_keys_checked` (PROBLEM 227 cookie).

## Owner decisions (2026-09-19)

- Edges: left = brightness, right = volume, top = video **scrub** —
  continuous, rate proportional to travel (a Touch Bar scrubber, never
  fixed 5 s hops), bottom = reserved (shown, disabled, "Later").
- ONE finger that STARTS inside the band. A finger that lands outside and
  drifts in never arms (normal pointing is never hijacked). A tap in the
  band with no slide does nothing. While live the pointer is frozen (the
  LL mouse hook eats WM_MOUSEMOVE and wheel) and released on lift.
- Everything OFF by default. The first-run screen (1) invites the top edge.
- Corner overlap: no default — screen 5a asks when the second overlapping
  band is switched on; 5b shows the answer; "Do the same in every corner"
  checkbox; "Shorten the band instead" escape. Only fingers that START in
  the square are affected.
- **Look:** the home THUMBNAIL always matches the app theme. The PAGE has
  a Settings row "Touchpad page" segmented **Matches the app / Chocolate**,
  default **Chocolate** (the design's dark tokens with the theme's accent:
  Earthy coral, Navy blue, Warcry red). "Matches the app" maps every
  `--sp-*` surface/line/text token onto the app's existing theme variables
  and keeps the drawn pad `--sp-inset` chocolate in every theme (the pad is
  a physical object). Same markup, two token sheets, one attribute
  `data-look="chocolate|app"` on the page root.
- Reversible rule (CLAUDE.md): every band action is undoable by the
  opposite slide; lifting the finger ends the gesture; nothing persists.

## Config (`config/schema.rs`, all `#[serde(default)]`)

```
touchpad: Touchpad { left: Band, right: Band, top: Band, bottom: Band(reserved, always off),
                     corner_rule: CornerRule::Ask | AlwaysHorizontal | AlwaysVertical,
                     corners: { tl, tr, bl, br: Option<Edge> },   // per-corner owner when set by hand
                     page_look: Look::Chocolate | App,
                     demo_seen: bool }
Band { enabled: bool, action: Brightness | Volume | Scrub | None,
       width: f32 (0.04..=0.25, fraction of the pad's short side, default 0.07),
       length: f32 (0.30..=1.0, fraction of the edge, centred, default 0.70 side / 0.80 top),
       sensitivity: u8 (1..=10, default 6), invert: bool }
```
The page shows width as px of the drawn 780×520 pad and length as %, as
the design does.

## Engine (`src-tauri/src/touchpad/`)

- `raw.rs` — T1's reader, promoted: runs on its own thread from boot when
  any band is enabled (and stops when none is), publishes contacts.
- `gesture.rs` — PURE state machine, unit-tested: contacts → `Idle |
  Armed(edge) | Live(edge, travel)`; enter Live only when exactly ONE
  contact is down and it LANDED inside one band (corner squares resolved
  by `corners`/`corner_rule`; `Ask` with an unresolved corner = no band);
  arm on the landing report; exit on lift or on a second contact;
  travel = signed displacement along the band's axis since entry, in
  fractions of the pad; hysteresis so a finger drifting 2 px out of the
  band does not end the gesture.
- `bands.rs` — published atomics for the LL mouse hook: `BAND_LIVE`
  (AtomicBool) and nothing else; the hook eats `WM_MOUSEMOVE`,
  `WM_MOUSEWHEEL` and `WM_MOUSEHWHEEL` while it is set (laws: atomics only
  in the callback). The app's existing WH_MOUSE_LL callback gets this one
  extra atomic test at its top; no second mouse hook.
- Actions, in-process and instant (NOT PowerShell — a slide ticks at
  30–60 Hz):
  - **Volume** — Core Audio `IAudioEndpointVolume` on the default render
    endpoint: read master scalar, set `clamp(cur + k·Δtravel)`, read back for
    the readout. Sensitivity scales k.
  - **Brightness** — WMI in-process via COM (`IWbemServices`,
    `WmiMonitorBrightnessMethods.WmiSetBrightness`) — the same class
    `actions/brightness.rs` drives through PowerShell; replace that
    module's PowerShell with this COM path too so both share one function
    (keep its tests). Rate-limit sets to 20 Hz; readout from
    `WmiMonitorBrightness.CurrentBrightness`.
  - **Scrub** — ←/→ taps through `send_keys_checked`, rate = f(travel,
    sensitivity): dead zone 3 % of the pad, then 2…25 taps/s linearly to
    full travel; direction from sign; `invert` flips. Works in YouTube,
    VLC, Netflix, Media Player with no per-app code.
- Events to the page (global `emit`, the only arrangement that works
  here): `touchpad-caps { available, pad_mm: [w,h] }` at boot and on
  device change; `touchpad-live { edge, action, value_pct, travel }` at
  ≤ 30 Hz while Live, then one `{ edge: null }` on end.

## Frontend

- `src/components/touchpad-page.ts` + `src/styles/touchpad.css`: the
  page, screens 1–5 transcribed; the one-finger demo runs 3 cycles on
  first run only (`demo_seen`) and on "Show me again"; drag handles
  (width from the inner edge, length from either end, 24 px hit areas,
  ghost line, live chip); the side panel per selected band (switch, Does
  what, The band steppers, sensitivity, flip); the live readout card and
  "The pointer is holding still" pill (was "Normal scrolling is paused") while `touchpad-live` fires; the
  unavailable state (4) when `touchpad-caps.available` is false with the
  settings kept editable-but-inert exactly as drawn.
- Entry: screen 6's thumbnail sits directly beneath the keyboard on the
  home dashboard (`index.html` / `main.ts`), app-themed, two states; click
  → the keyboard leaves and the page takes the stage; `Esc` or the
  "Keyboard" pill returns. Same motion as the key editor's bloom, ≤ 250 ms,
  spring.
- Settings: "Touchpad page — Matches the app / Chocolate" row (Appearance,
  after Hide the keyboard), with a per-option description via
  `setting-subs.ts`.
- `preview.ts` fixture: the page in both looks and all three states.

## Tests

`gesture.rs` (entry/exit/hysteresis/corner resolution/travel sign), rate
mapping for scrub, serde defaults + ranges, the corner-ownership resolver,
a test that the two token sheets define the same token NAMES (read both
CSS blocks, compare the `--sp-` set), `setting-subs` completeness.

## Docs + version

PROJECT_STATUS.md dated entry; V14_FIXES_AND_CODE.md §TOUCHPAD (T1 + T2,
with the HID usages and the scroll-eat mechanism written so another AI can
rebuild it); WHAT-CHANGED.md row in the owner's voice; README shortcut
table gets a "Touchpad edges" paragraph. Version → **1.0.120**.

## Gates

`cargo test --release --lib`, `cargo clippy --release --lib` 0 warnings,
`npx tsc --noEmit -p .`, `npm run build`, `node scripts/setting-subs.test.ts`.
Report: files, gates, what is UNPROVEN on hardware (everything that moves
a finger), any QUESTION one line each.
