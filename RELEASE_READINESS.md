# Release readiness — Spaceadom

**Updated 2026-08-22 for 1.0.72.** Originally rewritten 2026-08-20. The previous version of this file was
written for 1.0.0 and had gone false in three places: it said
`bundle.targets` was `["msi"]` (it is `["nsis", "msi"]`), that the app needs an
administrator account (it has not since PROBLEM 61 removed elevation), and that
it "currently cannot start at all" on a standard user account. None of that was
true. That is the failure mode of a readiness document — it describes the day
it was written and is read as describing today.

Two questions, answered separately, because they have very different answers:

1. **Can Nur hand this to a friend with a Windows laptop?** — Yes, today, with
   caveats they should be told.
2. **Can it go in the Microsoft Store?** — One thing left, and it is the
   signature. Everything else on the checklist is done.

---

## 1. Sharing with friends — READY, with three things to say out loud

`share-spaceadom/` holds the current installers, `READ-ME-FIRST.txt` and
`PRIVACY.md`, and is refreshed automatically by `scripts/archive-build.mjs`
(npm `posttauri`) so it can no longer drift behind the build.

**Tell them these three things**, all of which are in the README:

| What they will hit | Why | What to say |
| --- | --- | --- |
| **SmartScreen: "Windows protected your PC"** | The installer is unsigned. Windows warns about every unsigned installer from a publisher it has not seen before. | "Click More info → Run anyway." |
| **Antivirus may look twice** | An app that watches every keystroke looks, to a scanner, exactly like one that records them. | It records nothing and sends nothing; `PRIVACY.md` says precisely what the local log contains. |
| **Shortcuts pause over an admin window** | Windows UIPI: a non-elevated hook receives nothing while an elevated window has focus. | Affects every app of this kind, including PowerToys. Click a normal window and it resumes. |

**Known limits worth being honest about, also in the README:** Smart Search
cannot reach the text box in WhatsApp or the Spotify desktop app (neither
publishes a way in); Guide HUD is primary-monitor-only by explicit decision.

---

## 2. Stability on an arbitrary laptop — what is done, and what is left

### Already handled

- **WebView2** — the friend build uses `embedBootstrapper` (fetches the
  runtime at install time on the rare machine without it; Windows 11 always
  has it). The Store build embeds it whole — see §3.
- **Small screens** — Rust clamps the window to 92% of the monitor and the
  frontend scales the board on both axes.
- **No GPU path** — a pixel self-test detects a machine that cannot composite
  the transparent overlay and relaunches the webview with `--disable-gpu`.
  Software compositing is treated as normal, **not** as a reason to degrade
  visuals (that mistake shipped in 1.0.64 and was corrected in 1.0.65).
- **Low-power scene** — `body.lite-scene`, driven by the Visual effects switch
  or Windows' reduced-motion setting, halves the storm's blur radii.
- **Display changes** — `display_watch.rs` rebuilds the overlay when the
  monitor set changes and re-homes an off-screen dashboard.
- **Hook eviction** — a watchdog re-installs the hook, and after two failed
  attempts rebuilds the whole hook thread.
- **Crash reporting** — one panic hook, symbols shipped, last-action context.
- **Corrupt config** — since 1.0.71 the newest backup that parses is restored
  rather than factory-resetting the user (PROBLEM 159), covered by four tests.
- **Two installs at once** — detected, and removable in one prompt.

### Known gaps — ALL FIXED 2026-08-22 (1.0.72)

The five gaps this section used to list were closed in the release-readiness
pass. Kept here as a record of what they were:

| # | Gap | Fixed in |
| --- | --- | --- |
| 1 | A dead hook was invisible — Rust reported it, the frontend dropped the field | PROBLEM 161 |
| 2 | Key editor unusable at 1366×768 @150% — no max-height, no scroll | PROBLEM 162 |
| 3 | Sea tiles regenerated per rebuild under a fresh URL, defeating the cache | PROBLEM 163 |
| 4 | `shell:allow-execute` granted to the webview with no caller | PROBLEM 164 |
| 5 | Store and friend installers shared a filename (210 MB vs 5.6 MB) | PROBLEM 165 |

**Gap 3 from the original list — "a webview that fails to rebuild at startup
leaves a tray icon that opens nothing" — is NOT fixed.** It is genuinely rare
(it needs the webview to fail twice at startup), the log records it, and the
change would touch `lib.rs`'s startup ordering, which is the most
consequence-heavy code in the app. Recorded here rather than done quietly.



---

## 3. Microsoft Store — one blocker left, and it needs your signature

An unpackaged Win32 app **can** be listed: you submit an HTTPS download URL to
your own installer rather than an MSIX package
([App package requirements for MSI/EXE apps](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msi/app-package-requirements)).
Registration is free for individuals. One of that page's requirements is still
unmet; the other was fixed on 2026-08-22.

### Blocker 1 — the installer is unsigned (Store Policy 10.2.9)

