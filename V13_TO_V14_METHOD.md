# How to go from V13 → V14

**You are being handed: (a) a WORKING app, `SpaceToggle-V13`, and (b) design
files. Your job is to close the gap between them — nothing else.**

Two AI attempts at this have already failed. Neither failed at coding; both
failed at *method*. Read §1 before you write a line, or you will produce a
third failure that looks plausible and gets thrown away.

Written 2026-08-10 by Claude Fable 5, after being the second failure.

---

## 0. The inputs

| What | Where |
| --- | --- |
| The working app (functionality is CORRECT — do not redesign it) | `D:\Claude-Projects\SpaceToggle-V13` |
| The design | `D:\Claude-Projects\design-overhaul\Design system overhaul project\design-upgrade-using-claude\` |
| ↳ **the dashboard, FINAL** | `Dashboard Earthy v2.dc.html` |
| ↳ overlay motion, light / dark | `Motion Lab Earthy.dc.html` / `Motion Lab Nocturne.dc.html` |
| ↳ **vanilla port target** | `handoff/overlay-reference.html` |
| ↳ tokens + keyframes | `handoff/motion.css` |
| ↳ written spec + file mapping | `handoff/DESIGN_SPEC.md` |
| What the overlay must be, exactly | `OVERLAY_ACHIEVED.md` (next to this file) |
| **The actual working overlay CODE — copy it in** | `code/` (next to this file) |

### `code/` — this is the real deliverable for the overlay

| File | Where it goes | What it is |
| --- | --- | --- |
| `code/toast.ts` | `src/components/toast.ts` | The complete, verbatim working implementation of the radial Guide HUD **and** the island toasts. Drop-in replacement. |
| `code/overlay-earthy.css` | `src/styles/overlay-earthy.css` | Every overlay visual + keyframe, both palettes. |
| `code/RUST_AND_HTML_CHANGES.md` | — | The exact Rust/HTML/JSON edits, with before→after code, that the two files above depend on (transparent window, DWM border kill, HUD centring, capabilities). |

Copy those three in and the overlay works. That is the part of this project
that is DONE — do not redesign it, do not re-derive it.

**For the dashboard there is no code to hand you** — attempt #1's version was
correct but I destroyed it (see §1). You must transcribe
`Dashboard Earthy v2.dc.html` faithfully. That file contains every value you
need in its inline styles and `renderVals()`.

V13's own docs still apply in full: `CLAUDE.md`, `AI_HANDOFF.md`,
`NATIVE_SAFETY.md`, `PROJECT_STATUS.md`. The build traps, hook laws and
window rules there are real and were paid for in hours.

---

## 1. THE TWO FAILURES — read this first

### Attempt #1 (an earlier AI): **dashboard RIGHT, overlay not done**
Its dashboard was correct — the user confirmed this. What it missed was the
overlay: it **never touched it at all**, so holding Space still showed V13's
old rectangular panel instead of the radial HUD. Measured, the difference vs
`SpaceToggle-V13` was exactly three files — `index.html`, `src/main.ts`,
`src/styles.css`. `toast.ts`, `overlay.ts` and `guide_hud/` were untouched.

So the split is clean:
| Surface | Who got it right | Where the reference is now |
| --- | --- | --- |
| Dashboard | attempt #1 | **CODE LOST — rebuild from `Dashboard Earthy v2.dc.html`** |
| Toast + radial HUD | attempt #2 (me) | `OVERLAY_ACHIEVED.md`, next to this file |

**⚠️ I destroyed attempt #1's dashboard.** When I rebuilt V14 I deleted the
folder and re-cloned from V13, which overwrote those three files before
anyone knew they were the good ones. They are not recoverable. The design
file they were transcribed from is intact, and a faithful transcription of it
reproduces that result — which is exactly what attempt #1 evidently did.

**Lessons:**
1. The design covers FOUR surfaces — dashboard, toast, Guide HUD, shared
   theme. Diff against V13 when you finish; a surface whose files are
   unchanged was not done.
2. **Never delete a previous attempt before establishing which parts of it
   were good.** Diff it, screenshot it, ask the user which surfaces work —
   THEN decide what to keep. Re-cloning from the working base is only safe
   for the surfaces you have confirmed are wrong.

### Attempt #2 (me): **overlay RIGHT, dashboard wrong** — rebuilt it from imagination instead of porting it
I had `Dashboard Earthy v2.dc.html` — a complete, working implementation with
every colour, size, easing and behaviour in it — and I **wrote a new
dashboard from my reading of it** instead of transcribing it. The result was
rejected: wrong layout emphasis, wrong window sizing (the board ran off the
screen), pieces in the wrong places. I also claimed progress from a
screenshot that showed the window overflowing, which is not verification.

**Lessons, in order of importance:**
1. **The `.dc.html` file IS the implementation.** Open it, read its inline
   styles and its `renderVals()` — every value you need is literal in there.
   Transcribe it into vanilla TS/CSS. Do not paraphrase it, do not "improve"
   it, do not invent layout.
2. **Port surface by surface, and LOOK at each one before moving on.** One
   surface, build, screenshot, compare against the mockup, then the next.
3. **A screenshot you didn't actually examine is not verification.** Sample
   pixels if you're unsure (see `OVERLAY_ACHIEVED.md` §5.2).
4. **Never rewrite a whole working file to change its look.** V13's
   `keyboard-matrix.ts` etc. already carry assignment logic, drag/drop and
   context menus. `DESIGN_SPEC.md` §"File mapping" tells you which file gets
   which change — follow that table, keep the class names, swap the values.

### The meta-lesson
Between the two attempts, **every surface has already been built correctly
once** — just never in the same copy of the app. Nothing here is unsolved;
it is an integration job.

Where either attempt went wrong, the cause was identical: substituting its
own judgment for a provided artifact. The user supplied a complete design and
a working app. The task is *mechanical convergence*, not creative work. When
something is already specified, your job is fidelity — transcribe the
`.dc.html`, follow `OVERLAY_ACHIEVED.md`, and check each surface against its
reference before moving to the next.

---

## 2. Do it in this order

Each step ends with **build → install → look → compare to the mockup**.

1. **Fork V13, keep functionality untouched.**
   Copy `SpaceToggle-V13` → new folder. Change identity so it installs
   beside V13 and cannot clobber it or its config: `productName`,
   `identifier`, a NEW MSI `upgradeCode` GUID, npm name, and the data-dir /
   Run-key name (`SpaceToggleOS` → your new name, in `logger.rs` +
   `startup.rs`). Build it UNCHANGED first and confirm it runs. Do not skip
   this baseline.
2. **Tokens.** Replace the VALUES in `src/styles/design-system.css` with the
   `--st-*` set from `motion.css`, keeping every token NAME. Cream/terracotta/
   sage; warm brown-tinted shadows, never black; radii 13/16/999. This alone
   re-skins most components. Both palettes (Earthy + Nocturne) behind one
   setting — `body.nocturne`.
3. **Overlay: toast + radial HUD.** Port `handoff/overlay-reference.html`
   into `src/components/toast.ts`. **`OVERLAY_ACHIEVED.md` next to this file
   is the confirmed-correct spec — follow it exactly.** This surface was
   signed off by the user; it is the one thing that came out right.
4. **Dashboard.** Transcribe `Dashboard Earthy v2.dc.html` faithfully. It is
   a keyboard-only hero on one warm stage: profiles behind a top-right pill
   popover, settings behind a bottom-left gear popover, specials behind a
   bottom-centre pill, NO sidebar / header grid / status bar. Flat cream keys
   `rgba(253,246,233,.82)`, 1.5px `#d8c9ab`, radius 14, warm shadow; bound
   keys `#f6e2cf` / `#e0ac80` with brown text; SPACE reads SPACE with a
   terracotta border. Every control is a terracotta pill/toggle — no native
   blue sliders or checkboxes. Motion: cursor-follow glow (RAF lerp .09),
   breathing halo, drifting auras, key cascade (55ms/row + 16ms/key), press
   ripple, and the editor blooming out of the pressed key while the board
   blurs back. **Take the numbers from the file, not from this paragraph.**
