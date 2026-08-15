
# Win32 Keyboard Hooks

Everything here is a documented behaviour or a solution proven in a shipping
project. Where something is a known unfixable limitation, it says so — that is
as valuable as a fix, because it stops the search.

## 1. Never filter on LLKHF_INJECTED

This is the most common serious mistake in this class of app.

The intent is reasonable: ignore the input you synthesized yourself, so the hook
does not re-enter. `LLKHF_INJECTED` (bit 4, `0x10`) looks like the flag for that.
It is not. It means "not from a physical HID device right now", which includes:

- AutoHotkey and every macro tool
- Gaming and macro keyboard software that replays via `SendInput` — Razer
  Synapse, Logitech G HUB, Corsair iCUE
- Remote Desktop, VNC, VM guest input
- On-screen keyboard, accessibility tools, touch keyboards
- Some laptop keyboard drivers, for genuinely physical keys

Dropping all of it makes the app look completely dead, for no visible reason.

`KBDLLHOOKSTRUCT` also defines `LLKHF_LOWER_IL_INJECTED` (bit 1, `0x02`), for
injection from a lower integrity level. Microsoft documents that bit 4 is also
set when bit 1 is set, but bit 1 is not necessarily set when bit 4 is.
<https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-kbdllhookstruct>

### Do this instead: tag your own input with dwExtraInfo

`dwExtraInfo` is a free `ULONG_PTR` you set on your own `SendInput` calls and
check in the hook. Yours is the only code that writes your constant, so the test
is exact.

**Default-allow everything. Suppress only events carrying your signature.**

Microsoft's own PowerToys Keyboard Manager does exactly this, with
`KEYBOARDMANAGER_INJECTED_FLAG`, `KEYBOARDMANAGER_SINGLEKEY_FLAG` and
`KEYBOARDMANAGER_SHORTCUT_FLAG`, "to ensure that we don't read events generated
by the key or shortcut remap methods".
<https://github.com/microsoft/PowerToys/blob/main/doc/devdocs/modules/keyboardmanager/keyboardeventhandlers.md>

AutoHotkey generalises this into `#InputLevel` / `SendLevel`: a level 0–100
encoded into `dwExtraInfo`, so scripts can deliberately trigger each other. The
motivation for the redesign was precisely that a single sentinel made it
impossible for one script to trigger hotkeys in another.
<https://www.autohotkey.com/docs/v2/lib/SendLevel.htm> ·
<https://github.com/AutoHotkey/AutoHotkey/pull/7>

If you need to know which physical keyboard sent an event, `dwExtraInfo` will not
tell you — use Raw Input for device identity (section 8).

## 2. The hook thread, and why hooks silently die

`LowLevelKeyboardProc` is documented with three constraints that decide whether
this app works at all.
<https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc>

- **The installing thread must have a message loop.** No pump, no callbacks.
- **If the callback exceeds `LowLevelHooksTimeout`, Windows removes the hook
  silently.** Microsoft: "There is no way for the application to know whether the
  hook is removed." The value lives at
  `HKEY_CURRENT_USER\Control Panel\Desktop\LowLevelHooksTimeout` in milliseconds,
  and on Windows 10 1709 and later it is capped at 1000ms regardless.
- Microsoft's own prescription: run the hook on a dedicated thread that hands
  work to a worker and returns immediately.

This produces a symptom that is almost always misdiagnosed: **the keyboard stops
responding after a while, and restarting the app fixes it.** That reads like a
memory leak or a state bug. It is hook eviction.

In a Tauri or Electron app it is close to guaranteed if you get this wrong,
because a WebView2 garbage collection or layout pass can stall the UI thread well
past 1000ms.

### Rules for the callback

Inside the hook, do only this: read the event, check your `dwExtraInfo` tag,
consult an atomic or lock-free structure, decide pass-or-suppress, return.

Never, inside the callback:

- Call COM. Cross-apartment marshalling can block for hundreds of milliseconds.
- Send IPC to the frontend, emit a Tauri event, or touch a webview.
- Allocate on the heap, log to a file, or read config from disk.
- Take a lock that the UI thread also takes. That is a deadlock and an eviction.
- Panic. In Rust, a panic unwinding out of an `extern "system"` callback is
  undefined behaviour. Wrap the body in `catch_unwind`, or build with
  `panic = "abort"` and keep the body infallible.

