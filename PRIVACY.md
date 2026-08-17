# Privacy Policy — Spaceadom

**Last updated: 17 August 2026**

Spaceadom is a keyboard utility for Windows. To do its job it has to watch your
keyboard, so it is reasonable to want a straight answer about what happens to
what you type.

**Spaceadom has no network code. It cannot send anything anywhere, because
there is nothing in it that talks to the internet.** There are no accounts, no
servers, no analytics and no telemetry. Everything below concerns files kept on
your own computer, which only you can read.

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

### Backups

`%LOCALAPPDATA%\SpaceadomBackups` holds automatic copies of `config.json` so
that a corrupted settings file can be recovered. Same contents as above, older.

---

## Who can see any of this

Only you, and anyone who can already use your Windows account. These are
ordinary files in your user profile with no special permissions.

Nothing is uploaded. Nothing is shared. Spaceadom has no publisher-side
infrastructure of any kind.

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

- There is no HTTP client, no socket code and no networking crate anywhere in
  the dependency tree. Verified 2026-08-17 with `cargo tree`: no `reqwest`,
  `hyper`, `ureq`, `curl`, `rustls`, `native-tls` or `openssl`. The frontend
  contains no `fetch`, `XMLHttpRequest` or `WebSocket` call. The one exception is at **install** time, where
  Microsoft's own WebView2 bootstrapper may download the WebView2 runtime if
  your copy of Windows does not already include it. That is a Microsoft
  component, downloaded from Microsoft, and Spaceadom itself never uses it to
  send anything.
- The keyboard hook lives in `src-tauri/src/hook/mod.rs`. It holds one
  timestamp and one key code at a time and keeps no history.
- Everything written to disk goes through `src-tauri/src/config/mod.rs` and
  `src-tauri/src/logger.rs`. There are no other writers.
