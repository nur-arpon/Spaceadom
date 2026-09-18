# Phase A brief 2 — the `Toggle` action (1.0.117)

Owner, 2026-09-19 01:10, after trying 1.0.116: "the pointing to windows
setting is a useless feature, i thought those would toggle settings
automatically" → "all, build it". So: a key under Space FLIPS the thing and
a toast says the new state. "Open the page" rows stay, but are demoted.

Read first: `docs/PHASE-A-BRIEF-1.md` (the model you are extending),
`src-tauri/src/engine/actions/brightness.rs` (the shape to copy: pure
`script()` + `toast_text()` + `adjust()` on its own thread, hidden
`powershell.exe`, never blocks the engine actor, NEVER run from a test),
`src-tauri/src/config/schema.rs` (`Action`), `src-tauri/src/engine/mod.rs`
(`run_action`), `src/components/key-detail-panel.ts` (`actionFromItem`,
`BASIC_GROUPS` gate, the catalogue list), `src/data/windows-catalogue.json`,
`src/types.ts`, `CLAUDE.md` (laws: no config reads in the hook callback, no
elevation, tests are the proof).

## The action

```
Action::Toggle { what: String }
```
`what` ∈ `bluetooth | wifi | dark_mode | night_light | taskbar_autohide |
screen_off | sleep | lock | show_desktop`. Serde: `{"kind":"toggle","what":"bluetooth"}`.
Unknown `what` → `log::warn!` + toast "Unknown toggle", nothing else (a
config from a newer build). Each one is ONE hidden PowerShell 5.1
(`powershell.exe`, the same spawn as brightness) unless noted. The script
prints the NEW state on its last line (`on` / `off`, or `done` for one-shots);
the toast is built from that. Exit 3 = "not available on this machine" →
toast says so. Timeout the wait at 8 s (Wi‑Fi/Bluetooth radio calls can be
slow); on timeout log + toast "… didn't answer".

New file `src-tauri/src/engine/actions/toggle.rs`, one `fn script(what) ->
Option<String>` (pure, tested for every id: non-empty, contains the key API
name), `toast_text(what, outcome)` (pure, tested), `run(what, app_handle)`
(thread, never tested).

### Scripts (Windows' own, no third-party, no elevation)

- **bluetooth / wifi** — WinRT `Windows.Devices.Radios.Radio`:
  `[Windows.Devices.Radios.Radio,Windows.System.Devices,ContentType=WindowsRuntime]`,
  `AsTask` via `System.Runtime.WindowsRuntime`, `RequestAccessAsync` then
  `GetRadiosAsync`, pick `Kind -eq 'Bluetooth'` / `'WiFi'`, `SetStateAsync`
  to the opposite of `.State`. No radio of that kind → exit 3. Access denied
  → exit 4 → toast "Windows refused (Settings › Privacy › Radios)".
- **dark_mode** — `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`,
  read `AppsUseLightTheme`, write BOTH `AppsUseLightTheme` and
  `SystemUsesLightTheme` to the flipped value, then broadcast
  `WM_SETTINGCHANGE` with lParam `"ImmersiveColorSet"` via
  `SendMessageTimeout(HWND_BROADCAST, …, SMTO_ABORTIFHUNG, 100)` (P/Invoke in
  the script) so the taskbar and open apps repaint. Prints `dark`/`light`.
- **night_light** — the CloudStore blob Windows itself writes:
  `HKCU\Software\Microsoft\Windows\CurrentVersion\CloudStore\Store\DefaultAccount\Current\default$windows.data.bluelightreduction.bluelightreductionstate\windows.data.bluelightreduction.bluelightreductionstate`,
  value `Data` (REG_BINARY). Enabled blobs carry `0x15 0x00` at offset 18
  followed by two extra bytes `0x10 0x00` at offset 23; disabled blobs carry
  `0x13 0x00` at 18 with no extra bytes. Toggle = flip that and bump the
  8-byte timestamp at offset 10 (bytes 10..14 incremented) so Windows
  notices — this is the algorithm every public "toggle night light" script
  uses (2019–2025), and it is undocumented: put that sentence in the module
  doc AND in the catalogue row's description ("Windows has no public switch
  — this uses the same setting Settings writes; may need re-checking after
  a Windows update"). Key missing → exit 3.