Put the hook on its own thread with its own pump. Send decisions to the rest of
the app over a channel.

## 3. Reentrancy

`SendInput` called from inside the hook re-enters the hook chain. PowerToys
documents this explicitly in its test harness. Without the `dwExtraInfo` tag from
section 1 you get doubled keys or infinite recursion.

PowerToys also documents the **dummy key event**: after setting modifiers with no
action key, send a synthetic press/release so the system does not sit in an
ambiguous modifiers-only state.

## 4. Tap versus hold, and why fast typing scrambles

A dual-role key must emit its tap character **on release**, not on press, because
until release you do not know which role it is. This is inherent, and it is why
the output can reorder relative to fast typing.

The canonical failure, from `lydell/dual`: typing "don't" quickly with `d` as a
dual-role Ctrl fires Ctrl+O and opens a dialog. That project's honest conclusion
after extensive tuning is that **no single set of timing values works for all
keys**. <https://github.com/lydell/dual>

Timers alone will not fix this. Use the resolution heuristics the keyboard-firmware
world converged on:

- **Permissive hold.** If another key is pressed *and released* while the
  dual-role key is held, resolve as hold. If the other key is still down when the
  dual-role key releases, resolve as tap.
- **Hold on other key press.** Any other key going down resolves it as hold
  immediately, without waiting out the timeout.
- **Chordal hold.** Same-hand keys resolve as tap, opposite-hand as hold. This is
  the modern answer to rollover scrambling specifically.

**Buffer and replay in original order.** While the decision is pending, queue the
events. When it resolves, emit them in the order they arrived. Emitting the space
and the letter down different paths is exactly what produces "hte" for "the".

Reference implementations worth reading: **kanata** (Rust, Windows-native,
`tap-hold`, `tap-hold-press`, `tap-hold-release`)
<https://github.com/jtroo/kanata> · **keyd** (C, cleanest state machine, Linux —
read for the algorithm) <https://github.com/rvaiya/keyd> · **kmonad**
<https://github.com/kmonad/kmonad> · **lydell/spacefn-win**, a working SpaceFn
for Windows in AutoHotkey <https://github.com/lydell/spacefn-win>

## 5. Stuck modifiers, and why your app "minimizes the Windows shell"

If a modifier goes down and never comes up in your model — or worse, in the OS's
model — subsequent keys become shell hotkeys. A stuck Win key turns ordinary
typing into `Win+D` (show desktop), `Win+M` (minimize all), `Win+E`, `Win+Down`.
That is what "the app minimized parts of the Windows shell" looks like from the
outside. It is not the app enumerating windows. It is a leaked key-down.

Documented causes: release events missed while Windows locks or sleeps with a
modifier held; RDP session interruption; two remappers running at once. The
kanata maintainer's position is that this is partly outside any app's control —
mitigate, do not expect to eliminate.
<https://github.com/jtroo/kanata/discussions/423>

Mitigations to implement:

- **Idle reset.** Clear all tracked key state after a period of inactivity
  (kanata uses 60s) to recover from events missed during a lock.
- **Force-release on session and power events.** Handle
  `WM_WTSSESSION_CHANGE` (lock, unlock, RDP connect and disconnect),
  `WM_POWERBROADCAST` (resume), and `WM_ACTIVATEAPP` — release every modifier you
  synthesized.
- **Reconcile against the OS.** Periodically compare your model to
  `GetAsyncKeyState` for the modifier virtual-key codes and force-release
  anything you believe is down that the OS says is up.
- **Always clean up on exit.** `UnhookWindowsHookEx`, and send key-up for every
  modifier you injected — including on panic and abort paths.

## 6. Keys you cannot have, and hooks you cannot win

Stop trying to fix these. They are not your bug.

- **Win+Space** is the OS input-language switcher, handled above the hook.
- **Ctrl+Space** and **Alt+Shift** belong to the IME and text services layer.
  <https://blog.keyman.com/2007/11/ctrlspace-altle/>
