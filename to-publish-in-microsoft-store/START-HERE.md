# START HERE — submitting Spaceadom to the Microsoft Store

One page, in the order you'll actually do things. Longer files behind this
one: `RUNBOOK.md`, `SUBMIT-CHECKLIST.md` ("Route B" section), `LISTING.md`.
This page alone is enough to finish the job.

**Where things stand:** the version keeps moving while this is being
prepared — **check `package.json`'s `"version"` field for the exact current
number** rather than trusting a figure written here, and use that same
number everywhere below (`Spaceadom_<version>_x64.msix`, the git tag, etc).
As of this writing it's **1.0.106**, with a code fix for a keyboard-hook edge
case (the radial guide could stop appearing for a couple of minutes under a
rare condition) already written but not yet built or installed — whatever
build you run today will include it, and it'll likely ship as **1.0.107**.
Version 1.0.101 already passed a full install-and-run test on this machine
(MSIX installed, launched, hook and overlay both worked). You don't need to
re-test all of that — see §F for the short list still owed, and the "State
of play" section at the end of this file for the full picture.

---

## A. Before you start (5 minutes)

Cleanup from testing on this machine. None of it affects the package you'll
build in §C.

**1. Remove the local test certificate.** This machine trusted a fake
certificate to install test copies of the app; it has no purpose now (the
Store signs the real package). Open PowerShell **as Administrator**:

```powershell
Get-ChildItem Cert:\LocalMachine\TrustedPeople |
  Where-Object Thumbprint -eq '710EB524F40B8233E6D485BCCC649839295BA0A1' |
  Remove-Item
```

**2. Optional — the matching private-key copy.** The build script also left
a private test certificate here (used only to *make* test packages, never to
trust them). Harmless to leave; if you want it gone too, no admin needed:

```powershell
Get-ChildItem Cert:\CurrentUser\My |
  Where-Object FriendlyName -eq 'Spaceadom LOCAL MSIX TEST - not for distribution' |
  Remove-Item
```

**3. Optional — two leftover test files**, harmless byproducts the app never
reads again: `%APPDATA%\Spaceadom\packaged-first-run.txt` and
`%APPDATA%\Spaceadom\packaged-migration\` (a folder). Delete or ignore.

**4. Ignore — `debug.log` has a few foreign-looking blocks in it**, appended
by test/probe scripts during this machine's testing. They don't match the
normal log format but are harmless; the app never reads its own log back.

---

## B. Partner Center — your part

The one part nobody can do for you: proving who you are and picking the name.

1. **Create the account.** Go to **https://storedeveloper.microsoft.com**
   (this exact URL — the only entry point for the free flow; any other link
   shows the old $19 flow). **Get started for free** → **Individual developer
   (free)** → sign in with a Microsoft account.
2. **Identity verification.** Government-issued ID + a selfie, on a phone, in
   good light, with the real physical document. **Start this first, today** —
   it's the one step outside your control and Microsoft doesn't publish a
   turnaround time. Everything below waits on this.
3. **Reserve the name.** Once verified: `https://aka.ms/submitwindowsapp` →
   **New product** → **MSIX or PWA app** → type `Spaceadom` → **Check
   availability** → **Reserve product name**.
4. **Copy three values.** Spaceadom product → **Product management** →
   **Product identity**:

   | Partner Center field | Goes into `identity.json` field |
   | --- | --- |
   | `Package/Identity/Name` | `name` |
   | `Package/Identity/Publisher` (starts `CN=`) | `publisher` |
   | `Package/Properties/PublisherDisplayName` | `publisherDisplayName` |

5. **Paste them in.** From the repo root:

   ```powershell
   copy src-tauri\msix\identity.example.json src-tauri\msix\identity.json
   notepad src-tauri\msix\identity.json
   ```

   Fill in exactly those three fields; leave the rest (`_README` is just
   documentation) alone. This file is gitignored — never committed.

   **Do not skip this and build anyway** — a build without real values prints
   a large warning and Partner Center rejects it at upload (identity mismatch).

---

## C. Build the Store package

From the repo root, in **PowerShell** (not the Bash tool — a known issue
rewrites the signing password there, see CLAUDE.md):

```powershell
$env:CARGO_HOME="D:\RUST-DOWNLOADED-HERE\cargo"
$env:RUSTUP_HOME="D:\RUST-DOWNLOADED-HERE\rustup"
$env:PATH="D:\RUST-DOWNLOADED-HERE\cargo\bin;$env:PATH"

npm run build          # tsc + vite frontend build
npm run tauri build    # the plain release exe
npm run msix           # lays out, packs, and validates the .msix — UNSIGNED
```

**Not `npm run store`** — that's a different, ~200 MB build for a route this
project isn't using. **Do not pass `-Sign`** — that flag is only for testing
on a second machine before you have an account; the real submission goes up
**unsigned**, because the Store re-signs every package it distributes.

**Expected result:** `src-tauri\target\release\bundle\msix\Spaceadom_<version>_x64.msix`
(`<version>` is whatever `package.json` currently says — e.g. `1.0.107`),
roughly **11 MB** (if it's ~200 MB, `npm run store` ran somewhere upstream by
mistake). The build script prints its own verdict — look for exactly:

