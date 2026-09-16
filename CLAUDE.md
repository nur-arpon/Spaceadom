# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# Spaceadom (v1.0.0 — formerly SpaceToggle V14)

Turns the Spacebar into a system-wide modifier on Windows: hold Space + tap a
key to launch/focus/minimize an app; tap Space alone and it must always type a
normal space. Rust + Tauri v2 backend, vanilla TypeScript + Vite frontend
(NO React, NO Tailwind — never add them). windows crate 0.58.

**Identity (since the 1.0.0 release pass, PROBLEM 45):** productName
`Spaceadom`, identifier `com.spaceadom.app`, exe `spaceadom.exe`, data dir
`%APPDATA%\Spaceadom`, install dir `%LOCALAPPDATA%\Spaceadom` (per-user).
**Autostart is a per-user Scheduled Task named `Spaceadom` since 1.0.109
(PROBLEM 266), registered by the app itself through the Task Scheduler COM
API (`Register-ScheduledTask` from `startup.rs::register_task_script`), with a
this-user-only logon trigger, `PT10S` delay, `RunLevel Limited`,
`LogonType Interactive`, battery-safe, no time limit. The HKCU Run value
`Spaceadom` is the FALLBACK only** — written when that registration is
refused, and deleted by the app on the first launch that registers the task,
which is how every existing install migrates. That reverses what this file
said from 1.0.41 to 1.0.108 ("the Run value, NOT a task"), and the reason it
said so is worth keeping in full, because both halves are still true:
(1) `schtasks.exe /Create /SC ONLOGON` IS denied to a non-elevated user, on
this machine and every other non-admin one — PROBLEM 64 measured that
correctly and blamed the wrong thing (the root folder). The denial is the
"at log on of ANY user" trigger schtasks writes and cannot narrow; the COM
API's `-AtLogOn -User <me>` trigger is a different request and a standard
user may make it (measured 2026-09-12, probes A–D in PROBLEM 266). Every
other schtasks call the app makes (`/Query /XML`, `/Change`, `/Delete`)
still works against the task the API registered. (2) A task created while
ELEVATED cannot be deleted by the non-elevated app (PROBLEM 61 removed
elevation), so a stale elevated task became permanent and launched an OLD
build alongside the new one (PROBLEM 129) — that is why the task must be
registered `Limited` by the non-elevated app and never by an installer or an
admin shell, and why `ensure_startup_task` still triages an existing task
(PROBLEM 75) and deletes legacy tasks and legacy Run names when it can. Why
it matters: a Run value is started by the shell a minute or more after logon
(80 s and 107 s measured on 1.0.107/1.0.108); the task fires 10 s after
logon. **The logon-time gain has not been measured yet** — the task's first
run is the owner's next logon. `V14_FIXES_AND_CODE.md` §PROBLEM 45 has the
identity table and the elevation flow; §PROBLEM 266 has the task XML.
The repo folder is still `SpaceToggle-V14`; rename at git-init.

**Spaceadom = V13's engine + the Earthy design.** V13 (`..\SpaceToggle-V13`)
is the functional baseline and stays untouched; Spaceadom installs beside it
with its own identity so the two can never clobber each other's config.

## The design is a specification, not a suggestion

Source files live in
`..\design\Design system overhaul project\design-upgrade-using-claude\`:

| File | What it governs |
| --- | --- |
| `Dashboard Earthy v2.dc.html` | The dashboard. **Every value is literal in its inline styles and `renderVals()` — transcribe, never paraphrase.** |
| `Motion Lab Earthy/Nocturne.dc.html` | Overlay motion, both palettes |
| `handoff/overlay-reference.html` | Vanilla port target for the overlay |
| `handoff/motion.css` | Tokens + keyframes |
| `handoff/DESIGN_SPEC.md` | Written spec + file mapping |

The 2026-08-20 overhaul added four more, all in `..\design\`, and these are
the authority for the night scene — do not re-derive any of it:

| File | What it governs |
| --- | --- |
| `design-system-overhaul-3.md` | Settings descriptions, toggle/slider characters, the special-key cards, the 3-way theme pill |
| `night-scene4.md` | The night scene delta: moon cycle, 20 constellations in exclusive bands, the crest-field sea, the rigged galleon |
| `moon.md` | The moon in full — glow construction, maria tables, the 7-leg wander |
| `storm-clouds.md` | The storm. **Standalone and authoritative.** Four attempts were made without it; `starry-sky.ts`'s `STORM_MASSES` must stay diff-able against it |
| `constellations.js` | The 20 figures' geometry, copied byte-identical to `src/constellations.js` |
| `sounds.js` | The WebAudio kit, copied byte-identical to `src/sounds.js` |

In this repo: `V13_TO_V14_METHOD.md` (how two earlier attempts failed and
why), `OVERLAY_ACHIEVED.md` (the confirmed-correct overlay — do not
re-derive it), `OVERLAY_RUST_HTML_CHANGES.md` (the exact non-TS edits).

Design rules that are not negotiable: radii 13 keys/cards, 16 containers, 999
everything interactive; shadows warm brown `rgba(90,60,30,…)` in light and
black-tinted in Nocturne, never pure black on cream; exits run at ~65% of
entrance time with `--ease-in`; nothing may assume a fixed width around an app
name; `prefers-reduced-motion` renders final states.

**Theme rule: ONE setting drives everything.** `body.nocturne` on the
dashboard AND the overlay. The overlay learns about it two ways and needs
both — `save_config` re-emits `theme-changed`/`sound-changed` from Rust
(global `emit`; `emit_to` has never worked here), and `overlay.ts` seeds
itself from `get_config` on load, because an event that only fires on CHANGE
leaves a freshly-opened overlay in the wrong palette.

## Dashboard architecture (V14)

One stage, keyboard as the hero. No sidebar, header grid or status bar.

```
index.html        #stage → auras, halo, cursor glow, topbar, keyboard, docks
src/main.ts       bootstrap + stage motion (cursor-glow RAF lerp .09, press
                  ripples, board fit) + popover plumbing
src/styles.css    the stage; imports styles/design-system.css
  keyboard-matrix.ts   16-unit board, U=56/G=10, DESIGN_W=1048 (NOT 1046 —
                       fractional keys round up 2px/row; measured)
  key-detail-panel.ts  editor that blooms out of the pressed key
  profile-editor.ts    top-right pill popover
  settings-panel.ts    bottom-left gear popover
  toast.ts             overlay only — toasts + radial HUD (drop-in, verbatim)
