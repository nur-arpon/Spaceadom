# RUNBOOK — Microsoft Store submission, click by click

Written 2026-09-05 against Microsoft's live documentation (every section
below names the page it came from, with the fetch date, so a stale claim is
easy to spot later). This is Route B — the **MSIX** route described in
`SUBMIT-CHECKLIST.md` — because it removes the one blocker Route A has never
got past (a paid code-signing certificate): the Store signs the package it
distributes, so no certificate purchase is needed at all.

**What the owner does, and nothing else:** create the account, reserve the
name, copy three identity values, paste them into one file, run three
commands, then click through a wizard that is 90% already filled in by this
document and `LISTING.md`. Everything else in this runbook is either already
done (the assets, the copy, the manifest) or is a "click Next" step spelled
out below so there's nothing to figure out live.

---

## 0. The two-day budget, mapped

| When | What | Owner time |
| --- | --- | --- |
| Day 1, first thing | §1 Create the developer account | 10–20 min active, **plus identity-verification wait — see the warning in §1.3** |
| Day 1 | §2 Reserve the product name | 2 min |
| Day 1 | §3–4 Copy the three identity values, paste into `identity.json` | 5 min |
| Day 1 | §5 Build the package | 5 min, one command chain |
| Day 1 | §6 Fill in the submission wizard | 30–45 min, mostly pasting from `LISTING.md` |
| Day 1 (or as soon as ready) | §7 Submit for certification | 1 click |
| Day 1→3 | Certification runs in the background | **up to 3 business days** — not owner time, but budget for it |
| Whenever it passes | Publishing | automatic, ~15 minutes after certification passes |

**The one item that can blow the two-day budget is identity verification in
§1.3**, because it depends on Microsoft's review, not on anything the owner
controls. Start §1 first, before anything else, for exactly that reason.

---

## 1. Create the Partner Center developer account (individual, free)

