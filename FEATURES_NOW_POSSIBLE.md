# FEATURES NOW POSSIBLE — stripped or failed before, stable solutions exist now

**Written:** 2026-08-10 by Claude (Cowork session), after auditing the imported
skills against this codebase and its history. For any AI or human picking this
up: these are features the user wanted, tried, and had to strip or water down —
each now has a documented, stable path. The detailed how-to for every item
lives in the merged skill `arpons-windows-apps-building-skills` (references
named below). Read `AI_HANDOFF.md` and `NATIVE_SAFETY.md` first, as always.

## 1. The glassmorphism Guide HUD (CORE_AIM asks for it; currently opaque)
CSS/webview transparency provably composes zero pixels on this machine
(PROJECT_STATUS #14, and the July "white box" saga). The stable path is to keep
the current v11-style opaque, on-demand, click-through overlay window and add
**native DWM acrylic/blur at the window level** with the `window-vibrancy`
crate (or Tauri `windowEffects`) — the compositor does the glass, the webview
stays opaque, nothing depends on webview transparency. Apply the effect to the
small on-demand overlay only (never fullscreen). Skill ref:
`windows-platform.md`. Test on this machine before trusting it; if it fails,
the opaque window stays — degraded but safe.

## 2. UAC prompt on every launch (open item in PROJECT_STATUS)
Two documented, stable alternatives to self-relaunch-elevated:
a) ~~**Scheduled Task**~~ SUPERSEDED - autostart is the HKCU Run value now, not
a task. A task created while ELEVATED cannot be deleted by the non-elevated app
(PROBLEM 61 removed elevation), so a stale one became permanent and launched an
OLD build alongside the new one (PROBLEM 129). Original text kept below for the
reasoning, which is still worth reading:

a) **Scheduled Task** created once (with one UAC consent) with "Run with
highest privileges" at logon — app then starts elevated silently every boot.
b) **uiAccess manifest** — requires an Authenticode-signed binary installed
under %ProgramFiles%; heavier, only worth it for shipping. Skill ref:
`win32-keyboard-hook.md` §6.

## 3. Fast-typing scrambling / rollover ("hte" for "the")
A static `rollover_ms` can never fit all fingers (proven by the lydell/dual
project). The keyboard-firmware world converged on heuristics that DO work:
**permissive hold**, **hold-on-other-key-press**, and **chordal hold**, plus
**buffer-and-replay in original order** while a tap/hold decision is pending.
Reference implementations: kanata (Rust — directly readable), keyd, kmonad.
This also unlocks FUTURE_IDEAS #1 (typing calibration) properly. Skill ref:
`win32-keyboard-hook.md` §4.

## 4. Stuck modifiers after lock / sleep / RDP
Beyond the current 30-second latch: force-release all synthesized modifiers on
`WM_WTSSESSION_CHANGE`, `WM_POWERBROADCAST` (resume), and `WM_ACTIVATEAPP`;
idle-reset tracked state after ~60 s of inactivity; periodically reconcile
against `GetAsyncKeyState` — but ONLY for keys not being suppressed (a
suppressed Space always reads UP; that trap already bit this project,
PROJECT_STATUS #12). Skill ref: `win32-keyboard-hook.md` §5.

## 5. Compatibility with macro keyboards, Remote Desktop, on-screen keyboards
If the hook filters `LLKHF_INJECTED`, those users see a dead app. Correct
design: tag our own `SendInput` with a `dwExtraInfo` signature and suppress
only that; use **Raw Input** (`RIDEV_INPUTSINK`) when actual device identity is
needed. Verify the current hook does this. Skill ref:
`win32-keyboard-hook.md` §1, §8.

## 6. Boss Key audio that can never get out of sync
Muting by synthesizing `VK_VOLUME_MUTE` is a toggle race. Core Audio
(`IAudioEndpointVolume::SetMute` with a non-null `guidEventContext`, dedicated
COM thread, re-acquire endpoint on device change) sets the exact state wanted
and can read it back. The Cargo.toml already carries the audio-endpoint
features — verify the implementation matches. Skill ref:
`win32-keyboard-hook.md` §7.

## 7. A panic-proof kill switch
A rescue shortcut registered with `RegisterHotKey` (independent of our own
hook) plus an overlay watchdog means even a wedged hook or a stuck overlay can
always be escaped without Task Manager. Skill ref: `win32-keyboard-hook.md` §9.

## 8. App-State Awareness (FUTURE_IDEAS #2)
The fullscreen-watcher thread already polls the foreground window; the same
mechanism can key profile overrides off the active process name. No new OS
capability needed — this is now an engineering task, not a research one.

## 9. UI polish without React and without a rewrite
The dashboard is vanilla TS — that is a strength (lighter, faster). Stable
upgrades that work framework-free: `open-props` + `@radix-ui/colors` design
tokens, `@fluentui/web-components` for native-looking controls,
`@formkit/auto-animate` for list animation, GSAP/Motion vanilla APIs, and
CSS-native View Transitions / `@starting-style` (WebView2 is Chromium 151).
Never add React to get any of this. Skill refs: `vanilla-ts.md`,
`libraries-frontend.md`, `animation-catalog.md`.

## Already fixed in this session (2026-08-10, Cowork)
- Google Fonts CDN removed: `Outfit` variable font now bundled at
  `src/assets/fonts/outfit-latin-wght-normal.woff2`, `@font-face` in
  `design-system.css`, CSP tightened to `'self'` for styles/fonts. The
  dashboard now renders identically offline. (Rebuild + eyes-on check still
  needed — fonts untested on the real machine.)