- **taskbar_autohide** — NOT the StuckRects registry. `SHAppBarMessage`
  (shell32, P/Invoke in the script): `ABM_GETSTATE` (4) → flip `ABS_AUTOHIDE`
  (1) → `ABM_SETSTATE` (10) with `lParam` = new state. Prints `on`/`off`.
  Takes effect instantly, no Explorer restart.
- **screen_off** — no PowerShell: from Rust, `SendMessageW(HWND_BROADCAST,
  WM_SYSCOMMAND, SC_MONITORPOWER (0xF170), 2)` on the worker thread. Toast
  first ("Screen off"), then send, because the toast cannot be seen after.
- **sleep** — `Add-Type -AssemblyName System.Windows.Forms;
  [System.Windows.Forms.Application]::SetSuspendState('Suspend', $false, $false)`
  (Suspend, NOT Hibernate). Toast before.
- **lock** — Rust `LockWorkStation()` (user32). One-shot.
- **show_desktop** — a chord `Win+D` through the existing
  `send_keys_checked` one-batch path (PROBLEM 227) — reuse
  `actions::chord`, do not reimplement.

`screen_off`, `sleep`, `lock`: NEVER executed by any test or by you.

## Catalogue + editor

- `src/data/windows-catalogue.json`: add a `"toggle"` kind with rows
  `system.toggle_bluetooth` "Bluetooth on/off", `…_wifi` "Wi‑Fi on/off",
  `…_dark_mode` "Dark / light mode", `…_night_light` "Night light on/off"
  (with the caveat sentence in a `"note"` field, shown under the row),
  `…_taskbar_autohide` "Taskbar auto-hide", `…_screen_off` "Screen off",
  `…_sleep` "Sleep", `…_lock` "Lock", `…_show_desktop` "Show desktop";
  plus chord rows `Volume up` (VK_VOLUME_UP 0xAF), `Volume down` (0xAE),
  `Mute` (0xAD) if not already present. `target` = the `what` id.
- The editor's "Windows setting" tab: rename its label to **"Windows"**.
  Ordering inside the list: toggles + brightness + volume FIRST (a small
  "Toggles & controls" heading), then "Open a settings page" rows below
  (heading). Search still matches both. The `BASIC_GROUPS` gate: every
  `toggle`, `brightness` and the three volume chords are basic regardless
  of group (the brightness carve-out is already there — generalise it).
- `actionFromItem`: `toggle` → `{ kind: "toggle", what: target }`.
- `src/types.ts`: the `Action` union gains `{ kind: "toggle"; what: string }`.
- The Space ring / HUD label for a toggle binding is the catalogue row's
  name (`engine::specials::binding_name` or wherever chord labels come
  from — same place).

## Proof you may do on this machine (the owner IS using the laptop)

- You MAY run the `dark_mode`, `night_light` and `taskbar_autohide` scripts
  directly in PowerShell **twice in a row** (flip, flip back — a one-second
  blink) and paste the two printed states into your report.
- `bluetooth` / `wifi`: READ the radio state only (run the script with
  `SetStateAsync` commented out or behind a `-WhatIf` switch you add) — do
  NOT flip either; the owner's connection and headphones are on them.
- `screen_off`, `sleep`, `lock`, `show_desktop`: never.
- Do not touch `%APPDATA%\Spaceadom\config.json`, do not build an installer,
  do not install, do not commit. No input injection.

## Gates

`cargo test --release --lib` (add tests: every `what` has a script /
toast; serde round-trip of `Action::Toggle`; unknown `what` gives the warn
path), `cargo clippy --release --lib` 0 warnings, `npx tsc --noEmit -p .`,
`npm run build`. Version → **1.0.117** in `package.json`,
`src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`,
`scripts/install-real.cmd`. Append a §PHASE A — step 2 entry to
`V14_FIXES_AND_CODE.md` and a dated entry to `PROJECT_STATUS.md` (write via
temp file + rename like step 1). Report: files, gates, the flip-flip
outputs, and any QUESTION as a one-liner.