- **Win+L and Ctrl+Alt+Del** are secure-attention-sequence, kernel level,
  interceptable by nothing.
- **Elevated windows.** UIPI blocks a medium-integrity process from affecting a
  high-integrity foreground window. While an elevated app or a UAC prompt has
  focus, your hook receives nothing — and a dual-role key can be left stuck down
  as a result. Options are to run elevated, or ship a `uiAccess="true"` manifest,
  which requires an Authenticode-signed binary installed to a secure location
  such as `%ProgramFiles%`.
- **Two low-level hooks fighting.** Your app plus AutoHotkey plus vendor keyboard
  software is a known-unfixable class of bug. Detect a previous version of your
  own app running and refuse to start, or the two will feed each other.

A hyper-modifier design must treat Win+Space and Ctrl+Space as reserved.

## 7. Muting audio

Do not synthesize `VK_VOLUME_MUTE`. It is a toggle, so you cannot know the
resulting state; it races other apps; it is itself a keystroke another hook can
swallow; and it fails entirely when an elevated window has focus.

Use Core Audio: `IMMDeviceEnumerator` → `GetDefaultAudioEndpoint(eRender,
eConsole)` → `Activate(IID_IAudioEndpointVolume)` → `SetMute` / `GetMute`.
<https://learn.microsoft.com/en-us/windows/win32/coreaudio/endpoint-volume-controls>

- `CoInitializeEx(None, COINIT_MULTITHREADED)` on the same thread that uses the
  interfaces, and `CoUninitialize` on that thread. COM pointers are not freely
  `Send` across apartments — simplest correct design is a dedicated audio thread
  you message.
- **Never from the hook callback** (section 2).
- Pass a non-null `guidEventContext` to `SetMute` so your own
  `IAudioEndpointVolumeCallback` can ignore the change you caused. Same idea as
  `dwExtraInfo`, for audio.
- Re-acquire the endpoint on device change via `IMMNotificationClient` — the
  default endpoint moves when a headset is plugged in.

## 8. Raw Input, for device identity only

Raw Input gives you the source device handle (`RAWINPUTHEADER.hDevice`),
scancodes, and background delivery with `RIDEV_INPUTSINK`. It **cannot block or
consume input**. <https://learn.microsoft.com/en-us/windows/win32/inputdev/about-raw-input>

The mature pattern is hook for suppression plus Raw Input for identity,
correlated by timing. This is also a far better macro-keyboard discriminator than
`LLKHF_INJECTED`: a software replay has no device handle, real hardware does.

## 9. Click-through overlay windows

Needs `WS_EX_LAYERED | WS_EX_TRANSPARENT` **together**. `WS_EX_TRANSPARENT` alone
does not reliably make a top-level window hit-test-transparent.
<https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features>

- In Tauri, `set_ignore_cursor_events(true)` returns a `Result`. **Check it.** If
  it fails and you continue, you have a full-screen invisible window eating every
  click on the desktop. Fail closed: hide or destroy the overlay if it errors.
- Add `focusable: false` / `WS_EX_NOACTIVATE`. A layered click-through window can
  still take keyboard focus, which breaks the very hotkeys the app exists for.
- Re-apply the extended style after anything that can clobber it: a later
  `SetWindowLongPtr`, a DPI change, a monitor reconfiguration.
- Ship a watchdog that destroys the overlay if it has been visible beyond a
  sensible limit without the modifier held, plus a kill shortcut registered with
  `RegisterHotKey` — which does not depend on your own hook.

**Confirm the window exists before debugging how it looks.** A correctly built,
correctly styled overlay that nobody ever created renders into nothing and
reports no error. Rounds of CSS changes that fix nothing are the signature.

## 10. Verify at the OS, not in your code

For anything in this file, confirm the OS agrees rather than trusting that your
call succeeded: query the window's extended style back, query mute state back,
query `GetAsyncKeyState` for the modifiers. Most of these APIs fail by doing
nothing rather than by returning an error you would notice.

Automated testing cannot reach some of this. Synthetic input does not create the
physical key state that `GetAsyncKeyState` reports, so a hold-based shortcut can
never fire under a synthetic test, no matter how the test is written. Prove that
limit once, write it down, and ask for a human check rather than rewriting the
test.
