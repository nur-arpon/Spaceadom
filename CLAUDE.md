# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# Spaceadom (v1.0.0 — formerly SpaceToggle V14)

Turns the Spacebar into a system-wide modifier on Windows: hold Space + tap a
key to launch/focus/minimize an app; tap Space alone and it must always type a
normal space. Rust + Tauri v2 backend, vanilla TypeScript + Vite frontend
(NO React, NO Tailwind — never add them). windows crate 0.58.

**Identity (since the 1.0.0 release pass, PROBLEM 45):** productName
`Spaceadom`, identifier `com.spaceadom.app`, exe `spaceadom.exe`, data dir
`%APPDATA%\Spaceadom`, install dir `%LOCALAPPDATA%\Spaceadom` (per-user).
**Autostart is the HKCU Run value `Spaceadom`, NOT a Scheduled Task** — that
reverses what this file said before 1.0.41. The task approach failed for a
concrete reason worth keeping: a task created while elevated cannot be deleted
by the non-elevated app (PROBLEM 61 removed elevation), so a stale task became
permanent and launched an OLD build alongside the new one (PROBLEM 129). The
app deletes legacy tasks and legacy Run names when it can.
`V14_FIXES_AND_CODE.md` §PROBLEM 45 has the full table and the elevation
flow. The repo folder is still `SpaceToggle-V14`; rename at git-init.

**Spaceadom = V13's engine + the Earthy design.** V13 (`..\SpaceToggle-V13`)
is the functional baseline and stays untouched; Spaceadom installs beside it
with its own identity so the two can never clobber each other's config.

## The design is a specification, not a suggestion

Source files live in
`..\design\Design system overhaul project\design-upgrade-using-claude\`:

| File | What it governs |
| --- | --- |
| `Dashboard Earthy v2.dc.html` | The dashboard. **Every value is literal in its inline styles and `renderVals()` — transcribe, never paraphrase.** |
| `Motion Lab Earthy/Nocturne.dc.html` | Overlay motion, both palettes |
| `handoff/overlay-reference.html` | Vanilla port target for the overlay |
| `handoff/motion.css` | Tokens + keyframes |
| `handoff/DESIGN_SPEC.md` | Written spec + file mapping |

In this repo: `V13_TO_V14_METHOD.md` (how two earlier attempts failed and
why), `OVERLAY_ACHIEVED.md` (the confirmed-correct overlay — do not
re-derive it), `OVERLAY_RUST_HTML_CHANGES.md` (the exact non-TS edits).

Design rules that are not negotiable: radii 13 keys/cards, 16 containers, 999
everything interactive; shadows warm brown `rgba(90,60,30,…)` in light and
black-tinted in Nocturne, never pure black on cream; exits run at ~65% of
entrance time with `--ease-in`; nothing may assume a fixed width around an app
name; `prefers-reduced-motion` renders final states.

**Theme rule: ONE setting drives everything.** `body.nocturne` on the
dashboard AND the overlay. The overlay learns about it two ways and needs
both — `save_config` re-emits `theme-changed`/`sound-changed` from Rust
(global `emit`; `emit_to` has never worked here), and `overlay.ts` seeds
itself from `get_config` on load, because an event that only fires on CHANGE
leaves a freshly-opened overlay in the wrong palette.

## Dashboard architecture (V14)

One stage, keyboard as the hero. No sidebar, header grid or status bar.

```
index.html        #stage → auras, halo, cursor glow, topbar, keyboard, docks
src/main.ts       bootstrap + stage motion (cursor-glow RAF lerp .09, press
                  ripples, board fit) + popover plumbing
src/styles.css    the stage; imports styles/design-system.css
  keyboard-matrix.ts   16-unit board, U=56/G=10, DESIGN_W=1048 (NOT 1046 —
                       fractional keys round up 2px/row; measured)
  key-detail-panel.ts  editor that blooms out of the pressed key
  profile-editor.ts    top-right pill popover
  settings-panel.ts    bottom-left gear popover
  toast.ts             overlay only — toasts + radial HUD (drop-in, verbatim)
