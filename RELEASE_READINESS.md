# RELEASE READINESS — SpaceToggle OS V13

**Assessed:** 2026-08-10 by Claude Fable 5 (via Claude Code), against the
build installed and verified that day.
**Verdict: WORKS ON THIS MACHINE. NOT READY FOR MASS PUBLIC RELEASE.**

Nothing here is a design flaw — the app does what CORE_AIM promises, and the
core loop is verified working on real hardware. What is missing is the layer
between "works for its author" and "works for a stranger on their laptop".

Each item says WHY it blocks, WHERE the evidence is, and WHAT fixing it takes.
Tick them off in order; P0 items are release-blocking.

---

## Will it run on other Windows laptops?

**Technically yes**, with these requirements:
- Windows 10 1809+ / Windows 11, **x64 only** (no ARM64 target is built —
  `bundle.targets` is `["msi"]`, built x64; Surface Pro X-class ARM devices
  will not run it natively).
- WebView2 runtime — preinstalled on Win11; on Win10 the installer uses
  `downloadBootstrapper`, so **first install needs an internet connection**.
- An **administrator account** — see P0-2. On a standard user account the app
  currently cannot start at all.

**Untested on any machine but the author's** (single 2560×1440 display,
Win11, 100%-ish scaling): laptop resolutions, 125%/150% DPI, multi-monitor,
Windows 10, non-US keyboard layouts.

---

## P0 — release blockers

### P0-1. The installer is unsigned → SmartScreen will block it
`src-tauri/tauri.conf.json` → `"certificateThumbprint": null`.
Every downloader sees *"Windows protected your PC"*. For an app that installs
a **global keyboard hook**, a scary unsigned warning is fatal to trust — a
large share of users will assume a keylogger and delete it.
**Fix:** buy an OV or EV code-signing certificate and set the thumbprint +
`timestampUrl`. EV gets instant SmartScreen reputation; OV builds reputation
over time/downloads. This is a purchase, not a code change.

### P0-2. Forced UAC elevation on EVERY launch
`src-tauri/src/lib.rs:39` calls `startup::maybe_relaunch_elevated()`
unconditionally, before anything else.
Consequences for a public user:
- A UAC prompt at **every single launch** — and since V13 now autostarts,
  **at every boot**.
- On a **standard (non-admin) user account** — extremely common on family,
  school and work laptops — they get an admin-password prompt they cannot
  satisfy. **The app is unusable for them.**
Elevation buys exactly one thing: the hook keeps working while an *elevated*
window has focus (UIPI). That is a nice-to-have, not a reason to demand
admin at launch.
**Fix:** run unelevated by default and make elevation opt-in ("Run with
administrator rights so shortcuts work over admin apps"), or implement the
scheduled-task approach in `FEATURES_NOW_POSSIBLE.md` #2 (elevated at logon,
no prompt). Either way, the app must work — degraded — without admin.

### P0-3. Writes the autostart registry key on every launch, without asking
`src-tauri/src/lib.rs:54` calls `startup::register_startup()` unconditionally;
it writes `HKCU\...\CurrentVersion\Run\SpaceToggleOS` pointing at the current
exe path (`startup.rs:23`).
Silently adding yourself to a stranger's autostart is user-hostile, is what
security tooling looks for, and nothing removes the key on uninstall — a
dead entry pointing at a deleted exe remains forever.
**Fix:** make it a Settings toggle (default OFF, or ON only after asking on
first run) and delete the value on uninstall.

### P0-4. The default profiles are the author's personal setup
`src-tauri/src/config/defaults.rs` seeds Founders with `cinemaos.live`,
uTorrent, and a personal Google workflow; Gamers/Professionals likewise.
A stranger's first run shows 26 bindings to sites they never chose and apps
they do not have.
**Fix:** ship neutral defaults (browser, file explorer, terminal, mail,
calculator — things that exist on every Windows box) and add a short first-run
onboarding that offers to detect installed apps. Keep the author's set as an
importable preset if desired.

---

## P1 — will generate support tickets

