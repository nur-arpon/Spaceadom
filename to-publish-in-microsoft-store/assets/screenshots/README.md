# Store screenshots — real captures of Spaceadom 1.0.107

Captured **2026-09-07, 15:03–15:12**, from the **live installed build** on the
owner's machine: `%LOCALAPPDATA%\Spaceadom\spaceadom.exe`, FileVersion
**1.0.107**, 21,947,904 bytes, SHA-256
`F900092A6108CF68878BCB46E76F8A6778841BF261AB91D207054C40D4039F1A`. Nothing was
mocked, staged, or re-rendered — every pixel is the running app.

All five are **2377 × 1484, 8-bit RGBA PNG** — comfortably over Partner
Center's `1366 × 768` desktop minimum, PNG, and far under the 50 MB / image cap
(1–10 images per device family).

| File | Pixels | Bytes | What it shows | Wizard slot (Partner Center → Store listing → Screenshots → **Desktop**) |
| --- | --- | ---: | --- | --- |
| `01-dashboard.png` | 2377 × 1484 | 958,110 | The dashboard on the Starry night theme (`theme: auto`, and Windows is in light mode, which resolves to starry — see `theme_watch` in `debug.log`). The full keyboard board with the bound keys lit: A Gemini, D Discord, F Files, G/Z YouTube, H Claude, J/M Google, K/V/B Brave, L Spotify, C Claude, N Samsung, X `D:\`, plus PiP Cycle, Force Close, Alt Profile and the Scroll keys. Captured 15:03:46. | **Slot 1** — the lead image. |
| `02-ring.png` | 2377 × 1484 | 1,241,751 | The real Guide HUD ring: the lit SPACE pill with 18 app chips fanned around it over the dashboard. Raised through Settings → "Check the ring", which calls `preview_hud_layout` and shows the genuine overlay — `debug.log` 15:05:32.186 `guide_hud: overlay window shown (hold #5)` and 15:05:32.152 `guide_hud: PREVIEW #5 — showing the real ring … 18 app(s)`. Captured 15:05:33. | **Slot 2** — the radial guide. |
| `03-tour.png` | 2377 × 1484 | 944,066 | The first-run walkthrough entry card: "Hold Space, tap any app's initial letter — boom! it opens. / Hold Space, tap the app's initial again — boom! it's gone." with **Show me** and **Skip**. Captured 15:10:18 on a purpose-made run with `tour_done` temporarily false; the owner's config was restored byte-for-byte afterwards (see "How the tour shot was taken"). | **Slot 3** (optional). |
| `04-about.png` | 2377 × 1484 | 225,025 | Settings scrolled to the end: the Conflicts grid, "The ring isn't showing?", Maintenance, Danger zone, Privacy, and **About — "Spaceadom · v1.0.107"**, Third-party software · 571. Captured 15:11:56. | **Slot 4** (optional; documents the version). |
| `05-settings.png` | 2377 × 1484 | 315,523 | The expanded Settings panel: the three setting groups side by side (**Appearance** — theme picker, Fun mode, Sound ticks, Visual effects, Hide the keyboard; **Behaviour** — Run at startup, Typing speed, Opacity floor; **The Space Ring** — Point to launch, Ring layout, Show special keys, Guide-to-toast motion, Guide HUD delay), then **App exceptions**, then the **Conflicts** section in its new 1.0.106 side-by-side grid (`spacedesk` and `PowerToys` as two compact cards, not the old full-width pills). Captured 15:04:38. | **Slot 5** (optional). |

Upload order is the table order; Partner Center uses the first screenshot as
the listing's lead image.

## Read before uploading

Four things an uploader should decide on deliberately, not discover later:

1. **`04-about.png` says "Installer (setup.exe)", not "Microsoft Store".** The
   machine these were shot on runs the NSIS build, so the About line reports
   that install kind honestly. The **version** is now correct (v1.0.107), which
   was the blocking defect in the previous set. If the listing needs the words
   "Microsoft Store" under the version, that single shot has to be retaken from
   an installed MSIX package (`Add-AppxPackage` + launch by AUMID, per
   `CLAUDE.md` PROBLEM 143 / the `_probe\msix-test` run) — everything else in
   the five is install-kind-agnostic.
2. **`04-about.png` also carries the line "The update couldn't be completed.
   Nothing changed, and Spaceadom will try again tomorrow."** That is real,
   pre-existing state from an earlier update check on this machine, not
   something the capture caused; it survives a restart. It reads as a fault to
   a shopper. Either crop the About block, or reshoot after a successful update
   check.
3. **These are the owner's real profile, not a clean one.** The profile chip
   `sexy_tumar_mexy` is top-right in `01`, `02` and `03`; real app names sit on
   the keys and on the ring chips; `msaccess` shows as an app exception and the
   owner's two live conflicting programs (spacedesk, PowerToys) are named in
   `04` and `05`. `SUBMIT-CHECKLIST.md` §"Screenshots to capture" asks for a
   clean profile. Nothing personal beyond that appears — no taskbar, no
   wallpaper, no clock, because every shot is the app window alone.
4. **`05-launch.png` (875,123 B, from 1.0.101, 2026-09-05) is still in this
   folder and is now superseded.** The 1.0.107 set replaces it with
   `05-settings.png`. It was left on disk rather than deleted — deleting it is
   the owner's call. Do not upload it: it is a 1.0.101 capture.

Not captured: `SUBMIT-CHECKLIST.md` shots 1 (**Earthy** theme dashboard) and 3
(a special-key card open) still do not exist. Capturing the Earthy one would
mean switching the owner's live theme, which was deliberately not done.

## How these were captured

Every machine-touching step ran through `explorer.exe`, outside the agent's
MSIX container (`CLAUDE.md` PROBLEM 143). Harness:
`D:\Claude-Projects\_probe\shots-1.0.107\` — `shot-input.ps1` (`SendInput`
clicks/keys/scroll, `PrintWindow` with `PW_RENDERFULLCONTENT`, `BitBlt` +
`CAPTUREBLT` full-screen, PNG crop), `shot-snap.ps1` (version / config /
`debug.log` evidence), launched by `run-shot-input.vbs` and `run-shot-snap.vbs`.
Adapted from the proven `_probe\msix-test\a3b-*` scripts, which were copied,
not modified.

- `01`, `03`, `04`, `05` are `PrintWindow(PW_RENDERFULLCONTENT)` of the
  Spaceadom window (2377 × 1484 on a 2560 × 1600 primary display).
- `02` is a full-screen `BitBlt` + `CAPTUREBLT` — the ring lives in a separate
  layered overlay window that a `PrintWindow` of the dashboard cannot include —
  then cropped to the same 2377 × 1484 window rect so the set is uniform.
- **The ring was NOT faked and Space was NOT injected.** Holding Space cannot be
  injected at all: `SendInput` keystrokes never reach the low-level hook
  (PROBLEM 257). The ring here was raised by clicking Settings → **Check the
  ring**, which runs the same `preview_hud_layout` path the real hold uses and
  shows the real overlay window. The `debug.log` lines are quoted in the table.

## How the tour shot was taken, and the proof the config came back

`tour_done` is `true` in the live config, so the walkthrough card cannot appear
on its own. The sequence, all through `explorer.exe`:

1. Copied `%APPDATA%\Spaceadom\config.json` out as a backup —
   87,845 B, SHA-256 `AA9DCF18FBE656963555ECB805D8453CE7575E38B5D5227D1172378A4DC940AD`.
2. Set **only** `"tour_done": false` (atomic `File.Replace` of a temp file);
   the file became 87,846 B, SHA-256 `8FDE91BE…19F4`.
3. Restarted the app, captured `03-tour.png`.
4. Restored the backup byte-for-byte with the same atomic replace.

**Verified after the restore and again at the end of the session: 87,845 B,
SHA-256 `AA9DCF18FBE656963555ECB805D8453CE7575E38B5D5227D1172378A4DC940AD` —
identical to the backup.** Only the file's modified timestamp differs (the
restore rewrote it); the bytes do not. `tour_done` is `true` again, the theme
is still `auto`, and no other setting was touched — the config's SHA-256 was
also unchanged across all the Settings navigation done for `04` and `05`.

---

## Before you upload — two things (added 2026-09-07)

**Do not upload `05-launch.png`.** It is left over from 1.0.101 and shows the
old design. The current set is `01`–`04` plus `05-settings.png`. The file is
kept rather than deleted so nothing is lost by accident; just skip it in the
wizard.

**`04-about.png` carries a transient message.** The About panel in that shot
reads "The update couldn't be completed. Nothing changed, and Spaceadom will
try again tomorrow." That is true of this machine on this day, not of the app,
and it looks worse in a Store listing than it deserves. Either crop the shot to
the version block, or simply upload the other four. The Store needs a minimum
of one screenshot, so four is comfortably enough.

**`04-about.png` also says "Installer (setup.exe)"** because it was captured
from the normal build. A Store copy shows "Microsoft Store" there instead.
Nothing is wrong; it is only the wrong sentence for a Store listing, which is
the second reason to prefer the other four.
