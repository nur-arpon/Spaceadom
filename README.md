# Spaceadom 🚀

**Hold Space, tap a key, and the app you want is there.**

Space + `c` opens Chrome. Press it again and Chrome minimises. Space + `w` for
WhatsApp, Space + `s` for Spotify — twenty-six keys, three profiles, no
Alt-Tab.

Tap Space on its own and it types a space, exactly like always.

Windows only. Rust + Tauri v2. No telemetry, no network, no account.

---

## Install

Download the latest **`Spaceadom_*_x64-setup.exe`** from
[Releases](../../releases) and run it.

Windows will warn that the publisher is unknown — the build isn't code-signed.
Choose **More info → Run anyway**.

That's it. Spaceadom starts with Windows and lives in your tray.

---

## How it works

Hold **Space** for a moment and a radial guide appears showing every key you've
bound. Tap one.

| Gesture | What happens |
| --- | --- |
| `Space` + letter | Launch the app, focus it if it's already open, minimise it if it's already focused |
| `Space` + `Right Alt` | Cycle profiles — Founders → Gamers → Professionals |
| `Space` + `Esc` | Boss key: hide everything |
| `Space` + `` ` `` | Picture-in-picture cycle |
| `Space` + scroll | Fade the window under the cursor |
| `Space` + `.` | Pause Spaceadom |
| `Space` alone | A space. Always. |

Bindings are edited from the dashboard — click any key on the on-screen
keyboard and pick an app, or paste a URL.

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

## Building it yourself

Requires [Rust](https://rustup.rs/), [Node 20+](https://nodejs.org/), and the
WebView2 runtime (already on Windows 11).

```bash
npm install
npm run tauri dev     # run it
npm run tauri build   # installers → src-tauri/target/release/bundle/
```

`npm run build` must run before any cargo command — Tauri reads the built
frontend at compile time.

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

That last file is unusual for a public repo and deliberate. Most of the hard
problems here were things that **failed silently** — an overlay that reported
itself visible while painting nothing, a list that stopped at 60 items, a
confirmation dialog that never rendered, a watchdog that logged 260 errors
without catching a single real fault. They're written up so the next person
doesn't have to rediscover them.

---

## Licence

MIT — see [LICENSE](LICENSE).