```

The board is FIXED geometry scaled to fit. Two guards, both required: Rust
clamps the window to 92% of the monitor (`lib.rs` step 9c) and the frontend
scales the board on BOTH axes (`wireKeyboardFit`). Attempt #2 scaled on width
alone and the keyboard ran off the display — that is the failure the user saw.

`preview.html` + `src/preview.ts` render the real components with a stub
config so the dashboard can be inspected without the backend. Dev-only; not a
Vite build input, so it never ships.

A control that does nothing is worse than a missing control. The mockup's
"Run at startup" toggle is deliberately absent: there is no backend command
for it. Add the command first, then the toggle.

## Required reading, in this order, before changing anything

0. **`V14_FIXES_AND_CODE.md` — every V14 problem with its root cause, the
   exact file, the exact code that fixed it, and how it was verified.** Read
   this before re-diagnosing anything; it exists so you do not have to search.
1. `AI_HANDOFF.md` — self-contained orientation: history, hard rules, build
   environment, testing traps.
2. `CORE_AIM.md` — the non-negotiable feature contract. Never remove or
   simplify a listed feature; fix it natively.
3. `NATIVE_SAFETY.md` — do-not-touch table for Win32 calls. This app once
   broke the user's touchpad by minimizing explorer.exe shell windows.
4. `PROJECT_STATUS.md` — append-only dev log, newest at top. Append your own
   dated, named entry for every problem you solve. NEVER delete entries.
5. `FEATURES_NOW_POSSIBLE.md` — previously stripped features that now have
   stable implementation paths.

`install-v11.ps1` (AutoHotkey) is the functional gold standard — when
behaviour is ambiguous, match v11.

## Skill

The skill `arpons-windows-apps-building-skills` governs all work here
(debugging discipline, keyboard-hook iron laws, design/motion scales). It
ships in `skill-package/`; if `.claude/skills/arpons-windows-apps-building-skills/`
is missing, copy it there. NOTE: the destination folder must exist first or
PowerShell flattens the copy:

```powershell
New-Item -ItemType Directory -Force .claude\skills | Out-Null
Copy-Item -Recurse -Force "skill-package\arpons-windows-apps-building-skills" ".claude\skills\"
```

## Build

Rust lives ONLY in `D:\RUST-DOWNLOADED-HERE` (cargo 1.97.1). Never let it
reinstall to `C:\Users\beamu\.cargo`.

```powershell
$env:CARGO_HOME="D:\RUST-DOWNLOADED-HERE\cargo"
$env:RUSTUP_HOME="D:\RUST-DOWNLOADED-HERE\rustup"
$env:PATH="D:\RUST-DOWNLOADED-HERE\cargo\bin;$env:PATH"
npm run build        # tsc + vite — MUST run before ANY cargo command
npm run tauri build  # setup.exe → src-tauri\target\release\bundle\nsis\
```

**One installer, per-user, since 1.0.41 (PROBLEM 129).** `bundle.targets` is
`["nsis"]` and `nsis.installMode` is `"currentUser"`. Do NOT re-add the `msi`
target: Tauri's WiX bundler is per-machine only, so shipping both put TWO
copies of the app on the owner's machine — Program Files and `%LOCALAPPDATA%`,
each with its own autostart, both grabbing the spacebar at logon. The MSI also
has no `installerHooks` equivalent, so it silently defers updates over the
running app (PROBLEM 127). Restoring it requires a custom WiX template with
`InstallScope="perUser"` AND a `util:CloseApplication` action, in that order.

- **`windows` crate features are per-API.** A missing feature reads as
  `unresolved import` on a path that plainly exists in the docs. Store/UWP
  window matching needs `Win32_UI_Shell_PropertiesSystem` +
  `Win32_System_Variant` — inherited code that names features in a markdown
  file has not necessarily had them added to `Cargo.toml` (PROBLEM 30).

- `npm run build` first, always: `tauri::generate_context!` reads **`../dist2`**
  (named in `tauri.conf.json` as `frontendDist`, produced by `package.json`'s
  `--outDir dist2`). `dist/` and `dist-stale/` are leftovers on disk and are
  NOT what ships. If it's missing you get a bare `error: proc macro panicked`
  naming no cause.
- **0-byte shim trap** (this machine does it reproducibly): if cargo/rustc
  fail with "The system cannot find the file specified", the shims in
  `cargo\bin` are 0 bytes. `Test-Path` says True; check `Length`. Repair:
  delete each empty shim and copy `rustup.exe` over it (`rustup default
  stable` does NOT fix it). Full script in AI_HANDOFF.md §4.
- Keep the build at 0 errors, 0 warnings.
- **Installing: `setup.exe /S`, and NEVER trust its exit code.** Four separate
  installs exited 0 having replaced nothing (PROBLEM 127 — the app is running
  during its own upgrade, and the MSI deferred the file swap to a reboot). The
  NSIS `installerHooks` PREINSTALL macro now kills the app first, but the rule
  stands for any installer: **verify the installed exe by version stamp AND by
  an ASCII content marker** before believing a fix shipped. A verdict taken on
  an unverified build is worse than no verdict — it discredits a working fix.
- **A FIX THAT IS NOT INSTALLED DOES NOT EXIST.** The user boots from
  `%LOCALAPPDATA%\Spaceadom\`, never from `target\release\`. Testing
  from the repo build is fine, but the session is NOT finished until the
  setup.exe has been run and the installed exe verified. Per-user means no UAC
  prompt at all now; if one ever appears and is declined,
  say so loudly and treat the work as UNDELIVERED — quietly continuing to
  test from the repo is how five hours of fixes failed to reach the user's
  startup (PROBLEM 42), leaving them booting a build that still had the
  overlay-killing glow.
- You can prove which fixes a binary contains WITHOUT running it: `log::info!`
  format strings and bundled CSS names are ASCII-searchable inside the exe.
  `[Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($exe))` then test
  for markers like `url_focus:`, `aumid_focus:`, `st-hud-glow`.
- **Debug symbols SHIP (PROBLEM 131). Do not remove any of this plumbing:**
  `src-tauri/symbols/spaceadom.pdb` → installed beside the exe via
  `bundle.resources`. `build.rs` writes an invalid STUB there if missing (it
  must run on every cargo invocation — Tauri's before-hooks only run for
  `tauri build`, and without the stub `cargo test` fails with `resource path
  symbols\spaceadom.pdb doesn't exist`). `beforeBundleCommand` then copies the
  freshly-linked pdb over it. **Never stage a previous build's pdb to satisfy
  the check**: mismatched symbols do not fail, they resolve to confidently
  wrong lines. Cost measured: installer 4.6 → 5.6 MB.
