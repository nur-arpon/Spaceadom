# Spaceadom 1.0.72 — Microsoft Store submission

Everything in this folder is what you upload or paste. **The installer here is
NOT the one you give friends** — it is 210 MB because the WebView2 runtime is
embedded inside it, which the Store requires and a friend does not need.

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
