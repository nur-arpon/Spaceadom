# Store listing — copy/paste into Partner Center

Everything below is written to be pasted as-is. Where a field has a character
limit, the count is noted.

---

## Product name

```
Spaceadom
```

## Publisher display name

```
Nur Ifran Arpon
```

Must not equal the product name — that is a named rejection cause.

---

## Short description (max 500 characters)

```
Hold the spacebar and tap a key to launch, focus or minimise any app. Space + C
for Chrome, Space + S for Spotify — twenty-six keys, your apps, one hand, no
menus. Tapping Space on its own always types a space, exactly as it should.
Hold Space for a moment and a ring appears showing every shortcut you have set.
Works everywhere in Windows, not just inside one app.
```

---

## Description

```
Spaceadom turns your spacebar into a modifier key.

Hold Space and tap a letter, and the app you bound to it launches. Tap the same
combination again and it minimises. Tap once more and it comes back. Your hands
never leave the keyboard and you never go looking through a taskbar again.

Tapping Space on its own always types a space. Always. That rule is the whole
design: a modifier you already have, that costs you nothing.

HOLD SPACE TO SEE EVERYTHING
Hold the spacebar for a moment and a ring appears around the SPACE key showing
every shortcut you have set up, so you never have to remember them.

SET IT UP BY CLICKING A KEYBOARD
The dashboard is a keyboard. Click a key, pick an app or paste a web address,
and it is bound. Drag a shortcut onto a key and that works too.

PROFILES
Three sets of bindings — Founders, Gamers, Professionals — or make your own.
Space + Right Alt cycles between them, so the same keys can mean different
things while you work and while you play.

MORE THAN LAUNCHING
  Space + Esc      Boss key: hide every window and mute the PC in one press
  Space + `        Shrink the current window into a corner, or fullscreen it
  Space + Backspace Force-quit the app in front, even when it is frozen
  Space + ,        Put the cursor where you type on this app or page
  Space + up up    Jump to the top of what you are reading
  Space + down down Jump to the bottom
  Space + scroll   Fade the window under your cursor to see what is behind it

Press any of them in the app and a card explains what it does.

IT LOOKS HOW YOU WANT
Three themes: Earthy daylight, Warcry in iron and blood, and a Starry night
that is a real night sky — constellations you can press for a fact about each,
an ocean, and a black galleon under a storm. Turn Fun mode off and it all
becomes plain and quiet. Hide the keyboard entirely and just watch the sky.

NOTHING LEAVES YOUR COMPUTER
Spaceadom has no account, no telemetry and no network code. It never contacts
a server. Everything it stores — your bindings and a local log — sits in your
own AppData folder, and the privacy policy says exactly what is in it.

WHY IT NEEDS TO WATCH THE KEYBOARD
The whole point is that Space behaves differently in every application, so
Spaceadom uses a system-wide keyboard hook. It reads key codes to decide
"typing or shortcut" in microseconds and keeps no history of what you type.
```

---

## Features (short bullets, if the form asks for them)

```
Hold Space + tap a key to launch, focus or minimise any app
Space on its own always types a space
A radial guide appears while you hold Space
Set up by clicking a picture of your own keyboard
Three profiles, switchable with Space + Right Alt
Boss key, picture-in-picture, force-quit, smart search, window fading
Three themes, including an animated night sky
Works everywhere in Windows, not just in one app
No account, no telemetry, never contacts a server
```

---

## Certification notes — REQUIRED, paste verbatim

This is the field that decides whether a manual review approves or rejects.
Do not shorten it.

```
WHAT THIS APP DOES AND WHY IT NEEDS THE PERMISSIONS IT USES

1. GLOBAL KEYBOARD HOOK
Spaceadom installs a system-wide low-level keyboard hook (WH_KEYBOARD_LL). This
is inherent to the feature: the app makes the spacebar act as a modifier key in
every application, which cannot be done from inside a single process. The hook
reads virtual key codes only, to decide within microseconds whether a press is
ordinary typing or a shortcut. It keeps one timestamp and one key code at a
time and no history whatsoever. Nothing typed is recorded, stored or
transmitted. The app has no network code and contacts no server.

2. SendInput
The app synthesises keystrokes in two situations: to replay a suppressed
spacebar press when a hold turns out to be ordinary typing, and to send an
application's own focus shortcut (for example "/" on YouTube, Ctrl+L for a
browser address bar). Every synthetic event is tagged with a private
dwExtraInfo cookie so the app never re-processes its own input.

3. CLOSING A CONFLICTING PROGRAM
Only one program on a PC can own the spacebar. Spaceadom detects other
keyboard-remapping software (PowerToys, AutoHotkey, spacedesk and similar) and
lists it in Settings. On the user's explicit request only — never
automatically, and behind two confirmations that state what will happen — it
can close one of those programs, and optionally remove that program's
start-with-Windows entry from HKCU\...\CurrentVersion\Run and the user's
Startup folder.

Constraints enforced in code (src-tauri/src/hook/conflict_close.rs):
  - It will only act on a process from a fixed built-in list of known keyboard
    conflicts. Any other process name is refused and the refusal is logged.
  - It asks the program to close normally (WM_CLOSE) before forcing it.
  - It never elevates silently. If Windows requires elevation, the standard
    Windows permission prompt is shown and the user may decline.
  - Scheduled Tasks and installed files are never touched, and no program is
    uninstalled or modified.

4. AUTOSTART
Optional, user-controlled from Settings. Uses the HKCU Run value only — the
current user's own registry hive, never machine-wide, and no elevation.

5. NO ELEVATION
The app itself never runs elevated. It installs per-user into
%LOCALAPPDATA%\Spaceadom and shows no UAC prompt at install or at any point
during normal use.

HOW TO REPRODUCE THE MAIN FEATURE
  1. Install and let the dashboard open.
  2. Click any letter key on the on-screen keyboard, choose an app, close the
     editor.
  3. Hold the spacebar for about half a second — a ring of your shortcuts
     appears.
  4. While still holding Space, tap that letter. The app launches.
  5. Repeat the combination to minimise it, and again to restore it.
  6. Tap Space on its own in any text field to confirm it still types a space.

PRIVACY POLICY
Included as PRIVACY.md and hosted at the URL given in the listing. It documents
every file written, exactly what the local log contains, and how to delete
everything.
```

---

## Age rating questionnaire

Answer **no** to all content questions — no violence, no user-generated
content, no data collection, no advertising, no in-app purchases, no location.
The one that catches people out:

- **"Does your app collect personal information?"** → **No.** Nothing leaves
  the device. The local log records which shortcut was pressed and which app
  was launched, on the user's own disk; that is documented in the privacy
  policy and is not collection.

---

## Category

**Productivity** (secondary: Utilities & tools)

## Search terms

```
spacebar, keyboard shortcut, app launcher, window switcher, remap, hotkey,
productivity, keyboard modifier, alt-tab replacement
```

## System requirements

```
Windows 10 version 1809 or later, or Windows 11. 64-bit x64 only — not ARM.
Roughly 30 MB of disk space. No internet connection required at any point.
```