5. **Fit the window to the screen.** The board is fixed-geometry (1046px
   design width, `U=56`, `G=10`); scale it to fit BOTH axes and size/centre
   the window against the monitor work area at startup. My build opened wider
   than the display and the keyboard ran off the edge — this is the failure
   the user saw.
6. **Re-verify the whole app** against §3.

---

## 3. Functional gaps still open in V13 (fix these too)

These are real bugs the user reported. Diagnoses are sound; **my fixes for
the first two were written but NEVER verified — treat them as leads.**

1. **Apps flash in the taskbar instead of coming to the front.** Root cause
   is Windows' **foreground lock**: only the process owning the most recent
   input may raise a window, everyone else gets a taskbar flash.
   `AttachThreadInput` alone is not enough. The ladder that should work, each
   step verified by re-reading `GetForegroundWindow()`:
   (a) AttachThreadInput to the current foreground thread + `BringWindowToTop`
   + `SetForegroundWindow`; (b) a synthetic **Alt tap** via `SendInput`
   tagged with the hook cookie `0x7A7A7A7A`, which makes your process the last
   input owner and releases the lock; (c) `SwitchToThisWindow`. Log which
   step was needed; never fail silently.
   **The user's acceptance test: Space+W must raise the app, and pressing it
   again must minimise it, every time, for any app or website.**
