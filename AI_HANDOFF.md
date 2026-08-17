# AI HANDOFF — SpaceToggle OS V13

**Written:** 2026-08-10 by Claude Opus 5 (via Claude Code)
**For:** any AI or developer picking this up cold, in any tool.

Read this file top to bottom before touching anything. It is deliberately
self-contained: you should not need to reconstruct context from chat history.

Companion files, in the order worth reading:
1. `WHAT_HAPPENED.md` — **the plain-English story of 2026-08-10.** Written for
   a human, not a machine. If you are a person rather than an AI, read that one
   instead of this. It covers the same ground without the jargon.
2. **This file** — orientation, state, rules, learnings.
3. `PROJECT_STATUS.md` — the append-only development log. Newest at top.
   Every bug and its root cause is written up there. **Never delete entries.**
4. `CORE_AIM.md` — the non-negotiable feature contract.
5. `install-v11.ps1` — the functional gold standard (see below).
6. `D:\RUST-DOWNLOADED-HERE\README-RUST-LIVES-HERE.txt` — the build toolchain.

---

## 1. What this project is, and the one sentence that matters

SpaceToggle turns the **Spacebar into a system-wide modifier** on Windows, so
the whole keyboard becomes a launcher — hold Space, tap `F`, File Explorer
appears. Tap Space on its own and it must type a normal space, always.

**The one sentence:** *a feature that exists but cannot actually be used is
worthless.* The user has said this explicitly and it is the standard to hold
work to. Do not build shells. Do not report something as done because it
compiles. This project's history is a graveyard of premature "it's fixed".

## 2. The project's history — why there are so many folders

The user has rebuilt this repeatedly across stacks since ~May 2026. You may be
given these as read-only reference:

| Location | What it is |
|---|---|
| `D:\GITHUB PROJECT\` | Many earlier attempts: Python, PowerShell, Windhawk, AHK, Neon. Includes a `Failed App or EXE file` folder and `TEXT files` with frustration logs. |
| `D:\Neon\` | The Neon remix — Tauri, has UI |
| `D:\SpaceToggle-July_Revisit_2026\` | The Rust/Tauri rewrite. Most mature before V13. |
| `D:\Claude-Projects\SpaceToggle-V13\` | **This fork. The active one.** |

**The single most important fact in this table**, in the user's own words:

> *"install v11 was actually most stable according to the functionality side,
> although it wasn't changeable in UI app. July revisit and Neon had the app
> interface but not full functionality."*
>
> *"the overlay of install v11 was good, but it didn't have any app, neither
> could the desired apps be changed."*

So:
- **v11 (AutoHotkey)** = functionality and a working overlay, no editable UI.
- **July / Neon (Tauri)** = editable UI, hollow functionality.
- **V13's whole purpose** = v11's working behaviour + the Tauri editable UI.

`install-v11.ps1` is the **functional bible**. When behaviour is ambiguous,
match v11. It is an AutoHotkey script; read it directly, it is very legible.

## 3. HARD RULES — violating these breaks the user's trust

0. **Read `NATIVE_SAFETY.md` before touching any Win32 call.** It is the
   do-not-touch table for native Windows features (shell windows, injected
   input, startup keys, audio, transparency). It exists because this app
   broke the user's touchpad gestures on 2026-08-10, and the user explicitly
   demanded the table.
1. **Do not modify** `D:\SpaceToggle-July_Revisit_2026\`, `D:\GITHUB PROJECT\`,
   or `D:\Neon\`. Read and copy from them freely. All new work goes in
   `D:\Claude-Projects\`.
2. **Never delete entries from `PROJECT_STATUS.md`.** Append to the top, dated,
   with who you are. The file says so itself in capitals.
3. **Never remove a CORE_AIM feature to make something build.** Fix it natively.
4. **Nothing hollow.** If you cannot verify something works, say so plainly and
   label it untested. Do not pad a summary with things you assumed.
5. **Ask.** The user has explicitly invited questions and decisions. A blocked
   question asked early is far cheaper than a wrong assumption shipped.

## 4. Build environment — the part that wasted the most time

### Rust lives in ONE folder, on purpose

```
D:\RUST-DOWNLOADED-HERE\
  cargo\      <- CARGO_HOME  (bin\ + the ~1.9 GB registry cache)
  rustup\     <- RUSTUP_HOME (toolchains\)
