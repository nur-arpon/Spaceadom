# Spaceadom

*Source-visible, proprietary — see [LICENSE](LICENSE).*

**Hold Space, tap an app's initial — it opens. Tap it again — it's gone.
Tap Space on its own and it types a space, always.**

Spaceadom turns the spacebar into a modifier key for launching, focusing and
minimising applications, without taking anything away from typing. It is a
native Windows application: a Rust core with a system-wide keyboard hook, a
Tauri v2 shell for the dashboard, and an on-demand transparent overlay for the
radial guide and toasts.

Windows 10/11, x64 — and, since 1.0.113, Windows on ARM64 (`*_arm64-setup.exe`, built and signed but not yet run on ARM hardware). No account, no analytics, no ads.

---

## Install

Download the latest **`Spaceadom_*_x64-setup.exe`** from
[Releases](../../releases) and run it. This is the **recommended** installer:
it installs into your own user folder, never asks for an admin password, and
is the one that keeps itself updated (see "Updates" below).

Windows will warn that the publisher is unknown — the build isn't code-signed.
Choose **More info → Run anyway**.

A **`.msi`** is also published on every release, for anyone whose workplace
requires that installer format. It installs for the whole PC and asks for
admin once. **Don't run the `.msi` if you already installed with `setup.exe`**
— it installs itself into wherever Spaceadom already lives, and Windows then
believes those files belong to the `.msi`; uninstalling that entry later
deletes them. Full detail, including a correction to an earlier version of
this warning, is in
[`all-versions/WHAT-CHANGED.md`](all-versions/WHAT-CHANGED.md). Install one,
never both.

**Microsoft Store:** submitted (1.0.109, in certification as of 17 September
2026). The MSIX package and the listing live in
[`to-publish-in-microsoft-store/`](to-publish-in-microsoft-store/).

That's it. Spaceadom starts with Windows and lives in your tray.

---

## Portable

Don't want an installer touching your machine at all? Download
**`Spaceadom_*_x64-portable.zip`** instead, unzip it anywhere, and run
`spaceadom.exe` directly. Nothing is written to Program Files, the registry,
or Task Scheduler — a `data` folder appears next to the exe on first run and
that is the ONLY place Spaceadom ever writes: your config, the log, your
rolling backups, the app-picker cache. Move the folder and your settings move
with it; delete the folder and Spaceadom, and every trace of it, is gone.

Two trade-offs, both a direct consequence of having no installer: a portable
copy does **not** start with Windows on its own (add a shortcut to
`spaceadom.exe` in your own Startup folder — `Win+R` → `shell:startup` — if
you want that), and it does **not** update itself (grab a newer zip from
[Releases](../../releases) when one comes out). It also needs the WebView2
runtime already on the machine, same as every other build here, but with no
installer step to fetch it if it's missing — see the zip's own
`README-portable.txt` for the one-time fix if the window never appears.

Don't run a portable copy alongside an installed one (`setup.exe` or `.msi`)
on the same machine — both hook the spacebar, and the dashboard's "second
copy found" banner applies here exactly as it does to two installed copies.

---

## How it works

- **Hold Space, tap a letter** — the app bound to that letter launches, comes
  to the front if it's already open, or minimises if it's already in front.
- **Hold Space for a moment** and a radial guide appears over the keyboard
  showing every shortcut you've set, so you never have to memorise them.
- **Hold the middle mouse button** and a ring of your apps' own icons blooms
  around the cursor — release on one to launch it. Near a screen edge or
  corner the ring becomes an arc that stays on screen, centred on the cursor.
  Choose which apps ("Favourites", up to 13) or show all of them, as rings
  or as a golden-angle spiral.
- **Tap Space on its own and it always types a space** — exactly as it always
  has. That rule is the whole design: a modifier you already have, that costs
  you nothing.

**The first time you open it, Spaceadom walks you through one binding** — pick
a letter, give it something to open, then hold Space and tap it. It shows up
once; Settings has **"Show me the walkthrough"** as the way back in.

Bindings are edited from the dashboard — click any key on the on-screen
keyboard and pick an app, or paste a URL. Drag a shortcut onto a key and that
works too.

### Every shortcut