- **There is exactly ONE `std::panic::set_hook` call, in `lib.rs`.** There were
  two, and the second silently replaced the first for months (PROBLEM 131) —
  `set_hook` replaces, it does not chain unless you make it. If you add
  another, the last one installed wins and the loser leaves no trace.
- **There ARE automated tests now** — 13, run with `cargo test --lib` from
  `src-tauri`. They cover the self-updating-app path repair (PROBLEM 116) and
  the opacity floor arithmetic (PROBLEM 119). Everything else is still
  verified by hand on the real machine (see Testing laws). Add a test when
  the logic is pure and the branch is one a user reaches only after something
  has already gone wrong — PROBLEM 118 is what shipping an unexercised
  recovery branch costs.

## Architecture

Two processes' worth of logic in one app; the hook thread and the UI must
never touch each other directly.

```
src-tauri/src/
  hook/mod.rs      WH_KEYBOARD_LL + WH_MOUSE_LL on a dedicated thread with its
                   own message pump. Decides typing vs command in microseconds.
                   Sends HookEvent over a crossbeam channel. Never touches
                   Tauri, COM, or the heap in the callback.
  engine/mod.rs    Async actor receiving HookEvents; dispatches to actions.
  engine/actions/  smart_cascade (launch/focus/minimize cycling), boss_key,
                   pip, opacity, focus_engine.
  guide_hud/       Emits Tauri events to the OVERLAY window, not the dashboard.
  config/          %APPDATA%\Spaceadom\config.json — profiles → bindings.
                   set_active_profile SAVES; don't also persistConfig() from
                   the frontend (that was the double-save bug).
  lib.rs           Startup: config load, window creation, tray. NO elevation
                   (PROBLEM 61 removed it).
  display_watch.rs Rebuilds the overlay when the display setup changes
                   (PROBLEM 117/118) and re-homes an off-screen dashboard.
src/               Dashboard UI (vanilla TS): keyboard-matrix, key-detail-panel,
                   profile-editor, settings-panel; main.ts wires them.
                   (V14 removed hook-status-bar.ts and app-picker.ts — the
                   status bar is gone from the design and the picker became
                   the editor's inline app grid.)
src/overlay.ts +
overlay.html       The on-demand HUD/toast surface (see window rules below).
```

