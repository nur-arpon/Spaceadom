# Release readiness — Spaceadom

**Rewritten 2026-08-20 for 1.0.71.** The previous version of this file was
written for 1.0.0 and had gone false in three places: it said
`bundle.targets` was `["msi"]` (it is `["nsis", "msi"]`), that the app needs an
administrator account (it has not since PROBLEM 61 removed elevation), and that
it "currently cannot start at all" on a standard user account. None of that was
true. That is the failure mode of a readiness document — it describes the day
it was written and is read as describing today.

Two questions, answered separately, because they have very different answers:

1. **Can Nur hand this to a friend with a Windows laptop?** — Yes, today, with
   caveats they should be told.
2. **Can it go in the Microsoft Store?** — Not yet. Two hard blockers, both
   solvable, one costs money.

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

- **WebView2** — `embedBootstrapper`, so the runtime is fetched at install
  time on the rare machine without it. Windows 11 always has it; Windows 10
  usually does.
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

### Known gaps, worst first

These came out of a 61-agent audit on 2026-08-20. Each was adversarially
verified against the source before being written down here.

| # | Gap | Effect on a friend | Effort |
| --- | --- | --- | --- |
| 1 | **A dead hook is invisible.** Rust knows (`HOOK_INSTALLED`) and the frontend discards it. If the hook never installs, the app looks perfectly healthy and no shortcut works. | "It just doesn't do anything" with no explanation and nothing to report. | hours |
| 2 | **The key editor has no `max-height` and no scroll.** At 1280×720 or 1366×768 with 150% scaling, Assign/Done can sit below the window. | Cannot finish assigning an app on a common cheap laptop. | hours |
| 3 | **A webview that fails to rebuild at startup leaves a working tray icon that opens nothing**, permanently and silently. | Tray icon does nothing; only the log says why. | hours |
| 4 | **Sea tiles are ~129 KB of generated SVG regenerated under a fresh URL on every scene rebuild**, so the image cache never helps. Toggling theme/fun repeatedly re-pays it. | A stutter on each toggle on a weak machine. | hours |
| 5 | **`shell:allow-execute` is granted to the webview with no scope.** Not exploitable today (no untrusted content is loaded), but it is a broad grant with no caller that needs it. | None today; it is a latent hazard and a Store reviewer will ask. | minutes |

None of these is a reason to withhold the app from a friend. All five are
reasons not to submit it to the Store yet.

---

## 3. Microsoft Store — two hard blockers

An unpackaged Win32 app **can** be listed: you submit an HTTPS download URL to
your own installer rather than an MSIX package
([App package requirements for MSI/EXE apps](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msi/app-package-requirements)).
Registration is free for individuals. Two of that page's requirements are not
met today.

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

### Blocker 2 — the installer downloads WebView2 at install time

> "The installer is a standalone installer and is not a downloader stub/web
> installer that downloads bits when run."

`webviewInstallMode` is `embedBootstrapper`, which embeds Microsoft's
*bootstrapper* — and the bootstrapper downloads the runtime. That is precisely
a downloader.

**Fix:** switch to `offlineInstaller`, which Tauri v2 supports
(`WebviewInstallMode::OfflineInstaller`). It embeds the full WebView2
installer, so nothing is fetched at install time.

**Cost:** the installer grows from ~5.6 MB to roughly 130 MB. That is a bad
trade for handing a file to a friend and the right trade for the Store, so
this should be a **separate build target**, not a change to the default.

### Also required before submitting, none of them blockers

- **One installer URL, not two.** The Store takes exactly one. Submit the
  NSIS `setup.exe` (per-user, no UAC). Publishing both would also fight the
  app's own two-installs detector.
- **Silent install must work.** NSIS supports `/S` and the app is installed
  that way on every build, so this is already exercised.
- **Publisher name must differ from the product name.** "Spaceadom" cannot be
  both, and a name that implies a company you do not have will be rejected.
- **Disclose the keyboard hook and the process-closing feature** in the
  description and in the certification notes, and give reviewers steps to
  reproduce. A remapper is acceptable — PowerToys is in the Store — but a
  global hook plus terminating other processes plus a `runas` elevation will
  get a manual review, and an undisclosed one gets rejected.
- **Privacy policy URL.** `PRIVACY.md` exists and now covers the
  process-closing capability; it needs to be reachable at a public URL.
- **Age rating, screenshots, description.**

### The honest recommendation

Do the signing first, on its own. It removes the SmartScreen warning for every
friend, which is the single biggest thing standing between this app and someone
who does not already trust it — and it is a prerequisite for the Store anyway.
Then close gaps 1–3 above, then build the offline-installer variant and submit.

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
