# NATIVE SAFETY — things this app must NEVER touch

**Read this before writing or changing ANY code that calls Win32 APIs.**
This file exists because on 2026-08-10 the app broke the user's touchpad
gestures and window management by manipulating shell windows. The user's
instruction, verbatim:

> "I think you should have a proper safety table that these are the things you
> cannot touch to like break, because I do not want this similar type of thing
> to happen in the future where you break the native Windows features while
> trying to build something of your own."

A launcher that breaks the OS it runs on is worse than no launcher. When in
doubt: **don't act on the window — skip it.**

---

## 1. The DO-NOT-TOUCH table

| Never do this | To | Because | Incident |
|---|---|---|---|
| `ShowWindow`, `SetForegroundWindow`, `SW_MINIMIZE/RESTORE`, move/resize | Any **explorer.exe** window that is not class `CabinetWClass` | explorer.exe IS the shell: taskbar (`Shell_TrayWnd`), desktop (`Progman`, `WorkerW`), Task-View/gesture overlays (`XamlExplorerHostIslandWindow`), helpers (`ThumbnailDeviceHelperWnd`, …). Touching them kills 3/4-finger gestures, snap and window management until Explorer restarts | 2026-08-10: Space+F minimized a shell window → gestures dead |
| Act on ANY window with an empty title (non-explorer apps) | Helper/IME/tray windows of the target process | They are infrastructure, not the app the user meant | Same day: cascade "restored" `ThumbnailDeviceHelperWnd` |
| Suppress or synthesize input while ANOTHER hook app runs (v11 AHK) | The keyboard | Two spacebar hooks re-process each other's injections → feedback loop | 2026-08-10: v11 found running mid-test |
| Blanket-ignore `LLKHF_INJECTED` | The keyboard hook | Kills the app for AHK/macro/on-screen-keyboard/RDP users. Filter ONLY on our own `0x7A7A7A7A` cookie | July builds were dead for injected-input users |
| Swallow `Ctrl/Alt/Win + Space` | The hook | IME switching, IDE autocomplete, the window menu are OS features | Fixed 2026-08-10 |
| Leave a fullscreen window without click-through | The overlay | If `set_ignore_cursor_events` fails on an always-on-top fullscreen window, the user cannot click ANYTHING | Guarded same day |
| Write the `HKCU\...\Run` startup key implicitly | The user's autostart | Launching a dev build silently repointed the user's startup entry at a build folder | 2026-08-10, twice |
| Simulate `VK_VOLUME_MUTE` as a toggle | Audio | It's a hardware toggle: "mute" un-mutes an already-muted system. Use `IAudioEndpointVolume` with explicit state | Logged 2026-07-10 |
| `backdrop-filter`, and transparent **FULLSCREEN** Tauri windows | The overlay | White boxes (2026-07-10) or fully invisible content (2026-08-10) on this machine. **Scope matters: a SMALL on-demand transparent window works fine — verified 2026-08-10.** Do not read this row as "never use transparency" | Both logged |

## 2. The rules behind the table

1. **Positive filters, not denylists.** We enumerated shell window classes to
   exclude and immediately met one more (`ThumbnailDeviceHelperWnd`). For
   explorer.exe the ONLY acceptable target class is `CabinetWClass` (a real
   File Explorer file window). A denylist of shell classes WILL be incomplete.
2. **The shell is not an app.** If the "app" you resolved is explorer.exe, you
   are one wrong HWND away from the taskbar. Treat it as hazardous material.
3. **Never trust a cached HWND.** Windows recycles handles; a cached handle
   must be re-validated (`IsWindow` + the class rule above) before acting.
4. **Anything that changes global OS state needs an undo path in the same
   commit.** Mute must unmute, minimize-all must restore-all, hooks must
   uninstall on exit.
5. **Test on the real machine before claiming it works.** Every incident above
   compiled cleanly.

## 3. If a native feature breaks anyway

- Gestures / taskbar / snap misbehaving → stop the app, then restart the
  shell: `Stop-Process -Name explorer -Force` (it auto-restarts). Reboot if
  residue remains.
- Keys acting held-down → check `GetAsyncKeyState` for stuck modifiers, send
  corrective KEYUPs.
- Log every incident in `PROJECT_STATUS.md` and add the new rule HERE.
