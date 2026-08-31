# Privacy Policy — Spaceadom

**Last updated: 26 August 2026 (version 1.0.82).**

Spaceadom is a keyboard utility for Windows. To do its job it has to watch your
keyboard, so it is reasonable to want a straight answer about what happens to
what you type.

**Spaceadom sends one kind of thing, and only one: a report when it crashes or
hits an error.** There are no accounts, no analytics, and nothing that watches
how you use the app. Everything else in this document concerns files kept on
your own computer, which only you can read.

> **What changed, and why this document no longer says what it used to.**
> Until version 1.0.81 this page said Spaceadom had no network code at all and
> could not send anything anywhere. That was true then and it is not true now,
> so the sentence is gone rather than softened. Version 1.0.82 added crash
> reporting. The section below says exactly what it sends and how to switch it
> off.

---

## Crash and error reports

**What it is.** When Spaceadom crashes, or hits an error it can recover from
but should not have hit, it sends a report to **Sentry** — a third-party
crash-reporting service used by a large number of desktop and mobile
applications. Their privacy policy is at <https://sentry.io/privacy/>.

**Why.** Spaceadom runs on other people's machines, and a crash there used to
be invisible unless that person found `%APPDATA%\Spaceadom\debug.log` and
emailed it. Almost nobody does. This is the difference between a bug that gets
fixed and a bug that stays.

**What a report contains:**

- The error message, and the file and line number in Spaceadom's own source
  where it happened.
- The call stack — which of Spaceadom's functions were running at that moment.
- Which version of Spaceadom, which version of Windows, and basic machine
  facts (processor architecture, amount of memory).

**What "an error" means precisely.** Three things, and nothing else:

1. A crash — the program hitting a fault it cannot continue past.
2. A log line written at **ERROR** level — a failure Spaceadom survived but
   should not have had, for example "could not read the config file". This now
   includes a failure in Spaceadom's own on-screen interface: a JavaScript
   error in the dashboard or in the little pop-up overlay. Those used to stay
   on your machine, which meant "the window looks broken" was the one problem
   that could never be reported.
3. **A short, fixed list of "still running, but half of it has stopped
   working" conditions** — the states where Spaceadom keeps going and you
   cannot necessarily tell that something is wrong. The list is exactly five,
   it is written out in the source (`Degraded` in `src-tauri/src/telemetry.rs`)
   and nothing joins it without being added there by hand:
   - Spaceadom's keyboard hook has gone deaf — keys are reaching Windows and
     Spaceadom is no longer seeing them, so every shortcut is silently dead.
   - The pop-up overlay was on screen and drew nothing.
   - Windows' graphics compositing was measured as dead on this machine.
   - The overlay was switched off, so the pop-up and its sounds are suppressed.
   - The overlay could not be rebuilt after a display change.

   Each of these is sent **at most three times per run of the app, and at most
   once every fifteen minutes**, so a machine that is broken says so once
   rather than hundreds of times.

Spaceadom's log also records ordinary activity, other warnings, and diagnostic
detail. **None of that is sent.** Not which shortcuts you pressed, not which
apps you opened, not your screen layout, not the names of other programs
running on your machine — all of which the local log file *does* contain, and
all of which stay on your machine. In particular the routine warnings about
other keyboard software being installed alongside Spaceadom, which are the most
common lines in the log, are **not** sent.

**What is never sent:** what you type; your keystrokes; your profiles or
bindings; the paths of the apps you have bound; your Windows username; your
name or email address. Spaceadom explicitly turns off the option in Sentry's
software that would attach a username or machine name to a report.

Because an error can quote text that Spaceadom was working with when it failed,
**every report is scrubbed before it is sent**: any Windows file path
(`C:\Users\…`, `\\server\share`) becomes `<path>`, and any web address that is
not Spaceadom's own internal one becomes `<url>`. Reports that come from the
interface get a second pass first, in which anything the interface had quoted —
a profile name, for instance — becomes `<redacted>` before the report is even
handed over. What survives is the fault itself and the location inside
Spaceadom's own bundled code.