2. **Store apps launch but never minimise.** Packaged apps' windows belong to
   host processes (ApplicationFrameHost), so exe-name matching cannot find
   them. Match on **PKEY_AppUserModel_ID** via `SHGetPropertyStoreForWindow`.
   windows-rs 0.58 notes: `PROPVARIANT` is opaque — use
   `PropVariantToStringAlloc` from `Win32::System::Com::StructuredStorage`
   (NOT `PropertiesSystem`); features `Win32_UI_Shell_PropertiesSystem` +
   `Win32_System_Variant`.
3. **App icons in the picker** — already fixed and verified in V13 via
   `IShellItemImageFactory` (handles .exe, .lnk AND Store AUMIDs). Keep it;
   render `icon_base64` as an `<img>`, never a letter disc.
4. **Drag & drop** an .exe/.lnk or URL onto a key — was reported not working.
5. **Special functions must be discoverable**: label them on their own keys
   in the dashboard (Esc → "Boss Key" etc.), not only in a tray.

---

## 4. Explicitly deferred (the user agreed these are future work)

Per-key binding for EVERY key including numpad detection; PowerShell-script
bindings; conflict warnings when a key is already used; turning the preset
special functions on/off; bundling Caprasimo + Figtree (OFL — must be local,
no CDN; falls back to Outfit until then); VRAM / low-RAM tuning for 16GB
laptops.

---

## 5. Rules that are not negotiable

- Vanilla TS + Vite. **No React, no Tailwind, no CDN** — the mockups are
  React-flavoured; port them, don't import their stack.
- Animate `transform` / `opacity` only. Honour `prefers-reduced-motion`.
- A new Tauri window is deaf until listed in `capabilities/default.json`.
- Never `innerHTML` an app name — it's arbitrary user data.
- Read `NATIVE_SAFETY.md` before any Win32 call. This app once broke the
  user's touchpad by minimising shell windows.
- Append every problem + root cause to `PROJECT_STATUS.md`, dated, newest at
  top, and record the CONDITION a failure occurred under plus how to re-test
  it — a bare "X doesn't work" note already cost one session two redesigns.
- **Never report anything as fixed, working or verified unless you watched it
  work.** Label untested things untested. The user's standard, in his words:
  *a feature that exists but cannot actually be used is worthless.*