```

The board is FIXED geometry scaled to fit. Two guards, both required: Rust
clamps the window to 92% of the monitor (`lib.rs` step 9c) and the frontend
scales the board on BOTH axes (`wireKeyboardFit`). Attempt #2 scaled on width
alone and the keyboard ran off the display — that is the failure the user saw.

`preview.html` + `src/preview.ts` render the real components with a stub
config so the dashboard can be inspected without the backend. Dev-only; not a
Vite build input, so it never ships.

**A control that does nothing is worse than a missing control.** That rule
still stands; the example it used to give is stale. "Run at startup" now HAS a
backend command (`set_startup_enabled`, which persists the config and calls
`apply_task_enabled` — `schtasks /Change /ENABLE|/DISABLE` on the task, or
the HKCU Run value when no task exists — in one call) and the toggle ships. The rule is what to
keep: add the command first, then the toggle.

## Required reading, in this order, before changing anything

0. **`V14_FIXES_AND_CODE.md` — every V14 problem with its root cause, the
   exact file, the exact code that fixed it, and how it was verified.** Read
   this before re-diagnosing anything; it exists so you do not have to search.
1. `RELEASE_READINESS.md` — what is done, what is left, and the two
   Microsoft Store blockers. Rewritten 2026-08-20.
   (`AI_HANDOFF.md`, `FINAL_RELEASE_README.md` and `HANDOVER_PROMPT.md` are
   V12/V13 fossils, marked SUPERSEDED at the top of each. Read them for the
   reasoning they record, never for current fact.)
2. `CORE_AIM.md` — the non-negotiable feature contract. Never remove or
   simplify a listed feature; fix it natively.
3. `NATIVE_SAFETY.md` — do-not-touch table for Win32 calls. This app once
   broke the user's touchpad by minimizing explorer.exe shell windows.
4. `PROJECT_STATUS.md` — append-only dev log, newest at top. Append your own
   dated, named entry for every problem you solve. NEVER delete entries.
5. `FEATURES_NOW_POSSIBLE.md` — previously stripped features that now have
   stable implementation paths.

`install-v11.ps1` (AutoHotkey) is the functional gold standard — when
behaviour is ambiguous, match v11.

## Skill

The skill `arpons-windows-apps-building-skills` governs all work here
(debugging discipline, keyboard-hook iron laws, design/motion scales). It
ships in `skill-package/`; if `.claude/skills/arpons-windows-apps-building-skills/`
is missing, copy it there. NOTE: the destination folder must exist first or
PowerShell flattens the copy:

```powershell
New-Item -ItemType Directory -Force .claude\skills | Out-Null
Copy-Item -Recurse -Force "skill-package\arpons-windows-apps-building-skills" ".claude\skills\"
```

## Build

Rust lives ONLY in `D:\RUST-DOWNLOADED-HERE` (cargo 1.97.1). Never let it
reinstall to `C:\Users\beamu\.cargo`.

```powershell
$env:CARGO_HOME="D:\RUST-DOWNLOADED-HERE\cargo"
$env:RUSTUP_HOME="D:\RUST-DOWNLOADED-HERE\rustup"
$env:PATH="D:\RUST-DOWNLOADED-HERE\cargo\bin;$env:PATH"
npm run build        # tsc + vite — MUST run before ANY cargo command
npm run tauri build  # setup.exe → src-tauri\target\release\bundle\nsis\
```

**TWO installers since 1.0.54.** `bundle.targets` is `["nsis", "msi"]`.

- `setup.exe` (NSIS) installs **per-user** into `%LOCALAPPDATA%\Spaceadom`,
  `nsis.installMode = "currentUser"`, no UAC ever. This is the recommended one.
- `.msi` (WiX) installs **per-machine** into Program Files, as every .msi up to
  1.0.40 did. It is built from a custom template, `src-tauri/wix/main.wxs`,
  forked from tauri-bundler 2.9.4's stock file with **exactly FOUR changes,
  each marked `SPACEADOM CHANGE n` in the file** (its own header comment is the
  authority; read that before editing it):
  1. `util:CloseApplication` **added** — PROBLEM 127's silent update deferral
     over a running app. Since REVIEW FIXES 2026-09-05 (H2) it sends
     `EndSessionMessage="yes"`, not `CloseMessage="yes"`: WM_CLOSE makes this
     app hide to the tray, so the process survived and `TerminateProcess`
     killed it — every MSI update ended in a hard kill of a healthy app.
  2. `<Property Id="INSTALLDIR">` and its two `RegistrySearch` elements
     **deleted** — PROBLEM 244/246. NSIS writes that HKCU key with its own
     per-user directory, so a double-clicked `.msi` used to install INTO
     `%LOCALAPPDATA%\Spaceadom` and register the per-user app's files as its
     own components; `msiexec /X` of that product then deleted the running
     app. With it gone, `INSTALLDIR` has one source and can only ever be
     `C:\Program Files\Spaceadom`.
  3. The "Keep your settings?" uninstall question and the two
     `util:RemoveFolderEx` rows it gates **added** — PROBLEM 252, the MSI half
     of PROBLEM 247's NSIS `installer-hooks.nsh`. One feature in four places.
  4. `MajorUpgrade Schedule` **changed** from the stock
     `afterInstallInitialize` to `afterInstallExecute` — REVIEW FIXES
     2026-09-05 (H2). The stock order put `RemoveExistingProducts` at sequence
     1501, 2,498 numbers before `WixCloseApplications` (3999) closed the
     running app, so Windows Installer queued a delete-on-REBOOT against the
     exe PATH, `InstallFiles` then wrote the NEW exe to that path, and the
     next restart deleted it. Measured after the fix: 1501 → 6501.

  Everything else is stock, deliberately, so a bundler upgrade can be diffed
  against the new original rather than re-derived.

**Building the `.msi` writes an MsiInstaller 1033 "installed the product" event
into the Application log — it is NOT an install** (measured 2026-09-04 across
45 versions). WiX's `light.exe` validates the package it just wrote by running
it through the Windows Installer engine, which logs 11707 + 1033 five to ten
seconds after the `.msi` file's own mtime, every single build. The tell that
separates it from a real install is the **transaction pair**: a genuine
install/uninstall is bracketed by 1040 "Beginning a Windows Installer
transaction" and 1042 "Ending…", naming either the `.msi` path or the
ProductCode. The build-time 1033s have neither. Do not read one as evidence
that a ship step ran the `.msi` — no ship step does. (The one real per-machine
MSI install on this machine was 2026-08-30 15:09, 1040/1042 naming a
`Spaceadom_1.0.94_x64_en-US.msi` inside WhatsApp Desktop's transfers folder:
the owner double-clicked a build he had shared. That install is what created
the registration PROBLEM 244's `msiexec /X` then acted on.)

**THE APP UPDATES ITSELF since 1.0.100 (PROBLEM 245).** Every installed copy
polls `releases/latest/download/latest.json` on GitHub once a day (first
check 15 s after launch, past the autostart settle, on the `st-updater`
thread) and installs a newer release SILENTLY — `setup.exe /S /UPDATE /R
/ARGS --autostart`, the plugin exits the process, the NSIS installer
relaunches it quietly. Everything is in `src-tauri/src/updater.rs`. Rules:

- **The signing key is NEVER committed.** `src-tauri/.tauri/spaceadom.key`
  (+ `.password.txt`, + `.pub`) is gitignored, exactly like the Sentry DSN;
  CI gets it from the `TAURI_SIGNING_PRIVATE_KEY` /
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secrets and release.yml fails loudly
  without them. The PUBLIC key is baked into every exe
  (`tauri.conf.json` → `plugins.updater.pubkey`), so **losing the private
  key means no existing install can ever accept an update again** — back it
  up somewhere that is not this repo. A local `npm run tauri build` only
  signs when both env vars are set (key CONTENT or path, and password);
  without them it builds fine and simply writes no `.sig`.
- **Set those env vars from PowerShell, NEVER from the Bash tool.** The
  password is base64, so about one in three starts with `/`, and MSYS2
  rewrites a POSIX-looking value into `C:/Program Files/Git/…` before it
  reaches a native `.exe` — as an ARGUMENT (`-p`) *and* as an exported
  environment variable, quoted or not, with `MSYS_NO_PATHCONV=1` set or not.
  The result is `Wrong password for that key` from a password that is
  perfectly correct. This cost a day on 2026-09-05; the full account is in
  `V14_FIXES_AND_CODE.md` § MEASUREMENT TRAP → RESOLUTION. **Any secret
  handed to a native Windows binary goes through PowerShell.**
- **An install is only ever updated by the kind of installer that made it.**
  The app decides at runtime — `uninstall.exe` beside the exe = NSIS, an
  HKLM `MsiExec /X` product registered for the exe's folder = MSI, neither
  = never updated — and reads `latest.json` (setup.exe) or `latest-msi.json`
  (.msi). Both manifests are written by
  `scripts/write-updater-manifests.ps1`, from CI and from the local proof.
  Feeding an NSIS install the `.msi` is PROBLEM 129/244; do not "simplify"
  this to one manifest.
- **The MSI leg is ON since PROBLEM 246** (`updater::MSI_AUTO_UPDATE = true`),
  reversing what this file said while 1.0.100 was being built. The constraint
  that kept it off is real and unchanged — a per-machine `.msi` cannot install
  silently from a non-elevated process, because at UI level none (`/qn`,
  `/quiet`) there is no client UI through which the Installer service can ask
  for consent, so it simply fails. `/passive` is the LOWEST UI level at which
  the UAC dialog can appear at all (measured from `/L*v` logs; the last UI
  switch on the command line wins). The owner's decision on 2026-09-05 was to
  accept **one UAC prompt per update** rather than leave a published channel
  switched off. `updater.rs` composes and WAITS ON the msiexec command itself
  rather than handing bytes to the plugin, for two reasons that are both
  load-bearing: the plugin `exit(0)`s immediately, so a declined prompt would
  leave the machine with no Spaceadom running at all; and
  `plugins.updater.windows.installMode` is ONE value shared by both legs, so
  switching it to `passive` for the MSI's sake would put a progress window on
  every NSIS user's screen. Setting `MSI_AUTO_UPDATE = false` puts the leg back
  to "detected, never driven" as a one-word change. **No live MSI update has
  ever been observed** — see V14_FIXES_AND_CODE.md §PROBLEM 246's second-PC
  recipe before claiming it works.
- **The relaunch is the installer's `/R`, not `AppHandle::restart()`** —
  PROBLEM 233: restart spawns before it exits and single-instance kills the
  newcomer. Exit first, launch later. `on_before_exit` stops the hook.
- **To test an update locally — THIS NEEDS A DEBUG BUILD SINCE REVIEW FIXES
  2026-09-05 (MEDIUM).** `st-updater-endpoint.txt` beside the exe replaces the
  manifest URL *and* turns on `danger_accept_invalid_certs`, in a directory
  anything running as this user can write, so `updater::read_override()` is
  gated on `cfg!(debug_assertions)` and **a release build ignores the file
  entirely** — silently, because there is nothing to report about a file it
  never reads. The recipe is therefore:
  1. `cargo run` (or `npm run tauri dev`) a DEBUG build, and put
     `st-updater-endpoint.txt` beside THAT exe — first line the manifest URL.
     A self-signed HTTPS cert is accepted while the file exists; the shipped
     config keeps `dangerousInsecureTransportProtocol` OFF.
  2. Or, to exercise a real installed release exe, delete the
     `cfg!(debug_assertions)` gate in `updater.rs::read_override` for the
     duration of the experiment **and put it back** — never ship a build with
     it lifted.

  Delete the endpoint file afterwards either way. The 1.0.99 → 1.0.100 proof
  in V14_FIXES_AND_CODE.md §PROBLEM 245 is the worked example, and it was run
  BEFORE the gate existed — it drops the file beside an installed release exe,
  which no longer does anything.
- The escape hatch is `"auto_update": false` in config.json. No UI switch,
  by the owner's decision.

**A THIRD build target exists for the Microsoft Store: `npm run store`.**
It overrides `webviewInstallMode` to `offlineInstaller`
(`src-tauri/tauri.store.conf.json`), because the Store forbids an installer
that "downloads bits when run" and the default `embedBootstrapper` does exactly
that. Result: ~210 MB instead of 5.6 MB — right for the Store, wrong for a
friend. `poststore` renames it to `…-setup-STORE.exe`, drops it in
`to-publish-in-microsoft-store/` with the submission paperwork, and leaves the
normal installer path EMPTY. **So after `npm run store`, run
`npm run tauri build` before installing locally** — otherwise
`scripts/install-real.cmd` has nothing to install, which is deliberate: an
obvious failure beats silently installing the 210 MB build (PROBLEM 165).

**A FOURTH build target exists, and it produces a DIFFERENT PROGRAM:
`npm run msix`** (PROBLEM 250). It packs the plain `npm run tauri build` release
binary into `Spaceadom_<v>_x64.msix` for the Microsoft Store, using a
hand-written `src-tauri/msix/AppxManifest.xml` and MakeAppx from the Windows
SDK — Tauri v2 has no MSIX target and the one third-party tool for it was
evaluated and declined. It needs three Partner Center values in a gitignored
`src-tauri/msix/identity.json` (`identity.example.json` is the committed
template). **NOT `npm run store`:** you cannot run an installer from inside a
package, so the 210 MB offline-WebView2 build has nothing to contribute; the
MSIX is 10 MB and takes WebView2 from the system Evergreen runtime.

- **THE MSIX HAS RUN ON THIS MACHINE, on 2026-09-05** (attempt 2 in
  `_probe/msix-test/`; log at `_probe/msix-test/log.txt` §"ATTEMPT 2"). The
  packaged branches are verified — hook, overlay, ring, launch and the
  StartupTask toggle all worked, and four defects were found and fixed; see
  V14_FIXES_AND_CODE.md §PROBLEM 250 ▸ "LIVE TEST 2026-09-05". **AppData was
  NOT virtualised** (the packaged copy read and wrote the real
  `%APPDATA%\Spaceadom`) **and HKCU WAS** (its writes went to the package's
  private `…\Packages\<PFN>\SystemAppData\Helium\User.dat` and never reached
  the hive the shell reads). Do not re-derive either fact — the documented
  rules behind them are quoted in `packaged.rs::migrate_legacy_data_once`.
- **INSTALLING THE .MSIX HERE IS ALLOWED, BY THE PROVEN PROCEDURE BELOW, AND
  BY NO OTHER.** This paragraph used to say "NEVER INSTALL THE .MSIX ON THIS
  MACHINE". The hazard it named is real and unchanged — a packaged copy beside
  the NSIS one is two `WH_KEYBOARD_LL` hooks fighting over the spacebar
  (PROBLEM 129/141/236) — but the answer is to kill the NSIS copy first, not to
  leave the packaged half of the app permanently untestable. Run every
  machine-touching step through `explorer.exe` (PROBLEM 143), and run them in
  this order:

  ```powershell
  # 1. The hazard: stop the NSIS copy FIRST. Nothing else may run before this.
  taskkill /IM spaceadom.exe /F

  # 2. Trust the local test cert. LocalMachine\TrustedPeople — CurrentUser
  #    is NOT enough (attempt 1 failed there) and needs an admin shell once.
  Import-Certificate -FilePath .\local-test-public.cer `
      -CertStoreLocation Cert:\LocalMachine\TrustedPeople

  # 3. Install, launch by AUMID, test.
  Add-AppxPackage -Path <...>\Spaceadom_<v>_x64.msix
  explorer.exe shell:AppsFolder\<PackageFamilyName>!Spaceadom

  # 4. Remove it again, and relaunch the NSIS copy.
  taskkill /IM spaceadom.exe /F
  Remove-AppxPackage -Package <PackageFullName>
  Start-Process "$env:LOCALAPPDATA\Spaceadom\spaceadom.exe" -ArgumentList '--autostart'
  ```

  Measured on 2026-09-05: `Remove-AppxPackage` took 1,340 ms and took the
  StartupTask, `Program Files\WindowsApps\<full>`, `%LOCALAPPDATA%\Packages\<PFN>`
  and the `SystemAppData\<PFN>` key with it; HKCU Run was never touched and the
  config came back semantically identical. **Two things it leaves behind that
  you must clean up by hand:** the test certificate in
  `LocalMachine\TrustedPeople` (still present — the owner removes it), and
  `%APPDATA%\Spaceadom\packaged-first-run.txt` + `packaged-migration\` written
  by the old unconditional snapshot. `build-msix.ps1` still has no
  `Add-AppxPackage` anywhere and still leaves the package unsigned unless you
  pass `-Sign`; that stays deliberate — installing is an explicit act, never a
  side effect of a build. The second-machine recipe in
  `to-publish-in-microsoft-store/SUBMIT-CHECKLIST.md`, "Route B", is still the
  right way to test a machine with no NSIS copy at all, which this one is not.
- **A packaged Spaceadom behaves differently in four places, and every
  difference is silent.** One runtime probe decides them all —
  `packaged::is_packaged()` (`GetCurrentPackageFullName`, cached, logged at boot
  behind the marker `package-identity-probe-msix-store-mode-spaceadom`, which is
  the third line of every log). When packaged: **the in-app updater is inert**
  (the Store owns updates; a downloaded `setup.exe` would install a second,
  unpackaged copy); **autostart is the manifest's `windows.startupTask`, never
  an HKCU Run value** (a Run value would name a `WindowsApps\…_<VERSION>_…`
  path that the next Store update deletes); **config gets a one-time snapshot on
  first packaged launch — ONLY when the resolved data dir is not already the
  classic `%APPDATA%\Spaceadom`** (2026-09-05: it was, and the unconditional
  version copied 1.3 MB from that folder into a subfolder of itself; it now
  logs "AppData not virtualised — nothing to migrate" and writes nothing);
  and **the rival-install banner offers directions, not a button** — a packaged
  app must not elevate to delete files outside its package, and
  `rival_install::repair` refuses independently of the banner. A FIFTH
  difference was added on 2026-09-05: **a packaged copy does not touch
  `NotifyIconSettings` at all**, because HKCU writes go to the package's
  private hive and the shell never reads them — Store users promote the tray
  icon themselves, and there is no supported API to do it for them.
- **Windows owns "Run at startup" in a package and the app must not fight it.**
  `RequestEnableAsync` is documented to refuse to override a user who switched
  the app off in Task Manager. So the boot path reads the state and changes
  nothing, and the Settings row goes INERT with a sentence naming who is holding
  it off — the same `paintInert` treatment as "Show special keys". A switch that
  flips back on its own is the failure this avoids.
- **`packaged::STARTUP_TASK_ID` must equal the manifest's
  `desktop:StartupTask/@TaskId`.** `StartupTask::GetAsync` fails with
  `E_INVALIDARG` for an unknown id, which in a log is indistinguishable from
  "the WinRT API is unavailable". `build-msix.ps1` checks the two against each
  other and refuses to pack if they differ.
- **The `.msix` goes to CI artifacts ONLY, never to a GitHub release asset.**
  The Store owns MSIX distribution; a `.msix` on the releases page would hand
  somebody a second copy beside the `setup.exe` they already have.
- **The packaged branches HAVE executed, once, on 2026-09-05** — this bullet
  used to say "none of them has ever executed. The package has never been
  installed anywhere." Both sentences are now false; see the live-test bullet
  above. What is still true and still needs the second-machine run in
  SUBMIT-CHECKLIST.md: **a machine with NO unpackaged Spaceadom on it has never
  run this package.** Everything measured on 2026-09-05 was measured beside an
  existing NSIS install, and at least one finding depends on that — AppData
  fell through to the real folder BECAUSE `%APPDATA%\Spaceadom` already
  existed. A clean Store machine may well see its config land in the package's
  private store instead, and nothing here has observed that.

**Do not try to make the .msi per-user** (PROBLEM 139/140). It trips WiX ICE38,
which demands an HKCU-registry KeyPath for every component installed into the
user profile; three of ours are generated by tauri-bundler itself and cannot be
changed from a template, and tauri exposes no way to pass `light`'s `-sice`.
Suppressing it would ship an installer whose repair/uninstall semantics are
knowingly wrong. The two-installs risk it was meant to prevent is handled where
it belongs instead: PROBLEM 141's dashboard banner detects a second install and
removes it with one permission prompt.


- **`windows` crate features are per-API.** A missing feature reads as
  `unresolved import` on a path that plainly exists in the docs. Store/UWP
  window matching needs `Win32_UI_Shell_PropertiesSystem` +
  `Win32_System_Variant` — inherited code that names features in a markdown
  file has not necessarily had them added to `Cargo.toml` (PROBLEM 30).

- `npm run build` first, always: `tauri::generate_context!` reads **`../dist2`**
  (named in `tauri.conf.json` as `frontendDist`, produced by `package.json`'s
  `--outDir dist2`). `dist/` and `dist-stale/` are leftovers on disk and are
  NOT what ships. If it's missing you get a bare `error: proc macro panicked`
  naming no cause.
- **0-byte shim trap** (this machine does it reproducibly): if cargo/rustc
  fail with "The system cannot find the file specified", the shims in
  `cargo\bin` are 0 bytes. `Test-Path` says True; check `Length`. Repair:
  delete each empty shim and copy `rustup.exe` over it (`rustup default
  stable` does NOT fix it). Full script in AI_HANDOFF.md §4.
- Keep the build at 0 errors, 0 warnings.
- **Installing: `setup.exe /S`, and NEVER trust its exit code.** Four separate
  installs exited 0 having replaced nothing (PROBLEM 127 — the app is running
  during its own upgrade, and the MSI deferred the file swap to a reboot). The
  NSIS `installerHooks` PREINSTALL macro now kills the app first, but the rule
  stands for any installer: **verify the installed exe by version stamp AND by
  an ASCII content marker** before believing a fix shipped. A verdict taken on
  an unverified build is worse than no verdict — it discredits a working fix.
- **A FIX THAT IS NOT INSTALLED DOES NOT EXIST.** The user boots from
  `%LOCALAPPDATA%\Spaceadom\`, never from `target\release\`. Testing
  from the repo build is fine, but the session is NOT finished until the
  setup.exe has been run and the installed exe verified. Per-user means no UAC
  prompt at all now; if one ever appears and is declined,
  say so loudly and treat the work as UNDELIVERED — quietly continuing to
  test from the repo is how five hours of fixes failed to reach the user's
  startup (PROBLEM 42), leaving them booting a build that still had the
  overlay-killing glow.
- You can prove which RUST fixes a binary contains WITHOUT running it:
  `log::info!` format strings are ASCII-searchable inside the exe.
  `[Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($exe))` then test
  for markers like `url_focus:`, `aumid_focus:`.
- **NOT EVERY RUST LITERAL IS A USABLE MARKER (measured 2026-08-27).** A SHORT
  literal that is only ever copied into a `String` may never exist contiguously
  in the binary at all. `st-hud-pointer` (14 bytes, a thread name) tested
  **False in a freshly-built exe that certainly contained it**: at offset
  0x2C8B3 the compiler materialises it with two OVERLAPPING immediate stores
  (`mov rcx,"-pointer"` → `[rax+6]`, then `mov rcx,"st-hud-p"` → `[rax]`), so
  the bytes are assembled at runtime and never sit together on disk. Longer
  thread names in the same file DO survive in `.rodata`
  (`st-exclusion-watcher` at 20 bytes, `st-hook-supervisor` at 18).
  **Use a long `log::` FORMAT STRING as the marker, never a short identifier —
  and confirm the marker is present in the freshly-built exe BEFORE trusting
  its absence from the installed one as evidence.** Shipping an unconfirmed
  marker means a False reads as "the fix did not ship" when it means "the
  marker was never findable", which is how a working build gets discredited.
  Same family as the env-var trap above: **a check that cannot produce a
  negative result — or that produces one for the wrong reason — is not a check.**
- **This does NOT work for FRONTEND markers any more (measured 2026-08-20).**
  Tauri v2 compresses the embedded `dist2` assets, so CSS class names and JS
  strings are not findable in the exe. `st-hud-glow` — the example this file
  used to give — now tests False in a binary that certainly contains it, and so
  do `toggle-thumb`, `keyboard-scale` and every other frontend name. A frontend
  string that IS found is one that also exists in the Rust source. **Do not
  read a False here as "the fix did not ship."** Prove a frontend change with
  the chain instead: grep the marker in `dist2/assets/*`, then confirm the
  exe's `LastWriteTime` is LATER than the newest file in `dist2`, then confirm
  the installed exe's version stamp — and read all of it from outside the
  sandbox (PROBLEM 143). `scripts/install-real.cmd` does the install and the
  proof in one pass.
- **Debug symbols SHIP (PROBLEM 131). Do not remove any of this plumbing:**
  `src-tauri/symbols/spaceadom.pdb` → installed beside the exe via
  `bundle.resources`. `build.rs` writes an invalid STUB there if missing (it
  must run on every cargo invocation — Tauri's before-hooks only run for
  `tauri build`, and without the stub `cargo test` fails with `resource path
  symbols\spaceadom.pdb doesn't exist`). `beforeBundleCommand` then copies the
  freshly-linked pdb over it. **Never stage a previous build's pdb to satisfy
  the check**: mismatched symbols do not fail, they resolve to confidently
  wrong lines. Cost measured: installer 4.6 → 5.6 MB.
- **There is exactly ONE `std::panic::set_hook` call, in `lib.rs`.** There were
  two, and the second silently replaced the first for months (PROBLEM 131) —
  `set_hook` replaces, it does not chain unless you make it. If you add
  another, the last one installed wins and the loser leaves no trace.
- **There ARE automated tests now** — 388 as of 1.0.100 (13 when this bullet
  was written), run with `cargo test --lib` from
  `src-tauri`. They cover the self-updating-app path repair (PROBLEM 116) and
  the opacity floor arithmetic (PROBLEM 119). Everything else is still
  verified by hand on the real machine (see Testing laws). Add a test when
  the logic is pure and the branch is one a user reaches only after something
  has already gone wrong — PROBLEM 118 is what shipping an unexercised
  recovery branch costs.

## Architecture

Two processes' worth of logic in one app; the hook thread and the UI must
never touch each other directly.

```
src-tauri/src/
  hook/mod.rs      WH_KEYBOARD_LL + WH_MOUSE_LL on a dedicated thread with its
                   own message pump. Decides typing vs command in microseconds.
                   Sends HookEvent over a crossbeam channel. Never touches
                   Tauri, COM, or the heap in the callback.
  engine/mod.rs    Async actor receiving HookEvents; dispatches to actions.
  engine/actions/  smart_cascade (launch/focus/minimize cycling), boss_key,
                   pip, opacity, focus_engine.
  guide_hud/       Emits Tauri events to the OVERLAY window, not the dashboard.
  config/          %APPDATA%\Spaceadom\config.json — profiles → bindings.
                   set_active_profile SAVES; don't also persistConfig() from
                   the frontend (that was the double-save bug).
  lib.rs           Startup: config load, window creation, tray. NO elevation
                   (PROBLEM 61 removed it).
  display_watch.rs Rebuilds the overlay when the display setup changes
                   (PROBLEM 117/118) and re-homes an off-screen dashboard.
  safe_mode.rs     PROBLEM 253 — the boot counter. Three launches in a row that
                   started and never stayed alive 30s, and the NEXT one comes up
                   with NO hook, NO overlay and a banner. A clean WM_ENDSESSION
                   exit is explicitly excluded (see below).
  diagnostics.rs   PROBLEM 253 — "Report a problem": one zip in
                   %APPDATA%\Spaceadom\reports\. NEVER uploads anything.
  hook/conflicts.rs        Detects other keyboard programs. ONE matcher
                           (`is_known_process`) shared with:
  hook/conflict_close.rs   Closes one, on request only. Closed list, WM_CLOSE
                           before force, never elevates silently (PROBLEM 155).
src/               Dashboard UI (vanilla TS): keyboard-matrix, key-detail-panel,
                   profile-editor, settings-panel; main.ts wires them.
                   (V14 removed hook-status-bar.ts and app-picker.ts — the
                   status bar is gone from the design and the picker became
                   the editor's inline app grid.)

  Added 2026-08-20, all LEAF modules (they import nothing from main.ts, so the
  preview harness can render them — see PROBLEM 148 for what happens otherwise):
    sfx.ts + sounds.js       The sound kit. sounds.js is byte-identical to the
                             design's; its types live in sounds.d.ts.
    components/controls.ts   The settings switch and slider markup + which
                             Fun-mode character each one performs.
    components/special-cards.ts   The nine special shortcuts and their cards.
    components/conflict-prompt.ts The top-centre "close it?" offer.
    components/starry-sky.ts      The whole night scene (moon, constellations,
                             sea generators, storm).
    components/night-markup.ts    The lab's ocean band, extracted VERBATIM.
    constellations.js        20 figures, byte-identical to the design's.
    styles/themes.css        The three palettes + the starry pointer carve-out.
    styles/characters.css    Fun-mode toggle/slider motion + the special cards.
    styles/starry-sky.css    The night scene's CSS.
src/overlay.ts +
overlay.html       The on-demand HUD/toast surface (see window rules below).
```

**THE RING HAS TWO TRIGGERS SINCE PROBLEM 263: holding Space, and holding the
MIDDLE MOUSE BUTTON.** Both raise the same ring in the same centred place and
run the same code from `HookEvent::SpaceDown` onwards — the middle-button event
is normalised to `SpaceDown` at the top of `engine::dispatch`, so the ring, the
cascade, pointer activation and the toast can never drift into two behaviours.
Internally there are **three witnesses**, not two, because the own-window
fallback (PROBLEM 259) is a third way for a Space hold to begin:
`MODIFIER_ACTIVE`, `OWN_HOLD_ACTIVE` and `MIDDLE_HOLD_ACTIVE`. Two of them
firing for one gesture would mean two rings, two launches and two toasts.
**The arbitration that prevents it is stated in ONE place — the comment block
headed `THE ARBITRATION — ONE PLACE, AND THIS IS IT` in `hook/mod.rs`,
immediately above `MIDDLE_TAP_MS` — and enforced in exactly three, each of
which names that comment: `middle_button_down_accepted` (rule A),
`kb_hook_proc`'s Space-down branch (rule B) and guard 2c of
`own_window_space_down_accepted` (rule C).** `hold_latched()` is the one
function that knows there are three witnesses; every consumer goes through it.
Separately, `hook/orbit_apps.rs` holds a built-in list of 3D, CAD and design
programs where the middle button is handed straight back to Windows — **that
list gates the MIDDLE BUTTON ONLY and is not the user's App exceptions**;
Space keeps working inside SolidWorks.

**SINCE PROBLEM 267 (2026-09-13) THE MIDDLE BUTTON RAISES A SECOND RING KIND
BY DEFAULT — the cursor-anchored ICON RING** (`middle_ring.rs` pure geometry,
`guide_hud::show_middle_ring` the one impure caller, `src/components/middle-ring.ts`
+ `styles/middle-ring.css` the page's half): eight real app icons at 45° on a
125 px radius blooming out of the cursor, a centre pill naming the hovered
tile, release-to-launch through the SAME `take_armed_key` → `PointerActivate`
path the Space ring uses (the poller runs `middle_ring::ring_pick` — band by
distance, sector by angle — when `pointer::RING_ACTIVE` is set, `sector_pick`
otherwise). **It is a CHOICE, not a replacement:** `middle_ring_style:
icon_ring | guide_hud`, and `guide_hud` is phase 1 byte-for-byte — the
`MiddleButtonDown → SpaceDown` normalisation still exists, one pure
`engine::routed_middle_event` away; the witness, rules A/B/C and the release
path are the same for both kinds. **ROUND 3 (owner
decisions 2026-09-13, after the 1.0.110 build was tested):** the scope is
**"Favourites"** (1..=15 ticked, `FAVOURITES_MAX`; `MyEight` stays the serde
variant; empty = the first 6 bound letters) or **"All"**, and the LAYOUT LAW
(`middle_ring::layout_arcs`) is: every ring EVENLY SPACED over the arc
available to it (`feasible_arc`), the inner ring up to 6, the next up to 9,
then as many as fit at `tile + 6` (`RING_CAPS`), rings filled inner-first,
70 px tiles unless the room forces smaller, the smallest radii that satisfy
spacing (125/201/277). For Favourites the centre NEVER moves from the press
point (the centre pill may be clipped by the screen edge — he likes it) and
only the TILES must lie inside the cursor's monitor's work area; near an edge
or a corner the rings become arcs (named `half-<dir>` / `quarter-<dir>` by
the first partial arc; `circle-clamped` only when nothing fits at any tile
size). **Clamp + warp is "All"'s path:** `clamp_ring_center` slides the
centre inward by exactly the overhang against the CURSOR's monitor's work
area (the actual outermost ring's extent) and `SetCursorPos` moves the OS
cursor to that centre on the engine thread, followed by `pointer::note_cursor`.
**THE WINDOW IS ONE BIG CANVAS — the whole work area of the monitor the
cursor is on** (`canvas_rect`; `MonitorFromPoint` of the press point; the
Space ring's `overlay_fit_hud` does the same through `overlay_monitor`),
never the exact monitor bounds (inset 2 px when the work area equals them);
the page draws the ring at `page_point(centre, canvas, scale)` — PHYSICAL PX
END TO END (1.0.110 handed a logical centre to a physical fitter and put the
ring centre/3 away from the cursor). The marker line prints the canvas, the
monitor and the page centre. **Motion is RIPPLE ONLY** (Bloom and
`middle_ring_motion` are gone; the key is ignored on read): the Space ring's
numbers plus the cursor-following fisheye wave fed by the poller's
`middle-ring-aim` event at ≤ 60 Hz; the rules live on `.st-mring` itself,
never on a bare `.ripple` — `styles.css`'s `.ripple` is loaded on the overlay
page too and matched the ring once. **The centre pill has two lines:** the
app's short name (vendor prefix stripped at display time: "Google Chrome" →
"Chrome") over the profile/account (`split_display_name`); it grows to the
free inner circle (170 px), then the text shrinks to 15/12 px floors, then
ellipsizes. **The Space ring's pills carry the app icon** with a letter badge
in place of the letter disc, from the picker's icon cache only
(`engine::hud_icons_for`). **Favicons are fetched ONCE, at bind time,
never at ring time:** the key editor's URL commit calls `site_icon::fetch_site_icon`
in the background and stores the `data:` URL in `KeyBinding::site_icon`; a
link with none draws a letter disc, and the next edit of that key is the one
retry. **App exceptions carry a SCOPE** (`ExceptionScope`: off entirely /
Space only / middle only; old plain-string rows read as off entirely through
`AppException`'s own `Deserialize`), resolved in ONE place —
`exclusions::resolve_scope` — into `EXCLUDED_ACTIVE` (Space's verdict,
meaning unchanged) and `MIDDLE_EXCLUDED_ACTIVE` (the middle button's own);
the built-in `orbit_apps` rows show in Settings pre-seeded at Space only, and
a user's own row for one of them mutes the built-in verdict. On a plain
release of the icon ring Rust does NOT hide the window — the page plays the
exit (143 ms) and `overlay_toasts_done` is the terminal hide. The marker
line, one per raise:
`middle-button ring v2: cursor-anchored-ring-raised-at-cursor-spaceadom-267`
— it carries `shape … (arc radii …, tiles …)`, `canvas WxH @ (x,y) physical on
the monitor at (mx,my) scale s` and `page centre (cx,cy) css`.

### Window rules (hard-won; violating them re-opens fixed bugs)

- Two windows: `settings` (dashboard, closes to tray) and `overlay`
  (transparent, on-demand, click-through, NoActivate, always-on-top).
  While a ring owns it the overlay is **the canvas** — the whole work area
  of the cursor's monitor (`overlay_fit_hud` / `overlay_fit_canvas`,
  `place_overlay_canvas`; the Space ring's content sits on a fixed `stage`
  centred on the monitor) — and **bottom-centred** for toasts (`overlay_fit`).
  Do not unify those two placements.
- A window MUST be listed in `src-tauri/capabilities/default.json` or every
  `listen()` in it rejects silently — the window is deaf.
- Transparency is CONDITIONAL, not banned: a **fullscreen** transparent
  webview composes zero pixels on this machine, but the **on-demand**
  overlay is `transparent: true` and works (verified 2026-08-10 at ring
  size). **Since PROBLEM 267 round 3 (decided 2026-09-13) the ring window is
  the WORK AREA of the cursor's monitor — 2560×1552 physical on this panel —
  and NEVER the exact monitor rectangle: `canvas_rect` insets it 2 px per
  side when the work area equals the bounds (auto-hide taskbar). That the
  work-area-sized transparent window composes is UNPROVEN on hardware as of
  this writing — it is the first thing to check on the round-3 build.**
  `backdrop-filter` is still banned (white boxes).
- **`filter: blur()` on the overlay has a size limit.** A 560x320 element at
  `blur(34px)` made the ENTIRE overlay window compose zero pixels — HUD and
  toasts both gone — while Rust reported it correctly sized, centred and
  `visible: true` and the JS ran to completion with no error (PROBLEM 37).
  The toast glow (340x150, `blur(22px)`) is under the threshold and works.
  Do not exceed it. Bake softness into gradient stops instead.
- The overlay cannot be validated in a browser harness: its failure mode lives
  in the OS compositor, not the page. Hold Space and LOOK before shipping any
  visual change to it.
- `overlay_fit` / `overlay_fit_hud` log the requested size, the monitor, and
  the resulting size/position/visibility. **Never remove that logging** —
  without it, a wrong size, a wrong position and a window that never moved are
  indistinguishable, which cost a full diagnostic round trip.
- **That rule covers `hide()` and `show()` too (PROBLEM 135).** `hide_guide_hud`
  called `win.hide()` silently, and the engine cancels the HUD BEFORE
  dispatching an action — so on every shortcut the overlay window was hidden
  ~500-1000ms before the toast existed. Three builds of animation work played
  out inside an invisible window while in-page instrumentation (geometry,
  decision, JS errors) reported perfect health, **because a page cannot observe
  that its own window is hidden.** Any call that changes what the user can see
  must say so in the log. If a visual is missing and everything measurable
  inside the page is fine, suspect the WINDOW before the page.
- **A hide that races a pending action must be told about it.**
  `hide_guide_hud_pending(action_pending)` keeps the window up when a combo has
  fired and its toast is still coming; `overlay_toasts_done` remains the single
  terminal path that actually hides it.
- Undecorated Win11 windows draw a 1px DWM border that reads as a "box"
  around overlay content — clear it with `DWMWA_BORDER_COLOR = 0xFFFFFFFE`
  plus `DWMWCP_DONOTROUND` (done in lib.rs overlay setup).
- Diagnosing a visual artifact: SAMPLE THE PIXELS
  (`Bitmap.GetPixel` over a screenshot) instead of eyeballing a zoom. That
  is what proved the overlay interior was genuinely transparent (R31,G31,B30
  = the desktop behind it) and that the leftover "box" was a 1px R27 border
  line — two rounds of guesswork replaced by one measurement.
- Show the window BEFORE emitting content; hidden webviews don't paint.
- Global `emit` + single listener (overlay page) is the only event
  arrangement that delivered; `emit_to` never worked here.
- If `set_ignore_cursor_events` fails the code hides the overlay — fail
  closed; never simplify that away.
- CSP: `style-src` needs `'unsafe-inline'` or every `style="..."` dies
  silently. No external hosts ever (fonts are bundled in `src/assets/fonts/`).

### Keyboard-hook laws (details in the skill)

1. Filter injected input ONLY by our `dwExtraInfo` cookie `0x7A7A7A7A`, never
   by `LLKHF_INJECTED`.
2. Need two keys in guaranteed order → one `SendInput` batch; `SendInput` then
   `CallNextHookEx` does not preserve order (the `hte`-for-`the` bug).
3. `GetAsyncKeyState` reports a key we SUPPRESS as UP — never build a failsafe
   on it for hidden keys (stuck-modifier detection uses a timestamp latch).
4. Pass Space through when Ctrl/Alt/Win is physically held (IME, autocomplete,
   window menu). Shift deliberately excluded.
5. **REFERENCE HOOK GOES IN FIRST. DO NOT REORDER. DO NOT "SIMPLIFY"
   `install_hooks()`.** Windows calls low-level hooks NEWEST-first —
   `SetWindowsHookExW` inserts each new hook at the HEAD of the chain, so the
   hook installed last is the one Windows calls first. `CallNextHookEx` is
   synchronous, so that newest hook's `LowLevelHooksTimeout` clock has to
   cover its own body PLUS the entire chain running underneath it, not just
   itself. A witness/reference hook — installed only to prove the real hook
   is still alive — is therefore the SLOWEST hook in the chain if it is
   installed last, and Windows evicts the slowest hook first. A dead witness
   can no longer report anything but "all quiet," so the deaf-detector reads
   that as permanent health while the real watchdog keeps tearing down a hook
   that is genuinely working — measured **514 times in one 3.8-hour session**
   (baseline 2026-09-01, root-caused as PROBLEM 230, shipped fixed in 1.0.96
   on 2026-09-04). **Symptom the owner sees:** shortcuts randomly stop
   mid-hold, especially while the dashboard is focused. **The one-line test
   that proves it's still fixed:** `grep "genuinely fired" debug.log | tail`
   — the `(N total)` counter must keep climbing — and `grep -c "WATCHDOG — "
   debug.log` must stay near 0. Full writeup, disproved hypotheses, and the
   teach-back explanation: **`docs/IF-SHORTCUTS-DIE-AGAIN.md`.** **See law 7
   for what an evicted hook looks like from inside the process afterwards —
   the handle stays valid and the mouse hook keeps firing, which is why
   `install_hooks()` re-installing all three is the only repair that exists.**
6. **THE RING MUST SHOW OVER OUR OWN WINDOW — before ANY ship, hold Space
   with the dashboard focused and grep `guide_hud: shown over own window`.**
   The install-proof script MUST assert that line exists in the NEW build's
   log (after that build's `Spaceadom build — version` banner), preceded by
   a `hold start (hold #N)` line that also says `over own window` — a
   PREVIEW from the "Check the ring" button prints the shown-over line too,
   and proves nothing about the hook. Owner or agent holds Space; if the
   agent cannot inject (it cannot, from the container — testing laws), the
   ship report must say **UNPROVEN** in capitals. Why this is a law: 1.0.101
   and 1.0.102 shipped "proved" on 2026-09-05 while every Space held inside
   the dashboard was invisible to the keyboard hook (PROBLEM 257 — the mouse
   hook on the same thread fired 2,705 times in the minute both keyboard
   hooks fired 0). The 60-second `N of them while the Spaceadom window itself
   had focus` line is a rate, not a proof; the per-hold line is the proof.
   `grep "KEYBOARD DEAF, PROVEN" debug.log` names the occurrences.

   **THERE IS NOW A FALLBACK, AND ITS PROOF LINE IS A DIFFERENT ONE
   (PROBLEM 259).** `src/own-window-keys.ts` runs the tap/hold/combo state
   machine in the DASHBOARD PAGE and feeds the Space to the engine through
   `own_window_space_down` / `own_window_key` / `own_window_space_up`, because
   the page still receives `keydown` for keys no hook in this process is
   called for. So the ring and Space+letter can work inside the app **while
   PROBLEM 257 is still happening**, and that is exactly why the two proofs
   are kept apart:

   | What fired | The pair to grep |
   | --- | --- |
   | The keyboard HOOK (law 6's proof) | `hold start (hold #N) … over own window` **then** `guide_hud: shown over own window` |
   | The own-window FALLBACK | `own-window fallback:` **then** `guide_hud: shown over own window` |
   | The MIDDLE BUTTON (PROBLEM 263) | `middle-button ring: the-guide-hud-ring-was-raised-by-a-middle-mouse-button-hold-spaceadom` |

   **The middle-button row can NEVER be law 6's proof and is not a third way to
   satisfy it.** That gesture never produced a Space-down and the keyboard hook
   was never asked anything, so a ring you raised with the middle button is
   evidence about the MOUSE hook — which PROBLEM 260 established was never the
   thing in doubt. Its line contains no `hold start` either, for the same
   reason the fallback's does not.

   The fallback's line deliberately does NOT contain the words `hold start`,
   so it can never satisfy `install-proof.ps1`'s law-6 assertion. **A ring you
   saw inside the dashboard is therefore no longer evidence that the hook is
   alive** — read which of the two lines produced it before writing PASS, and
   a ship report still says UNPROVEN when only the fallback line is there.
   Rust refuses the injection unless our window is genuinely foreground and
   the hook has not stamped a Space-down in the last 100 ms, so the two can
   never both serve one press; if you ever see both lines for the same
   keystroke, that dedupe has failed and it is a bug, not a curiosity.
7. **A TIMED-OUT LL KEYBOARD HOOK STAYS INSTALLED AND IS NEVER CALLED AGAIN —
   AND A *PROVEN*-DEAF VERDICT MUST NEVER BE BLOCKED BY A COOLDOWN
   (PROBLEM 260).** Two facts, and every liveness gate in `hook/mod.rs`
   depends on both. **(a)** When a `WH_KEYBOARD_LL` callback overruns
   `LowLevelHooksTimeout` (`HKCU\Control Panel\Desktop`, 1000 ms by default),
   Windows stops calling it and **leaves the handle valid** — no message, no
   error, no return code, and `UnhookWindowsHookEx` on it still succeeds.
   Inside the process an evicted hook and a hook nobody has typed into are the
   same observation. This is law 5's eviction seen from the other side: law 5
   is about *which* hook gets evicted, this is about what an evicted hook
   looks like afterwards. **(b)** `WH_MOUSE_LL` is a **separate hook with its
   own timeout record**, even on the same thread from the same pump, so it
   keeps firing at 30–60 Hz while the keyboard hooks are dead. **The only
   cure a process has is to change the chain: unhook and install afresh.**
   Consequences that are law, not preference:
   * **A live mouse callback may never veto a keyboard-deaf verdict.** It
     proves the *pump*, not the hook — the same rule the reference hook was
     corrected under on 2026-09-04 ("an instrument installed to witness a
     failure must never vote that the failure did not happen").
   * **Cooldowns throttle UNEVIDENCED alarms only.** The 60 s cooldown and the
     "last repair delivered events" test exist for PROBLEM 236's churn and
     keep that job in full — but a verdict proven from callback-only clocks
     (`proven_keyboard_deaf`) bypasses both. Waiting is strictly worse than
     repairing when the app is provably deaf. Do not "unify" the two paths.
   * **A repair is an unhook plus a fresh `SetWindowsHookExW`, and the log
     prints old → new HHOOK values to prove it.** Measured cost of not doing
     this: 130 seconds of provable deafness on 2026-09-07 in which the
     watchdog printed *nothing*, because `both_dead` needs the mouse to fall
     silent and `kb_only_dead` needs a live reference — and neither was true.
   * **A number printed beside the word "session" must read a counter nothing
     drains.** `hook diagnostics` swaps its counters to zero every 60 s, which
     is how `repair #1 this session` came to be printed three separate times
     and be read as "the repair never happened".
   Full writeup and the probe evidence: **PROBLEM 260** in
   `V14_FIXES_AND_CODE.md` and the 2026-09-07 follow-up in
   `docs/IF-SHORTCUTS-DIE-AGAIN.md`. The signature to grep:
   `hook liveness split — primary_real:0 … reference:0 mouse:<nonzero>`.

## Testing laws

- **The app does NOT elevate** (PROBLEM 61). Consequences: a non-elevated
  hook receives NOTHING while an elevated window has focus (Task Manager,
  regedit, an admin terminal) — that is Windows UIPI, it affects every
  remapper, and the watchdog logs it correctly rather than treating it as a
  fault. An input-injection harness still fails from a containerised agent
  shell: `SendInput` returns success and the hook sees nothing. ALWAYS assert
  a positive control and declare the run VOID when it fails — without one,
  2026-08-16's harness would have reported a false negative.
- Kill the v11 AutoHotkey process (`SpaceToggleRuntime.exe`) before testing
  V13 — two spacebar hooks feedback-loop. Leave its Startup shortcut alone.
- Validate any harness against a known-good baseline (app stopped) first.
  Check `SendInput`'s return value; the x64 C# `INPUT` struct is 40 bytes.
- `SetForegroundWindow` is blocked for the agent — ask the user to click the
  target window.
- Logs: `%APPDATA%\Spaceadom\debug.log` — fastest way to see engine
  decisions. Config: `config.json` next to it. NOTE: **the agent shell runs inside an MSIX container that redirects
  `%LOCALAPPDATA%` AND virtualises `HKCU` (PROBLEM 143).** This is not limited
  to reading a stale log folder — it silently swallowed an entire session's
  worth of INSTALLS. `setup.exe /S` from this shell installs into
  `…\Packages\Claude_*\LocalCache\Local\`, and every check made from the
  same shell (version stamp, byte size, ASCII marker, registry read-back) then
  agrees with itself and is wrong. The real machine had no app and no Run key
  while every verification passed. To touch or verify the REAL machine, go
  through `explorer.exe`, which runs outside the container:
  `Start-Process explorer.exe -ArgumentList 'C:\path	o\script.cmd'`.
  **A verification performed by the sandboxed process cannot detect the
  sandbox.**
- **PRINTING `%LOCALAPPDATA%` IS NOT PROOF THAT YOU ESCAPED THE CONTAINER**
  (measured 2026-08-27). MSIX redirects at the FILESYSTEM layer, so the path
  STRING is byte-identical inside and outside — an agent printed
  `C:\Users\beamu\AppData\Local` from an explorer-launched probe, concluded it
  had escaped, and was right only by luck: the same env var reads the same from
  inside. The env-var test cannot fail, which is exactly why it proves nothing.
  **The only valid proof is a DIFFERENTIAL** — read the SAME path string from
  both contexts and compare what comes back. The real measurement that night:
  `…\Local\Spaceadom\spaceadom.exe` returned **v1.0.53, 14,109,184 bytes**
  in-sandbox and **v1.0.86, 18,868,224 bytes** via `explorer.exe`. Two
  different files at one path is the only thing that demonstrates redirection.
  Generalise: **a check that cannot produce a negative result is not a check.**
  Validate any sandbox-escape technique against a known-redirected path before
  trusting it (see also "Verification techniques expire").
- **`config.json` is shadowed even though `debug.log` beside it is not**
  (observed twice, 2026-08-26/27). Both live in `%APPDATA%\Spaceadom\`, which
  is Roaming and NOT supposed to be redirected — yet `debug.log` tracks the real
  clock to the second while `config.json` returned a months-stale copy (47,754
  bytes, dated Aug 18) against a real file of 62,463 bytes. **So the usual tell
  — "this whole folder looks stale" — is absent.** Never diagnose from a
  `config.json` read in this shell. Copy it out via `explorer.exe` first, and
  cross-check its byte size against what `debug.log` says was last saved; if
  they disagree you are reading a shadow.
  **EXPLAINED 2026-09-05 (PROBLEM 250), and it is not arbitrary after all.** The
  redirection is a **copy-on-write UNION view, not a redirect**: a file the
  container has WRITTEN exists in its private store and shadows the real one;
  a file it has never written is not there and READS STRAIGHT THROUGH to the
  real user file. Measured, listing `%APPDATA%\Spaceadom` from this shell:
  `config.json` 47,754 B dated 18 Aug (the private copy) beside `debug.log`
  4,753,847 B and `picker-cache.json` 827,272 B both tracking today's clock (the
  real files) — and the container's own
  `…\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Spaceadom\` holds
  `config.json` and **no** `debug.log`. So the rule generalises: **in this shell,
  any file some in-container process has ever written to is stale; every other
  file in the same folder is live.** That is why the folder cannot look stale as
  a whole, and it is why per-file cross-checking is the only safe habit.
  Separately, and worth knowing before reasoning about MSIX at all:
  `GetCurrentPackageFullName` in this same shell returns
  `APPMODEL_ERROR_NO_PACKAGE` — **a process can be inside the redirection view
  with no package identity of its own**, so `packaged::is_packaged()` is not a
  test for redirection and must never be used as one.
- **SAFE MODE EXISTS SINCE PROBLEM 253, AND IT CAN LOOK LIKE A BUG.** If the
  app comes up with the dashboard but Space does nothing, no Guide HUD, no
  toasts, and `debug.log` has no `hook: rollover window` line — check the
  banner and `grep safe-mode debug.log` BEFORE diagnosing a dead hook. Three
  launches in a row that started and never stayed alive 30 seconds put the next
  one in safe mode: no `WH_KEYBOARD_LL`, no overlay window, no display watcher.
  The marker is
  `safe-mode-entered-after-three-consecutive-startup-crashes-spaceadom`.
  **To clear it:** press "Turn back on" in the banner (installs the hook
  immediately, no restart), or delete `%APPDATA%\Spaceadom\boot-attempts.json`,
  or set its `failed_starts` to 0 — through `explorer.exe`, never from the agent
  shell (PROBLEM 143). **A clean shutdown never counts**: `session_end.rs` and
  `lib.rs`'s `RunEvent` closure both call `safe_mode::note_clean_exit()`, and
  removing either is how three reboots shortly after logon would disarm a
  perfectly healthy app.
- Never report anything as fixed/working/verified unless you observed it
  working. Label untested things untested. Ask the user to hand-test what
  injection can't reach.

## Hard rules

- **MSIEXEC /X CAN DELETE THE LIVE APP.** An MSI product's file list may point
  ANYWHERE, including the current install folder — `wix/main.wxs` resolves
  `INSTALLDIR` from an HKCU `RegistrySearch` that NSIS fills with
  `%LOCALAPPDATA%\Spaceadom`, so a double-clicked `.msi` installs INTO the
  live per-user folder and registers those files as its own components. On
  **2026-09-04** the PROBLEM 238 banner offered to remove a "leftover" HKLM
  entry (`{C68DC702-9414-421F-A3E4-12EDBBAD76C5}`, DisplayVersion 1.0.94,
  InstallLocation = the live folder); `repair()` ran `msiexec /X{GUID}`; the
  Restart Manager failed to close the running 1.0.97 (RestartManager 10010,
  "SID does not match") and Windows Installer deleted the product's registered
  FILES anyway — the live `spaceadom.exe` — logging MsiInstaller 1034
  "removed the product … status 0". The app vanished; only Roaming config
  survived. **A "leftover entry" is removed from the REGISTRY ONLY, never
  through the Installer.** `msiexec` may only ever be aimed at a product whose
  recorded location is a different directory that is not an ancestor of ours,
  and even then with `/qn REBOOT=ReallySuppress MSIRESTARTMANAGERCONTROL=Disable`
  so Restart Manager can never close us. The decision lives in one pure,
  unit-tested function (`rival_install::plan_removal`) — PROBLEM 244.
  Generalise: **a removal that trusts a registry entry's own description of
  what it owns is a removal aimed by the thing being removed.**
- **AGENTS NEVER DELETE, RENAME OR OVERWRITE A FILE THEY DID NOT CREATE IN
  THIS TASK** — stale-looking or not, `.tmp`/`.bak`/`.old` or not, "obviously
  a leftover" or not. Report it and let the owner decide. A
  `PROJECT_STATUS.md.tmp` was deleted by an agent on 2026-09-04 on exactly that
  reasoning; the file it looked like a leftover of was 580 KB of append-only
  log, and nothing but luck decided whether the `.tmp` was garbage or the only
  copy of an in-flight write. The rule has no judgement clause on purpose:
  "it looked stale" is the sentence that precedes every one of these.
- Do NOT modify `D:\SpaceToggle-July_Revisit_2026`, `D:\GITHUB PROJECT`, or
  `D:\Neon` — read-only reference.
- Never remove a CORE_AIM feature to make something build.
- **Every solved problem gets TWO entries, and neither is optional.** The
  user's reason, in his words: *without documentation an AI has to start from
  scratch, and that costs a huge number of tokens.* Documentation here is a
  deliverable, not a courtesy.
  1. `PROJECT_STATUS.md` — dated, signed, newest at top, append-only. Never
     delete an entry. Say what happened and under what CONDITION it failed.
  2. `V14_FIXES_AND_CODE.md` — the technical record, in this exact shape:
     **Symptom → Root cause → Exact file → The actual code (paste it, before
     and after where it helps) → How it was verified.** Plus a
     "generalise this" line when the bug has a class.
     The test for a good entry: **could another AI apply the fix from this
     file alone, without opening the codebase to search for it?** If not, it
     is not finished.
- Record the *class* of a bug, not just the instance — "an ID rule that sets
  `display` needs its own `#id[hidden]` companion" is reusable; "the profile
  popover was open" is not.
- When a fix turns out to be unnecessary or a diagnosis was wrong, write that
  down too. `V14_FIXES_AND_CODE.md` §"measurement traps" exists because two
  false bug reports were nearly filed; that is worth more than a clean story.
- No CDN anything; the app must be fully offline.
- The user's laptop panel is **2560×1600 at 150%** (1707×1067 logical), and he
  plugs a SECOND display in and out through the day — display changes are
  routine here, not an edge case (PROBLEM 117/118). Both rings appear on
  the monitor the CURSOR is on (owner decision 2026-09-13, PROBLEM 267
  round 3 — reversing the earlier primary-only rule for the icon ring, and
  PROBLEM 169 for the Space ring); "primary" plays no role.