*(Corrected 2026-08-31. This paragraph used to say every report was scrubbed
"twice", with the `<redacted>` pass described as applying to all of them. The
second pass only ever ran on interface errors, and one route — a crash inside
Spaceadom's own Rust code — was reaching the reporting service with no scrub at
all. That route is scrubbed as of this correction; the promise now describes
what each route actually does.)*

**How to switch it off.** Settings → the last switch, at the very bottom:
**"Don't send logs."** Turn it on and nothing leaves your machine — no reports,
no connections, nothing queued for later. It takes effect immediately; you do
not need to restart Spaceadom.

It is off by default, which means reports are on by default. If you would
rather it did not, the switch is one click and Spaceadom works exactly the same
either way.

**Where to check this yourself:** `src-tauri/src/telemetry.rs`. The severity
floor is the constant `SENTRY_MINIMUM_LEVEL` (set to `Error`), and the switch
is the `SENDING_ENABLED` flag it checks before anything is sent.

---

## What Spaceadom does with your keystrokes

Spaceadom asks Windows to show it every key press before other applications see
them. This is a standard, documented Windows feature (`SetWindowsHookEx` with
`WH_KEYBOARD_LL`), and it is the only way an application can turn the spacebar
into a modifier.

For each key, Spaceadom decides one thing: **was that typing, or a shortcut?**
The decision is made in memory in a fraction of a millisecond, and then the key
is either passed on untouched or acted upon.

**Spaceadom does not record what you type.** Your keystrokes are not written to
disk, not kept in memory after the decision, and not transmitted. There is no
keystroke history anywhere in the program.

---

## What Spaceadom stores on your computer

Two files, both on your machine only.

### `%APPDATA%\Spaceadom\config.json`

Your settings. This contains:

- Your profiles and which key is bound to which application.
- **The full path of each application you bind**, for example
  `C:\Users\<your name>\AppData\Local\Discord\app-1.0.9253\Discord.exe`. These
  paths usually contain your Windows username.
- Website addresses for any key you bind to a URL.
- Small pictures (icons) of the applications you have bound.
- Your preferences: theme, sound, typing speed, and so on.

### `%APPDATA%\Spaceadom\debug.log`

A record of what the program decided, so that a problem can be diagnosed. **It
is worth being specific about this, because it does contain information about
you:**

- Which shortcuts you pressed and when — for example
  `engine: combo Space+d received`.
- Which applications were launched, focused or minimised, **including their
  full paths**.
- Your screen resolution and how many monitors are attached.
- The names of other keyboard software running on your machine.

It does **not** contain what you typed. Only which of *your own* shortcuts
fired, and what the program did about it.

The log is capped at two files of 5 MB and older entries are discarded
automatically.

**The whole log file is never sent anywhere.** The crash reporting described
above takes only the ERROR-level lines and the crashes — a small fraction of
what is in this file — and only while "Don't send logs" is off. The four kinds
of information listed just above (your shortcuts, your app paths, your screen
layout, other programs) are written at INFO and WARN level, so none of them are
included. If you want the developer to have the full log, you still have to
send it yourself, deliberately.

### Backups

`%LOCALAPPDATA%\SpaceadomBackups` holds automatic copies of `config.json` so
that a corrupted settings file can be recovered. Same contents as above, older.

---

## Who can see any of this

Only you, and anyone who can already use your Windows account. These are
ordinary files in your user profile with no special permissions.

None of these files is uploaded. Nothing in this section is shared. The one
thing that does leave your machine is the crash and error report described at
the top of this page, which is not built from these files — and even that stops
completely when "Don't send logs" is switched on.

**If you send a log file to the developer to report a bug**, you are choosing
to share the contents described above. Please read it first if that matters to
you — it is a plain text file. Nothing is sent unless you send it yourself.

---

## How to delete everything

Uninstalling removes the program and its startup entry, but deliberately leaves
your settings behind, so that reinstalling does not cost you every binding you
have set.

To remove your data as well, delete these two folders after uninstalling:

```
%APPDATA%\Spaceadom
%LOCALAPPDATA%\SpaceadomBackups
```

Paste either path into the address bar of any File Explorer window to go
straight there.

