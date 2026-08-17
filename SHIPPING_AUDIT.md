# Shipping audit — what breaks when strangers use this

**Written 2026-08-17 against 1.0.36 (built) / 1.0.35 (installed).**

The question this answers: *if this goes to the Microsoft Store tomorrow and
people all over the world install it, what goes wrong?*

Everything below is either read from the code, measured on the machine, or
quoted from Microsoft's own policy document. Where something is a guess, it
says so.

---

## 0. The short answer

**It cannot go in the Microsoft Store today.** Two hard blockers, both from the
policy text, neither of them about code quality:

| Blocker | Policy | Cost to fix |
| --- | --- | --- |
| The binaries are **unsigned** | 10.2.9 | Money — a code-signing certificate |
| There is **no privacy policy** | 10.5.1 | Free — write one, host it |

Beyond that, the single biggest *functional* risk to a worldwide audience is
**not a crash**. It is that the app assumes a US QWERTY keyboard and has no
awareness of IME input. That affects a very large fraction of the world and has
never been tested.

---

## 1. Crash and hang risks, ranked

### 1.1 The app can panic during start-up on a loaded machine — REAL, unfixed

`src-tauri/src/hook/fullscreen.rs:60`

```rust
.expect("failed to spawn fullscreen watcher thread");
```

`std::thread::Builder::spawn` returns `Err` when the OS cannot create a thread —
memory pressure, a thread-count limit, a restrictive job object. On this
machine it never fails. On a 4 GB laptop with fifty Chrome tabs, or inside a
locked-down corporate image, it can.

The consequence is not a degraded feature. It is a **panic before the keyboard
hook is installed**, so the app dies at launch with no window, no tray icon and
a log the user will never read. Indistinguishable from "it doesn't work".

This directly violates Store policy **10.4.2**: *"Products must start up
promptly, continue to run and remain responsive... must not close unexpectedly.
The product must handle exceptions raised by any of the managed or native
system APIs."*

The rest of this file already gets it right — the probe itself fails OPEN, with
the comment *"a broken probe must never be able to disable every shortcut"*.
The spawn should follow the same rule: log an error, carry on without
full-screen detection.

**Fix:** replace `.expect(...)` with a match that logs and returns.

### 1.2 Two applications can wedge together — mitigated in 1.0.35, unproven

`force_foreground` attaches Spaceadom's input thread to the foreground
application's to defeat Windows' focus lock. Attached threads **share one input
queue**: if the other side stops pumping messages, both stall.

1.0.35 checks `IsHungAppWindow` first and skips the attach. That can only help,
but it has never been observed doing so, and the original hangs were never
proven to come from here. See PROBLEM 121.

### 1.3 The overlay stops compositing — three fixes deep, still the fragile part

PROBLEM 117 (display changes), 118 (the repair itself was broken), 122 (the
detector had switched itself off). Transparent, layered, always-on-top windows
behave differently across GPU vendors, hybrid-graphics laptops, RDP and virtual
displays. This machine alone produced two distinct failures in one evening.

Expect this to be the most common support complaint. The mitigations now are:
software-rendering fallback, display-change rebuild, and a verify-on-use check
that rebuilds up to three times. The last of these has never fired.

### 1.4 Memory that only grows — minor, worth knowing

- `IconCacheState` is a `HashMap<String, String>` of base64 PNGs with **no
  eviction**. A machine with 400 Start Menu entries holds every icon it has
  ever rendered for the life of the process. Megabytes, not gigabytes, but it
  never comes back.
- `smart_cascade`'s app cache holds `HWND`s that go stale when the target
  closes. Bounded by the number of distinct apps launched; harmless in practice.

Logs are fine: `FixedWindowRoller` with 2 files at 5 MB, and the logger
degrades gracefully when `%APPDATA%` is unwritable.

---

## 2. Software that will fight this app

The app already detects two and names them in the log. The rest are inferred
from how they work, not observed.

**Detected today (`hook/conflicts.rs`):**

- **PowerToys Keyboard Manager** — remaps keys system-wide, installs its own
  hook. Present on this developer's machine right now.
- **spacedesk** — forwards input to a virtual second display. Implicated in the
  overlay failures of 2026-08-16.
- **AutoHotkey** — including this project's own v11 predecessor. Two spacebar
  hooks feed back into each other.

**Not detected, and likely:**

- **Gaming keyboard software** — Razer Synapse, Logitech G HUB, Corsair iCUE,
  SteelSeries GG. All install low-level hooks and remap keys.