> "The binary and all of its Portable Executable (PE) files must be digitally
> signed with a code signing certificate that chains up to a certificate issued
> by a Certificate Authority (CA) that is part of the Microsoft Trusted Root
> Program."

Spaceadom's installers are unsigned. This is also what causes the SmartScreen
warning friends see, so fixing it solves both problems at once.

**Options** ([code signing options](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options)):
**Azure Trusted Signing** is the cheapest current route (a Microsoft-run
service, monthly, no hardware token, and individuals are eligible if the
identity check passes); a traditional **OV/EV certificate** from a CA costs
several hundred USD a year and EV ships on a hardware token. Signing must
cover `spaceadom.exe`, the NSIS `setup.exe` and the `.msi`.

### Blocker 2 — SOLVED: `npm run store`

> "The installer is a standalone installer and is not a downloader stub/web
> installer that downloads bits when run."

`webviewInstallMode: embedBootstrapper` embeds Microsoft's *bootstrapper*, and
the bootstrapper downloads the runtime — precisely a downloader stub.

**Fixed 2026-08-22.** `src-tauri/tauri.store.conf.json` overrides it to
`offlineInstaller`, and `npm run store` builds with it. **Verified by
building:** 209.8 MB against the friend build's 5.6 MB, and the difference is
the embedded runtime.

`poststore` renames the output to `Spaceadom_<v>_x64-setup-STORE.exe` and
leaves the normal path empty, so the 210 MB file can never be handed to a
friend or installed locally by accident (PROBLEM 165). It also refuses to
label a build under 100 MB, which would mean the offline config silently did
not apply.

### Also required before submitting, none of them blockers

- **One installer URL, not two.** The Store takes exactly one. Submit the
  NSIS `setup.exe` (per-user, no UAC). Publishing both would also fight the
  app's own two-installs detector.
- **Silent install must work.** NSIS supports `/S` and the app is installed
  that way on every build, so this is already exercised.
- ~~**Publisher name must differ from the product name.**~~ **Done** —
  `bundle.publisher` is now `Nur Ifran Arpon`, product stays `Spaceadom`.
- **Disclose the keyboard hook and the process-closing feature** in the
  description and in the certification notes, and give reviewers steps to
  reproduce. A remapper is acceptable — PowerToys is in the Store — but a
  global hook plus terminating other processes plus a `runas` elevation will
  get a manual review, and an undisclosed one gets rejected.
- **Privacy policy URL.** `PRIVACY.md` exists and now covers the
  process-closing capability; it needs to be reachable at a public URL.
- **Age rating, screenshots, description.**

### Disclosure text, ready to paste into certification notes

> Spaceadom is a keyboard productivity tool. It installs a global low-level
> keyboard hook (`WH_KEYBOARD_LL`) so that holding the spacebar acts as a
> modifier: Space+letter launches, focuses or minimises an app. Tapping Space
> alone always types a space. The hook reads key codes only; nothing is
> recorded, stored or transmitted — the app is fully offline and contacts no
> server. It also uses `SendInput` to send the shortcut keystrokes that focus a
> text box (for example `/` on YouTube), tagged with a private `dwExtraInfo`
> cookie so it never re-processes its own input.
>
> Spaceadom detects other keyboard-remapping programs (PowerToys, AutoHotkey,
> spacedesk and similar), because only one program can own the spacebar. On
> the user's explicit request — two confirmations, never automatically — it can
> close one of them, and optionally remove that program's start-with-Windows
> entry from HKCU\…\Run and the Startup folder. It will only ever act on a
> program from its own built-in list, and it never elevates without showing the
> standard Windows permission prompt.
>
> To reproduce: install, hold Space to see the shortcut guide, click a key on
> the on-screen keyboard to bind an app, then hold Space and tap that key.

### What is left, in order

1. **Sign.** Yours to buy. Sign `spaceadom.exe`, the NSIS `setup.exe` and the
   `.msi`. This removes the SmartScreen warning for every friend as well — it
   is the single biggest thing between this app and someone who does not
   already trust you.
2. **Build the Store variant** — `npm run store` — and host
   `…-setup-STORE.exe` at a versioned HTTPS URL that never changes content.
3. **Submit** with the disclosure text above in the certification notes.

Nothing else is outstanding.

---

## How to check this file is still true

Everything above is checkable from the repo:

```bash
# blocker 2: still a downloader?
grep -A2 webviewInstallMode src-tauri/tauri.conf.json

# blocker 1: is anything signed?
powershell -c "Get-AuthenticodeSignature 'src-tauri\target\release\bundle\nsis\Spaceadom_*_x64-setup.exe' | Format-List Status,SignerCertificate"

# gap 1: does the frontend use the hook status it is given?
grep -rn "installed" src/main.ts src/components/*.ts | grep -i hook
```

If any of those answers change, edit this file in the same commit. A readiness
document that is not maintained is worse than none, because it is believed.