Source: [Free developer registration for individual developers](https://learn.microsoft.com/en-us/windows/apps/publish/whats-new-individual-developer)
(Microsoft Learn, fetched 2026-09-05, page dated current as of 2026-08-07).
**Confirmed: the $19 individual registration fee is waived** in this flow —
the page states it in plain language: "The $19 registration fee is waived in
the new flow."

### 1.1 The one supported entry point

Go to **https://storedeveloper.microsoft.com** — this is called out as "the
only supported entry point for the new \[free\] flow." Reaching account
creation any other way (direct Partner Center link, Visual Studio, Xbox)
shows the old, paid flow instead.

### 1.2 Click through

1. Click **"Get started for free."**
2. Choose **Individual developer (free)** — not "Company account."
3. Sign in with an existing Microsoft account, or create a new one.

### 1.3 Identity verification — the step that can eat the timeline

4. **Begin identity verification with a government-issued ID and a selfie.**
   Do this on a phone, in good lighting, with the original physical document
   (not a photo of a photo).
5. Review the auto-filled profile info pulled from the ID and correct
   anything wrong.
6. Finish account setup and click **"Go to Partner Center dashboard."**

**Microsoft's own docs do not state a turnaround time for this
verification step**, and nothing else fetched during this research pass
gave one either — **verify in the wizard / by simply doing it**, and do it
literally first, before writing a single Partner Center field, because it is
the one part of this whole process outside the owner's control. If it takes
more than a few hours, everything downstream in this runbook still works
identically once it clears — nothing else in the two-day plan depends on
exact timing here, only on it finishing before Day 1 is over.

### 1.4 After verification

7. You'll be prompted for the Microsoft account picker — pick the **same**
   account used to start the flow.
8. Once signed in you land on the **Apps & Games overview** page. If it
   doesn't appear immediately, Microsoft's doc says to **wait ~5 minutes and
   refresh**, or go directly to
   [the Partner Center apps and games page](https://aka.ms/submitwindowsapp).

That's account creation. No credit card, no yearly fee, nothing to buy.

---

## 2. Reserve the product name

Source: [Reserve your MSIX app's name](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/reserve-your-apps-name)
(fetched 2026-09-05).

1. On the [Partner Center apps and games page](https://aka.ms/submitwindowsapp),
   click **New product**.
2. Click **MSIX or PWA app** (not "Game" — Spaceadom isn't one).
3. Type `Spaceadom` and click **Check availability**. A green check mark
   means it's free.
4. Click **Reserve product name**.

That's it — this single click is also what creates the app entry that
everything else in this runbook attaches to. Two things worth knowing:

- **The reservation holds for three months** if you don't publish. Not a
  concern here, but if this stalls past that window, redo this step.
- Reserving the name does **not** by itself set `Package/Identity/Name` (the
  MSIX identity string) — that's a *related but separate* value you'll copy
  in §3. Reserving `Spaceadom` as the display name is what makes the
  Product identity page (next) exist to read from.

---

## 3. Find the three identity values

`src-tauri/msix/AppxManifest.xml` needs exactly three account-specific
strings before a package can be built, and `identity.example.json`'s own
header comment already names where to find them — this section just
confirms that's still accurate and adds the click path:

1. In Partner Center, open the **Spaceadom** product you just reserved.
2. In the left nav, expand **Product management**.
3. Click **Product identity**.

That page lists exactly three values, ready to copy:

| Partner Center label | Goes into `identity.json` | Looks like |
| --- | --- | --- |
| `Package/Identity/Name` | `name` | `12345NurIfranArpon.Spaceadom` |
| `Package/Identity/Publisher` | `publisher` | `CN=` followed by a GUID |
| `Package/Properties/PublisherDisplayName` | `publisherDisplayName` | `Nur Ifran Arpon` |

This exact page location (**Product management → Product identity**) was
also independently written into `SUBMIT-CHECKLIST.md` and
`identity.example.json`'s own comments earlier the same day this runbook was
written — three independent passes agreeing is as close to confirmed as this
gets without a live account to click through. If Partner Center's UI has
moved this by the time you read it, the values themselves (Identity Name,
Identity Publisher, PublisherDisplayName) are what to search Partner Center's
own in-product search for.

---

## 4. Paste the three values into `identity.json`

```powershell
copy src-tauri\msix\identity.example.json src-tauri\msix\identity.json
notepad src-tauri\msix\identity.json
```

Fill in exactly the three fields the file already has placeholders for:

```json
{
  "name": "<Package/Identity/Name from Partner Center>",
  "publisher": "<Package/Identity/Publisher from Partner Center — starts CN=>",
  "publisherDisplayName": "<Package/Properties/PublisherDisplayName from Partner Center>"
}
```

Leave everything else in the file alone — the `_README` block is
documentation, not a field, and the version comes from `package.json`
automatically. This file is gitignored; it never gets committed and never
needs to be, because these three values are meaningless without your Partner
Center account attached.

**Do not skip this and build anyway.** `build-msix.ps1` will run and produce
a structurally valid `.msix` using the placeholder `LOCALTEST.Spaceadom` /
`CN=LOCAL-TEST-SPACEADOM-NOT-A-REAL-PUBLISHER` values still sitting in
`identity.json` from earlier testing, printing a large yellow warning as it
does — and Partner Center will reject that package at ingestion with an
identity-mismatch error. The warning exists precisely so this isn't a silent
mistake; read it if it appears.

---

## 5. Build the package

Three commands, in this order, from the repo root:

```powershell
$env:CARGO_HOME="D:\RUST-DOWNLOADED-HERE\cargo"
$env:RUSTUP_HOME="D:\RUST-DOWNLOADED-HERE\rustup"
$env:PATH="D:\RUST-DOWNLOADED-HERE\cargo\bin;$env:PATH"

npm run build          # tsc + vite frontend build
npm run tauri build    # the plain release exe — NOT npm run store
npm run msix           # lays out, packs, and validates the .msix
```

**Not `npm run store`.** That target exists only for Route A (it embeds the
~200 MB offline WebView2 installer, which nothing can run from inside a
package). `npm run msix` packs the plain release binary from `npm run tauri
build` — see "The WebView2 decision" in §8 below for why that's the correct
choice and not a shortcut.

The result lands at
`src-tauri\target\release\bundle\msix\Spaceadom_<version>_x64.msix`. Confirm
it exists and note its size (should be roughly 10 MB, not ~200 MB — if it's
the large number, `npm run store` ran instead of `npm run tauri build`
somewhere upstream).

**This package is deliberately unsigned.** Do not try to sign it yourself
and do not run `npm run msix -- -Sign` for this submission — that flag exists
only for the second-machine structural test described in
`SUBMIT-CHECKLIST.md`, using a throwaway local certificate that must never be
treated as the real signature. **The Store re-signs every package it
distributes with a Microsoft-issued certificate at publishing time**
(confirmed: [The app certification process for MSIX app](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-certification-process),
fetched 2026-09-05 — "You don't need to provide your own code signing
certificate for Store distribution—the Store handles this automatically").
Uploading the plain unsigned `.msix` is correct and expected.

**Do not install this file on this machine.** See `CLAUDE.md` and
`SUBMIT-CHECKLIST.md` for why — two `WH_KEYBOARD_LL` hooks (this repo's dev
install plus a packaged copy) fight over the same spacebar.

---

## 6. Start the submission

On the Spaceadom product's overview page, in **Product release**, click
**Start submission**. A draft submission appears with six sections, all
shown as incomplete. Confirmed order and required/optional status for each,
from [Create an app submission for your MSIX app](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/create-app-submission)
(fetched 2026-09-05) — you don't have to do them in this order, but it's the
order the page lists them in and the order this runbook follows:

### 6.1 Pricing and availability

| Field | What to set | Why |
| --- | --- | --- |
| **Markets** | All possible markets (the default) | No reason to restrict; a keyboard utility has no region-specific content |
| **Audience** | Public audience (default) | |
| **Discoverability** | "Make this product available and discoverable" (default) | |
| **Schedule** | Release as soon as possible; stop acquisition never (default) | |
| **Base price** | **Free** | |
| Free trial, sale pricing, org licensing | Leave unset | Not applicable to a free app |

Nothing here needs to deviate from the defaults except confirming **Free** as
the base price. This page is "Required" but every field has a working
default per Microsoft's own submission checklist.

### 6.2 Properties

| Field | What to set |
| --- | --- |
| **Category** | **Productivity** — see `LISTING.md`'s Category section for the exact justification against Microsoft's own category definitions |
| **Subcategory** | None available under Productivity — leave blank |
| **Secondary category** | **Utilities + tools** |
| **Privacy policy URL** | `https://github.com/nur-arpon/Spaceadom/blob/main/PRIVACY.md` — required because the app's declared capabilities can access input; if Partner Center doesn't force this, enter it anyway |
| **Website** | `https://github.com/nur-arpon/Spaceadom` |
| **Support contact info** | `https://github.com/nur-arpon/Spaceadom/issues` |
| **Game settings** | Not shown — only appears for the Games category |
| **Display mode** | Leave everything unchecked — Spaceadom is not a Mixed Reality / HoloLens / 4K-HDR app |
| **Product declarations** | Review the checkbox list live in the wizard. If any privacy, data collection, or diagnostics-related declarations exist, enable them and note that crash diagnostic data is collected and sent to Sentry (with personal identifiers redacted), with user opt-out available via the "Don't send logs" setting. **Verify in the wizard** — this list wasn't itemised in the fetched docs |
| **System requirements** | See `LISTING.md`'s System requirements section — this is a hardware-feature checklist (memory, DirectX, etc.), **not** an OS-version field; the OS minimum comes from the package manifest automatically (Windows 10 1809+, already set) |

### 6.3 Age ratings

Source: [Age ratings for MSIX app](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/age-ratings)
(fetched 2026-09-05). **This is a real IARC (International Age Ratings
Coalition) questionnaire**, not a Microsoft-only checkbox list — confirmed
from the doc: "we share your publisher display name and email address with
IARC," and you'll separately receive a rating-confirmation email from IARC
once published.

1. Answer the first question (app category) with whichever option is
   closest to a utility / productivity tool.
2. Answer most follow-up questions **no** (violence, user-generated content,
   chat/social, advertising, in-app purchases, location, gambling) — but see
   `LISTING.md`'s Age rating questionnaire section for the one exception: the
   data collection question, which must be answered **yes** with a full
   explanation of what diagnostic data the app collects, how it is redacted,
   and how users can opt out. See the section for the exact wording and the
   caveat about Partner Center possibly overriding the answer.
3. Click **Save and generate**. You'll see all assigned ratings immediately
   for every market.

This step is required — the app cannot be submitted without it.

### 6.4 Packages

1. Click **Upload** (or drag-and-drop) and select the `.msix` file from §5.
2. Wait for it to validate. Partner Center runs the same security,
   technical-compliance, and structural checks certification will later
   run, at a lighter level, so problems can surface here first.
3. **Expect a "runFullTrust" notice.** Partner Center detects declared
   restricted capabilities on upload and flags them — this is expected and
   is not itself a failure. The justification text goes on the
   **Submission options** page (§6.6), not here.
4. **Device family availability**: leave at the default (all supported).

Note from Microsoft's own docs: this section can show **"Incomplete" even
after the package itself shows "Validated"** — package validation and
section completion are two different checks. Don't assume something is
wrong if the section header doesn't turn green the instant the upload
finishes; other required sections (Store listings, in particular) also feed
into whether Packages reads as complete.

### 6.5 Store listings

Everything textual here comes from `LISTING.md`, field for field, with the
corrected 2026 limits already applied (short description, features, search
terms, copyright). Images come from `to-publish-in-microsoft-store/assets/`
— see that folder's `README.md` for exactly which file goes in which upload
slot. **Real screenshots already sit in `assets/screenshots/`
(`01-dashboard.png` through `05-launch.png`) — but as of this writing they
were captured from an older build (v1.0.101) and must be recaptured before
submission: they show the old full-width conflict-card layout (replaced by a
side-by-side grid in 1.0.106) and `04-about.png` has the old version number
baked into the visible About row.** See `assets/screenshots/README.md` for
the exact replacement list. At least one screenshot is the one image Partner
Center actually requires to consider this section complete.

| Field | Source |
| --- | --- |
| Description | `LISTING.md` § Description |
| What's new in this version | Leave blank — first submission |
| Product features | `LISTING.md` § Product features |
| Screenshots (≥1, up to 10, 1366×768+) | `assets/screenshots/01-dashboard.png`–`05-launch.png` — **re-capture these from the current build first, see the note above** |
| Store logo (1:1 App tile icon, 300×300) | `assets/StoreLogo-300x300.png` |
| 1:1 box art (1080×1080), if shown | `assets/AppTile-1080x1080.png` — may not appear; see `assets/README.md` |
| 16:9 Super hero art (1920×1080), optional | `assets/Hero-1920x1080-textfree.png` — the text-free one, not the tagline one |
| Short description | `LISTING.md` § Short description |
| Search terms / Keywords | `LISTING.md` § Search terms (already trimmed to 7) |
| Copyright and trademark info | `LISTING.md` § Copyright and trademark info |

**Do not upload `Hero-1920x1080.png` or `Hero-2400x1200.png`** (the ones
with the tagline baked in) to the Super Hero art slot — Microsoft's own
guidance for that field says not to include text, and `assets/README.md`
explains why a separate text-free version exists for exactly this reason.
Those two files are for the README and social use, not Partner Center.

### 6.6 Submission options

Source: [Manage submission options for MSIX app](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/manage-submission-options)
(fetched 2026-09-05).

1. **Publishing hold options** — leave at the default ("publish as soon as it
   passes certification"). No reason to hold this back.
2. **Notes for certification** — paste `LISTING.md`'s certification-notes
   block here (or at minimum the "HOW TO REPRODUCE THE MAIN FEATURE" section
   of it — a real person reads this, and Microsoft's own guidance says
   testers appreciate short, clear repro steps).
3. **Restricted capabilities** — this section only appears because the
   package declares `runFullTrust`. Paste `LISTING.md`'s full
   certification-notes block here. Confirmed from the docs: "For each
   capability, tell us why your app needs to declare the capability and how
   it is used… this may add some additional time for your submission to
   complete the certification process." Budget for that extra time inside
   the up-to-3-business-day window in §7, not on top of it.
4. **Submission notification audience** — the account owner is always
   notified automatically; add other team members here only if there are
   any (there aren't, for this account).

Once all six sections show complete, the **Submit for certification** button
appears on the application overview page.

---

## 7. Submit → what happens, and how long it takes

Source: [The app certification process for MSIX app](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-certification-process)
(fetched 2026-09-05).

**Certification typically takes up to three business days**, though
Microsoft's own FAQ says it's "usually completed within a few hours" for
many submissions. Three phases run in sequence:

1. **Security tests** — malware/virus scan of the package.
2. **Technical compliance** — the Windows App Certification Kit, run by
   Microsoft against the uploaded package. (`SUBMIT-CHECKLIST.md` already
   describes running the WACK locally first, on a second machine, as a way
   to catch the same failures before they cost a submission cycle.)
3. **Content compliance** — a human reviewer checks the listing and the
   app's behaviour against Store Policy, reading the certification notes
   from §6.6 as they go. This is the phase the restricted-capability
   justification and the reproduction steps are written for.

After certification finishes you get a **report** — pass or fail, and if
fail, which specific test or policy. A failure is not final: fix the
specific issue named in the report and create a **new submission**; you do
not have to redo the account or the name reservation.

**After it passes**, publishing takes a few minutes and the listing becomes
visible to customers within about 15 minutes on average. You'll get an
email and an Action Center notification either way.

### Common failure reasons for a `runFullTrust` + keyboard-hook app, and how this submission's notes address each one

| Likely reviewer objection | Where it's already answered |
| --- | --- |
| "Why does this need `runFullTrust` / a global hook at all?" | LISTING.md certification notes §1 — the feature *is* Space behaving differently everywhere, which no narrower capability grants |
| "The app also reads keys via a plain page listener sometimes — why?" | LISTING.md certification notes §2 — Windows never delivers the global hook a keystroke typed while the app's own window has focus, so a page-side fallback covers exactly that gap, with a dedupe against the hook and one documented, harmless behavioural difference |
| "It can close other programs — is that a stability or security risk?" | LISTING.md certification notes §4 — user-request only, twice-confirmed, fixed built-in list, WM_CLOSE before force, never elevates silently, source file named |
| "Does it log keystrokes?" | LISTING.md certification notes §1 and §PRIVACY POLICY — reads key codes only, keeps no history, no network code |
| "The app doesn't do anything visible when I open it" (a reviewer expecting an obvious first screen) | LISTING.md's "HOW TO REPRODUCE THE MAIN FEATURE" steps — walk the tester through hold-Space-tap-letter explicitly, since the core interaction (holding Space) is not discoverable by clicking around |
| "Why is there no visible login / account?" | There isn't one, by design — PRIVACY.md and the age-rating answers both say so consistently; consistency across the submission is what a reviewer is checking for |
| WACK flags missing scaled asset variants (`.scale-200`, etc.) | Known and accepted — see `SUBMIT-CHECKLIST.md`'s "What is deliberately simplified" section; Windows scales the 100%-scale assets that are shipped, so this is a WACK *warning*, not expected to be a certification *failure*, but **verify in the wizard/WACK report** since it hasn't been observed against a real submission yet |

---

## 8. The WebView2 question

The package ships **no WebView2 runtime**, and this section exists to record
that this was a considered decision, not an oversight — cross-checked
against `src-tauri/msix/AppxManifest.xml`'s own header comment and against
Microsoft's live WebView2 distribution docs (fetched 2026-09-05):

- **Is the Evergreen WebView2 Runtime present on certification machines?**
  Confirmed from Microsoft's docs: "The Evergreen WebView2 Runtime will be
  included as part of the Windows 11 operating system," and "the vast
  majority of Windows 10 devices have the WebView2 Runtime installed
  already." Since Microsoft's own certification kit (the WACK) and
  certification pipeline run on current, updated Windows, the runtime being
  absent there specifically is unlikely — but nothing in the fetched docs
  states outright "certification machines always have it," so this
  specific claim is **inference, not a confirmed fact — verify by watching
  what the certification report says if this submission ever fails on a
  missing-runtime symptom.**
- **Is there a Store/package framework dependency mechanism for WebView2?**
  Confirmed **no**, for this distribution path: `win32dependencies:
ExternalDependency` is documented as something you use "if you're using
  **App Installer** to deploy MSIX applications" — that's sideloading, not a
  Store-driven install. Nothing in the fetched docs describes an equivalent
  mechanism that Store-driven installs honor. This matches, word for word,
  the reasoning already written into `AppxManifest.xml`'s header comment.
- **The fallback, if this is ever wrong:** the Fixed Version WebView2
  runtime, bundled directly in the package (confirmed **+250 MB**, matching
  the manifest comment's estimate exactly). That is an explicit owner
  decision to make later if needed, not a default — see
  `SUBMIT-CHECKLIST.md`, "The WebView2 decision."
- **The listing text**, if this ever needs stating to a customer: "Requires
  the Microsoft Edge WebView2 Runtime, present on Windows 11 and most
  Windows 10 devices." This has not been added anywhere in `LISTING.md`
  because the residual risk is small enough that surfacing it prominently
  would raise more questions than it answers for the vast majority of
  customers — but it's here, worded and ready, if a WACK run or a real
  certification failure ever calls for it.

---

## What this runbook could not confirm from official docs

Flagged inline above, collected here for one scan before submitting:

- The exact wait time for identity verification in §1.3.
- Whether "Keyboard" is one of the checkable hardware items on the
  Properties → System requirements page (§6.2).
- The exact contents of the "Product declarations" checkbox list (§6.2).
- Whether the 1080×1080 "1:1 box art" upload slot appears at all for a
  Productivity-category app that isn't a game (§6.5, `assets/README.md`).
- A per-term character limit for Search terms/Keywords, if any (§ Search
  terms in `LISTING.md` — the 7-term *count* limit is confirmed, a per-term
  length limit is not).
- A character limit for the "Notes for certification" / "Restricted
  capabilities" free-text boxes (§6.6) — the pasted text is roughly 4,800
  characters as of the 2026-09-07 addition of the page-side-fallback section
  (§2), growing from the 3,233 characters measured 2026-09-05, with nothing
  in the fetched docs stating a ceiling. **Verify in the wizard** — if it
  truncates, LISTING.md already says which sections to keep first (1 and 4).
- Whether Windows Store certification machines specifically (as opposed to
  Windows 11/most Windows 10 generally) have the Evergreen WebView2 Runtime
  (§8) — reasoned as likely, not confirmed as fact.

None of these block starting or completing the submission — each has a
stated default or a safe fallback above — but each is called out so nothing
here is mistaken for more certain than it is.

## Sources (all fetched 2026-09-05)

- [Free developer registration for individual developers](https://learn.microsoft.com/en-us/windows/apps/publish/whats-new-individual-developer)
- [Reserve your MSIX app's name](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/reserve-your-apps-name)
- [Create an app submission for your MSIX app](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/create-app-submission)
- [Add and edit Store listing info for MSIX app](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/add-and-edit-store-listing-info)
- [App screenshots, images, and trailers for MSIX app](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/screenshots-and-images)
- [Age ratings for MSIX app](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/age-ratings)
- [Manage submission options for MSIX app](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/manage-submission-options)
- [Enter app properties for MSIX app](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/enter-app-properties)
- [Categories and subcategories for MSIX app](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/categories-and-subcategories)
- [System requirements for MSIX app](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/system-requirements)
- [Support info for MSIX app](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/support-info)
- [The app certification process for MSIX app](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-certification-process)
- [Distribute your app and the WebView2 Runtime](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)