- **Remote desktop and streaming** — TeamViewer, AnyDesk, Parsec, Moonlight.
  These inject and capture input; hook ordering decides who wins.
- **Screen readers** — NVDA and JAWS rely on hooks and on Space having its
  normal meaning. A conflict here is an accessibility failure, not an
  inconvenience.
- **Antivirus keystroke protection** — Kaspersky, Bitdefender Safepay and
  similar deliberately BLOCK low-level hooks inside banking windows. The app
  will appear dead there, correctly and by design.
- **Windows' own secure desktop** — no hook runs on the UAC prompt. Expected.

**The one that is not a conflict but will be reported as one:** any window
running as administrator. A non-elevated hook receives nothing while such a
window has focus. This is Windows UIPI, it affects every remapper ever written,
and the app already logs it correctly — but **nothing tells the user**, so it
reads as a bug. Store policy 10.1.1 requires important limitations to be stated
in the product description.

---

## 3. The world is not US QWERTY — the biggest untested risk

### 3.1 Key letters are positional, and the dashboard is not

`src-tauri/src/hook/mod.rs`

```rust
fn vk_to_char(vk: u16) -> Option<char> {
    if (0x41..=0x5A).contains(&vk) {
        char::from_u32((vk as u32) + 32) // map A→a, etc.
    } else { None }
}
```

Virtual-key codes are **positional**. `VK_A` (0x41) is the key that sits where
A sits on a US keyboard, regardless of what is printed on it.

- On a **French AZERTY** keyboard, that key is labelled **Q**.
- On a **German QWERTZ** keyboard, Y and Z are swapped.
- On **Dvorak**, almost nothing lines up.

So a French user who binds Chrome to "C" in the dashboard must press the key
labelled **C** — which happens to work — but one who binds it to "A" must press
the key labelled **Q**. Meanwhile the dashboard draws a **US QWERTY layout**,
so the picture on screen does not match the keyboard on the desk.

This is not a crash and it is arguably defensible (physical-position binding is
consistent), but it will read as broken to a large share of the world, and the
mismatch between the drawn keyboard and the real one is indefensible.

**Untested. No French, German, or Dvorak machine has ever run this.**

### 3.2 IME users — Chinese, Japanese, Korean — are unverified and at risk

The hook **always suppresses Space-down** (to stop auto-repeat leaking) and
injects a real space on key-up when no combo fired.

For a CJK user with an IME active, **Space is how you commit or cycle a
candidate**. Changing when the space arrives, and delivering it as injected
input carrying a `dwExtraInfo` cookie, may or may not be handled identically by
the IME candidate window.

The code passes Space through when Ctrl, Alt or Win is physically held — the
comment mentions "IME switch" — but there is no handling for *an IME candidate
window being open*, which is the case that matters.

**This has never been tested. It affects hundreds of millions of potential
users.** If it is broken, the symptom is "I cannot type in my own language",
which is the worst possible first-run experience.

### 3.3 No incompatible-device message

There is no ARM check, no Windows-version check, no "this needs WebView2"
message. Store policy **10.4.1**: *"If a product is downloaded on a device with
which it is not compatible, it must detect that at launch and display a message
to the customer detailing the requirements."*

---

## 4. Microsoft Store: what the policy actually requires

Quoted from **Microsoft Store Policies version 7.19**, effective 14 October
2025.

### 4.1 Hard blockers

**Code signing — 10.2.9.** A non-gaming Win32 product may be listed by
submitting an HTTPS download URL to its installer, subject to:

> The installer binary may only be an .msi or .exe. The binary and all of its
> Portable Executable (PE) files must be digitally signed with a code signing
> certificate that chains up to a certificate issued by a Certificate Authority
> (CA) that is part of the Microsoft Trusted Root Program.

`tauri.conf.json` currently has `"certificateThumbprint": null`. **Nothing is
signed.** This also explains the SmartScreen warning friends already see.

**Privacy policy — 10.5.1.** Not optional for this class of product:

> Product types that inherently have access to Personal Information must always
> have privacy policies. These include, but are not limited to, Desktop Bridge
> and **Win32 products**.

A global keyboard hook observes every keystroke on the machine. Even though
this app stores nothing and transmits nothing, the policy is unconditional for
Win32. The privacy policy must state plainly: keystrokes are inspected in
memory to decide typing-versus-command, nothing is recorded, nothing leaves the
device, there is no network code at all.

### 4.2 Requirements the app currently fails or has not checked