### Window rules (hard-won; violating them re-opens fixed bugs)

- Two windows: `settings` (dashboard, closes to tray) and `overlay`
  (transparent, on-demand, click-through, NoActivate, always-on-top).
  The overlay is **centred** while the radial HUD owns it (`overlay_fit_hud`,
  `place_overlay_centred`) and **bottom-centred** for toasts (`overlay_fit`).
  Do not unify those two placements.
- A window MUST be listed in `src-tauri/capabilities/default.json` or every
  `listen()` in it rejects silently — the window is deaf.
- Transparency is CONDITIONAL, not banned: a **fullscreen** transparent
  webview composes zero pixels on this machine, but the **small on-demand**
  overlay is `transparent: true` and works (verified 2026-08-10). Keep it
  small and on-demand. `backdrop-filter` is still banned (white boxes).
- **`filter: blur()` on the overlay has a size limit.** A 560x320 element at
  `blur(34px)` made the ENTIRE overlay window compose zero pixels — HUD and
  toasts both gone — while Rust reported it correctly sized, centred and
  `visible: true` and the JS ran to completion with no error (PROBLEM 37).
  The toast glow (340x150, `blur(22px)`) is under the threshold and works.
  Do not exceed it. Bake softness into gradient stops instead.
- The overlay cannot be validated in a browser harness: its failure mode lives
  in the OS compositor, not the page. Hold Space and LOOK before shipping any
  visual change to it.
- `overlay_fit` / `overlay_fit_hud` log the requested size, the monitor, and
  the resulting size/position/visibility. **Never remove that logging** —
  without it, a wrong size, a wrong position and a window that never moved are
  indistinguishable, which cost a full diagnostic round trip.
- **That rule covers `hide()` and `show()` too (PROBLEM 135).** `hide_guide_hud`
  called `win.hide()` silently, and the engine cancels the HUD BEFORE
  dispatching an action — so on every shortcut the overlay window was hidden
  ~500-1000ms before the toast existed. Three builds of animation work played
  out inside an invisible window while in-page instrumentation (geometry,
  decision, JS errors) reported perfect health, **because a page cannot observe
  that its own window is hidden.** Any call that changes what the user can see
  must say so in the log. If a visual is missing and everything measurable
  inside the page is fine, suspect the WINDOW before the page.
- **A hide that races a pending action must be told about it.**
  `hide_guide_hud_pending(action_pending)` keeps the window up when a combo has
  fired and its toast is still coming; `overlay_toasts_done` remains the single
  terminal path that actually hides it.
- Undecorated Win11 windows draw a 1px DWM border that reads as a "box"
  around overlay content — clear it with `DWMWA_BORDER_COLOR = 0xFFFFFFFE`
  plus `DWMWCP_DONOTROUND` (done in lib.rs overlay setup).
- Diagnosing a visual artifact: SAMPLE THE PIXELS
  (`Bitmap.GetPixel` over a screenshot) instead of eyeballing a zoom. That
  is what proved the overlay interior was genuinely transparent (R31,G31,B30
  = the desktop behind it) and that the leftover "box" was a 1px R27 border
  line — two rounds of guesswork replaced by one measurement.
- Show the window BEFORE emitting content; hidden webviews don't paint.
- Global `emit` + single listener (overlay page) is the only event
  arrangement that delivered; `emit_to` never worked here.