| Gesture | What happens |
| --- | --- |
| `Space` + letter | Launch the app, focus it if it's already open, minimise it if it's already focused |
| `Space` + `Right Alt` | Cycle profiles — Founders → Gamers → Professionals |
| `Space` + `Esc` | Boss key: hide everything and mute the PC |
| `Space` + `` ` `` | Picture-in-picture cycle |
| `Space` + `Tab` | Picture-in-picture that preserves fullscreen apps |
| `Space` + scroll | Fade the window under the cursor |
| `Space` + `.` | Pause Spaceadom |
| `Space` + `,` | Smart Search — put the cursor where you type on this app or page |
| `Space` + `;` | Voice typing — Windows dictation, typed wherever the cursor is |
| `Space` + `/` | Screenshot — Windows' region snip (drag a rectangle, or pick window / full screen from its bar) |
| `Space` + `'` | On-screen keyboard — Windows' own; press again to hide it |
| Middle mouse button, held | The icon ring at the cursor; release on an app to launch it |
| `Space` + `⌫` | Force close the app in front, even when it is frozen |
| `Space` + `↑↑` | Jump to the top of what you are reading |
| `Space` + `↓↓` | Jump to the bottom |
| `Space` alone | A space. Always. |
| One finger, from a touchpad edge inward | Change something along that edge — left edge brightness, right edge volume, top edge video scrub |

Every one of these is also explained inside the app: the row along the bottom
of the dashboard is pressable, and so is each of those keys on the on-screen
keyboard.

**Touchpad edges.** On a laptop with a Precision Touchpad, Spaceadom can turn
the edges of the pad into sliders: start one finger *inside* a strip along an
edge and slide, and it changes brightness (left), volume (right) or scrubs a
video (top) — the further and faster you slide, the more it moves. A finger
that starts anywhere in the middle just moves the pointer as it always did, and
while an edge slide is happening the pointer holds still so nothing jumps.
Everything is off until you switch it on from the touchpad page — the small
touchpad drawn under the keyboard on the home screen, or Settings. Lift your
finger to stop; slide back to undo. Every edge can also send any shortcut
you record — one for each direction — a step at a time as you slide.

---

## Features

*(The full non-negotiable contract these are drawn from is
[`CORE_AIM.md`](CORE_AIM.md).)*

- **Smart Cascade** — Space + a letter launches, focuses, or minimises an app
  in a reliable, repeatable loop, using native window-handle caching and OS
  focus forcing, not a guess.
- **Typing protection** — tapping Space always inserts a space; holding it
  never leaks a repeat; fast typing that clips the edge of a hold still comes
  through as ordinary text, never a misfired shortcut.
- **Three profiles** (Founders, Gamers, Professionals, or your own), switched
  with Space + Right Alt, so the same 26 keys mean different things at work
  and at play.
- **Boss Key** (Space + Esc) — minimises every window and mutes the system in
  one press natively; the same combination again restores and unmutes.
- **A visual guide, not a cheat sheet you have to remember** — hold Space and
  a glassmorphism HUD shows the current profile's bindings; every action also
  gets a toast confirming what just happened.
- **Four themes** — Earthy daylight, Warcry, a Starry night that is a real
  animated night sky (moon phases, 20 constellations, a storm, a rigged
  galleon), and Auto, which follows Windows' own light/dark setting — or
  turn Fun mode off for something plain and quiet.
- **Self-diagnosing.** The keyboard hook is watched by a witness hook and a
  raw-input clock; if Windows ever evicts it, the app notices and repairs it
  within seconds, and the log says exactly why.
- **Set up by clicking a keyboard**, not by editing a config file — the
  dashboard *is* the on-screen keyboard; click a key, pick an app or paste a
  URL, done.

---

## Privacy

Spaceadom's local files — your bindings and a debug log — stay on your own
computer. Its only network activity is an optional crash report (one click to
turn off) and, outside the Microsoft Store build, a daily check for a newer
version. Neither ever contains what you type or what you've bound.