| Policy | Requirement | Status |
| --- | --- | --- |
| 10.2.9 | Installer must be **standalone**, not a stub that downloads bits when run | ⚠️ WebView2 uses `embedBootstrapper`, which downloads WebView2 at install. Ambiguous — switch to `offlineInstaller` to be safe |
| 10.2.9 | Silent install, no installer UI (UAC allowed) | ✅ `msiexec /qn` works |
| 10.2.9 | Versioned download URL, binary never changes after submission | ✅ GitHub Releases satisfies this |
| 10.2.7 | Must cleanly uninstall and remove itself | ⚠️ **The uninstaller does not remove the `Spaceadom` scheduled task.** It is deleted only by a later *launch* of the app. After uninstall it becomes an orphan that fails at every logon |
| 10.4.1 | Detect an incompatible device at launch and say so | ❌ No check exists |
| 10.4.2 | Handle exceptions, never close unexpectedly | ❌ See 1.1 — a start-up panic path exists |
| 10.1.1 | Metadata must state important limitations | ❌ The elevated-window limitation is undisclosed |
| 11.11 | Age rating via the IARC questionnaire | ⬜ Not started; trivial for a utility |
| 10.14 | Individual account is fine for a solo developer | ✅ |
| 10.2.8 | User consent before changing the Windows experience, using **supported** methods | ✅ `SetWindowsHookEx` is documented and supported. Autostart uses a scheduled task. But consent is implicit — worth an explicit first-run screen |
| 10.2.3 | Must not be malware per the Unwanted Software criteria | ⚠️ Not a violation, but a global keyboard hook in an unsigned binary is exactly the shape antivirus heuristics flag. Signing is the mitigation |

---

## 5. What professional teams do that this does not

Observations from how shipped desktop utilities of this class are built, not
from any single source:

1. **Crash reporting.** There is no way to learn that a stranger's copy died.
   A local crash log written to `%APPDATA%` that the user can attach to a bug
   report is the offline-friendly minimum, and costs nothing in privacy.
2. **A first-run screen.** State what the app does to the keyboard, that it
   will start with Windows, that it cannot see keys over administrator windows,
   and that nothing leaves the machine. This satisfies 10.2.8's consent
   requirement and pre-empts the most common misunderstanding.
3. **A panic hook.** `std::panic::set_hook` to write the panic and its location
   to the log before the process dies. Right now a panic is silent.
4. **A kill switch that is easy to find.** Space+`.` pauses, which is excellent
   — but a user who thinks the app has broken their keyboard needs to find that
   without reading documentation. The tray menu should carry it.
5. **Staged rollout.** Ship to a handful of testers on different hardware
   before the world. Two friends on 1.0.15 was the right instinct; the same
   applies at Store scale.
6. **Telemetry — deliberately declined.** This app has no network code and that
   is a genuine feature. The cost is that failures are invisible. The
   compromise is (1): a local log the user can choose to send.

---

## 6. Suggested order of work

**Before any public release:**

1. Fix the start-up panic (1.1). One-line class of change, removes a real crash.
2. Add a panic hook so future crashes leave evidence.
3. Remove the scheduled task on uninstall (10.2.7).
4. Write the privacy policy and host it. Free, and a hard Store requirement.

**Before the Store specifically:**

5. Buy a code-signing certificate and sign the binaries. This is the gate, and
   it also removes the SmartScreen warning your friends already hit.
6. Switch WebView2 to `offlineInstaller` to avoid the downloader-stub question.
7. Add the incompatible-device check and the first-run disclosure screen.
8. Disclose the elevated-window limitation in the Store description.

**Before claiming worldwide support:**

9. Test on a non-QWERTY layout — AZERTY or QWERTZ.
10. Test with a CJK IME active. This is the one that could make the app
    unusable for a huge audience, and it is completely unknown today.

---

## 7. What is genuinely solid

Worth stating, because an audit that only lists faults is misleading:

- **No hardcoded screen geometry**, monitor handling goes through the platform
  API with empty-list guards, and tiny screens are explicitly handled.
- **No admin requirement at runtime**, with a working non-admin autostart
  fallback that was observed firing on 2026-08-16.
- **Lock-poison recovery at 66 call sites** — one thread panicking cannot wedge
  the others.
- **The logger degrades gracefully** when its own folder is unwritable, which
  is a failure mode most apps die on.
- **Config is backed up and auto-restored**, with time-spaced pruning.
- **The compositing verdict is measured per machine**, not assumed.
- **Eleven unit tests**, and a documentation habit that has repeatedly caught
  wrong diagnoses — including two in this audit.