- If `set_ignore_cursor_events` fails the code hides the overlay — fail
  closed; never simplify that away.
- CSP: `style-src` needs `'unsafe-inline'` or every `style="..."` dies
  silently. No external hosts ever (fonts are bundled in `src/assets/fonts/`).

### Keyboard-hook laws (details in the skill)

- Filter injected input ONLY by our `dwExtraInfo` cookie `0x7A7A7A7A`, never
  by `LLKHF_INJECTED`.
- Need two keys in guaranteed order → one `SendInput` batch; `SendInput` then
  `CallNextHookEx` does not preserve order (the `hte`-for-`the` bug).
- `GetAsyncKeyState` reports a key we SUPPRESS as UP — never build a failsafe
  on it for hidden keys (stuck-modifier detection uses a timestamp latch).
- Pass Space through when Ctrl/Alt/Win is physically held (IME, autocomplete,
  window menu). Shift deliberately excluded.

## Testing laws

- **The app does NOT elevate** (PROBLEM 61). Consequences: a non-elevated
  hook receives NOTHING while an elevated window has focus (Task Manager,
  regedit, an admin terminal) — that is Windows UIPI, it affects every
  remapper, and the watchdog logs it correctly rather than treating it as a
  fault. An input-injection harness still fails from a containerised agent
  shell: `SendInput` returns success and the hook sees nothing. ALWAYS assert
  a positive control and declare the run VOID when it fails — without one,
  2026-08-16's harness would have reported a false negative.
- Kill the v11 AutoHotkey process (`SpaceToggleRuntime.exe`) before testing
  V13 — two spacebar hooks feedback-loop. Leave its Startup shortcut alone.
- Validate any harness against a known-good baseline (app stopped) first.
  Check `SendInput`'s return value; the x64 C# `INPUT` struct is 40 bytes.
- `SetForegroundWindow` is blocked for the agent — ask the user to click the
  target window.
- Logs: `%APPDATA%\Spaceadom\debug.log` — fastest way to see engine
  decisions. Config: `config.json` next to it. NOTE: an agent shell may read
  a STALE containerised copy of that folder while File Explorer shows the
  live one — cross-check timestamps before trusting a read (2026-08-16).
- Never report anything as fixed/working/verified unless you observed it
  working. Label untested things untested. Ask the user to hand-test what
  injection can't reach.

## Hard rules

- Do NOT modify `D:\SpaceToggle-July_Revisit_2026`, `D:\GITHUB PROJECT`, or
  `D:\Neon` — read-only reference.
- Never remove a CORE_AIM feature to make something build.
- **Every solved problem gets TWO entries, and neither is optional.** The
  user's reason, in his words: *without documentation an AI has to start from
  scratch, and that costs a huge number of tokens.* Documentation here is a
  deliverable, not a courtesy.
  1. `PROJECT_STATUS.md` — dated, signed, newest at top, append-only. Never
     delete an entry. Say what happened and under what CONDITION it failed.
  2. `V14_FIXES_AND_CODE.md` — the technical record, in this exact shape:
     **Symptom → Root cause → Exact file → The actual code (paste it, before
     and after where it helps) → How it was verified.** Plus a
     "generalise this" line when the bug has a class.
     The test for a good entry: **could another AI apply the fix from this
     file alone, without opening the codebase to search for it?** If not, it
     is not finished.
- Record the *class* of a bug, not just the instance — "an ID rule that sets
  `display` needs its own `#id[hidden]` companion" is reusable; "the profile
  popover was open" is not.
- When a fix turns out to be unnecessary or a diagnosis was wrong, write that
  down too. `V14_FIXES_AND_CODE.md` §"measurement traps" exists because two
  false bug reports were nearly filed; that is worth more than a clean story.
- No CDN anything; the app must be fully offline.
- The user's laptop panel is **2560×1600 at 150%** (1707×1067 logical), and he
  plugs a SECOND display in and out through the day — display changes are
  routine here, not an edge case (PROBLEM 117/118). Guide HUD stays
  primary-monitor-only by explicit user decision — don't "fix" without asking.
