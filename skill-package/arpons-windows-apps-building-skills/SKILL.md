---
name: arpons-windows-apps-building-skills
description: "Arpon's all-in-one skill for building native-feeling Windows desktop apps with Rust + Tauri v2 — merged from build-windows-apps, tauri-motion, win32-keyboard-hook, and systematic-debugging. Use for ANY work on a Windows/Tauri desktop app: UI, layout, design, animation, window chrome, Mica/acrylic, tray, overlays, DPI, performance budgets, choosing frontend or Rust libraries, low-level keyboard hooks (WH_KEYBOARD_LL, SendInput, dwExtraInfo, tap-vs-hold, stuck modifiers), click-through overlays, Core Audio muting, and debugging any bug before claiming it fixed. Invoke proactively whenever writing Tauri, Rust desktop, WebView2, or Win32 code, when a hotkey/remapper/SpaceFn app misbehaves, when an app feels slow, janky, or dated, when asked what library to use, and ALWAYS before reporting anything as fixed, working, or done."
license: Merged compilation. Sources include MIT-licensed material (obra/superpowers ports, Microsoft/PowerToys/AutoHotkey/kanata docs compilations). Sources cited inline in references.
---

# Arpon's Windows Apps Building Skills

One skill for the whole job: building a Rust + Tauri v2 desktop app on Windows
that works correctly at the OS level, looks and feels native, and gets debugged
honestly. Versions in the references were verified against npm/crates.io on
2026-08-10 — re-check with `npm view <pkg> version` / `cargo add` before quoting.

## The four disciplines (all always in force)

1. **Import before you implement.** If a maintained library solves it, use it.
   Hand-write only what is genuinely specific to this app (the keyboard hook
   logic, the domain model). Catalogs: `references/libraries-frontend.md`,
   `references/libraries-rust.md`.
2. **Pick numbers from scales, not by feel.** Durations, spacing, type, radii,
   RAM budgets — the references hold the values no package can give you.
3. **Respect the OS.** Win32 hooks, overlays, audio, elevation, and the shell
   have documented laws. Breaking them produces bugs that look like your code.
4. **Prove it before you say it.** Root cause before fixing; observed behaviour
   before "done". `references/debugging.md` is mandatory before any completion
   claim.

## Reference router — load only what the task needs

| Load | When |
|---|---|
| `references/win32-keyboard-hook.md` | Keyboard hooks, hotkeys, remapping, SpaceFn/hyper-modifier, SendInput, stuck modifiers, injected input, click-through overlays, muting audio, Raw Input |
| `references/debugging.md` | Any bug, test failure, "still doesn't work" — and before claiming fixed/verified/done |
| `references/windows-platform.md` | Window chrome, Mica/acrylic, DPI, multi-monitor, tray, overlays, Win32 interop, native conventions |
| `references/design-system.md` | Spacing, type scale, color, elevation, radius, layout, dark mode |
| `references/motion-physics.md` | Anything that moves: durations, springs, easing, choreography, reduced-motion |
| `references/animation-catalog.md` | Choosing animation libraries/components (Motion, GSAP, AutoAnimate, Rive, CSS-native, shadcn/HeroUI — READ ITS FRAMEWORK GATE FIRST) |
| `references/performance-budget.md` | RAM, GPU, startup time, binary size, profiling |
| `references/libraries-frontend.md` | Picking a JS/CSS dependency |
| `references/libraries-rust.md` | Picking a crate, Tauri plugins, Win32 crates |
| `references/vanilla-ts.md` | Project has no frontend framework (plain TS + Vite) |

## Decide the stack before writing code

Check `package.json` first. **Vanilla TS + Vite** (no React) is a legitimate,
lighter choice — GSAP, Motion's vanilla API, AutoAnimate, Floating UI, and all
CSS-native techniques work there. Skip anything named `react-*`. **Adding a
framework to a working vanilla app is a rewrite, not an upgrade** — only if the
user asks, with the cost stated plainly.

## Non-negotiables for any Windows desktop app