```

Pinned by user environment variables `CARGO_HOME` and `RUSTUP_HOME`, with
`D:\RUST-DOWNLOADED-HERE\cargo\bin` on PATH. Rust will **not** reinstall into
`C:\Users\beamu\.cargo` again. Versions: cargo/rustc 1.97.1, rustup 1.29.0.

### THE 0-BYTE SHIM TRAP — this machine does this reproducibly

Symptom: `cargo`, `rustc`, `rustup` all fail with **"The system cannot find the
file specified"**, which reads exactly like Rust was never installed.

Reality: the shim files in `cargo\bin` are **0 bytes**. Windows reports that
error when you exec an empty file. `Test-Path` returns **True** for them, so
any "does cargo.exe exist?" check says yes and sends you down the wrong path.

> **CHECK `Length`, NOT JUST EXISTENCE.**
> `Get-ChildItem D:\RUST-DOWNLOADED-HERE\cargo\bin | Select Length,Name`

`rustup default stable` does **NOT** repair it — rustup sees the files, decides
the shims exist, and skips them. A shim is just a copy of `rustup.exe` that
dispatches on its own filename. Delete the empty ones and copy it over them:

```powershell
$bin="D:\RUST-DOWNLOADED-HERE\cargo\bin"; $src="$bin\rustup.exe"
$shims=@('cargo','rustc','rustdoc','rustfmt','cargo-fmt','cargo-clippy',
         'clippy-driver','cargo-miri','rls','rust-analyzer','rust-gdb',
         'rust-gdbgui','rust-lldb')
foreach($s in $shims){ $t="$bin\$s.exe"
  Remove-Item $t -Force -EA SilentlyContinue; Copy-Item $src $t -Force }
cargo --version
```

### Building

```powershell
$env:CARGO_HOME="D:\RUST-DOWNLOADED-HERE\cargo"
$env:RUSTUP_HOME="D:\RUST-DOWNLOADED-HERE\rustup"
$env:PATH="D:\RUST-DOWNLOADED-HERE\cargo\bin;$env:PATH"
cd D:\Claude-Projects\SpaceToggle-V13
npm install
npm run build        # MUST run before any cargo command - see below
npm run tauri build  # produces the MSI
```

**`npm run build` first, always.** `tauri::generate_context!` reads `../dist`.
If `dist/` is missing you get a bare **`error: proc macro panicked`** at
`lib.rs:184` that names no cause whatsoever. That is all it means.

Current state: **0 errors, 0 warnings.** Keep it that way.
Installer output: `src-tauri	argeteleaseundle
sis\Spaceadom_<version>_x64-setup.exe`

(The `.msi` target was REMOVED in 1.0.41 - PROBLEM 129. Tauri's WiX bundler is
per-machine while its NSIS bundler is per-user, so shipping both put two copies
of the app on one machine, each with its own autostart, both grabbing the
spacebar at logon. Do not re-add it without reading PROBLEM 129.)

## 5. Architecture in 60 seconds

```
hook/mod.rs        WH_KEYBOARD_LL + WH_MOUSE_LL on a dedicated thread with its
                   own message pump. Decides: is this typing, or a command?
                   Sends HookEvent over a crossbeam channel. Never touches Tauri.
engine/mod.rs      Async actor. Receives HookEvents, dispatches to actions.
engine/actions/    smart_cascade (launch/focus/minimise cycling), boss_key,
                   pip, opacity, focus_engine.