```
build-msix: VALIDATION PASSED.
```

`build-msix: VALIDATION FAILED` means stop and don't upload.

**Do not install this `.msix` on this machine** — it already runs the
regular copy, and two copies fight over the spacebar. To test-install, see
`RUNBOOK.md` §5 / `SUBMIT-CHECKLIST.md`'s second-machine recipe.

---

## D. Submit

Click-by-click detail: `RUNBOOK.md` §6–7. The whole 15-line checklist, in
order:

1–8. Account, verification, name reservation, the three identity values,
`identity.json`, build, confirm ~11 MB, don't install locally — all done
above, §B–C.
9. Screenshots already copied into `assets\`: `01-dashboard.png` through
   `05-launch.png`.
10. **Pricing and availability:** Free, all markets, everything else default.
11. **Properties:** Category = Productivity, Secondary = Utilities + tools;
    paste Privacy policy URL, Website, Support URL from `LISTING.md`.
12. **Age ratings:** IARC questionnaire, every question **No** except "does
    your app collect personal information?" → **Yes** (it sends an optional,
    off-switchable, identifier-redacted crash report — real data collection
    even with no account or analytics). Exact wording: `LISTING.md`'s Age
    rating section.
13. **Packages:** upload the `.msix`. Expect and ignore a "runFullTrust"
    notice here — the justification goes in step 15.
14. **Store listings:** paste every field from `LISTING.md` (description,
    short description, features, search terms, copyright). Images:
    - Screenshots: `assets\01-dashboard.png`–`05-launch.png`
    - Store logo (300×300): `assets\StoreLogo-300x300.png`
    - 1:1 box art (1080×1080), if the slot appears: `assets\AppTile-1080x1080.png`
    - 16:9 hero (optional): `assets\Hero-1920x1080-textfree.png` — the
      **text-free** one, never the tagline one
15. **Submission options:** paste `LISTING.md`'s certification-notes block
    into **both** "Notes for certification" and "Restricted capabilities"
    (the latter appears because the app declares `runFullTrust`). Then
    **Submit for certification**.

**Expected certification time:** up to 3 business days typically, though
Microsoft's FAQ says many clear in a few hours — runs in the background.

**The two questions a reviewer is most likely to ask** (full answers in
`LISTING.md`'s certification notes):

1. *"Why `runFullTrust` / a global keyboard hook at all?"* — The feature
   itself is Space behaving differently in every application, which no
   sandboxed process can do. No narrower capability grants this.
2. *"It can close other running programs — is that a risk?"* — Only on the
   user's explicit, twice-confirmed request, only for a fixed built-in list
   of known keyboard-remapping programs, always a polite close (WM_CLOSE)
   before forcing, never elevates without the standard Windows prompt.

---

## E. After submission (optional, same day)

Publishes the current version on GitHub for non-Store users, and is what
lets existing older installs self-update to it.

**Before tagging:** `package.json`, `src-tauri/tauri.conf.json`, and
`src-tauri/Cargo.toml` must all already read the **same** version number —
CI checks all three against the git tag and refuses to build on a mismatch.
Read that number out of `package.json` and use it for both the commit
message and the tag below (do not hand-type a number from memory — it's
what caused this doc to go stale the last several times).

```powershell
$ver = (Get-Content package.json -Raw | ConvertFrom-Json).version
git add -A
git commit -m "$ver"
git tag "v$ver"
git push
git push --tags
```

**The tree has been uncommitted since 1.0.95** (HEAD is `ad23e00`), so this
first commit will be unusually large — expected, not a mistake.

**What CI then does** (`.github/workflows/release.yml`, ~10 minutes): builds
both installers, signs the update files, creates a draft release, writes the
two updater manifests, uploads everything, then flips the release from draft
to public. Watch the repo's Releases page.

---

## F. Hand-tests still owed

Need a human — nothing an AI can click through — on the **installed** app:

1. **Win+.** in Notepad while Spaceadom runs — confirm no interference; check
   `debug.log` for lines starting `emoji panel:`.
2. **Drag a shortcut from one key onto another** — confirm it reassigns, not
   duplicates.
3. **"Double" ring layout** on the profile `sexy_tumar_mexy` — confirm the
   radial guide draws as two rings.
4. **Theme = Auto** — flip Windows light/dark (Settings → Personalisation →
   Colours) with the dashboard open; both the dashboard and the ring should
   follow within a couple of seconds, no restart.
5. **First-run tour on a fresh config** — rename/delete
   `%APPDATA%\Spaceadom\config.json` (back it up first) and confirm the tour
   card appears.

---

## G. If something goes wrong

- **Logs:** `%APPDATA%\Spaceadom\debug.log` — fastest way to see what the app
  actually did.
- **Shortcuts die mid-session:** `docs\IF-SHORTCUTS-DIE-AGAIN.md` — this
  exact symptom happened before; full diagnosis and a one-line check for
  whether it's the same bug.
- **App won't start reliably (safe mode):** after three failed launches in a
  row it disables its own hook and overlay and shows a banner — **"Turn back
  on"** (re-enables immediately) and **"Report a problem"** (writes a
  scrubbed zip to `%APPDATA%\Spaceadom\reports\`, never uploads anything).
- **Need an older version:** every past installer is kept in `all-versions\`
  — pick one and reinstall it normally (`setup.exe`).

---

## H. State of play (audited 2026-09-07)

**DONE:**

- The MSIX build pipeline (`npm run msix`, `scripts/build-msix.ps1`) is
  written, tested against a local placeholder identity, and produces a
  validated, correctly-sized (~10-11 MB) unsigned package.
- `identity.example.json`'s three fields match exactly what
  `build-msix.ps1` reads and what `AppxManifest.xml` needs.
- All required Store art exists at the right pixel dimensions: `StoreLogo-
  300x300.png` (300×300), `AppTile-1080x1080.png` (1080×1080), both hero
  images (1920×1080), `Hero-2400x1200.png` (2400×1200) — all verified by
  reading each file's own PNG header, not by filename.
- `LISTING.md`'s copy has been cross-checked against the current source for
  every factual claim (special keys, profiles, themes, the update-checker's
  packaged-mode gate, what data leaves the machine) and corrected where the
  app had moved on since it was written — see this audit's full report for
  specifics; the version in your hands may have moved on again since.
- `PRIVACY.md`'s claims (Sentry redaction, the "Don't send logs" opt-out,
  the daily update-check interval, no keystroke/binding data in any
  outbound payload) were checked against the actual Rust source and match.
- The repo's git remote and `LISTING.md`'s Privacy-policy/Website/Support
  URLs agree (`github.com/nur-arpon/Spaceadom`), and the Privacy-policy URL
  currently resolves (verified live, 200 OK) — see the caveat below.
- The manifest's `TargetDeviceFamily MinVersion` (10.0.17763.0 / Windows 10
  1809) matches what `LISTING.md`'s System requirements section claims.

**PENDING (in the order you'd hit them):**

1. **A fresh build has not been made or installed since the latest code fix
   landed** (a keyboard-hook edge case where the radial guide could stop
   appearing for a couple of minutes — fixed in code 2026-09-07, per
   `PROJECT_STATUS.md`'s newest entry, but not yet built or installed by
   anyone). Nothing for you to do here except run the normal §C build —
   the fix is already in the working tree and will be included automatically.
2. **All five screenshots in `assets\screenshots\` are stale** — captured
   2026-09-05 from v1.0.101. They show the old full-width Settings conflict
   cards (replaced by a compact grid in 1.0.106) and `04-about.png` has the
   literal text "v1.0.101" baked into the image. **These need re-capturing
   from whatever build you actually ship, before you upload them.** Full
   per-file replacement list: `assets\screenshots\README.md`'s top section.
3. **The GitHub repo has been uncommitted since 1.0.95** (current HEAD is
   `ad23e00`). This means the *live* `PRIVACY.md` a Store reviewer or
   customer reaches by clicking the Privacy-policy URL today is the stale
   1.0.95 version, not the current one this audit checked — the URL
   resolves (it's not a broken link), but its content is behind. This
   self-corrects the moment §E's commit-and-push happens; until then, treat
   the live copy as **not yet representative**.
4. **`src-tauri/msix/identity.json` does not exist yet** (only the
   `.example.json` template does) — this is expected; it's the file that
   needs your three Partner Center values.
5. Everything in §F ("Hand-tests still owed") is, by definition, still owed.

**Exact remaining owner actions, in order:**

1. Register on Partner Center and start identity verification (§B.1–2) —
   do this literally first, it's the one step outside your control.
2. Reserve the product name `Spaceadom` (§B.3).
3. Copy the three identity values from Product management → Product
   identity into `src-tauri\msix\identity.json` (§B.4–5).
4. Run the three build commands (§C). This also picks up the pending code
   fix from PENDING item 1 automatically — nothing extra to do for that.
5. Re-capture the five screenshots against this new build and drop them
   into `assets\screenshots\`, replacing the stale ones (PENDING item 2,
   full detail in that folder's `README.md`).
6. Upload and fill in the submission wizard using `LISTING.md` (§D), then
   **Submit for certification**.
7. Optional, same day: commit, tag, and push (§E) — this is also what
   fixes PENDING item 3 (the stale live `PRIVACY.md`).

---

*Cross-checked 2026-09-07 against `package.json` (version 1.0.106),
`scripts/build-msix.ps1`, `identity.example.json`,
`.github/workflows/release.yml`, `PROJECT_STATUS.md`'s newest entry, the
actual PNG headers of every file in `assets/` and `assets/screenshots/`, the
live `git log`/`git status` of this repo, and a fetch of the public
Privacy-policy URL. Not independently verified here — see `RUNBOOK.md`'s
"What this runbook could not confirm" section: per-field character limits,
the exact Product-declarations checklist, whether the 1080×1080 image slot
appears for a Productivity app.*
