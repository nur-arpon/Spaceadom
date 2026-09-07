# Spaceadom — Microsoft Store submission

*(This file's title deliberately carries no version number any more — an
earlier version of this doc said "1.0.72" in the title while the source tree
had moved to 1.0.100, which was confusing. Check `package.json` for the
actual current version; everything version-specific below says `<version>`
instead of a number for the same reason.)*

> **THERE ARE TWO ROUTES INTO THE STORE. `START-HERE.md` uses Route B, below
> — this section is kept for reference only; nothing here is part of the
> owner's actual remaining path.** Added 2026-09-05, PROBLEM 250.
>
> | | **Route A — signed EXE** | **Route B — MSIX package** |
> | --- | --- | --- |
> | What you submit | a **link** to a hosted `setup.exe` | the **`.msix` file** itself |
> | Who signs it | **you**, with a bought certificate | **the Store**, on ingestion |
> | Who updates it | Spaceadom's own updater (PROBLEM 245) | **the Store** |
> | Blocked on | **buying a code-signing certificate** | Partner Center identity values (the only remaining blocker — see `START-HERE.md`) |
> | Status | everything below is Route A, unchanged, not the chosen path | built and validated **and installed and hand-tested once**, 2026-09-05 — see "What `npm run msix` produced" and CLAUDE.md's MSIX section for the confirmed results |
>
> **Route B removes the one blocker Route A has never got past.** The whole of
> "Step 1 — SIGN (the only thing still outstanding)" below exists because an
> unpackaged submission must chain to a trusted CA, and that costs money every
> year. A Store-distributed MSIX is re-signed by Microsoft, so the certificate
> problem disappears — the local test certificate `npm run msix -- -Sign`
> creates never leaves the machine.
>
> **Route B's cost:** the app becomes a packaged app, which changes four of its
> behaviours (the updater goes inert, autostart moves to Windows, the config
> lives somewhere Windows decides, and the "remove the old copy" button is
> replaced by directions). All four are handled in `src-tauri/src/packaged.rs`.
> **Update, 2026-09-07: all four WERE hand-tested on a real packaged install on
> 2026-09-05** (CLAUDE.md's MSIX section, "LIVE TEST 2026-09-05") — hook,
> overlay, ring, launch and the StartupTask toggle all worked, and four defects
> found in that pass were fixed. The one thing still genuinely untested: **a
> machine with no pre-existing unpackaged Spaceadom on it** — the 2026-09-05
> test ran beside an existing NSIS install, and at least one finding (AppData
> not virtualising) depended on that folder already existing.
>
> **You do not have to choose today.** The two are not exclusive at the
> repository level: `npm run tauri build` still makes Route A's installer and
> `npm run msix` still makes Route B's package. You choose in Partner Center.

Everything in this folder is what you upload or paste. **The installer here is
NOT the one you give friends** — it is 210 MB because the WebView2 runtime is
embedded inside it, which the Store requires and a friend does not need.

> **Note, 2026-09-05, still true 2026-09-07:** the two files this table
> describes (`…-setup-STORE.exe` and `PRIVACY.md`) are **not currently in this
> folder**, and Route A below has not been kept in step with the source tree
> (currently 1.0.106, moving to 1.0.107) since it was written against 1.0.72.
> Nothing is lost — `npm run store` regenerates both — but do not submit from
> this folder until it has been re-run, and in any case **Route A is not the
> path `START-HERE.md` uses**. Route A's content below is left exactly as it was
> written; only this note and the routes table above are new.

| File | What it is |
| --- | --- |
| `Spaceadom_1.0.72_x64-setup-STORE.exe` | The submission binary. 209.8 MB. **Sign this before uploading.** |
| `PRIVACY.md` | The privacy policy. Host it at a public URL; the listing needs a link, not a file. |
| `LISTING.md` | Description, feature list, disclosure text and reviewer instructions — copy/paste into Partner Center. |
| This file | The checklist. |

Regenerate everything here with **`npm run store`**. Do not assemble it by
hand — the build fills this folder, which is the only way it stays in step with
the version you actually built.

⚠️ **After `npm run store`, run `npm run tauri build` before installing
locally.** The Store build takes over the normal installer path and the labeller
deliberately leaves it empty, so you get an obvious failure instead of silently
installing a 210 MB build on your own machine.

---

## Step 1 — SIGN (the only thing still outstanding)

Store Policy 10.2.9:

> "The binary and all of its Portable Executable (PE) files must be digitally
> signed with a code signing certificate that chains up to a certificate issued
> by a Certificate Authority (CA) that is part of the Microsoft Trusted Root
> Program."

**"All of its PE files" is not just the installer.** Sign, in this order:

1. `src-tauri/target/release/spaceadom.exe` — the app itself
2. then rebuild the installer so it wraps the signed exe
3. then `Spaceadom_<version>_x64-setup-STORE.exe` — the installer

Signing the installer alone leaves an unsigned `spaceadom.exe` inside it, which
is a rejection.

**Certificate options** — Azure Trusted Signing is the cheapest route in 2026
(Microsoft-run, monthly, no hardware token, individuals eligible after an
identity check). A traditional OV/EV certificate is several hundred USD a year
and EV ships on a physical token.

Verify before uploading:

```powershell
Get-AuthenticodeSignature .\Spaceadom_1.0.72_x64-setup-STORE.exe |
  Format-List Status, SignerCertificate
# Status must be "Valid"
```

This also removes the SmartScreen warning for everyone who downloads it
directly, so it is worth doing whether or not the Store submission goes ahead.

---

## Step 2 — Host the installer at a versioned URL

The Store does not accept an upload for unpackaged apps; you give it an HTTPS
link ([requirements](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msi/app-package-requirements)).

- **The bytes at that URL must never change.** Put the version in the URL and
  publish a NEW url for every update — a GitHub release asset is fine.
- One URL only. Submit the **`setup.exe`**, not the `.msi`: it installs
  per-user, never shows a UAC prompt, and the app's own duplicate-install
  detector would otherwise have two copies of itself to argue about.

---

## Step 3 — Fill in the listing

Everything to paste is in `LISTING.md`. The two that get submissions rejected
if skipped:

- **Privacy policy URL** — required for any app that touches user input.
- **Certification notes** — the reviewer must be told about the global keyboard
  hook and the process-closing feature *before* they find them. A remapper is
  perfectly acceptable (PowerToys is in the Store); an undisclosed global hook
  is not.

---

## Requirements already met — do not re-do these

| Requirement | Status |
| --- | --- |
| Installer is `.exe` or `.msi` | ✅ NSIS `.exe` |
| **Standalone installer, not a downloader stub** | ✅ `offlineInstaller` — verified by size: 209.8 MB vs the 5.6 MB bootstrapper build |
| Silent install supported | ✅ NSIS `/S`; every build is installed that way by `scripts/install-real.cmd` |
| UAC prompt allowed but not required | ✅ per-user install, no UAC at all |
| Publisher name ≠ product name | ✅ publisher `Nur Ifran Arpon`, product `Spaceadom` |
| Version numbering managed by the installer | ✅ from `package.json` |
| Privacy policy exists | ✅ `PRIVACY.md` — still needs hosting |

---

## Screenshots to capture (1366×768 or larger, PNG)

*(This four-shot brief is Route A's original one. The actual captures taken
so far — five of them — live in `assets\screenshots\`, are stale as of
2026-09-07, and need re-capturing; see that folder's `README.md` for the
current, authoritative shot list and replacement instructions.)*

The Store wants at least one; four tells the story:

1. **The dashboard, Earthy theme** — the keyboard with several keys bound.
2. **The radial guide** — hold Space so the ring of shortcuts is showing.
3. **A special-key card open** — press "Boss Key" in the bottom row.
4. **Starry night with Fun mode on** — the sky, the ship and the storm.

Take them on a clean profile so no personal app names appear.

---

## What a reviewer is most likely to ask

**"Why does this need a global keyboard hook?"** Because the feature *is* the
spacebar behaving differently everywhere. There is no way to make Space act as
a modifier in other applications without a system-wide hook. The disclosure
text in `LISTING.md` says so plainly.

**"Why can it terminate other processes?"** Only ever on the user's request,
twice-confirmed, and only for programs on a built-in list of keyboard
remappers, because only one program can own the spacebar. It never elevates
without showing the standard Windows prompt. Source: `conflict_close.rs`.

**"Does it record keystrokes?"** No. The hook reads key codes to decide
"typing or command" and keeps no history; the app contacts no server at all.
`PRIVACY.md` documents exactly what the local log contains and how to delete
it.

---
---

# Route B — the MSIX package

Added 2026-09-05 (PROBLEM 250). Everything above this line is Route A and is
unchanged.

The short version: **fill in three values from Partner Center, run one command,
upload one file.** The certificate problem that has held Route A up since
1.0.72 does not exist here, because the Store signs the package it distributes.

---

## The 15-line checklist — tick these in order

Full detail, click-by-click, for every line below is in
**`RUNBOOK.md`** (section numbers noted). This list is the whole job; nothing
below it in this file is a *new* step — it's the reasoning, the measurements,
and the second-machine test recipe behind these 15 lines.

1. [ ] Create the Partner Center account — individual, free, ID + selfie verification (RUNBOOK §1)
2. [ ] Wait for identity verification to clear before doing anything else (RUNBOOK §1.3 — the one step outside your control)
3. [ ] Reserve the product name "Spaceadom" (RUNBOOK §2)
4. [ ] Copy the three values from Product management → Product identity (RUNBOOK §3)
5. [ ] Paste them into `src-tauri\msix\identity.json` (copy from `identity.example.json` first) (RUNBOOK §4)
6. [ ] Run `npm run build`, then `npm run tauri build`, then `npm run msix` (RUNBOOK §5)
7. [ ] Confirm the `.msix` is ~10 MB, not ~200 MB — wrong size means `npm run store` ran somewhere upstream by mistake (RUNBOOK §5)
8. [ ] **Do not install the `.msix` on this machine** — see "DO NOT INSTALL" below
9. [ ] Real screenshots already sit in `to-publish-in-microsoft-store\assets\screenshots\`, but they are from an OLD build (v1.0.101) and show the old full-width conflict cards and a stale version number — **re-capture all five from the current build before uploading** (see `assets\screenshots\README.md`)
10. [ ] Pricing and availability: Free, all markets, defaults otherwise (RUNBOOK §6.1)
11. [ ] Properties: category Productivity / secondary Utilities + tools, privacy policy URL, website, support URL (RUNBOOK §6.2)
12. [ ] Age ratings: answer the IARC questionnaire — every follow-up "no" (RUNBOOK §6.3, `LISTING.md`)
13. [ ] Packages: upload the `.msix`; expect (and ignore) a runFullTrust notice here — the justification goes on Submission options, not here (RUNBOOK §6.4)
14. [ ] Store listings: paste every field from `LISTING.md`, upload the images named in `assets/README.md` (RUNBOOK §6.5)
15. [ ] Submission options: paste the certification notes into both the general notes box and the Restricted capabilities box, then click **Submit for certification** (RUNBOOK §6.6, §7)

---

## What `npm run msix` produced, measured

Run on the dev machine on 2026-09-05 against the **1.0.100** release binary
(the source tree has since moved on — check `package.json` for the current
version; the numbers below are historical evidence that the pipeline works,
not a live measurement of today's build), with a deliberately fake local
identity (see "What is still on you" below):

| | |
| --- | --- |
| Package | `src-tauri/target/release/bundle/msix/Spaceadom_<version>_x64.msix` (was `Spaceadom_1.0.100_x64.msix` when measured) |
| Size | **10,652,847 bytes unsigned / 10,655,808 signed** (10.2 MB) as of 1.0.100 — expect a similar size at the current version |
| Contents | 9 files: `spaceadom.exe` (21,237,760 B), `spaceadom.pdb`, and six logos under `Assets\` |
| Layout on disk | 31.6 MB before compression |
| Packed with | `makeappx.exe` from Windows SDK **10.0.26100.0** |
| Validated by | `makeappx unpack` round-trip, SHA-256 per file, manifest structure, asset resolution |

For comparison: Route A's Store installer is **~210 MB**, because it has to
carry the whole WebView2 offline runtime. The MSIX is 10 MB because it does
not — see "The WebView2 decision" below, which is the one real trade.

---

## The four steps

### 1. Get the identity values from Partner Center

Partner Center ▸ Spaceadom ▸ **Product management ▸ Product identity**. That
page lists exactly three things:

| Partner Center label | Goes into |
| --- | --- |
| `Package/Identity/Name` | `identity.json` → `name` |
| `Package/Identity/Publisher` | `identity.json` → `publisher` (starts `CN=`) |
| `Package/Properties/PublisherDisplayName` | `identity.json` → `publisherDisplayName` |

### 2. Write them into `src-tauri/msix/identity.json`

```powershell
copy src-tauri\msix\identity.example.json src-tauri\msix\identity.json
notepad src-tauri\msix\identity.json
```

The file is **gitignored** (verified with `git check-ignore` before it was ever
created). The version is not in it — that comes from `package.json` and is
written as `a.b.c.0`, because the Store reserves the fourth field.

### 3. Build

```powershell
npm run build          # tsc + vite, as always
npm run tauri build    # the PLAIN release exe - NOT npm run store
npm run msix           # lay out, pack, validate
```

**`npm run tauri build`, not `npm run store`.** Route A's Store build exists to
embed the WebView2 offline *installer*, and you cannot run an installer from
inside a package, so it has nothing to contribute. `npm run msix` packs the
plain release binary. The script refuses to run if that binary's `FileVersion`
disagrees with `package.json`.

`npm run msix -- -Sign` additionally signs with a self-signed certificate it
creates into `src-tauri/msix/test-signing.pfx` (also gitignored). **That is for
structural testing only.** The Store re-signs. Without `-Sign` the package is
unsigned and Windows will refuse to install it, which is deliberate — see the
next section.

### 4. Upload

Partner Center ▸ Packages ▸ upload the `.msix`. Nothing else from this folder
is Route B's; `LISTING.md`'s description, disclosure text and certification
notes are the same either way, and the **privacy policy URL is still required**.

CI also builds it: the release workflow's last two steps produce the `.msix` and
attach it to the **workflow artifacts only, never to the GitHub release** — a
`.msix` on the releases page would give somebody a second copy beside the
`setup.exe` they already have. Add the three `MSIX_IDENTITY_*` repository
secrets to switch that on; without them the step logs a notice and skips, and
the `.exe`/`.msi` release is unaffected.

---

## DO NOT INSTALL THE .MSIX ON THE DEV MACHINE

This is a hard boundary, and the build script is built around it — no
`Add-AppxPackage` anywhere, unsigned by default.

**Why:** the dev machine already runs the NSIS Spaceadom. A packaged copy
alongside it is two processes, each installing a `WH_KEYBOARD_LL` hook, each
starting at logon, both wanting the spacebar. That is PROBLEM 129/141/236, and
the app's own rival-install banner exists because it has happened before.

### The second-machine test recipe

On a **different** Windows 11 machine with **no Spaceadom on it**:

```powershell
# 1. Trust the test certificate (LOCAL TEST MACHINE ONLY - this is a real
#    trust decision; do not do it on a machine that matters, and undo it after).
#    Copy src-tauri\msix\test-signing.pfx over, then:
Import-PfxCertificate -FilePath .\test-signing.pfx `
  -CertStoreLocation Cert:\LocalMachine\TrustedPeople `
  -Password (ConvertTo-SecureString "spaceadom-local-test" -AsPlainText -Force)

# 2. Install
Add-AppxPackage .\Spaceadom_<version>_x64.msix

# 3. Confirm it really is packaged - this is the whole point of the trip
Get-AppxPackage *Spaceadom* | Format-List Name, PackageFullName, InstallLocation, Version
```

Then check each of the four packaged behaviours. **Every one of these is
currently unverified**, so this list is the test plan, not a formality:

| # | What to check | How | Expected |
| --- | --- | --- | --- |
| 1 | It knows it is packaged | `Select-String "package-identity-probe" $env:APPDATA\Spaceadom\debug.log` | one line saying **PACKAGED** and naming the package full name |
| 2 | The updater is inert | same log, near it | a line saying the check is skipped because the Store owns updates |
| 3 | Autostart is the startupTask | Settings ▸ Apps ▸ Startup | **Spaceadom listed and On**; and `HKCU\...\Run` has **no** `Spaceadom` value |
| 4 | The startup switch tells the truth | turn Spaceadom **off** in Task Manager ▸ Startup apps, then open Spaceadom's Settings | the "Run at startup" row is **greyed with a sentence explaining Windows is holding it off** — not a switch that flips back |
| 5 | Config survived | dashboard | your profiles and bindings are there; the log has a `FIRST PACKAGED LAUNCH` line saying what it copied |
| 6 | The rival banner is honest | install the NSIS copy too, deliberately, on this test machine | the banner says the Store version cannot remove the other one and offers **"Open Installed apps"** |
| 7 | It actually works | hold Space, tap a bound key | the app launches. If the window never appears, suspect WebView2 — see below |

### Run the Windows App Certification Kit there too

The WACK is what Partner Center runs against your package anyway, so finding
its complaints on your own machine is strictly cheaper:

```powershell
$appcert = "C:\Program Files (x86)\Windows Kits\10\App Certification Kit\appcert.exe"
& $appcert reset
& $appcert test -appxpackagepath .\Spaceadom_<version>_x64.msix -reportoutputpath .\wack-report.xml
```

Expect it to have opinions about the missing scaled-asset variants (see below).
Read the report; do not assume a pass.

### Uninstall afterwards

```powershell
Get-AppxPackage *Spaceadom* | Remove-AppxPackage
# and remove the test certificate you trusted in step 1
Get-ChildItem Cert:\LocalMachine\TrustedPeople |
  Where-Object { $_.Subject -like "*LOCAL-TEST*" } | Remove-Item
```

---

## The WebView2 decision, and the risk it leaves

**The package does not contain a WebView2 runtime.** The reasoning, in full, is
in the header comment of `src-tauri/msix/AppxManifest.xml`; the summary:

- You cannot run an installer from inside a package, so Route A's embedded
  offline bootstrapper is unreachable here.
- The Evergreen runtime is a component of Windows 11 and is on all but a small
  number of Windows 10 machines.
- `win32dependencies:ExternalDependency` is honoured only by the App Installer
  app, **not** by Store installs, so it would do nothing for this case.

**The residual risk, stated plainly:** on a Windows 10 machine with no Evergreen
runtime, the packaged Spaceadom starts and shows no window. Step 7 of the test
plan is where you would see it.

**If you decide that is unacceptable**, the supported fix is the WebView2
**Fixed Version** runtime laid into the package — roughly **+250 MB**, taking
the `.msix` from 10 MB to ~260 MB. That is your call, not a default, and it is
the only reason this decision is written down at this length.

---

## What is still on you

1. **The three Partner Center values.** `src-tauri/msix/identity.json` currently
   holds deliberately fake ones (`LOCALTEST.Spaceadom`,
   `CN=LOCAL-TEST-SPACEADOM-NOT-A-REAL-PUBLISHER`) so that the pipeline could be
   run and validated without an account. **`build-msix.ps1` prints a large
   yellow warning on every run while they are still there.** A package built
   with them is structurally perfect and will be rejected at ingestion.
2. ~~Deciding between Route A and Route B, or doing both.~~ **Already decided
   — `START-HERE.md` uses Route B exclusively.** Left here only as a record
   of when this was still open.
3. **A clean rebuild.** The binary inside the package that was validated on
   2026-09-05 was built by a concurrent agent from the shared working tree at
   09:00:26, so it carries other in-flight work as well as this. It is a fine
   structural payload and a poor submission one. Run `npm run build` then
   `npm run tauri build` then `npm run msix` in one go before submitting —
   this also picks up the 2026-09-07 keyboard-hook fix noted in
   `START-HERE.md`'s "Where things stand," which is not yet in any built
   package.
4. ~~The second-machine test. Nothing in the table above has been
   observed.~~ **Update, 2026-09-07: this WAS run, on 2026-09-05** — see
   CLAUDE.md's MSIX section, "LIVE TEST 2026-09-05" — hook, overlay, ring,
   launch and the StartupTask toggle all worked; four defects found in that
   pass were fixed. **What genuinely remains untested: a machine with no
   pre-existing unpackaged Spaceadom on it** — the 2026-09-05 run was beside
   an existing NSIS install, and at least one result (AppData not
   virtualising) depended on that folder already existing.
5. ~~The privacy policy URL — still required, still unhosted (Route A, Step
   3).~~ **It is hosted** — `PRIVACY.md` is a public file in this repo and
   the URL in `LISTING.md` resolves (verified live, 200 OK) — **but the repo
   has been uncommitted since 1.0.95, so the live page is a stale version
   until the `git push` in `START-HERE.md` §E happens.**
6. **The WebView2 fixed-runtime question**, if step 7 ever fails.

## What is deliberately simplified, and might draw a WACK warning

- **No `resources.pri`, no `.scale-200` assets.** Only 100%-scale logos are
  generated. Scaled variants are inert without a `makepri.exe` step, so
  shipping them would look like correctness and be nothing. Windows scales the
  100% assets instead. If the WACK or the listing wants crisper tiles, the fix
  is a makepri step.
- **x64 only.** No arm64 slice and no `.msixbundle`. The app has never shipped
  an arm64 build through any route.