The full policy — exactly what each file and each network request contains,
and how to delete everything — is
**[PRIVACY.md](https://github.com/nur-arpon/Spaceadom/blob/main/PRIVACY.md)**.

---

## Updates

**Spaceadom updates itself.** Every `setup.exe`/`.msi` install checks once a
day for a newer release, downloads it, verifies its signature, and installs it
silently in the background — no prompt, no admin password. You'll see
"Updated to …" for a few seconds next time you open the dashboard. To pin a
version instead, set `"auto_update": false` in
`%APPDATA%\Spaceadom\config.json`.

**If you install from the Microsoft Store**, the Store handles updates the
normal Store way, and Spaceadom's own updater doesn't run at all.

See [PRIVACY.md](https://github.com/nur-arpon/Spaceadom/blob/main/PRIVACY.md#checking-for-updates)
for exactly what the update check does and does not send.

---

## Typing speed matters

Spaceadom has to tell "I'm typing" from "I'm giving a command", and the only
thing separating them is **how long Space is held before the next key**.

Set your speed under **Settings → Typing speed**. If shortcuts ever fire in the
middle of a sentence, choose a *slower* setting — that widens the window and
pushes ordinary typing further from the threshold. If shortcuts feel
unresponsive, choose a *faster* one.

The default suits most people. It is deliberately conservative: a false launch
mid-sentence is far more annoying than holding Space a fraction longer.

---

## If the guide or the toasts stop appearing

Some graphics drivers can't composite the transparent overlay this app uses.
Everything still works — apps launch, sounds play — but nothing is drawn.

Spaceadom detects this and switches to software rendering by itself. If it
doesn't, turn on **Settings → Software overlay** and restart.

---

## Build from source

Requires [Rust](https://rustup.rs/), [Node 20+](https://nodejs.org/), and the
WebView2 runtime (already on Windows 11).

```bash
npm install
npm run build         # tsc + vite — must run before any cargo command
npm run tauri dev      # run it
npm run tauri build    # installers -> src-tauri/target/release/bundle/
npm run arm64          # Windows on ARM64 -> src-tauri/target/aarch64-pc-windows-msvc/release/bundle/
```

The ARM64 build needs `rustup target add aarch64-pc-windows-msvc` and the
"MSVC v143 – VS 2022 C++ ARM64 build tools" component of Visual Studio Build
Tools. TLS is Windows' own Schannel on every target, so no C crypto is
compiled.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full environment, the checks a
change needs to pass, and this project's documentation rules.

---

## Licence

**Source-visible, proprietary** — see [LICENSE](LICENSE). You may read and
build this source for personal, non-commercial use and evaluation. No
redistribution of source or binaries, no selling, no derivative products. This
is not an OSI open-source licence. Third-party components keep their own
licences — see [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md). Official
binaries only come from this repository's
[Releases](../../releases) and, once published, the Microsoft Store.

---

## Support

- **Bugs:** [open an issue](../../issues/new/choose) — the template asks for
  exactly what gets a Spaceadom bug fixed fast (version, installer, Windows
  build, `debug.log`).
- **Questions and ideas:** [Discussions](../../discussions).
- **Security issues:** see [SECURITY.md](SECURITY.md) — please don't file
  those as a public issue.

---

## Project history

This is a rewrite. The original
[SpaceToggle OS](https://github.com/nur-arpon/SpaceToggle-OS) was AutoHotkey +
PowerShell; Spaceadom is a native Rust application with the same idea and a new
interface.

- **[What changed in each version](all-versions/WHAT-CHANGED.md)** — plain
  English, one or two lines per release
- **[PROJECT_STATUS.md](PROJECT_STATUS.md)** — the development log, newest
  first
- **[V14_FIXES_AND_CODE.md](V14_FIXES_AND_CODE.md)** — every bug with its root
  cause and the exact code that fixed it, including the diagnoses that turned
  out to be wrong

Those last two files are unusual for a public repository, and they are
published on purpose and unedited. Most of the hard problems here were things
that **failed silently** — an overlay that reported itself visible while
painting nothing, a list that stopped at 60 items, a confirmation dialog that
never rendered, a watchdog that logged 260 errors without catching a single
real fault, a "proven deaf" detector that was measuring the touchpad. Each is
written up with what was tried, what turned out to be wrong, and the exact
code that fixed it, so the next person doesn't have to rediscover them. Read
them as an engineering journal, not as documentation: the polished reference
material is [`CORE_AIM.md`](CORE_AIM.md), [`DEVELOPER-GUIDE.md`](DEVELOPER-GUIDE.md)
and [`NATIVE_SAFETY.md`](NATIVE_SAFETY.md).
