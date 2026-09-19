# Brief 4 — 1.0.119: Space ring recentre + focused-app centre, mouse ring fold-by-count (2026-09-19)

Three owner requests after using 1.0.117/118. Two are about the **Space
ring** (the Space-hold HUD: `src/components/toast.ts`,
`src/styles/overlay-earthy.css`, `src-tauri/src/guide_hud/`), one about
the **mouse ring** (middle-button icon ring: `src-tauri/src/middle_ring.rs`,
`src/components/middle-ring.ts`, `src/styles/middle-ring.css`). They are
different surfaces — CLAUDE.md names both; do not mix them. Rules: no
installer, no install, no commit, no input injection, never write
`%APPDATA%\Spaceadom\config.json`, never delete/rename files you did not
create. Read `docs/PHASE-A-BRIEF-1.md` §HUD and brief 3 §7 (the ring
specials bug and its fix) first.

## 1. Space ring is ~80 px right of centre — find the cause, don't nudge

Evidence (owner screenshots 11:39–11:40): on the external 1707×1067
monitor the SPACE pill's centre is at x≈930 (centre 853, +77 px); on the
laptop panel the same shift, ≈+80 px real. The WHOLE cloud of pills moves
with the pill, so it is the container/window offset, not the scatter
layout. It appeared with 1.0.116 (the HUD rows became derived — 12 inner
specials instead of 9). Method: `git diff v1.0.115..HEAD -- src/components/toast.ts src/styles/overlay-earthy.css src-tauri/src/guide_hud src-tauri/src/commands.rs`
and read every changed line that touches width, left, margin, padding,
transform, the overlay window's `overlay_fit` size/position, or the
number of rows/columns. Then MEASURE: a test or a dev-page check that the
pill's centre equals the overlay window's width/2 (a pure layout function
if one exists; otherwise a DOM read on the localhost preview — DOM reads
over screenshots, never present the preview to the owner). Fix the cause;
state it in one sentence in the report. Both monitors must be centred.

## 2. Centre pill shows the FOCUSED APP, not "SPACE"

Like the mouse ring's centre. At hold start the engine already resolves
the foreground window (the `hold start` log line); pass its process/app
name and icon (reuse the picker's icon cache / `extract` used by the
ring) to the HUD in the same event that opens it. The pill shows icon +
short name ("Brave", "Word", "Explorer"). Fallbacks that keep the word
**SPACE**: the desktop/shell (explorer with no document window), the lock
screen, Spaceadom's own windows, an unreadable process (the ring's
"process could not be read" case), and any error. Truncate at ~14 chars
with an ellipsis. Same pill size — the name must not resize the pill or
move the cloud (a test on the pill's box). The `preview.ts` fixture gets a
sample focused app.

## 3. Mouse ring: fold by TILE COUNT, not by scope

Today: Favourites folds into an edge/corner arc; All always keeps its
full shape and relocates (to the middle). Owner rule: "adapt only when it
cannot hold any more in a readable way." So:

- Compute `fits_as_arc(tile_count, edge_or_corner)`: true when every tile
  fits the arc at Favourites' current tile size and spacing (the same
  numbers Favourites uses today — do not shrink tiles). Corner arcs hold
  fewer than edge arcs; reuse the existing arc capacity maths.
- If it fits → fold, for EITHER scope. If not → relocate exactly as All
  does today (the owner is fine with that for big profiles).
- Specials count as tiles when the scope includes them.
- Scope stays: Favourites = the pinned set, no specials; All = every
  bound key + specials. Nothing else about scope changes. (Later, in the
  ring-palette work, Favourites becomes "My ring"; not now.)
- Tests: 10 tiles at a corner folds; 30 tiles at a corner relocates; the
  threshold equals the arc capacity constant; Favourites' behaviour is
  unchanged for its usual sizes.

## Docs + version

`PROJECT_STATUS.md` dated entry (cause of §1 in one sentence, the fold
rule in one sentence), `V14_FIXES_AND_CODE.md` entry, `all-versions/
WHAT-CHANGED.md` row for 1.0.119 in the owner's voice (see 1.0.118's).
Version → **1.0.119** in package.json, tauri.conf.json, Cargo.toml,
scripts/install-real.cmd.

## Gates

`cargo test --release --lib`, `cargo clippy --release --lib` 0 warnings,
`npx tsc --noEmit -p .`, `npm run build`. Report: files, gate output, §1's
cause, any QUESTION one line each.