### P1-5. Non-US keyboard layouts show the wrong letters
`hook/mod.rs:531 vk_to_char()` maps VK `0x41–0x5A` straight to `a–z`. VK codes
are **positional**, so on AZERTY the physical `A` key reports `VK_Q`, and on
QWERTZ `Y`/`Z` swap. The dashboard matrix and the HUD will disagree with the
user's keycaps.
**Fix:** translate for DISPLAY with `ToUnicodeEx`/`MapVirtualKeyEx` against
the active layout (`GetKeyboardLayout`), keeping VK as the storage key.

### P1-6. DPI, small screens and multi-monitor are untested
The Guide HUD is a fixed 680×600, primary-monitor only (an accepted decision
for the author's single 1440p display). On a 1366×768 laptop at 150% scaling
that panel is enormous and may clip; `devicePixelRatio` measured a non-obvious
**1.09** on the author's machine, so DPI handling is clearly not trivial here.
**Fix:** size the HUD as a percentage of the *work area* with min/max clamps,
and test at 100/125/150% and 1366×768.

### P1-7. Antivirus false positives are likely
Global `WH_KEYBOARD_LL` hook + `SendInput` injection + self-elevation +
unsigned binary is the exact profile heuristic AV flags.
**Fix:** P0-1 signing removes most of it; submit the binary to Microsoft and
major vendors for whitelisting before launch.

### P1-8. Windows 10 is unverified
`DWMWA_BORDER_COLOR` / `DWMWA_WINDOW_CORNER_PREFERENCE` (lib.rs overlay setup)
are Windows 11 attributes. The calls are `let _ =`-guarded so they degrade
harmlessly, but **overlay transparency itself has never been tested on Win10**
— and transparency is exactly what broke on this project before.
**Fix:** test the overlay on a Win10 VM before claiming Win10 support.

### P1-9. No auto-updater
There is no way to ship a fix to users who already installed. For an app that
hooks the keyboard, being unable to push a fix is a real risk.
**Fix:** `tauri-plugin-updater` + a signed release feed.

---

## P2 — polish, legal, known gaps

- **Ship the font licence.** Outfit is bundled from `src/assets/fonts/`;
  `OUTFIT-LICENSE.txt` lives there but only the `.woff2` reaches `dist/`.
  OFL requires the licence to travel with the font. Add it to bundle
  resources or an in-app About box.
- **Publish a plain-English privacy statement.** This app hooks every
  keystroke; users deserve an explicit "keystrokes are never recorded or
  transmitted". **Verified true today:** the release logger runs at Info and
  the per-key `log::debug!` calls are compiled out — 0 DEBUG lines in the
  live log after the latest install, and no key content anywhere in it.
  Say so publicly, and keep it true.
- **Store apps don't minimise on second press** (known limitation, logged
  2026-08-10). Launch and focus work; cascade parity needs AUMID→window
  mapping.
- **No hook watchdog.** Windows evicts a low-level hook that ever exceeds its
  timeout. The callback is disciplined, but an all-day tray app should detect
  eviction and reinstall the hook rather than go quietly dead.
- **Uninstall leaves** `%APPDATA%\SpaceToggleOS\` (config + logs). Acceptable,
  but offer a "remove my settings too" checkbox.

---

## What is already production-grade (verified, don't re-litigate)

- Core loop verified on real hardware: typing protection, tap-vs-hold,
  rollover, smart cascade launch/focus/minimise, Boss Key with true Core Audio
  mute, PiP, profile cycling, Guide HUD, force close.
- Logging: Info level, 5 MB rotation, 2 backups, **no keystroke content**.
- Config safety: tolerates a UTF-8 BOM and preserves an unparseable file as
  `config.json.corrupt` instead of destroying it.
- Hook discipline follows the skill's iron laws (dwExtraInfo cookie only,
  dedicated pump thread, no COM/IPC/alloc in the callback).
- `NATIVE_SAFETY.md` rules are enforced in code (shell-window protection,
  click-through fail-closed).
- Single-instance, tray lifecycle, offline-only assets (no CDN).

---

## Suggested order of work

1. P0-2 elevation (biggest UX + reach win, pure code)
2. P0-3 autostart consent (small, and it is a trust issue)
3. P0-4 neutral defaults + first-run onboarding
4. P1-5 keyboard layout, P1-6 DPI/laptop testing
5. P0-1 code signing + P1-7 AV submissions (purchase + lead time — start early)
6. P1-9 updater, then P2 polish
