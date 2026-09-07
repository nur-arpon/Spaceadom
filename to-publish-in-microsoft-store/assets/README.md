# Store listing images — what's here, and where each one goes

Generated 2026-09-05 by `generate-store-assets.ps1`, which is kept in this
folder — System.Drawing / PowerShell, because no `sharp` or other image
library is installed on this machine. Re-run it (`powershell -NoProfile
-ExecutionPolicy Bypass -File to-publish-in-microsoft-store\assets\generate-store-assets.ps1`)
any time the icon or the theme palette changes; it always regenerates all
five files. Source icon: `src-tauri/icons/_original-square-icons/app-icon.png`
(1024×1024 — the highest-resolution copy in the repo; the working
`icons/icon.png` is only 512×512, which is why that folder and not the usual
one was used). Colours are pasted verbatim from `src/styles/design-system.css`
`--st-*` tokens (Earthy = light/default theme, Nocturne/Starry = dark theme).
Text uses **Segoe UI Semibold** — the CSS calls for Outfit (body) over
Caprasimo (headings), and neither is installed as a system font on this
machine (only bundled as a web font for the app itself), so this is the
nearest installed geometric sans, per the brief's "use a system fallback."

**These are placeholders.** Every file with a tagline is meant to be swapped
for a real screenshot or a designed hero once one exists — see "Real
screenshots" below.

| File | Size | Goes in the wizard's… | Required? |
| --- | --- | --- | --- |
| `StoreLogo-300x300.png` | 300×300 | **Store listings** page → **1:1 App tile icon (300 x 300 pixels)** | **Yes** — this is the one Microsoft calls the "Store logo" in its FAQ; without it the Store falls back to the icon baked into the package. Plain resize of the app icon, no background added — the source already fills its own square. |
| `AppTile-1080x1080.png` | 1080×1080 | Store listings page → **1:1 box art (1080 x 1080 pixels)**, *if the wizard offers it* | Optional, and possibly not shown at all — see note below. Icon on the Earthy (`#f5ead8`) background. |
| `Hero-1920x1080-textfree.png` | 1920×1080, **no text** | Store listings page → **16:9 Super hero art (1920 x 1080 pixels)** | Optional but recommended for all apps (not just games) — Microsoft's own guidance says this slot must **not** contain text or the product title, which is why this is the text-free one. Starry (`#0d141f`) background, icon centred with a soft accent-tinted glow behind it. |
| `Hero-1920x1080.png` | 1920×1080, **with tagline** | **Not for that wizard slot** — README / GitHub social-preview / press use only | Icon + "Hold Space, tap any app's initial letter — boom! it opens." on the Starry background. Kept separate from the text-free hero on purpose, so the one that violates the Store's own "no text" rule never gets uploaded there by mistake. |
| `Hero-2400x1200.png` | 2400×1200, **with tagline** | **Not applicable in Partner Center for this app** — README / social banner only | This exact size maps to the "2:1 Holographic image" field, which only appears for apps that declare Windows Mixed Reality / HoloLens support. Spaceadom's manifest doesn't (`TargetDeviceFamily` is `Windows.Universal`, desktop-only), so this field won't appear for this product. Icon + tagline on the Earthy background, wide crop — useful anywhere a 2:1 banner is wanted outside the Store. |

## What Microsoft's docs actually say about the two "1:1" sizes — verify in the wizard

The screenshots-and-images doc (fetched 2026-09-05) is explicit that its
**300×300 "1:1 App tile icon"** and **1080×1080 "1:1 box art"** are two
different fields, and that the 1080×1080 one **"does not apply to apps"** —
its own text says it's for games, shown on Xbox/game pages. Spaceadom is not
a game. It's entirely possible the Properties → Category choice of
**Productivity** (see `RUNBOOK.md`) simply never shows the 1080×1080 upload
slot at all. `AppTile-1080x1080.png` is generated anyway, per the brief, in
case the slot appears — if it doesn't, the 300×300 Store logo above is the
one that actually matters, and is already required-complete.

## Real screenshots

Five real captures already live in `screenshots/` (`01-dashboard.png` through
`05-launch.png`) — see `screenshots/README.md` for what each one shows and
its Partner Center slot. **They are stale as of this writing: captured
2026-09-05 from v1.0.101, before the 1.0.106 conflict-card layout fix, so
they show the OLD full-width conflict cards, and `04-about.png` has the old
version number ("v1.0.101") visibly baked into the About row.** A separate
recapture is expected before submission — `screenshots/README.md` names
exactly which files must be replaced and what each must show once retaken
against the current build. None of the files in this top-level `assets/`
folder are a substitute for real screenshots; a Store listing with only
placeholder art and no real screenshot of the app cannot be submitted.

## If the icon or palette changes later

The generator script pulls two things by hand and would need re-running (not
re-derived automatically): the source icon path, and the four `--st-*` hex
values per theme from `src/styles/design-system.css`. Nothing here reads the
CSS or the icon file at submission time — these are static PNGs.
