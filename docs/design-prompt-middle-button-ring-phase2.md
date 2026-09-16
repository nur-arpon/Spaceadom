# Claude Design prompt — Spaceadom, middle-button ring "phase 2"

Copy everything below the line into Claude Design.

---

Design the **cursor-anchored app ring** for Spaceadom, a Windows utility that
turns the spacebar into a modifier key (hold Space + tap a letter to launch,
focus or minimise the app bound to that letter). Since 1.0.109 the app also
raises a ring when the user **holds the middle mouse button**; today that ring
is the same one Space raises: a wide horizontal "Guide HUD" of labelled pills
around a SPACE chip, centred on the primary monitor. I want a second, mouse-first
ring for the middle button. Design that.

## What the middle-button ring must do

1. **Appears around the cursor**, not at screen centre. It is raised by holding
   the middle button (~250 ms). While held, moving the cursor towards an item
   highlights it; **releasing the button on a highlighted item launches it**
   (focus / minimise if already open, exactly like Space + letter). Releasing
   with nothing highlighted just closes the ring. A quick middle-click (under
   250 ms) is passed through to Windows untouched, so links still open in new
   tabs.
2. **Circular and compact.** One ring of items, evenly spaced. Research on
   radial menus says 8 items is the comfortable maximum, so the default set is
   the user's **"my eight"** — eight favourites they pick — with a setting to
   show **"all of them"** instead (up to 26 letters + specials; you must show
   how the ring copes: a second concentric ring, or smaller items, your call,
   but never a reflow that moves items after the ring is up).
3. **Icons only.** No text on the items themselves. The **hovered item's name
   appears in the centre circle** (this is the Kando / Adwaita-Circles pattern:
   centre pill = current selection label; empty = nothing selected). Items are
   app icons; links show the site favicon (fetched once when the link is bound);
   files and folders show the normal Windows shell icon. Each item also shows
   its **letter** small, because the same item is reachable by Space + that
   letter — that link between the two gestures is the teaching moment.
4. **Never off-screen.** When the cursor is near an edge or corner, the ring
   **clamps inward** so the whole circle stays visible, and the cursor is
   **warped** to the new centre so the geometry stays honest (Kando does
   exactly this). Show the corner case.
5. Same three themes as the app, same tokens: **Earthy** (bg `#f5ead8`,
   surface `#faf1de`, text `#201e1d`, accent terracotta `#c67139`, warm brown
   shadows `rgba(90,60,30,…)`), **Starry night / nocturne** (bg `#0d141f`,
   surface `#131a29`, text `#e8e4dc`, accent `#6b8cd6`, black-tinted shadows),
   **Warcry** (bg `#140b09`, surface `#1f100d`, text `#f0ded0`, accent blood
   crimson `#b83024`, second voice cold steel `#7b8792`). Radii: 13 px on keys
   and cards, 16 px on containers, 999 on everything interactive. Font: Outfit.
   Never pure black on cream.
6. **Motion**: ring blooms out from the cursor (scale 0.6→1 + fade, ~180 ms,
   spring-ish ease-out); exit runs at ~65 % of entrance time with an ease-in;
   hover highlight is a soft accent glow on the item and the name fading into
   the centre pill; the launched item does a quick "pop" before the ring
   exits. `prefers-reduced-motion` renders final states with no scale.
7. **Fun mode off** = plain and quiet (no glow, no pop); design both.

## Also design the settings for it

Inside the existing Settings popover, section "The Space ring" already has
rows: *Point to launch* (switch), *Middle button opens the ring* (switch),
*Ring layout* (Compact · Wide · Double segmented pill), *Show special keys*,
*Guide HUD delay* (slider), *App exceptions* (a grid of app tiles + "Add an
app"). Add:

- **Middle-button ring shows:** a two-option pill — *My eight* / *All of
  them* — and, when *My eight* is chosen, a picker to choose the eight (design
  the picker: it should reuse the bound keys the user already has, not a new
  app search).
- **App exceptions with three states.** Today an exception means "Spaceadom
  is off in this app". Make each exception tile a 3-way choice named by what
  *still works*: **Off entirely / Space only / Middle only**. The list is
  pre-seeded with CAD/3D programs (SolidWorks, Fusion 360, Blender, AutoCAD,
  Rhino, Unity, Unreal…) at *Space only*, because those programs use the
  middle button to orbit — show a couple of those rows, and make clear they are
  built-in defaults the user can change.
- A one-line disclosure under the link items: "Site icons are fetched once,
  when you bind the link."

## Deliverables (artboards, 1707×1067 logical = 2560×1600 at 150 %)

1. Ring open over a busy desktop, Earthy, "my eight", one item hovered (name
   in centre). 2. Same, Starry night. 3. Same, Warcry. 4. "All of them"
   variant. 5. Edge/corner clamp case with the warp indicated. 6. The launch
   moment (item pop, ring exiting). 7. Fun mode off. 8. Settings popover with
   the new rows and the three-state exception tiles. 9. The "my eight" picker.
   Plus a small motion spec sheet (durations, easings, what moves).

Constraints that are not negotiable: the Space ring stays exactly as it is —
this is a second ring, not a replacement; nothing may assume a fixed width
around an app name; the ring is primary-monitor only.