- **No CDN, ever.** No Google Fonts, no remote scripts or images. Bundle fonts
  (Fontsource), icons (Lucide/Iconify), assets in `src/assets/`. If the CSP
  mentions an external host, that is a bug.
- **Animate `transform` and `opacity` only** (light `filter` allowed). Never
  `width`/`height`/`top`/`left`/`box-shadow`/`border-radius` — layout/paint
  every frame.
- **Honor `prefers-reduced-motion`** and the system theme/accent color.
- **Never block the UI thread on Rust work** — Tokio task + event channel.
- **Show the window after first paint** (`"visible": false`, show from
  frontend) — no white flash.
- **Persist window position** (`tauri-plugin-window-state`).
- **Tray utility budgets:** idle RAM < 40 MB good / > 120 MB investigate;
  idle CPU ~0%. Details in `references/performance-budget.md`.

## Iron laws of the keyboard hook (full detail in win32-keyboard-hook.md)

- **Never filter on `LLKHF_INJECTED`** — it also drops RDP, macro keyboards,
  and accessibility input. Tag your own `SendInput` with a `dwExtraInfo`
  signature and suppress only that. Default-allow everything else.
- **The hook callback must return in microseconds.** No COM, no IPC, no Tauri
  events, no heap allocation, no locks shared with the UI thread, no panics
  unwinding out. Exceed ~1000 ms once and Windows silently evicts the hook —
  the "keyboard stops working after a while, restart fixes it" symptom.
  Dedicated thread + message pump; decisions out over a channel.
- **Tap-vs-hold resolves on release**, with permissive-hold / hold-on-other-key
  / chordal-hold heuristics; buffer and replay pending events in original
  order or fast typing scrambles ("hte" for "the").
- **Stuck modifiers**: idle reset, force-release on session/power events,
  reconcile against the OS, always clean up on exit — but note that
  `GetAsyncKeyState` reports a key you are SUPPRESSING as UP, so never build a
  failsafe on it for keys you hide from the OS.
- **Keys you cannot have:** Win+Space, Ctrl+Space (IME), Win+L, Ctrl+Alt+Del,
  anything while an elevated window has focus (unless you run elevated).
  Design around them; stop debugging them.
- **Mute via Core Audio** (`IAudioEndpointVolume::SetMute`), never by
  synthesizing `VK_VOLUME_MUTE`. Toggles are not states.
- **Click-through overlays** need `WS_EX_LAYERED | WS_EX_TRANSPARENT` together,
  plus `focusable: false`. Check the `Result` of
  `set_ignore_cursor_events` and fail closed. Confirm the window exists (and is
  listed in Tauri 2 `capabilities/*.json` — an unlisted window is deaf: every
  `listen()` rejects silently) before debugging how it looks.

## Debugging discipline (full detail in debugging.md)

Find the cause before changing anything; prove it worked before saying it
worked. Reproduce deliberately, distrust the test harness (run it against a
known-good target; check every syscall return; check binaries have non-zero
size), one hypothesis at a time, confirm the build succeeded before judging a
fix. Never report as done anything you have not observed working — "compiles
and installs; X untested on this machine" is a useful status, "verified
flawless" about an untested path poisons every report. Three failed fixes means
the layer is wrong, not the code. Simulating input is guessing — prefer real
APIs. Synthetic input cannot create physical key state, and Windows discards
synthetic input sent from an unelevated process to an elevated hook — test
harnesses for an elevated app must run elevated, or every result is void.

## Working method for UI work

1. Read the existing code and match it — no second styling system.
2. Check the catalogs before writing anything.
3. Pick numbers from the scales.
4. Prefer CSS-native (View Transitions, `@starting-style`, scroll-driven
   animations, popover, anchor positioning — all in WebView2/Chromium 151,
   compositor-threaded).
5. Verify versions before claiming them.
6. For anything visual/subjective, build a small standalone HTML preview and
   offer 2–3 directions before wiring it in.
7. Ask when a choice is genuinely open.

Avoid the AI-generated tells: indigo-purple gradients, bouncy overshoot easing
everywhere, three-card grids, purposeless glassmorphism, scroll-jacking.
Restraint reads as intentional.