guide_hud/         Emits Tauri events to the OVERLAY window (not the dashboard).
config/            %APPDATA%\SpaceToggleOS\config.json. Profiles -> key bindings.
src/ (TypeScript)  The dashboard UI: keyboard matrix, key detail panel, settings.
overlay.html       The transparent always-on-top HUD/toast surface.
```

Two windows exist, and the distinction is the heart of PROBLEM 8 below:
- **`settings`** — the normal dashboard. Opaque, decorated, closes to tray.
- **`overlay`** — fullscreen, transparent, click-through, always-on-top. This is
  where the Guide HUD and all toasts render.

## 6. What was wrong, and what is fixed

Full root-cause write-ups are in `PROJECT_STATUS.md`. Summary of the 11 fixed:

| # | Bug | Why it mattered |
|---|---|---|
| 0 | Rust toolchain corrupted (0-byte shims) | Nothing could build at all |
| 1 | Rollover **ordering race** | `SendInput` then `CallNextHookEx` do not preserve order → fast typing gave `hte` for `the` |
| 2 | `LLKHF_INJECTED` blanket-ignored | Silently disabled the app for AHK, macro keyboards, on-screen keyboard, RDP |
| 3 | Every keypress force-showed the dashboard | Space+B threw the settings window in your face |
| 4 | Ctrl/Alt/Win+Space hijacked | Broke IME switch, IDE autocomplete, window menu |
| 5 | Guide HUD delay hardcoded `300` | The Settings slider was a dead control |
| 6 | `Box::into_raw` with no `from_raw` | Memory leak on every uncached keypress, in an all-day tray app |
| 7 | Build warnings | Handover asked for a clean build |
| 8 | **Overlay window never created** | **THE hollow-shell bug — see below** |
| 9 | OS notification per keypress | Action Center spam |
| 10 | v11 + V13 both hooking | Feedback loop; must not run together |
| 11 | Silent startup-entry hijack | Repointed user's autostart at a build folder |

### The one to actually understand: #8

`overlay.html` and `overlay.ts` existed and were correct. Vite built them. But
`tauri.conf.json` declared only the `settings` window and there was no
`WebviewWindowBuilder` anywhere — **so nothing ever loaded them.** The HUD and
every toast rendered inside the dashboard. Minimised to tray (normal use), they
were invisible.

That is why July and Neon felt hollow. And #3 was a *workaround for it* —
force-showing the dashboard was how a previous AI made the trapped toast
visible. Two AIs before me debugged the toast CSS instead of checking whether
the window existed.

> **Lesson: when a feature "exists but does nothing", confirm it is WIRED UP
> before debugging its internals.**

Verified fixed empirically: the live overlay window reports ExStyle
`0x000C0138` = `WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_TOPMOST |
WS_EX_TOOLWINDOW` — an exact match for v11's `+AlwaysOnTop -Caption
+ToolWindow +E0x20`.

**SAFETY:** the overlay is fullscreen and always-on-top. If
`set_ignore_cursor_events(true)` fails, the user cannot click anything at all.
The code hides the overlay on failure. Do not simplify that away.

## 7. TESTING — read this before writing a single test

### Update (2026-08-10, part 3): combos ARE now testable by injection

The old "hard limit" below is PARTIALLY obsolete: the GetAsyncKeyState
failsafe that made combos untestable was itself a bug (it broke combos for
REAL keys too) and was replaced with a timestamp-based latch timeout. Combos
now fire under injection — **but only if the injector runs at the same
elevation as the app.** The app self-elevates via UAC on every launch
(lib.rs); Windows silently discards unelevated synthetic input relative to
elevated processes while SendInput still returns success. Run test harnesses
elevated (`Start-Process powershell -Verb RunAs`) or every result is void.

Also learned the hard way, in one afternoon:
- A new Tauri window is DEAF until added to `capabilities/default.json`
  ("windows": [...]). listen() rejects silently. The `overlay_log` command
  forwards overlay JS errors into the Rust log — check there first.
- **Transparency: SCOPE THIS CORRECTLY.** A *fullscreen* transparent Tauri
  window renders nothing on this machine (2026-07-10 and 2026-08-10). A
  **small, on-demand transparent window WORKS** — re-tested and verified
  2026-08-10 16:11. The overlay is transparent today. The earlier note here
  said only "transparent windows render nothing", and that over-broad
  wording cost a later session two failed redesigns. **When you record a
  failure, record the CONDITION it failed under and how to re-test it.**
- Undecorated Windows 11 windows still get a 1px DWM border. Kill it with
  `DWMWA_BORDER_COLOR = DWMWA_COLOR_NONE (0xFFFFFFFE)`, and use
  `DWMWCP_DONOTROUND` so the frame doesn't clip your own rounded corners.
- Show the window BEFORE emitting content; hidden webviews don't paint.
- CSP without style 'unsafe-inline' silently kills every style="..." attr.
- Global emit + single listener is the only event arrangement that works;
  emit_to() never delivered in any combination we tried.
- `NATIVE_SAFETY.md` is mandatory reading before Win32 window manipulation —
  the cascade minimized the Windows SHELL and broke touchpad gestures.

### The hard limit as originally discovered (context, partially obsolete)

**You cannot test Space+key combos with `SendInput`. Do not try.**

`GetAsyncKeyState(VK_SPACE)` returns **False** the entire time an injected Space
is held. Structural reason: the hook suppresses Space-down (`LRESULT(1)`), so it
never propagates to update the key-state table. A real key sets that state at
the hardware layer regardless of suppression; an injected key has no hardware
behind it. So the engine's stuck-modifier failsafe always bails, and no combo
can ever fire under injection.

**Combo behaviour requires a human pressing a real key.** Ask the user.

### Three harness traps I fell into — do not repeat them

1. **WinForms TextBox + `Application.DoEvents()` from PowerShell does not
   work.** The console keeps focus. Everything reads empty, which looks
   identical to "the app ate my keystrokes".
2. **Win11 Notepad PID mismatch.** `Start-Process notepad -PassThru` returned
   PID 41868 while the actual window belonged to PID 48520 (Store app model).
   Match the foreground process by **name**.
3. **The C# `INPUT` struct must be 40 bytes on x64, not 32.** The union must be
   sized for `MOUSEINPUT` (32) not `KEYBDINPUT` (24). Wrong size →
   `SendInput` returns **0**, `LastError=87`, and injects **nothing**, silently.
   **Always check SendInput's return value.**

Also: `SetForegroundWindow` is blocked for the agent process in this
environment. Any automated test must ask the user to click the target window.

### The rule that would have saved all of it

> **Validate the harness against a known-good baseline FIRST.** Run it with the
> app stopped. If it still fails, the harness is broken — not the app.

A working harness lives at:
`<scratchpad>\v13_verify.ps1` (opens Notepad, waits for the user to click it,
then runs the suite). Recreate it from the pattern above if it is gone.

### Verified passing as of 2026-08-10

Ordinary typing with spaces · no stuck modifier after a command · tapped space
gives exactly one space · Ctrl+Space not swallowed or duplicated · long-held
space leaks no auto-repeat.

### Awaiting human verification

Overlapped space+letter ordering (the `hte` bug) · held Space+key firing a
command · the Guide HUD actually appearing · Boss Key audio mute · PiP ·
smart-cascade launch→minimise→restore cycling.

## 8. Outstanding work

1. **Config saved TWICE on every change.** Duplicate `config: saved N bytes`
   pairs ~2ms apart. Pre-existing — present in the 2026-07-10 logs too.
   `save_config` writes once, so the frontend is calling `persistConfig()`
   twice. Start at the callers in `src/main.ts` and `src/components/`.
2. **Startup entry is written on every launch** (`startup.rs`), silently
   overwriting the user's. Probably should be opt-in — user has been asked.
3. **Guide HUD is primary-monitor only.** v11 followed the monitor with the
   active window. **The user explicitly accepted primary-only.** Do not change
   without asking.
4. Everything in section 7 "awaiting human verification".

## 9. Operational notes

- **Config/logs:** `%APPDATA%\SpaceToggleOS\` — `config.json`, `debug.log`.
  The log is genuinely useful; it is the fastest way to see engine decisions.
- **v11 is still installed and autostarts** from
  `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\SpaceToggleV11.lnk`,
  running `SpaceToggleRuntime.exe` (AutoHotkey 64-bit). **Kill the process
  before testing V13**, but leave the shortcut alone — the user wants v11 back
  after a reboot until V13 fully replaces it.
- **The user's display is a single 2560x1440 monitor.**
- The app relaunches itself on start (elevation), so the PID from
  `Start-Process` is not the running app. Look it up by name.

---

### The short version, if you read nothing else

The Tauri rewrites were never far from working — they were **unwired**. The
overlay existed but no window loaded it, so every visible signal the app gave
was rendering into a hidden window. Fix wiring before internals, verify against
real behaviour rather than a clean compile, and when you cannot verify
something, **say so**.