---

## Children

Spaceadom is a general-purpose keyboard utility. It is not directed at
children, and it collects no information from anyone, of any age.

---

## The one thing Spaceadom can change outside its own folder

Added 2026-08-20, with version 1.0.63. It is the only capability in this app
that reaches beyond Spaceadom's own files, so it is spelled out in full.

**What it is.** Only one program on a PC can own the spacebar. If another
keyboard program — PowerToys, AutoHotkey, spacedesk and similar — is running,
Spaceadom lists it under Settings → Conflicts. Pressing that entry offers to
close it for you.

**What it can do, exactly:**

- **"Close it now"** ends that program. It asks the program to close properly
  first (the same request Windows sends when you click a window's ✕), and only
  forces it if that is refused. If the program runs with administrator rights,
  Windows shows you its own permission prompt — Spaceadom cannot get past that
  and does not try to.
- **"Close it and stop it from restarting"** does the above, and additionally
  removes that program's start-with-Windows entry from two places: your own
  `HKCU\…\CurrentVersion\Run` registry key, and your Startup folder. It tells
  you which entries it removed. Nothing else on your system is touched — in
  particular, Scheduled Tasks are deliberately left alone, and no program is
  uninstalled or modified.

**What it will not do:**

- It never acts on its own. Both actions need two presses and a confirmation
  that states what will happen.
- It will only ever act on a program from its own built-in list of known
  keyboard conflicts. A request naming any other program is refused, and the
  refusal is written to the log.
- It never elevates silently. If Windows requires permission, you see the
  standard Windows prompt and can decline it.

**Where to check this yourself:** `src-tauri/src/hook/conflict_close.rs`. The
list of programs it will act on is `KNOWN` in `src-tauri/src/hook/conflicts.rs`.

## Changes to this policy

If a future version of Spaceadom ever collects or transmits anything, this
document will be updated before that version is released, and the change will
be stated plainly in the release notes. The change history is public at
<https://github.com/nur-arpon/Spaceadom>.

---

## Contact

Questions about this policy, or about anything Spaceadom does with your data:

- Open an issue at <https://github.com/nur-arpon/Spaceadom/issues>

---

## One thing that is not data collection, but looks adjacent

Some keys are bound to websites — Space+G opens Gmail, for example. When you
press one, Spaceadom asks **your own browser** to open that address, exactly as
if you had clicked a bookmark. Spaceadom does not fetch the page, does not see
its contents, and is not involved after the browser takes over. Whatever
happens next is between you and that website, under their privacy policy and
your browser's.

---

## For the technically inclined

The claims above are checkable, which is the point of publishing the source:

- **There is exactly one thing in Spaceadom that talks to a network**, and it
  is the crash reporter: the `sentry` crate, at `src-tauri/src/telemetry.rs`.
  It is the only reason `reqwest`, `hyper` and `rustls` appear in the
  dependency tree, which they did not before 1.0.82. Nothing else in the Rust
  code opens a socket, and the frontend still contains no `fetch`,
  `XMLHttpRequest` or `WebSocket` call at all.
- **What that one thing can send is bounded in the source, not by policy.**
  `telemetry::log_filter` is the single function every log record passes
  through on its way to Sentry, and it returns `Ignore` — a drop, not a queue —
  for anything below `SENTRY_MINIMUM_LEVEL`, which is `log::Level::Error`, and
  for everything whatsoever when `SENDING_ENABLED` is false. The switch flips
  that flag directly, so "off" is enforced at the last possible moment before
  a record could become a report, not by a setting checked somewhere earlier.
- There is one other network event, at **install** time: Microsoft's own
  WebView2 bootstrapper may download the WebView2 runtime if your copy of
  Windows does not already include it. That is a Microsoft component,
  downloaded from Microsoft, and Spaceadom itself never uses it to send
  anything.
- The keyboard hook lives in `src-tauri/src/hook/mod.rs`. It holds one
  timestamp and one key code at a time and keeps no history.
- Everything written to disk goes through `src-tauri/src/config/mod.rs` and
  `src-tauri/src/logger.rs`. There are no other writers.
