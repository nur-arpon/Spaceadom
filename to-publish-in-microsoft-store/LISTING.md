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

## Privacy policy URL

```
https://github.com/nur-arpon/Spaceadom/blob/main/PRIVACY.md
```

Partner Center requires this as a real, reachable URL, not a file upload — the
policy lives in the public repo so the same document that governs the source
governs the Store listing. Paste it into the "Privacy policy URL" field under
**Properties → Support info** (confirmed location, 2026-09-05).

---

## Support info (Properties page)

Three fields on the same **Support info** section as the privacy policy URL
above. Website and support contact are both optional for a non-Xbox app, but
cheap to fill in and expected by reviewers:

**Website**
```
https://github.com/nur-arpon/Spaceadom
```

**Support contact info**
```
https://github.com/nur-arpon/Spaceadom/issues
```

A GitHub Issues URL is an accepted form here — Microsoft's own wording is
"the URL of the web page where your customers can go for support … or an
email address," which a repo's Issues page satisfies.

---

## Short description

**Field limit, verified against Microsoft's current docs (2026-09-05,
[Add and edit Store listing info](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/add-and-edit-store-listing-info#short-description)):
1,000 characters, but only the first ~270 are shown in most Store views
before a customer has to tap to expand it — so 270 is the real target, not
1,000.** (This field used to say "max 500" — that was never the current
limit; corrected here.)

```
Hold the spacebar and tap a key to launch, focus or minimise any app. Space +
C for Chrome, Space + S for Spotify — one hand, twenty-six keys, no menus.
Space on its own always still types a space.
```

197 characters — comfortably under the 270-character display cutoff, so
nothing of it gets truncated in any Store view.

---

## Description

**Field limit: 10,000 characters of plain text** (confirmed current,
[same source as above](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/add-and-edit-store-listing-info#description)).
The text below is roughly 2,900 characters — nowhere near the ceiling. Do not paste
HTML, code, or URLs into this field; Microsoft's guidance says those belong
in the dedicated Website/Support/Privacy-policy fields instead, not the
description.

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
  Space + Tab      Picture-in-picture mode that preserves fullscreen apps
  Space + Backspace Force-quit the app in front, even when it is frozen
  Space + ,        Put the cursor where you type on this app or page
  Space + up up    Jump to the top of what you are reading
  Space + down down Jump to the bottom
  Space + scroll   Fade the window under your cursor to see what is behind it
  Space + Right Alt Cycle between your profiles (see PROFILES above)
  Space + .        Pause Spaceadom entirely; the same combo turns it back on

Press any of them in the app and a card explains what it does.

IT LOOKS HOW YOU WANT
Four themes: Earthy daylight, Warcry in iron and blood, a Starry night that is
a real night sky — constellations you can press for a fact about each, an
ocean, and a black galleon under a storm — and Auto, which follows Windows'
own light/dark setting and switches within seconds when you change it, no
restart needed. Auto is the default on a fresh install. Turn Fun mode off and
it all becomes plain and quiet. Hide the keyboard entirely and just watch the
sky.

YOUR DATA STAYS ON YOUR COMPUTER
Spaceadom has no account and no analytics — nothing watches how you use it.
Everything it stores — your bindings and a local log — sits in your own
AppData folder, and the privacy policy says exactly what is in it. The only
things that ever leave your machine are an optional crash report if something
goes wrong (one click to turn off) and, outside the Store, a daily check for a
newer version; neither ever contains what you type or what you have bound.

WHY IT NEEDS TO WATCH THE KEYBOARD
The whole point is that Space behaves differently in every application, so
Spaceadom uses a system-wide keyboard hook. It reads key codes to decide
"typing or shortcut" in microseconds and keeps no history of what you type.
```

---

## Product features

**Field limits, confirmed:** up to **20** features, **200 characters** each,
no leading bullets (the Store adds its own). The nine below are the longest
at 74 characters — plenty of headroom to add more later without touching
these.

```
Hold Space + tap a key to launch, focus or minimise any app
Space on its own always types a space
A radial guide appears while you hold Space
Set up by clicking a picture of your own keyboard
Three profiles, switchable with Space + Right Alt
Boss key, picture-in-picture, force-quit, smart search, window fading
Four themes, including an animated night sky and an auto light/dark mode
Works everywhere in Windows, not just in one app
No account, no analytics — an optional crash report is the only thing sent
```

## What's new in this version

**Leave this field blank.** Microsoft's guidance is explicit: "if this is
the first time you're submitting your app, leave this field blank." Fill it
in starting with the *second* submission (1,500-character limit, confirmed).

## Copyright and trademark info

Optional field; not required, but cheap to fill in and removes an easy
future dispute:

```
© 2026 Nur Ifran Arpon. Spaceadom and its logo are trademarks of Nur Ifran
Arpon. All rights reserved.
```

---

## Certification notes — REQUIRED, paste verbatim

This is the field that decides whether a manual review approves or rejects.
Do not shorten it.

**Where this actually goes, corrected 2026-09-05 after reading the current
wizard docs:** both on the **Submission options** page — not the Packages
page. Partner Center shows two separate boxes there once it detects the
package's `runFullTrust` declaration: a general **"Notes for certification"**
box, and a **"Restricted capabilities"** box specific to `runFullTrust` that
only appears because the capability was detected. Paste this entire block
into the **Restricted capabilities** box (it exists to justify exactly this:
"why your app needs to declare the capability and how it is used," in
Microsoft's own words) — and if the wizard also offers a general notes box,
paste it there too, or at minimum add a one-line pointer ("see restricted
capability justification above") so a reviewer moving between boxes doesn't
miss it. No character limit for this field is stated in Microsoft's current
docs; **verify in the wizard** — if it truncates, keep sections 1 and 4
(the keyboard hook and the conflict-closing) first, since those are the two
a reviewer is most likely to stop on.

```
WHAT THIS APP DOES AND WHY IT NEEDS THE PERMISSIONS IT USES

1. GLOBAL KEYBOARD HOOK
Spaceadom installs a system-wide low-level keyboard hook (WH_KEYBOARD_LL). This
is inherent to the feature: the app makes the spacebar act as a modifier key in
every application, which cannot be done from inside a single process. The hook
reads virtual key codes only, to decide within microseconds whether a press is
ordinary typing or a shortcut. It keeps one timestamp and one key code at a
time and no history whatsoever. Nothing typed is recorded, stored or
transmitted. The only network use is an opt-out crash reporter (Sentry) that
receives anonymised crash reports with file paths, URLs and e-mail addresses
redacted before sending; the Store build never checks for updates itself
(the Store owns updates). Everything is documented in the privacy policy.

2. A PAGE-SIDE KEYBOARD FALLBACK, WHILE THE APP'S OWN WINDOW HAS FOCUS
Windows does not deliver WH_KEYBOARD_LL callbacks to a process while that
process's own window is the one in focus. So while Spaceadom's own dashboard
window is focused, its web page also runs an ordinary in-page keydown/keyup
listener that reimplements the same tap/hold/combo logic and feeds it to the
same engine — this is the only place the app reads keyboard input any way
other than the global hook, and it only ever sees keys typed while the app's
own window is focused, exactly like any other application. A private,
timestamp-based dedupe with the hook guarantees only one of the two paths
ever acts on a given keypress. One user-visible difference exists: because
this path cannot suppress-and-replay a keystroke the way the native hook
does, releasing a long Space-hold with no combo inside one of the app's own
text fields does not re-type a space (the hook path always does). This is
documented in the app's own source comments as a known, deliberate
divergence, not an oversight.

3. SendInput
The app synthesises keystrokes in two situations: to replay a suppressed
spacebar press when a hold turns out to be ordinary typing, and to send an
application's own focus shortcut (for example "/" on YouTube, Ctrl+L for a
browser address bar). Every synthetic event is tagged with a private
dwExtraInfo cookie so the app never re-processes its own input.

4. CLOSING A CONFLICTING PROGRAM
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

5. AUTOSTART
Optional, user-controlled from Settings. In this Store package it is the
manifest's windows.startupTask (visible and controllable in Settings > Apps >
Startup); the app never writes a Run value or a scheduled task when packaged.

6. NO ELEVATION
The app itself never runs elevated and shows no UAC prompt at any point
during normal use.

7. PROCESS LAUNCHES (why the package references cmd.exe / powershell.exe)
Spaceadom is an app launcher: it starts the programs, files and links the
user bound to keys (CreateProcess / ShellExecute). It also runs PowerShell
once in the background to list the Start-menu shortcuts for the app picker,
and cmd.exe only to open a folder or restart itself after an update. It
never runs scripts from the network, never modifies the registry outside its
own HKCU keys, and never touches other software except the single
user-confirmed "close a conflicting keyboard program" action described in 4.

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
Included as PRIVACY.md and hosted at
https://github.com/nur-arpon/Spaceadom/blob/main/PRIVACY.md, the same URL
entered in the listing's Privacy policy URL field. It documents every file
written, exactly what the local log contains, what the crash reporter and the
update checker each send (the Store build never runs the update checker —
Store handles updates itself), and how to delete everything.
```

---

## Age rating questionnaire

**This is a live IARC (International Age Ratings Coalition) questionnaire
embedded in Partner Center, not a Microsoft-only form** — confirmed from
Microsoft's current age-ratings doc (2026-09-05). It shares your publisher
display name and email with IARC, and the first question asks which category
best describes the app; answer with whichever option is closest to
"utility / productivity tool," then answer every follow-up **no**: no
violence, no user-generated content, no chat or social features, no data
collection, no advertising, no in-app purchases, no location, no gambling.
That combination lands at the lowest available rating (historically "3+" /
PEGI 3-equivalent) for a content-free utility. Once submitted, the same
rating carries forward to every future update automatically — you only retake
it if a later version's content changes what these answers say.

The one question that catches people out:

- **"Does your app collect personal information?"** → **Yes.** The app
  collects diagnostic crash data: error messages, stack traces, file paths,
  and line numbers from Spaceadom's own source — all with personal identifiers
  (Windows usernames, system paths, URLs unrelated to the app) redacted before
  sending to Sentry. No personal data is ever intentionally recorded or shared
  with third parties beyond the crash-reporting processor, and the user can
  opt out entirely with the "Don't send logs" setting (off by default, which
  means reporting is on by default). The app has no accounts, no analytics, no
  advertising, and no third-party sharing except for crash data to Sentry. The
  local log records which shortcut was pressed and which app was launched, on
  the user's own disk; that is documented in the privacy policy. See the
  Privacy policy URL for the complete list of what the app collects locally
  and what may leave the device. Note, though, that Partner Center may
  **override this answer automatically** based on the capabilities detected in
  the package — if that happens, the privacy policy URL on the Properties page
  already documents the exact data flow.

---

## Category

**Primary: Productivity** (no subcategory exists under Productivity —
Microsoft's category table lists it with subcategory "(None)"). **Secondary:
Utilities + tools** (the exact name in the Store's 2026 category list is
"Utilities + tools", not "Utilities & tools" — corrected here).

**Why Productivity, not Utilities + tools, as primary:** Microsoft's own
one-line definitions ([Categories and subcategories](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/categories-and-subcategories),
fetched 2026-09-05) describe Productivity as "apps which help user to
complete a particular task more efficiently" — which is the entire pitch —
versus Utilities + tools as "apps which assist user in solving problems or
completing specific tasks," illustrated with file managers, calculators and
barcode scanners. Spaceadom is closer to the first description than the
second, so it leads there and takes Utilities + tools as the secondary
category instead of the reverse.

## Search terms

**Field limit, confirmed: 7 unique terms maximum.** The list this file
carried before had nine — over the limit, and would have been silently
truncated or rejected. Trimmed to seven, dropping the two most redundant
with terms already on the list ("keyboard shortcut" overlaps "keyboard
modifier" and "hotkey"; "remap" overlaps the same two):

```
spacebar, app launcher, window switcher, hotkey, productivity, keyboard
modifier, alt-tab replacement
```

A per-term character limit is not stated anywhere in Microsoft's current
docs found during this pass — **verify in the wizard** if it complains about
any single term's length; none of the seven above are longer than 19
characters, which should be safe under any historical limit.

## System requirements

**Correction, 2026-09-05: the minimum OS version is not something you type
into a Properties-page text field for an MSIX submission.** It comes
straight from the uploaded package's own manifest —
`src-tauri/msix/AppxManifest.xml`'s `<TargetDeviceFamily MinVersion=
"10.0.17763.0" .../>`, which is **Windows 10 version 1809**, not 2004 (build
19041) as an earlier draft of this document assumed. Partner Center reads
that from the package; there is nothing to reconcile by hand as long as the
manifest and this text agree, which they now do.

What the **Properties → System requirements** page actually asks for
instead is a table of optional hardware declarations (memory, DirectX,
graphics, and similar) with **Minimum**/**Recommended** checkboxes per item —
confirmed from Microsoft's current docs, though the exact list of hardware
items (whether "Keyboard" is one of them) wasn't visible in the fetched page
text, so **verify in the wizard**. If a "Keyboard" item exists, check it
under **Minimum hardware** — the entire feature is unusable without a
physical keyboard, and there is no touch or mouse-only path. Leaving the
whole section blank is also fine (it's optional); it just means the Store
won't show a hardware warning to a customer on an unsuitable device.

The Store listing's own **"Additional system requirements"** supplemental
field (Properties page → Additional system requirements, under Minimum
hardware) is structured as **up to 11 short bullet items, 200 characters
each** — confirmed — not one free-text paragraph, so here it is split into
items that actually fit that shape:

```
Windows 10 version 1809 or later, or Windows 11
64-bit (x64) only — not ARM
Roughly 30 MB of disk space
No internet connection required for any feature to work
Only reaches the network for an optional crash report (off in one click) and, outside the Store build, a daily update check
```

If the wizard instead offers one free-text box (older UI, or a different
field than the one described above), the single paragraph below reads fine
there:

```
Windows 10 version 1809 or later, or Windows 11. 64-bit x64 only — not ARM.
Roughly 30 MB of disk space. No internet connection is required for any
feature to work — Spaceadom only reaches the network to send an optional
crash report (off in one click, see PRIVACY.md) and, outside the Store build,
to check for updates once a day.
```
