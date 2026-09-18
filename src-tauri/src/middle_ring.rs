/// middle_ring.rs — PROBLEM 267: the cursor-anchored, icon-first ring the
/// MIDDLE MOUSE BUTTON raises (phase 2 of PROBLEM 263).
///
/// Phase 1 (1.0.109) made a middle-button hold raise the SAME centred Guide
/// HUD that Space raises. This module is the second ring the design handoff
/// (`design/middle-mouse-ring/design_handoff_middle_ring/README.md`) asks for:
/// a ring of REAL app icons blooming out of the cursor, a centre disc naming
/// whatever the cursor points at, release-to-launch. It is ADDITIVE — the
/// Space ring is untouched, and the owner's `middle_ring_style` setting can
/// put phase 1 back exactly as it was.
///
/// THE NUMBER SYSTEM SINCE ROUND 4 (owner-approved, 2026-09-13, after the
/// round-3 build was held on hardware: tiles too big and overlapping, the
/// fan only right at the top-left):
///
/// * ONE unit `u` = 44 px (`TILE`) and the golden ratio φ = 1.618 (`PHI`)
///   derive everything: icon u/φ ≈ 27, centre disc u·φ² ≈ 115, arc spacing
///   between tile centres u·φ ≈ 71 (`ARC_STEP`). Badges stay 20 px.
/// * Ring radii r1 = 108, r_k = r1·φ^(k−1): 108 / 175 / 283 / 458
///   (`RING_RADII`); the fourth ring exists only for "All"'s overflow.
/// * Full-circle capacity per ring = Fibonacci 5 / 8 / 13 / 21 (`RING_CAPS`).
///   Favourites cap = 13 (`FAVOURITES_MAX`); "All" = 26 letters fill rings
///   1–3 exactly (5 + 8 + 13), the specials on ring 4.
/// * Within a ring: exact even spacing 360°/n, n = 1 at north, n = 2
///   opposite; each outer ring is rotated by the GOLDEN ANGLE 360°(1 − 1/φ)
///   ≈ 137.5° relative to the ring inside it (`stagger`, phyllotaxis —
///   2026-09-15; it was half the ring's own pitch, which put four of "All"'s
///   thirteen outer tiles at EXACTLY a ring-1 tile's bearing). A partial arc
///   on an outer ring is shifted off its own arc start by `best_arc_phase`
///   for the same reason.
/// * THE EDGE / CORNER LAW for Favourites (the owner's round-3 law, restored
///   in ROUND 6, 2026-09-17 — round 4 had replaced it with a snap of the
///   centre onto the edge line / corner point, which drew half or three
///   quarters of every circle off-screen; his own log showed the centre
///   moved up to 278 px from the press: "the ring going out of the viewing
///   screen"): THE CENTRE NEVER MOVES FROM THE PRESS POINT. `choose_shape`
///   lays the arcs out around the press point itself (`layout_arcs`, no
///   offset); near an edge or a corner the rings simply become the arcs the
///   room allows, named `half-<dir>` / `quarter-<dir>` by the direction the
///   FIRST partial arc opens toward (a cardinal = half, a diagonal =
///   quarter). Arcs fill inner-first at `ARC_STEP` spacing (capacity =
///   floor(arc_len / 71) + 1), tiles evenly over the arc that is actually
///   available at that radius, overflow outward ring by ring. Tiles never
///   overlap and never leave the work area; the centre disc may clip.
/// * SHRINK-TO-FIT is the SECOND step, before any move: when the count does
///   not fit around the press point at u = 44, the unit steps down the
///   `tile_ladder` (×0.95 a step, to the floor `TILE_MIN` = u/φ = 27, the
///   icon size) and EVERY derived length scales with it — `ring_radius`
///   and `arc_step` are functions of the tile — while `CLAMP_MARGIN` (the
///   halo) stays in px. The first size that fits wins, so tiles shrink only
///   when the room forces it. On a real monitor with ≤ 13 favourites the
///   corner point itself holds 2/3/6/10 at full size, so the ladder is never
///   descended there — it exists for slivers.
/// * Only when even the floor cannot hold the count around the press point
///   does the centre move — by the SMALLEST vector on an 8 px grid that
///   lets the floor-size arcs fit (`nudge`, named `nudged-<dir>`), and if no
///   such vector exists within the room, the old clamp + warp
///   (`circle-clamped`).
/// * "All" keeps clamp + warp with the new radii and capacities, at `TILE`.
/// * The overlay window is ONE BIG CANVAS — the whole work area of the
///   cursor's monitor (`canvas_rect`), never the exact monitor bounds — so no
///   tile, disc or scrim can ever meet a window edge.
///
/// EVERYTHING IN THIS FILE IS PURE, on purpose. Geometry (`layout_arcs`,
/// `choose_shape`), the canvas (`canvas_rect`, `page_point`), the edge clamp
/// (`clamp_ring_center`), the hit test (`ring_pick`), the favourite list
/// (`favourites_for`), the disc's name split (`split_display_name`) and the
/// payload builder (`build_entries`, extractor injected as a closure) all run
/// without a window, a mouse or a shell, so the tests at the bottom run on
/// CI. The impure halves — `GetCursorPos`, `SetCursorPos`, the icon shell
/// call, the window fit and the emit — live in
/// `guide_hud::mod_impl::show_middle_ring` and `engine::dispatch`, and each
/// of them calls into here for its decisions.
///
/// UNITS. Every length in this file is in the unit the caller says it is in:
/// the layout and the design constants are LOGICAL px; `clamp_ring_center`,
/// `canvas_rect` and `ring_pick` are unit-agnostic (they compare a cursor to
/// a rectangle / to radii in whatever unit both arrive in — the callers pass
/// PHYSICAL px, because that is what `MSLLHOOKSTRUCT.pt`, `GetCursorPos` and
/// `Monitor::work_area` all speak). Angles are COMPASS DEGREES, clockwise
/// from north (0 = straight up, 90 = right).
use crate::config::AppConfig;

// ---------------------------------------------------------------------------
// The number system — u = 44, φ = 1.618; every other length is derived
// ---------------------------------------------------------------------------

/// The golden ratio, as the owner wrote it.
pub const PHI: f64 = 1.618;
/// The unit: the tile box, `u` = 44 px logical.
pub const TILE: f64 = 44.0;
/// The icon inside a tile: u/φ = 44/1.618 = 27.19 → 27 px (the page's
/// `.mr-icon`; the CSS carries the same number).
pub const ICON_PX: f64 = 27.0;
/// The centre disc's resting diameter: u·φ² = 44 × 2.618 = 115.2 → 115 px.
pub const PILL_D: f64 = 115.0;
/// Arc spacing between neighbouring tile CENTRES on one ring: u·φ = 44 ×
/// 1.618 = 71.2 → 71 px (a 27 px gap between 44 px boxes).
pub const ARC_STEP: f64 = 71.0;
/// The first ring's radius.
pub const RING_R1: f64 = 108.0;
/// Ring radii, innermost first: r_k = r1·φ^(k−1) — 108, 108 × 1.618 =
/// 174.7 → 175, 108 × 2.618 = 282.7 → 283, 108 × 4.236 = 457.5 → 458. The
/// fourth is for "All"'s overflow (and a corner's quarter ring of 13).
pub const RING_RADII: [f64; 4] = [108.0, 175.0, 283.0, 458.0];
/// Full-circle capacity per ring — Fibonacci 5 / 8 / 13 / 21. Every one is
/// under what `ARC_STEP` would allow on its ring (9 / 15 / 25 / 40), so a
/// full ring is always spaced wider than 71 px.
pub const RING_CAPS: [usize; 4] = [5, 8, 13, 21];
/// Rings a layout may open: as many as there are radii.
pub const MAX_RINGS: u8 = RING_RADII.len() as u8;
/// The most favourites a user may tick (owner, round 4): 13 = 5 + 8, two
/// full rings in open space. Longer stored lists are truncated on read.
pub const FAVOURITES_MAX: usize = 13;
/// The favourites shown when none are chosen: the first `FAVOURITES_DEFAULT`
/// bound letters — one full inner ring.
pub const FAVOURITES_DEFAULT: usize = RING_CAPS[0];
/// The air the disc keeps from the innermost tiles' inner edges when it
/// grows for a long name (`pill_max_diameter`).
pub const PILL_FREE_MARGIN: f64 = 5.0;
/// Radial scrim diameter behind a single ring (README: "600px radial scrim").
pub const SCRIM_D_ONE: f64 = 600.0;
/// Radial scrim diameter behind two or more rings (artboard 4: 640 px).
pub const SCRIM_D_MULTI: f64 = 640.0;
/// A dashed guide circle sits this much outside its ring's radius ×2.
pub const GUIDE_EXTRA: f64 = 14.0;

// ---------------------------------------------------------------------------
// Shrink-to-fit (ROUND 6, 2026-09-17): the unit scales, the halo does not
// ---------------------------------------------------------------------------

/// The smallest tile the ladder descends to: u/φ = 27 px — the icon's own
/// size at u = 44, i.e. a tile that is all icon and no air. Below that the
/// icon itself would have to shrink, and the owner's priority is real,
/// recognisable icons.
pub const TILE_MIN: f64 = ICON_PX;
/// One rung of the ladder: the tile is multiplied by this until it reaches
/// `TILE_MIN`. 0.95 gives eleven sizes from 44 to 27 (44, 41.8, 39.7, 37.7,
/// 35.8, 34.0, 32.4, 30.7, 29.2, 27.7, 27) — fine enough that the first
/// fitting size is within 5 % of the largest that could, coarse enough that
/// a full descent costs eleven `layout_arcs` calls, not a hundred.
pub const SHRINK_STEP: f64 = 0.95;
/// The grid the `nudge` search walks, logical px, and how far it looks: a
/// centre moved by more than the fourth ring's extent would be clamping,
/// not nudging.
pub const NUDGE_STEP: f64 = 8.0;
pub const NUDGE_MAX: f64 = RING_RADII[3] + TILE / 2.0 + CLAMP_MARGIN;
/// How far, in px of arc, a dashed guide ARC may run past the feasible arc
/// at each end (`guide_arcs`). A tile centre on the feasible arc is at least
/// `tile/2 + CLAMP_MARGIN` ≥ 29.5 px from every wall and the guide sits
/// `GUIDE_EXTRA/2` = 7 px further out, so any point within 22 px of it along
/// the guide is still on-screen. 20 keeps 2 px of proof.
pub const GUIDE_PAD_PX: f64 = 20.0;

/// The tile sizes `choose_shape` tries, largest first: `TILE`, then
/// `TILE × SHRINK_STEP^k` while that stays above `TILE_MIN`, then `TILE_MIN`
/// itself.
pub fn tile_ladder() -> Vec<f64> {
    let mut out = Vec::new();
    let mut t = TILE;
    while t > TILE_MIN + 1e-9 {
        out.push(t);
        t *= SHRINK_STEP;
    }
    out.push(TILE_MIN);
    out
}

/// The radius of ring `ring` (0-based) for tiles of edge `tile`, logical
/// px: `RING_RADII[k] × tile / TILE` — the whole number system is one unit
/// scaled, so a smaller tile brings its rings in with it.
pub fn ring_radius(ring: u8, tile: f64) -> f64 {
    RING_RADII[(ring as usize).min(RING_RADII.len() - 1)] * tile / TILE
}

/// The arc spacing between tile centres for tiles of edge `tile`:
/// `ARC_STEP × tile / TILE` (u·φ at every size).
pub fn arc_step(tile: f64) -> f64 {
    ARC_STEP * tile / TILE
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// One tile's place on the ring.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingSlot {
    /// 0 = the inner ring, 1 = the next, and so on outward.
    pub ring: u8,
    /// Compass degrees, clockwise from north, in `[0, 360)`.
    pub angle_deg: f64,
    /// Distance from the cursor/centre to the tile's centre, logical px.
    pub radius: f64,
    /// Tile edge, logical px (`TILE`, always).
    pub tile: f64,
    /// Angular step between this tile and its neighbours on the same arc,
    /// degrees — 360/n for a full ring of n, span/(n−1) for a partial arc.
    /// The page's fisheye wave measures "one slot away" in this unit, so a
    /// fan's swell has the same width in tiles as the circle's.
    pub pitch_deg: f64,
}

impl RingSlot {
    /// The tile centre's offset from the ring centre, screen axes (y down).
    pub fn offset(&self) -> (f64, f64) {
        let a = self.angle_deg.to_radians();
        (self.radius * a.sin(), -self.radius * a.cos())
    }
}

/// `layout_ring(n_items, tile_px) -> Vec<(ring_index, angle_deg, radius_px)>`
/// — the open-space layout (full circles).
pub fn layout_ring(n_items: usize, tile_px: f64) -> Vec<(u8, f64, f64)> {
    layout_ring_slots(n_items, tile_px)
        .into_iter()
        .map(|s| (s.ring, s.angle_deg, s.radius))
        .collect()
}

/// The open-space layout: full circles at `RING_RADII`, `RING_CAPS` per
/// ring, every ring evenly spaced and staggered. This is `layout_arcs` with
/// unlimited room, and it is what "All" draws (after the clamp has made the
/// room unlimited in effect). Empty when `n_items` exceeds the four rings.
pub fn layout_ring_slots(n_items: usize, tile_px: f64) -> Vec<RingSlot> {
    layout_arcs(n_items, Room::OPEN, tile_px)
        .map(|(slots, _)| slots)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// The SPIRAL layout — "All"'s other shape (owner, 2026-09-15)
// ---------------------------------------------------------------------------

/// The radial step of the spiral, LOGICAL px, derived rather than tuned by
/// eye: in a Vogel spiral `r_i = sqrt(r0² + k²·i)` the area per tile is
/// exactly `π k²`, and a hexagonal packing whose nearest neighbours are `s`
/// apart has area per tile `s²·√3/2`. Equate the two and
/// `k = s · sqrt(√3 / 2π)`. With `s = ARC_STEP` (u·φ = 71, the same
/// tile-derived spacing the rings use) that is ≈ 37.3 px, and the measured
/// nearest-neighbour distance over n = 1..=40 comes out at ARC_STEP within a
/// few px — `the_spiral_never_overlaps_and_matches_the_rings_spacing` pins it.
pub fn spiral_step() -> f64 {
    ARC_STEP * (3.0f64.sqrt() / (2.0 * std::f64::consts::PI)).sqrt()
}

/// THE SPIRAL: `n` tiles, tile *i* at the golden angle × *i* and the radius
/// that keeps the area per tile constant, starting at `RING_R1` so the
/// centre disc keeps its room.
///
/// Every tile reports `ring = 0`: a spiral HAS no rings, and the band test
/// is not what picks it (`spiral_pick` is — nearest by distance, the only
/// sensible test for a layout with no bands). `pitch_deg` is the uniform
/// `360/n` so the page's fisheye wave has a sane width; the dashed guide
/// circles are suppressed for the same reason the bands are.
pub fn spiral_slots(n: usize, tile: f64) -> Vec<RingSlot> {
    let k = spiral_step();
    let r0 = RING_R1;
    (0..n)
        .map(|i| RingSlot {
            ring: 0,
            angle_deg: (GOLDEN_ANGLE_DEG * i as f64).rem_euclid(360.0),
            radius: (r0 * r0 + k * k * i as f64).sqrt(),
            tile,
            pitch_deg: 360.0 / n.max(1) as f64,
        })
        .collect()
}

/// How many tile centres fit around a FULL ring of radius `radius` at
/// `arc_step(tile)` spacing — the spacing limit the Fibonacci caps stay
/// under. Invariant under the shrink (radius and step scale together).
pub fn ring_capacity(radius: f64, tile: f64) -> usize {
    let circumference = 2.0 * std::f64::consts::PI * radius;
    (circumference / arc_step(tile)).floor() as usize
}

/// THE GOLDEN ANGLE, 360° × (1 − 1/φ) — the phyllotaxis angle a sunflower
/// or a pinecone turns by between successive whorls, and the reason no two
/// of their seeds ever line up along one ray. Owner decision (round 4
/// follow-up): the ring-to-ring stagger is this, tied to the same φ that
/// already governs `RING_RADII` and `RING_CAPS`.
///
/// WHY IT REPLACED "half the local pitch". `first_k = first_(k−1) +
/// pitch_k / 2` is optimal only when the two rings' tile counts share a
/// convenient factor, and it is CATASTROPHIC when they do not. For the
/// shipped caps 8 → 13 the two angle sets live on a lattice of
/// 360/lcm(8,13) = 3.4615°, and half of ring 2's pitch is 13.846° = exactly
/// 4 lattice steps — so four of ring 2's thirteen tiles sat at EXACTLY the
/// bearing of a ring-1 tile, one app hidden directly behind another. That
/// is the owner's "one app directly behind another when aiming at a fan",
/// and it was an exact zero, not a near miss. The golden angle is
/// irrational with respect to every such lattice, so an exact coincidence
/// is impossible for ANY pair of counts — `no_outer_tile_hides_behind_an_
/// inner_one` sweeps n = 1..=26 and measures the worst case.
pub const GOLDEN_ANGLE_DEG: f64 = 360.0 * (1.0 - 1.0 / PHI);

/// The golden SECTION, 1/φ — the fraction of a partial arc a lone outer
/// tile is placed at, and (as `1 − 1/φ`) the share of one pitch an outer
/// arc's run is shifted by. Same constant, used where a whole turn makes no
/// sense because the arc is not a circle.
pub const GOLDEN_SECTION: f64 = 1.0 / PHI;

/// The stagger: ring 0 starts at north; each outer FULL ring is rotated by
/// the GOLDEN ANGLE relative to the ring inside it (`first_k = first_(k−1)
/// + 137.5°`), so no outer tile can ever sit at an inner tile's bearing.
/// `pitch` is no longer read — kept in the signature because the caller
/// computes it anyway and a future rule may want it.
pub fn stagger(inner_first: f64, ring: u8, _pitch: f64) -> f64 {
    if ring == 0 {
        0.0
    } else {
        (inner_first + GOLDEN_ANGLE_DEG).rem_euclid(360.0)
    }
}

/// The WIDEST an outer partial arc's run may be shifted off its own arc
/// start: one nominal slot, and never more than a quarter of the arc (a
/// narrow arc must not have its run squeezed into one corner). Zero for
/// ring 0 and for a lone tile. The tiles still END on `hi` and the shift is
/// INWARD from `lo`, so every tile stays inside the arc `feasible_arc`
/// proved — containment can only improve, never regress.
pub fn arc_phase(ring: u8, span: f64, count: usize) -> f64 {
    if ring == 0 || count < 2 || span <= 0.0 {
        return 0.0;
    }
    (span / count as f64).min(span * 0.25).max(0.0)
}

/// How many phases `best_arc_phase` tries inside that window.
const PHASE_STEPS: usize = 32;

/// WHICH phase an outer partial arc actually takes: the one, within
/// `arc_phase`'s window, that leaves the widest angular gap between this
/// arc's tiles and `inner`'s (the ring immediately inside it).
///
/// A FIXED golden fraction is not enough here, and the test that found it
/// out is `a_fans_outer_arc_does_not_repeat_the_inner_arcs_phase`: on the
/// owner's own right-edge press the two feasible arcs start 8.1° apart and
/// the golden share of one slot is 8.46°, so a fixed shift landed the outer
/// run 0.36° from the inner one — the very alignment it was added to
/// prevent. A full circle has no such accident to dodge (every ring spans
/// the same 360°, so the golden angle alone does it); a fan's two arcs are
/// different lengths at different radii and their relationship is not
/// known until both are computed. So the phase is CHOSEN, over a window
/// whose every value is already safe, and the golden phase is where the
/// search starts and the tie-break it returns to.
pub fn best_arc_phase(inner: &[f64], ring: u8, arc: (f64, f64), count: usize) -> f64 {
    let span = arc.1 - arc.0;
    let max = arc_phase(ring, span, count);
    if max <= 0.0 || inner.is_empty() {
        return 0.0;
    }
    let golden = (span / count as f64) * (1.0 - GOLDEN_SECTION);
    let score = |phase: f64| -> f64 {
        let pitch = (span - phase) / (count - 1) as f64;
        let mut worst = f64::INFINITY;
        for i in 0..count {
            let a = arc.0 + phase + i as f64 * pitch;
            for &b in inner {
                worst = worst.min(compass_dist(a, b));
            }
        }
        worst
    };
    let mut best = (0.0f64, score(0.0));
    for k in 0..=PHASE_STEPS {
        let phase = max * k as f64 / PHASE_STEPS as f64;
        let s = score(phase);
        // A tie goes to whichever candidate is nearer the golden phase.
        if s > best.1 + 1e-9
            || ((s - best.1).abs() <= 1e-9 && (phase - golden).abs() < (best.0 - golden).abs())
        {
            best = (phase, s);
        }
    }
    best.0
}

// ---------------------------------------------------------------------------
// The room around the press point, and the arc available at each radius
// ---------------------------------------------------------------------------

/// Room around the ring CENTRE on each side, LOGICAL px, up to the work-area
/// edge: `left` is how far the centre is from the left edge, and so on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Room {
    pub left: f64,
    pub right: f64,
    pub up: f64,
    pub down: f64,
}

impl Room {
    /// Unlimited room on every side — open space.
    pub const OPEN: Room = Room {
        left: f64::INFINITY,
        right: f64::INFINITY,
        up: f64::INFINITY,
        down: f64::INFINITY,
    };

    /// From a PHYSICAL cursor and work area at `scale`.
    pub fn at(cursor: (f64, f64), area: WorkArea, scale: f64) -> Room {
        Room {
            left: (cursor.0 - area.x) / scale,
            right: (area.x + area.w - cursor.0) / scale,
            up: (cursor.1 - area.y) / scale,
            down: (area.y + area.h - cursor.1) / scale,
        }
    }

    /// The room seen from a centre moved by `(dx, dy)` logical px (screen
    /// axes, y down) — what a snapped centre sees.
    pub fn shifted(&self, dx: f64, dy: f64) -> Room {
        Room {
            left: self.left + dx,
            right: self.right - dx,
            up: self.up + dy,
            down: self.down - dy,
        }
    }

    /// Does a tile of edge `tile` centred at `(dx, dy)` from the centre lie
    /// inside this room with `CLAMP_MARGIN` to spare on every side?
    pub fn holds(&self, dx: f64, dy: f64, tile: f64) -> bool {
        let h = tile / 2.0 + CLAMP_MARGIN;
        -dx + h <= self.left + 1e-6
            && dx + h <= self.right + 1e-6
            && -dy + h <= self.up + 1e-6
            && dy + h <= self.down + 1e-6
    }
}

/// The safe margin between a tile's edge and the work-area edge, LOGICAL px:
/// the hovered tile's 8 px accent halo plus 8 px of air (`.mr-tile.armed`'s
/// `0 0 0 8px` ring). Scaled by the monitor's scale factor before it meets
/// the physical work area (`clamp_extent`).
pub const CLAMP_MARGIN: f64 = 16.0;

/// The angular resolution of `feasible_arc`. 360/0.25 is an integer, so a
/// mirrored room samples mirrored angles — the edges and corners come out
/// symmetric to the bit.
pub const ARC_SCAN_DEG: f64 = 0.25;

/// The arc available to a ring: the LONGEST contiguous run of compass angles
/// at which a tile of edge `tile`, centred `r` from the centre, lies inside
/// `room` with the margin. `Some((lo, hi))` with `lo ≤ hi`; `hi` may exceed
/// 360 when the arc crosses north; `Some((0, 360))` when every angle fits (a
/// full circle); `None` when no angle does. On an edge line the arc is the
/// on-screen 180° less the tilt a tile needs to clear the edge (acos((u/2 +
/// margin) / r) each side); at a corner point the 90° quadrant less the
/// same on each wall.
///
/// A scan at `ARC_SCAN_DEG`, not a closed form: the feasible set is the
/// circle of radius `r` cut by up to four half-planes, which is up to four
/// separate arcs, and the scan finds the longest of them without a case
/// analysis anyone has to trust. Every angle it returns is feasible by
/// construction; `slots_fit` re-checks the placed tiles regardless.
pub fn feasible_arc(room: Room, r: f64, tile: f64) -> Option<(f64, f64)> {
    let n = (360.0 / ARC_SCAN_DEG).round() as usize;
    let step = 360.0 / n as f64;
    let ok: Vec<bool> = (0..n)
        .map(|i| {
            let a = (i as f64 * step).to_radians();
            room.holds(r * a.sin(), -r * a.cos(), tile)
        })
        .collect();
    let total = ok.iter().filter(|&&b| b).count();
    if total == 0 {
        return None;
    }
    if total == n {
        return Some((0.0, 360.0));
    }
    // The longest circular run of feasible samples, walked from just after a
    // known-infeasible one so no run is split by the array boundary.
    let first_bad = ok.iter().position(|&b| !b).unwrap_or(0);
    let mut best = (0usize, 0usize);
    let mut start: Option<usize> = None;
    let mut len = 0usize;
    for k in 1..=n {
        let i = (first_bad + k) % n;
        if ok[i] {
            if start.is_none() {
                start = Some(i);
                len = 0;
            }
            len += 1;
            if len > best.1 {
                best = (start.unwrap_or(i), len);
            }
        } else {
            start = None;
            len = 0;
        }
    }
    let lo = best.0 as f64 * step;
    Some((lo, lo + (best.1 - 1) as f64 * step))
}

/// How many tiles an arc takes: a full circle holds its Fibonacci cap
/// (`RING_CAPS`, never more than `ring_capacity`); a partial arc holds
/// floor(arc_len / `ARC_STEP`) + 1 (a tile at the start, one per 71 px of
/// arc after it) — which on the nominal on-screen arcs is exactly the
/// owner's floor(arc_len / 71): a half ring 4 / 7 / 12 / 20, a quarter ring
/// 2 / 3 / 6 / 10 (`edge_capacities_are_the_owners_numbers`). Never less
/// than one.
///
/// The Fibonacci cap binds a PARTIAL arc too (round 6): before it did not,
/// so a 314° arc at the right edge held NINE inner tiles where a full
/// circle holds five, and a press ten px further in changed the ring's
/// whole character. The owner's half / quarter numbers are all under the
/// caps, so they are untouched.
pub fn arc_capacity(ring: u8, r: f64, tile: f64, arc: (f64, f64)) -> usize {
    let span = arc.1 - arc.0;
    let cap = RING_CAPS.get(ring as usize).copied().unwrap_or(usize::MAX);
    let by_spacing = if span >= 360.0 - 1e-6 {
        ring_capacity(r, tile)
    } else {
        (r * span.to_radians() / arc_step(tile)).floor() as usize + 1
    };
    by_spacing.min(cap).max(1)
}

/// One ring of a layout as `layout_arcs` decided it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcInfo {
    pub ring: u8,
    pub radius: f64,
    /// The arc the tiles were spread over (`feasible_arc`'s answer).
    pub lo: f64,
    pub hi: f64,
    pub count: usize,
}

impl ArcInfo {
    pub fn full(&self) -> bool {
        self.hi - self.lo >= 360.0 - 1e-6
    }
    pub fn span(&self) -> f64 {
        self.hi - self.lo
    }
    /// The arc's middle bearing, in `[0, 360)`.
    pub fn mid(&self) -> f64 {
        ((self.lo + self.hi) / 2.0).rem_euclid(360.0)
    }
}

/// THE LAYOUT LAW, as one function: `n` tiles of edge `tile` around the
/// centre with `room` on each side. Rings open outward at `ring_radius(k,
/// tile)` (`RING_RADII` scaled by the tile); each takes the arc
/// `feasible_arc` finds at its radius and as many tiles as `arc_capacity`
/// allows, evenly spread over that arc (a full ring at 360/k, staggered
/// against the ring inside it; a partial arc from end to end at
/// span/(k−1)); the rest overflow to the next ring. `None` when the room
/// cannot hold the count within `MAX_RINGS` — the caller then shrinks the
/// tile, nudges the centre or clamps (`choose_shape`).
pub fn layout_arcs(n: usize, room: Room, tile: f64) -> Option<(Vec<RingSlot>, Vec<ArcInfo>)> {
    let mut slots = Vec::with_capacity(n);
    let mut arcs = Vec::new();
    let mut remaining = n;
    let mut ring: u8 = 0;
    let mut inner_first = 0.0;
    while remaining > 0 {
        if ring >= MAX_RINGS {
            return None;
        }
        let r = ring_radius(ring, tile);
        let arc = feasible_arc(room, r, tile)?;
        let take = remaining.min(arc_capacity(ring, r, tile, arc));
        inner_first = place_arc(&mut slots, ring, take, r, tile, arc, inner_first);
        arcs.push(ArcInfo { ring, radius: r, lo: arc.0, hi: arc.1, count: take });
        remaining -= take;
        ring += 1;
    }
    Some((slots, arcs))
}

/// `count` tiles evenly over `arc` at `radius`: a full circle at 360/count,
/// starting at `stagger(inner_first, ring, pitch)`; a partial arc with a
/// tile at each end and `span/(count−1)` between; a single tile at the
/// arc's middle. Returns the first tile's bearing (the next ring's stagger
/// base).
fn place_arc(
    out: &mut Vec<RingSlot>,
    ring: u8,
    count: usize,
    radius: f64,
    tile: f64,
    arc: (f64, f64),
    inner_first: f64,
) -> f64 {
    if count == 0 {
        return inner_first;
    }
    let span = arc.1 - arc.0;
    let full = span >= 360.0 - 1e-6;
    let (first, pitch) = if full {
        let pitch = 360.0 / count as f64;
        (stagger(inner_first, ring, pitch), pitch)
    } else if count == 1 {
        // A lone tile: the arc's middle on the inner ring, its GOLDEN
        // SECTION on an outer one — still well inside the arc, and never
        // the same bearing the inner ring's own middle tile took.
        if ring == 0 {
            ((arc.0 + arc.1) / 2.0, 360.0)
        } else {
            (arc.0 + span * GOLDEN_SECTION, 360.0)
        }
    } else {
        // A run from end to end on the inner ring; on an outer ring the
        // run STARTS a chosen phase inside `lo` and still ends on `hi`, so
        // its phase and its pitch both differ from the arc inside it (the
        // owner's "one app directly behind another" on a fan). Never wider
        // than the feasible arc — the shift is inward.
        let inner: Vec<f64> = if ring == 0 {
            Vec::new()
        } else {
            out.iter().filter(|s| s.ring + 1 == ring).map(|s| s.angle_deg).collect()
        };
        let phase = best_arc_phase(&inner, ring, arc, count);
        (arc.0 + phase, (span - phase) / (count - 1) as f64)
    };
    for i in 0..count {
        out.push(RingSlot {
            ring,
            angle_deg: (first + i as f64 * pitch).rem_euclid(360.0),
            radius,
            tile,
            pitch_deg: pitch,
        });
    }
    first.rem_euclid(360.0)
}

/// Does every tile of `slots`, drawn around the centre, lie inside the room
/// with `CLAMP_MARGIN` to spare on every side? Logical px throughout.
pub fn slots_fit(slots: &[RingSlot], room: Room) -> bool {
    slots.iter().all(|s| {
        let (dx, dy) = s.offset();
        room.holds(dx, dy, s.tile)
    })
}

// ---------------------------------------------------------------------------
// The shape at the press point — the name the marker line prints
// ---------------------------------------------------------------------------

/// Which side of the centre a ring opens toward. Compass names: `N` is up.
/// A HALF ring "facing" `W` lies to the WEST of its centre (the press point
/// is near the right edge); a QUARTER ring opening `SW` fills the south-west
/// quadrant (the press point is near the top-right corner). SINCE ROUND 6
/// the centre of a half or quarter ring IS the press point — the names
/// describe the arcs, not a moved centre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingShape {
    /// Open space: every ring a full circle at the press point.
    Circle,
    /// The first partial arc opens toward a CARDINAL direction: one wall
    /// cuts it. Centre = the press point.
    Half(Dir),
    /// The first partial arc opens toward a DIAGONAL: two walls cut it.
    /// Centre = the press point.
    Quarter(Dir),
    /// Even the floor tile (`TILE_MIN`) could not hold the count around the
    /// press point, so the centre was MOVED by the smallest grid vector
    /// that lets the floor-size arcs fit (`nudge`); `Dir` is the direction
    /// it moved. The cursor is warped to the new centre.
    Nudged(Dir),
    /// Nothing fits anywhere near the press point (a work area smaller than
    /// the smallest ring) — and always for "All": the full circles, clamped
    /// on-screen and the cursor warped.
    CircleClamped,
    /// "All", arranged as one phyllotaxis SPIRAL (owner, 2026-09-15).
    /// Clamped and warped exactly as `CircleClamped` — it is a variant of
    /// "All", not of Favourites — and named apart only so the log and the
    /// page's `data-shape` can tell the two arrangements apart.
    Spiral,
}

/// The eight compass directions a shape can open toward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

impl Dir {
    /// Compass degrees, clockwise from north.
    pub fn deg(self) -> f64 {
        match self {
            Dir::N => 0.0,
            Dir::NE => 45.0,
            Dir::E => 90.0,
            Dir::SE => 135.0,
            Dir::S => 180.0,
            Dir::SW => 225.0,
            Dir::W => 270.0,
            Dir::NW => 315.0,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Dir::N => "N",
            Dir::NE => "NE",
            Dir::E => "E",
            Dir::SE => "SE",
            Dir::S => "S",
            Dir::SW => "SW",
            Dir::W => "W",
            Dir::NW => "NW",
        }
    }
    /// The direction nearest a compass bearing.
    pub fn nearest(bearing_deg: f64) -> Dir {
        const ALL: [Dir; 8] = [Dir::N, Dir::NE, Dir::E, Dir::SE, Dir::S, Dir::SW, Dir::W, Dir::NW];
        let b = bearing_deg.rem_euclid(360.0);
        let i = ((b + 22.5) / 45.0).floor() as usize % 8;
        ALL[i]
    }
    /// Is this one of the four cardinals (N/E/S/W)?
    pub fn is_cardinal(self) -> bool {
        matches!(self, Dir::N | Dir::E | Dir::S | Dir::W)
    }
}

impl RingShape {
    /// The marker line's spelling: `circle`, `half-W`, `quarter-SW`,
    /// `nudged-E`, `circle-clamped`, `spiral`.
    pub fn name(self) -> String {
        match self {
            RingShape::Circle => "circle".into(),
            RingShape::Half(d) => format!("half-{}", d.name()),
            RingShape::Quarter(d) => format!("quarter-{}", d.name()),
            RingShape::Nudged(d) => format!("nudged-{}", d.name()),
            RingShape::CircleClamped => "circle-clamped".into(),
            RingShape::Spiral => "spiral".into(),
        }
    }
    /// Does this shape decide its own centre (the press point plus
    /// `Placement.offset`) rather than leave it to `clamp_ring_center`?
    pub fn anchored(self) -> bool {
        !matches!(self, RingShape::CircleClamped | RingShape::Spiral)
    }
}

/// What `choose_shape` decided for a press.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub shape: RingShape,
    /// The ring centre relative to the press point, LOGICAL px, screen
    /// axes: `(0, 0)` for a circle and for every half / quarter ring since
    /// round 6; the nudge vector for `Nudged` (meaningless for
    /// `CircleClamped` — the caller clamps instead).
    pub offset: (f64, f64),
    pub slots: Vec<RingSlot>,
    /// One entry per ring used, innermost first — the arc each ring's tiles
    /// were spread over. Empty for a spiral. The page draws its dashed
    /// guides over exactly these arcs (`guide_arcs`).
    pub arcs: Vec<ArcInfo>,
}

/// The shape a set of arcs IS: `Circle` when every ring is full; otherwise
/// named by the first (innermost) partial arc's middle bearing — a cardinal
/// is a `Half` (one wall cut it), a diagonal a `Quarter` (two walls did).
pub fn shape_of_arcs(arcs: &[ArcInfo]) -> RingShape {
    match arcs.iter().find(|a| !a.full()) {
        None => RingShape::Circle,
        Some(a) => {
            let d = Dir::nearest(a.mid());
            if d.is_cardinal() {
                RingShape::Half(d)
            } else {
                RingShape::Quarter(d)
            }
        }
    }
}

/// THE DECISION for Favourites (round 6 — the owner's round-3 law, restored):
///
/// 1. ANCHOR. `layout_arcs` around the press point at `TILE`: full circles
///    in open space, arcs near a wall, named by `shape_of_arcs`. Offset
///    `(0, 0)` — the centre never moves.
/// 2. SHRINK. If that cannot hold the count, the tile descends the
///    `tile_ladder` (radii and spacing scale with it) and the first size
///    that fits wins — still around the press point, still offset `(0, 0)`.
/// 3. NUDGE. If even `TILE_MIN` cannot, the centre moves by the smallest
///    grid vector that lets the floor-size arcs fit (`nudge`) — `Nudged`,
///    with that vector as the offset.
/// 4. CLAMP. If no such vector exists, `CircleClamped` with the full-size
///    circles — the caller's `clamp_ring_center` + warp, as for "All".
///
/// Pure: `shape_tests`, `layout_law_tests` and `round6_tests` walk it.
pub fn choose_shape(n: usize, room: Room) -> Placement {
    for tile in tile_ladder() {
        if let Some((slots, arcs)) = layout_arcs(n, room, tile) {
            debug_assert!(slots_fit(&slots, room));
            return Placement { shape: shape_of_arcs(&arcs), offset: (0.0, 0.0), slots, arcs };
        }
    }
    if let Some((offset, slots, arcs)) = nudge(n, room) {
        let dir = Dir::nearest(offset.0.atan2(-offset.1).to_degrees());
        return Placement { shape: RingShape::Nudged(dir), offset, slots, arcs };
    }
    let (slots, arcs) = layout_arcs(n, Room::OPEN, TILE).unwrap_or_default();
    Placement { shape: RingShape::CircleClamped, offset: (0.0, 0.0), slots, arcs }
}

/// The candidate centre moves `nudge` tries, shortest first: every point of
/// the `NUDGE_STEP` grid within `NUDGE_MAX` of the press point along each
/// axis, and never past the middle of the room on an axis (beyond the
/// middle the far wall is the near wall — mirrored, never better). The
/// origin is excluded (it was already tried at every tile size).
pub fn nudge_candidates(room: Room) -> Vec<(f64, f64)> {
    let reach = |a: f64, b: f64| -> f64 {
        // Half the room's extent on this axis, or NUDGE_MAX when the room
        // is open on a side.
        let half = if a.is_finite() && b.is_finite() { (a + b) / 2.0 } else { NUDGE_MAX };
        half.min(NUDGE_MAX).max(0.0)
    };
    let rx = reach(room.left, room.right);
    let ry = reach(room.up, room.down);
    let kx = (rx / NUDGE_STEP).floor() as i64;
    let ky = (ry / NUDGE_STEP).floor() as i64;
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(((2 * kx + 1) * (2 * ky + 1)) as usize);
    for i in -kx..=kx {
        for j in -ky..=ky {
            if i == 0 && j == 0 {
                continue;
            }
            out.push((i as f64 * NUDGE_STEP, j as f64 * NUDGE_STEP));
        }
    }
    // Shortest first; ties broken toward the more spacious side of the room
    // (a move away from the nearer wall), then by (dx, dy) for determinism.
    let toward_space = |(dx, dy): (f64, f64)| -> f64 {
        let sx = if room.right >= room.left { dx } else { -dx };
        let sy = if room.down >= room.up { dy } else { -dy };
        -(sx + sy)
    };
    out.sort_by(|a, b| {
        let la = a.0 * a.0 + a.1 * a.1;
        let lb = b.0 * b.0 + b.1 * b.1;
        la.partial_cmp(&lb)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(toward_space(*a).partial_cmp(&toward_space(*b)).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    out
}

/// THE NUDGE: the shortest `nudge_candidates` vector from which the
/// floor-size (`TILE_MIN`) arcs hold `n`, with the layout as seen from
/// there. `None` when no candidate does — the room is simply too small.
/// Minimal on the grid by construction (candidates are tried shortest
/// first); the true minimum is therefore within one grid diagonal
/// (`NUDGE_STEP·√2` ≈ 11 px) of it.
pub fn nudge(n: usize, room: Room) -> Option<((f64, f64), Vec<RingSlot>, Vec<ArcInfo>)> {
    // A room narrower than one floor tile plus its halos on an axis can
    // never hold a tile anywhere — no search.
    let least = TILE_MIN + 2.0 * CLAMP_MARGIN;
    if room.left + room.right < least || room.up + room.down < least {
        return None;
    }
    for (dx, dy) in nudge_candidates(room) {
        let seen = room.shifted(dx, dy);
        if let Some((slots, arcs)) = layout_arcs(n, seen, TILE_MIN) {
            debug_assert!(slots_fit(&slots, seen));
            return Some(((dx, dy), slots, arcs));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Derived geometry: extent, scrim, guides, pill
// ---------------------------------------------------------------------------

/// The ring's outer extent — the radius that must stay on screen, tiles
/// included. Zero for an empty layout.
pub fn ring_extent(slots: &[RingSlot]) -> f64 {
    slots
        .iter()
        .map(|s| s.radius + s.tile / 2.0)
        .fold(0.0, f64::max)
}

/// The scrim the page draws for a layout: the design's two sizes (600 for a
/// single ring, 640 for more), grown when a layout reaches past them so the
/// fade always outlasts the outermost tiles. It now fades to nothing well
/// inside the canvas — it is cut only by the screen edge, never a window's.
pub fn scrim_diameter(slots: &[RingSlot]) -> f64 {
    let design = if slots.iter().any(|s| s.ring > 0) { SCRIM_D_MULTI } else { SCRIM_D_ONE };
    design.max(2.0 * ring_extent(slots) + 80.0)
}

/// The dashed guide circles' diameters, innermost first: `2 × radius + 14`
/// for every ring present (the artboard's 264 for r = 125).
pub fn guide_diameters(slots: &[RingSlot]) -> Vec<f64> {
    // A SPIRAL HAS NO RINGS TO DRAW. Its tiles all report `ring = 0` and
    // every one of them sits at a different radius, so the per-ring rule
    // below would draw one dashed circle through the innermost tile and
    // nothing through the rest — a guide that guides nowhere. The test is
    // the layout's own shape, not a flag: tiles sharing a ring but not a
    // radius are a spiral by construction.
    let mut by_ring: std::collections::BTreeMap<u8, Vec<f64>> = std::collections::BTreeMap::new();
    for s in slots {
        by_ring.entry(s.ring).or_default().push(s.radius);
    }
    if by_ring
        .values()
        .any(|rs| rs.iter().any(|r| (r - rs[0]).abs() > 0.5))
    {
        return Vec::new();
    }
    let mut rings: Vec<u8> = slots.iter().map(|s| s.ring).collect();
    rings.sort_unstable();
    rings.dedup();
    rings
        .into_iter()
        .map(|r| {
            let radius = slots.iter().find(|s| s.ring == r).map_or(0.0, |s| s.radius);
            radius * 2.0 + GUIDE_EXTRA
        })
        .collect()
}

/// The shrink factor a layout was drawn at: its smallest tile over `TILE`
/// (1 for every full-size layout, down to `TILE_MIN / TILE` ≈ 0.61 at the
/// floor). The pill's resting size and the dead zone scale by it so a
/// shrunken inner ring is never overlapped by a full-size pill.
pub fn tile_scale(slots: &[RingSlot]) -> f64 {
    let t = slots.iter().map(|s| s.tile).fold(f64::INFINITY, f64::min);
    if t.is_finite() && t > 0.0 {
        (t / TILE).min(1.0)
    } else {
        1.0
    }
}

/// How large the centre pill may grow for a long name: the free circle
/// inside the innermost tiles' inner edges, less `PILL_FREE_MARGIN` a side —
/// 162 px for normal tiles at r = 108 — and never below its resting `PILL_D`
/// (scaled with the tiles: at the floor the rest is 70 and the free circle
/// 96, and the page rests the pill at min(130, this) — `fitPill`).
pub fn pill_max_diameter(slots: &[RingSlot]) -> f64 {
    let inner_edge = slots
        .iter()
        .map(|s| s.radius - s.tile / 2.0)
        .fold(f64::INFINITY, f64::min);
    if !inner_edge.is_finite() {
        return PILL_D;
    }
    (2.0 * (inner_edge - PILL_FREE_MARGIN)).max(PILL_D * tile_scale(slots))
}

/// One dashed guide as the page draws it: a circle of diameter `d` around
/// the centre, of which only the compass span `lo..hi` is stroked (`0..360`
/// = the whole circle). Degrees clockwise from north, `hi` may exceed 360.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct GuideArc {
    pub d: f64,
    pub lo: f64,
    pub hi: f64,
}

/// The guides as ARCS (round 6): one per ring in `guide_diameters`' order,
/// each spanning its ring's feasible arc (`ArcInfo.lo..hi`) plus
/// `GUIDE_PAD_PX` of arc at each end — never more than half a pitch, and
/// never wrapping into a full circle. A full ring is `0..360`. Every point
/// on every arc is on-screen by construction (see `GUIDE_PAD_PX`), which
/// `round6_tests::guide_arcs_never_leave_the_room` measures. With no arc
/// info (a spiral has none; a full-circle layout with none passed) every
/// guide is a full circle — the pre-round-6 drawing.
pub fn guide_arcs(slots: &[RingSlot], arcs: &[ArcInfo]) -> Vec<GuideArc> {
    let ds = guide_diameters(slots);
    let mut rings: Vec<u8> = slots.iter().map(|s| s.ring).collect();
    rings.sort_unstable();
    rings.dedup();
    ds.into_iter()
        .zip(rings)
        .map(|(d, ring)| {
            let info = arcs.iter().find(|a| a.ring == ring);
            match info {
                Some(a) if !a.full() => {
                    let guide_r = d / 2.0;
                    let pad_deg = (GUIDE_PAD_PX / guide_r).to_degrees();
                    let pitch = if a.count > 1 { a.span() / (a.count - 1) as f64 } else { a.span() };
                    let pad = pad_deg.min(pitch / 2.0).max(0.0);
                    let lo = a.lo - pad;
                    let hi = (a.hi + pad).min(lo + 360.0);
                    GuideArc { d, lo, hi }
                }
                _ => GuideArc { d, lo: 0.0, hi: 360.0 },
            }
        })
        .collect()
}

/// A rectangle in the canvas page's CSS px — where a PHYSICAL rectangle
/// (the room) lands on the page, for the scrim's clip.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct PageRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// `page_point` for a whole rectangle: the room in the page's CSS px.
pub fn page_rect(phys: WorkArea, canvas: WorkArea, scale: f64) -> PageRect {
    let (x, y) = page_point((phys.x, phys.y), canvas, scale);
    PageRect { x, y, w: phys.w / scale, h: phys.h / scale }
}

/// The distance from the ring centre to the furthest thing that must stay
/// on-screen, PHYSICAL px: the outermost tile's outer edge plus the halo
/// margin, at `scale`. The scrim is deliberately NOT in it — it fades to
/// nothing and may be cut by the screen edge; a tile may not (artboard 5).
pub fn clamp_extent(slots: &[RingSlot], scale: f64) -> f64 {
    (ring_extent(slots) + CLAMP_MARGIN) * scale
}

// ---------------------------------------------------------------------------
// The canvas — ONE BIG WINDOW, the work area of the cursor's monitor
// ---------------------------------------------------------------------------

/// A rectangle in whatever unit the caller uses — a monitor's work area or
/// bounds in physical px, from `Monitor::work_area()` / `size()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkArea {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl WorkArea {
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
}

/// How far the canvas is inset from a work area that equals the monitor
/// bounds (an auto-hide taskbar): a transparent window whose rectangle IS
/// the monitor composes zero pixels on this machine (PROBLEM 37 family), so
/// the canvas is always at least this much smaller on every side.
pub const CANVAS_INSET: f64 = 2.0;

/// THE CANVAS (round 3): the overlay window's rectangle for either ring —
/// the whole work area of the target monitor (taskbar excluded), and NEVER
/// the exact monitor bounds. When the work area equals the bounds the canvas
/// is inset by `CANVAS_INSET` per side. Same unit in, same unit out
/// (physical px at the call sites).
pub fn canvas_rect(work: WorkArea, monitor: WorkArea) -> WorkArea {
    let same = (work.x - monitor.x).abs() < 0.5
        && (work.y - monitor.y).abs() < 0.5
        && (work.w - monitor.w).abs() < 0.5
        && (work.h - monitor.h).abs() < 0.5;
    if same {
        WorkArea {
            x: work.x + CANVAS_INSET,
            y: work.y + CANVAS_INSET,
            w: (work.w - 2.0 * CANVAS_INSET).max(1.0),
            h: (work.h - 2.0 * CANVAS_INSET).max(1.0),
        }
    } else {
        work
    }
}

/// THE ROOM THE LAYOUT MAY USE, in the same (physical) unit: the CANVAS,
/// less any band an AUTO-HIDDEN appbar will take back when it slides out.
///
/// TWO FIXES IN ONE FUNCTION, both measured on the owner's machine
/// 2026-09-15, and both of them right/bottom-only — which is exactly the
/// asymmetry he reported ("left and top fine, right and bottom clipped").
///
/// 1. **The layout was measured against the WORK AREA while the window is
///    the CANVAS.** `canvas_rect` insets the window 2 px per side whenever
///    the work area equals the monitor bounds — which is the owner's normal
///    state, because his taskbar is set to AUTO-HIDE and Windows does not
///    subtract an auto-hidden appbar from the work area (his log:
///    `canvas 2556x1596 @ (2,2)`). Moving the origin +2 pushes content 2 px
///    further INSIDE on the left and top, where it is harmless, and 2 px
///    OUTSIDE the window on the right and bottom, where it is clipped.
/// 2. **An auto-hidden taskbar is topmost and is not in the work area.**
///    Measured with `GetMonitorInfo`: `rcWork` = (0,48,2560,1600) — nothing
///    reserved at the bottom — while `Shell_TrayWnd` sits at
///    (0,1598)-(2560,1670), 72 px tall, auto-hide. The Favourites snap then
///    put the ring centre on y = 1600 and WARPED THE CURSOR THERE (his log,
///    hold #21: `centre (1482,1600) … clamp delta (0,193)`), which is the
///    one gesture that makes that taskbar slide out — over our topmost
///    overlay, over the bottom of the ring. Reserving the band keeps the
///    layout, the snap and the warp out of it.
///
/// `reserve` is (left, top, right, bottom) in the canvas's unit; zero on
/// every side when there is no auto-hidden bar. Pure — the impure probe is
/// `commands::autohide_reserve_for`.
pub fn room_rect(canvas: WorkArea, reserve: (f64, f64, f64, f64)) -> WorkArea {
    let (l, t, r, b) = reserve;
    let x = canvas.x + l.max(0.0);
    let y = canvas.y + t.max(0.0);
    let w = canvas.w - l.max(0.0) - r.max(0.0);
    let h = canvas.h - t.max(0.0) - b.max(0.0);
    if w < 1.0 || h < 1.0 {
        return canvas;
    }
    WorkArea { x, y, w, h }
}

/// How much of `monitor` an appbar at `bar` takes back, as
/// (left, top, right, bottom) — the overlap of the two rectangles resolved
/// to the single edge the bar is docked against, so a bar that is currently
/// slid off-screen (an auto-hidden taskbar shows a 2 px sliver) still
/// reserves its FULL height or width. Zero on every side when the bar does
/// not touch this monitor. Same unit in, same unit out.
pub fn appbar_reserve(monitor: WorkArea, bar: WorkArea) -> (f64, f64, f64, f64) {
    let ox = bar.x.max(monitor.x);
    let oy = bar.y.max(monitor.y);
    let ox2 = bar.right().min(monitor.right());
    let oy2 = bar.bottom().min(monitor.bottom());
    if ox2 <= ox || oy2 <= oy {
        return (0.0, 0.0, 0.0, 0.0);
    }
    // The docked edge is the one the bar's own thin axis lies against.
    if bar.w >= bar.h {
        // Horizontal bar: top or bottom.
        if bar.y + bar.h / 2.0 <= monitor.y + monitor.h / 2.0 {
            (0.0, (bar.bottom() - monitor.y).min(monitor.h), 0.0, 0.0)
        } else {
            (0.0, 0.0, 0.0, (monitor.bottom() - bar.y).min(monitor.h))
        }
    } else if bar.x + bar.w / 2.0 <= monitor.x + monitor.w / 2.0 {
        ((bar.right() - monitor.x).min(monitor.w), 0.0, 0.0, 0.0)
    } else {
        (0.0, 0.0, (monitor.right() - bar.x).min(monitor.w), 0.0)
    }
}

/// Where a PHYSICAL point lands in the canvas page's CSS px: the offset from
/// the canvas origin, divided by the monitor's scale. This is the arithmetic
/// that was wrong in 1.0.110 (a logical centre handed to a physical fitter),
/// pinned as one function with the log's numbers as its tests.
pub fn page_point(phys: (f64, f64), canvas: WorkArea, scale: f64) -> (f64, f64) {
    ((phys.0 - canvas.x) / scale, (phys.1 - canvas.y) / scale)
}

/// The Space ring's STAGE inside its canvas (round 3): a `w × h` CSS-px box
/// whose centre is the monitor's centre `centre_phys` — exactly where the
/// old, ring-sized window was centred — expressed as the box's top-left in
/// the canvas page's CSS px. Pure; `overlay_fit_hud` returns it to the page.
pub fn stage_box(canvas: WorkArea, centre_phys: (f64, f64), scale: f64, w: f64, h: f64) -> (f64, f64) {
    let (cx, cy) = page_point(centre_phys, canvas, scale);
    (cx - w / 2.0, cy - h / 2.0)
}

// ---------------------------------------------------------------------------
// Edge clamp — "All" only
// ---------------------------------------------------------------------------

/// Where the ring's centre goes so that the WHOLE ring — `extent` = radius
/// plus half a tile, in the same unit as `cursor` and `area` — stays inside
/// `area`. The cursor is the centre whenever that already holds; near an edge
/// or a corner the centre slides inward by exactly the overhang, and no
/// further (artboard 5). A work area too small to hold the ring at all pins
/// the centre to the area's own centre rather than off one edge.
pub fn clamp_ring_center(cursor: (f64, f64), area: WorkArea, extent: f64) -> (f64, f64) {
    let clamp_axis = |c: f64, lo: f64, len: f64| -> f64 {
        if len <= 2.0 * extent {
            return lo + len / 2.0;
        }
        c.max(lo + extent).min(lo + len - extent)
    };
    (
        clamp_axis(cursor.0, area.x, area.w),
        clamp_axis(cursor.1, area.y, area.h),
    )
}

// ---------------------------------------------------------------------------
// The centre pill's two lines — `split_display_name`
// ---------------------------------------------------------------------------

/// Vendor prefixes stripped from an app's name AT DISPLAY TIME ONLY (the
/// binding's label is never changed): "Google Chrome" → "Chrome", "Microsoft
/// Edge" → "Edge", "Mozilla Firefox" → "Firefox", and any "Google …".
const VENDOR_EXACT: &[(&str, &str)] = &[("Microsoft Edge", "Edge"), ("Mozilla Firefox", "Firefox")];
const VENDOR_PREFIXES: &[&str] = &["Google "];

/// Split a binding's display name into the pill's two lines: the app's
/// SHORT name and, when the name carries one, the profile / account part.
///
/// * The account is whatever follows the first em dash (the one
///   `browser_profiles::hud_label` writes between a browser and its profile),
///   trimmed; an e-mail address is shown as its local part ("arpon" for
///   "arpon@gmail.com"). Empty → `None`.
/// * The app name loses its vendor prefix (`VENDOR_EXACT`, then
///   `VENDOR_PREFIXES`), case-insensitively, unless that would leave nothing.
///
/// Pure; `name_split_tests` pin every rule.
pub fn split_display_name(name: &str) -> (String, Option<String>) {
    let name = name.trim();
    let (app, account) = match name.split_once('—') {
        Some((a, b)) => (a.trim(), Some(b.trim())),
        None => (name, None),
    };
    let account = account
        .map(|acc| acc.split_once('@').map_or(acc, |(local, _)| local).trim().to_string())
        .filter(|acc| !acc.is_empty());
    (short_app_name(app), account)
}

/// The vendor-prefix rule of `split_display_name`, on its own.
pub fn short_app_name(app: &str) -> String {
    let app = app.trim();
    for (full, short) in VENDOR_EXACT {
        if app.eq_ignore_ascii_case(full) {
            return (*short).to_string();
        }
    }
    for prefix in VENDOR_PREFIXES {
        if app.len() > prefix.len() && app[..prefix.len()].eq_ignore_ascii_case(prefix) {
            let rest = app[prefix.len()..].trim();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    app.to_string()
}

// ---------------------------------------------------------------------------
// Hit test
// ---------------------------------------------------------------------------

/// One item as the hit test sees it: which band, which direction, how far.
/// `radius` is in the CALLER's unit (physical px at ring time).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingHit {
    pub ring: u8,
    pub angle_deg: f64,
    pub radius: f64,
    /// The tile's edge, in the SAME unit as `radius` (physical px at ring
    /// time — `hits_for` scales it). The proximity override needs to know
    /// how big the thing under the cursor actually is; an angle-only test
    /// never did.
    pub tile: f64,
}

/// Compass-degree distance, folded into `[0, 180]`.
pub fn compass_dist(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    if d > 180.0 {
        360.0 - d
    } else {
        d
    }
}

/// Angular hysteresis, in degrees, for a tile that is already armed: it keeps
/// its sector until the cursor is this far PAST the midpoint toward a
/// neighbour. Same idea, same magnitude, as `pointer::HYSTERESIS_RAD` (3°).
pub const HYSTERESIS_DEG: f64 = 3.0;

/// Owner addition #2's hit test: **sector by angle, within the band picked by
/// distance from the centre, with a dead zone in the middle.**
///
/// * `dx, dy` — the cursor relative to the ring centre, screen axes (y down),
///   in the same unit as every `items[i].radius`.
/// * Inside `dead_r` nothing is picked — the cursor has no direction there.
/// * The BAND is the ring whose radius is nearest the cursor's distance, so
///   the boundary between two bands is the midpoint between their radii and
///   everything beyond the outermost ring belongs to it. Reaching further
///   out along one bearing therefore never changes the pick once the
///   outermost band is reached, which is the property the Space ring's
///   `sector_pick` also keeps.
/// * Within the band the nearest tile BY ANGLE wins; a tile more than 90°
///   from the cursor's bearing is never a candidate (a two-item band pointed
///   away from both picks nothing, which releases as "close, no action").
/// * `armed` keeps its sector through `HYSTERESIS_DEG` of overshoot, but only
///   inside its own band — crossing a band boundary is a deliberate move.
/// * SINCE 2026-09-15 a PROXIMITY OVERRIDE runs first (`proximity_pick`):
///   when the cursor is unambiguously on top of one tile's drawn box that
///   tile wins outright, whatever the band and the bearing say. It is an
///   addition, not a replacement — it declines whenever the cursor is far
///   from every tile or between two of them, and the angle test below then
///   answers exactly as it always did.
///
/// It follows whatever slots the layout returns — per-ring bearings — so a
/// fan's arcs and a full circle are the same to it.
pub fn ring_pick(
    items: &[RingHit],
    dead_r: f64,
    dx: f64,
    dy: f64,
    armed: Option<usize>,
) -> Option<usize> {
    if items.is_empty() {
        return None;
    }
    let dist = (dx * dx + dy * dy).sqrt();
    if dist < dead_r {
        return None;
    }
    // THE PROXIMITY OVERRIDE (owner, 2026-09-15) — "whichever icon the
    // cursor is over" beats "whichever direction I am pointing", but ONLY
    // when the cursor is genuinely over one specific tile. Angle-and-band
    // picking is what lets a fast flick from a distance work at all and is
    // NOT replaced; this runs first and falls through unless it is sure.
    if let Some(i) = proximity_pick(items, dx, dy, armed) {
        return Some(i);
    }
    // Compass bearing of the cursor: atan2(x, -y) puts north at 0 and turns
    // clockwise, matching `RingSlot::angle_deg`.
    let bearing = dx.atan2(-dy).to_degrees().rem_euclid(360.0);

    // The band: nearest radius to the cursor's distance.
    let band = items
        .iter()
        .map(|i| i.ring)
        .min_by(|a, b| {
            let ra = band_radius(items, *a);
            let rb = band_radius(items, *b);
            (ra - dist).abs().partial_cmp(&(rb - dist).abs()).unwrap_or(std::cmp::Ordering::Equal)
        })?;

    let mut best: Option<(usize, f64)> = None;
    for (i, it) in items.iter().enumerate() {
        if it.ring != band {
            continue;
        }
        let d = compass_dist(bearing, it.angle_deg);
        if d >= 90.0 {
            continue;
        }
        if best.map_or(true, |(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    let (nearest, d_nearest) = best?;
    if let Some(a) = armed {
        if a != nearest && a < items.len() && items[a].ring == band {
            let d_armed = compass_dist(bearing, items[a].angle_deg);
            if d_armed < 90.0 && d_armed - d_nearest < 2.0 * HYSTERESIS_DEG {
                return Some(a);
            }
        }
    }
    Some(nearest)
}

/// How far past a tile's own edge the cursor still counts as "over" it, as
/// a share of the tile: half the tile plus 8 px of slack at the 44 px
/// design tile. A share rather than a length so the same number works in
/// logical px (tests) and physical px (the poller), at any scale.
pub const PROXIMITY_REACH: f64 = 0.5 + 8.0 / TILE;
/// How much closer the winner must be than the runner-up before proximity
/// is allowed to overrule the angle, as a share of the tile. Below this the
/// cursor is between two tiles and the aim is the better judge.
pub const PROXIMITY_DECISIVE: f64 = 0.25;
/// A tile that is ALREADY armed keeps the pick while it is this close to
/// the winner (same share-of-a-tile unit) — the proximity twin of
/// `HYSTERESIS_DEG`, so a cursor resting between two tiles cannot flicker.
pub const PROXIMITY_HYSTERESIS: f64 = 0.35;

/// The proximity override of `ring_pick`: `Some(i)` when the cursor at
/// `(dx, dy)` (relative to the ring centre, the items' unit) is genuinely
/// over ONE tile, `None` when it is over none or cannot tell two apart.
///
/// Each tile's real drawn position is re-derived from its own ring/angle —
/// the same `(r·sin a, −r·cos a)` the page uses — and compared by plain
/// Euclidean distance, because that is the question being asked: which icon
/// is the pointer on top of? The band-by-distance / sector-by-angle test is
/// deliberately not consulted here; it is what this override exists to
/// correct when the two disagree.
pub fn proximity_pick(items: &[RingHit], dx: f64, dy: f64, armed: Option<usize>) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    let mut second = f64::INFINITY;
    for (i, it) in items.iter().enumerate() {
        if it.tile <= 0.0 {
            continue;
        }
        let a = it.angle_deg.to_radians();
        let (tx, ty) = (it.radius * a.sin(), -it.radius * a.cos());
        // Normalised by the tile so tiles of different sizes compare fairly.
        let d = ((dx - tx).powi(2) + (dy - ty).powi(2)).sqrt() / it.tile;
        match best {
            Some((_, bd)) if d >= bd => {
                if d < second {
                    second = d;
                }
            }
            _ => {
                if let Some((_, bd)) = best {
                    second = second.min(bd);
                }
                best = Some((i, d));
            }
        }
    }
    let (winner, d_win) = best?;
    if d_win > PROXIMITY_REACH {
        return None;
    }
    // Ambiguous: the runner-up is just as close. Let the angle decide —
    // unless one of the two is already armed, in which case the existing
    // hysteresis rule applies and the armed tile holds.
    if second - d_win < PROXIMITY_DECISIVE {
        if let Some(a) = armed {
            if a < items.len() && items[a].tile > 0.0 {
                let ang = items[a].angle_deg.to_radians();
                let (ax, ay) = (items[a].radius * ang.sin(), -items[a].radius * ang.cos());
                let d_armed = ((dx - ax).powi(2) + (dy - ay).powi(2)).sqrt() / items[a].tile;
                if d_armed <= PROXIMITY_REACH && d_armed - d_win < PROXIMITY_HYSTERESIS {
                    return Some(a);
                }
            }
        }
        return None;
    }
    // Unambiguous — but an armed tile that the cursor is still genuinely
    // over, and no further away than the hysteresis allows, keeps it.
    if let Some(a) = armed {
        if a != winner && a < items.len() && items[a].tile > 0.0 {
            let ang = items[a].angle_deg.to_radians();
            let (ax, ay) = (items[a].radius * ang.sin(), -items[a].radius * ang.cos());
            let d_armed = ((dx - ax).powi(2) + (dy - ay).powi(2)).sqrt() / items[a].tile;
            if d_armed <= PROXIMITY_REACH && d_armed - d_win < PROXIMITY_HYSTERESIS {
                return Some(a);
            }
        }
    }
    Some(winner)
}

/// THE SPIRAL'S HIT TEST: the nearest tile by plain Euclidean distance, with
/// the same dead zone and the same hysteresis discipline as `ring_pick`.
///
/// There is no band to choose and no sector to own — a phyllotaxis spiral
/// has neither — so "which icon is the pointer nearest?" is not merely the
/// natural test here, it is the only one that means anything. A flick from a
/// distance still works: nothing is capped, so the furthest cursor still
/// picks the tile nearest the line it is on.
pub fn spiral_pick(
    items: &[RingHit],
    dead_r: f64,
    dx: f64,
    dy: f64,
    armed: Option<usize>,
) -> Option<usize> {
    if items.is_empty() {
        return None;
    }
    if (dx * dx + dy * dy).sqrt() < dead_r {
        return None;
    }
    let d = |it: &RingHit| -> f64 {
        let a = it.angle_deg.to_radians();
        ((dx - it.radius * a.sin()).powi(2) + (dy + it.radius * a.cos()).powi(2)).sqrt()
    };
    let (winner, d_win) = items
        .iter()
        .enumerate()
        .map(|(i, it)| (i, d(it)))
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))?;
    // An armed tile holds the pick through the same slack the ring's own
    // proximity hysteresis allows, so a cursor between two tiles is steady.
    if let Some(a) = armed {
        if a != winner && a < items.len() && items[a].tile > 0.0 {
            let d_armed = d(&items[a]);
            if (d_armed - d_win) / items[a].tile < PROXIMITY_HYSTERESIS {
                return Some(a);
            }
        }
    }
    Some(winner)
}

fn band_radius(items: &[RingHit], ring: u8) -> f64 {
    items
        .iter()
        .find(|i| i.ring == ring)
        .map_or(f64::INFINITY, |i| i.radius)
}

/// The dead zone for a layout: just inside the innermost tiles' inner edge,
/// never smaller than the centre pill's own radius — a cursor resting on the
/// pill points at nothing. Same unit as `slots` (logical); the caller scales
/// it.
pub fn dead_zone_for(slots: &[RingSlot]) -> f64 {
    let inner_edge = slots
        .iter()
        .map(|s| s.radius - s.tile / 2.0)
        .fold(f64::INFINITY, f64::min);
    if !inner_edge.is_finite() {
        return PILL_D / 2.0;
    }
    (inner_edge - 4.0).max(PILL_D / 2.0 * tile_scale(slots))
}

// ---------------------------------------------------------------------------
// Which path a middle press takes — the owner's `middle_ring_style`
// ---------------------------------------------------------------------------

/// The two things a middle-button hold can raise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiddleRoute {
    /// Phase 1, byte-for-byte: normalise `MiddleButtonDown` to `SpaceDown` and
    /// raise the centred Guide HUD.
    GuideHud,
    /// Phase 2: this ring, at the cursor.
    IconRing,
}

/// `engine::dispatch` asks this ONE question about a `MiddleButtonDown`, so a
/// test can pin the answer for both values without a mouse.
pub fn middle_down_route(style: crate::config::MiddleRingStyle) -> MiddleRoute {
    match style {
        crate::config::MiddleRingStyle::GuideHud => MiddleRoute::GuideHud,
        crate::config::MiddleRingStyle::IconRing => MiddleRoute::IconRing,
    }
}

// ---------------------------------------------------------------------------
// Specials on the ring — "All" carries the actionable specials too
// ---------------------------------------------------------------------------

/// The specials the ring can carry as tiles — PHASE A (2026-09-18): DERIVED
/// from the active profile's non-letter bindings by
/// `engine::specials::ring_specials_for`, no longer a static list. The
/// scroll specials are left out (a tile release is one press; those need
/// two) and so are the Space ring's two gesture rows.
///
/// `code` is the char `pointer::take_armed_key` hands back for the tile —
/// Private Use Area (`'\u{E000}' + the key's index in `hook::keys::KEY_TABLE`),
/// so it can never collide with a bound letter, and stable because that
/// table is append-only; the seeded specials keep the U+E000–U+E009 codes the
/// ring has always used. `engine::dispatch` turns it back into the same
/// `KeyCombo::Vk` the keyboard produces, through `special_combo_for`. The
/// cascade is not forked: the release reaches `run_combo` exactly as a
/// Space+Esc does.
pub fn ring_specials_for(cfg: &AppConfig) -> Vec<(String, String, char)> {
    crate::engine::specials::ring_specials_for(cfg)
}

/// Is this the code of a ring special (as opposed to a bound letter)? Pure
/// and config-free: any code inside the key table's PUA range.
pub fn is_special_code(c: char) -> bool {
    crate::engine::specials::key_id_for_code(c).is_some()
}

/// The `KeyCombo` a ring special fires — the SAME `Vk` the keyboard hook
/// sends for that key, so the engine's one combo handler serves both.
pub fn special_combo_for(c: char) -> Option<crate::hook::KeyCombo> {
    let id = crate::engine::specials::key_id_for_code(c)?;
    crate::hook::vk_for_key_id(id).map(crate::hook::KeyCombo::Vk)
}

// ---------------------------------------------------------------------------
// Favourites and the item list
// ---------------------------------------------------------------------------

/// The letters bound in `profile`, sorted — the ring's raw material.
pub fn bound_letters(cfg: &AppConfig, profile: &str) -> Vec<char> {
    let Some(p) = cfg.profiles.iter().find(|p| p.name == profile) else {
        return Vec::new();
    };
    let mut keys: Vec<char> = p
        .bindings
        .iter()
        .filter(|(_, b)| b.is_mapped())
        .filter_map(|(k, _)| k.chars().next())
        .map(|c| c.to_ascii_lowercase())
        .filter(|c| c.is_ascii_lowercase())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// The favourites for `profile` (round 3: any number, 1..=`FAVOURITES_MAX`):
/// the user's stored list (`middle_ring_favourites`) filtered to letters that
/// are still bound, or — when that leaves nothing — the first
/// `FAVOURITES_DEFAULT` bound letters (one full inner ring). "Computed lazily
/// when empty": nothing is written back; an empty setting simply means "the
/// first few" until the user chooses.
pub fn favourites_for(cfg: &AppConfig, profile: &str) -> Vec<char> {
    let bound = bound_letters(cfg, profile);
    let chosen: Vec<char> = cfg
        .middle_ring_favourites
        .iter()
        .filter_map(|s| s.chars().next())
        .map(|c| c.to_ascii_lowercase())
        .filter(|c| bound.contains(c))
        .take(FAVOURITES_MAX)
        .collect();
    if !chosen.is_empty() {
        let mut dedup = Vec::with_capacity(chosen.len());
        for c in chosen {
            if !dedup.contains(&c) {
                dedup.push(c);
            }
        }
        return dedup;
    }
    bound.into_iter().take(FAVOURITES_DEFAULT).collect()
}

/// What kind of thing a tile launches — the page picks the fallback disc and
/// the picker's "· link" suffix from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    App,
    Folder,
    Link,
    Special,
}

/// One tile, as the page receives it. `icon` is a complete `data:` URL or
/// `None` (the page draws the letter disc); the payload carries the bytes so
/// the page needs NO IPC while the ring is up.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RingItem {
    /// Badge text: the bound letter upper-cased, or the special's key.
    pub key: String,
    /// The char `take_armed_key` returns for this tile, as a string for JSON.
    pub code: String,
    /// The pill's first line: the app's SHORT display name
    /// (`split_display_name` — vendor prefix stripped at display time).
    pub name: String,
    /// The pill's second line: the profile / account part of the binding's
    /// display name, if it has one ("Arpon", or "arpon" for arpon@gmail.com).
    pub account: Option<String>,
    pub icon: Option<String>,
    pub kind: ItemKind,
    pub ring: u8,
    pub angle_deg: f64,
    pub radius: f64,
    pub tile: f64,
    /// `RingSlot::pitch_deg` — the wave's slot width for this tile.
    pub pitch_deg: f64,
}

/// The whole event payload.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MiddleRingPayload {
    pub items: Vec<RingItem>,
    /// `"my_eight"` (Favourites — the serialised name is unchanged) | `"all"`.
    pub scope: String,
    /// Ring centre in the overlay window's CSS px.
    /// The monitor scale factor every css number here was divided by (round
    /// 6). The page compares it with its own `devicePixelRatio` and rescales:
    /// after a move across a DPI boundary WebView2's ratio can lag the
    /// monitor's — measured 1.12x on the owner's 1.0 monitor beside his 1.5
    /// panel — and a ring drawn 12 % too large reaches past the room.
    pub scale: f64,
    pub cx: f64,
    pub cy: f64,
    pub scrim: f64,
    /// The guides' diameters (`guide_diameters`) — kept for the preview
    /// harness and any reader of the old wire shape; the page draws from
    /// `guide_arcs` when it is present.
    pub guides: Vec<f64>,
    /// ROUND 6: the guides as ARCS — the same diameters, each stroked only
    /// over its ring's feasible arc (padded), so no dashed line ever runs
    /// off the screen. `0..360` = a full circle.
    pub guide_arcs: Vec<GuideArc>,
    /// ROUND 6: the layout room in the page's CSS px — the canvas less any
    /// auto-hidden appbar band. The page clips the scrim to it
    /// (`clip-path: inset(...)`), so the fade never paints into the band a
    /// sliding taskbar owns. `None` when unknown (the preview harness).
    pub room: Option<PageRect>,
    /// Fun mode on = glow, scrim, pop; off = flat (artboard 7).
    pub fun: bool,
    /// The app's own "Visual effects: reduced" — final states only.
    pub reduced: bool,
    /// How large the centre pill may grow for a long name, CSS px
    /// (`pill_max_diameter`); it rests at `PILL_D`.
    pub pill_max: f64,
    /// `RingShape::name()` — `circle`, `half-W`, `quarter-SW`,
    /// `circle-clamped` — for the page's `data-shape` (and the preview).
    pub shape: String,
}

/// A raw ring entry before layout: what to launch and how to draw it.
#[derive(Debug, Clone)]
pub struct RingEntry {
    pub key: String,
    pub code: char,
    pub name: String,
    pub icon: Option<String>,
    pub kind: ItemKind,
}

/// Classify a binding's target and name the shell target the icon comes from.
/// Returns `(kind, target)`; `target` is what the extractor is asked about.
fn classify(bind: &crate::config::KeyBinding) -> (ItemKind, Option<String>) {
    // PHASE A — a letter bound to an ACTION (a special moved onto a letter,
    // a URI, a chord, a command) is a special tile: no target to extract an
    // icon from, the letter disc carries the action's glyph.
    if bind.action.is_some() {
        return (ItemKind::Special, None);
    }
    if let Some(url) = bind.web_url.as_deref() {
        return (ItemKind::Link, Some(url.to_string()));
    }
    let Some(app) = bind.app.as_deref() else {
        return (ItemKind::App, None);
    };
    let p = std::path::Path::new(app);
    let is_dir = p.is_absolute() && p.is_dir();
    if is_dir {
        return (ItemKind::Folder, Some(app.to_string()));
    }
    (ItemKind::App, Some(app.to_string()))
}

/// Build the ring's entries for `profile` under `scope`, with EVERY icon
/// resolved through `extract`, a closure standing in for the shell:
/// `extract(target) -> Option<base64 PNG>`. Real callers pass a closure over
/// `icon_extractor::extract_icon` + the picker's `IconCache`; the test passes a
/// stub, which is what lets `payload_icon_tests` run on CI.
///
/// Icon precedence per binding, and each rule is a decision:
/// * a link → the binding's own `site_icon` (fetched ONCE at bind time; this
///   function never fetches — a ring that waited on the network would arrive
///   after the hand had let go);
/// * an app/folder with an `icon_override` (the picker's own PNG) → that;
/// * otherwise → `extract(target)`, and a bare exe name is resolved the same
///   way `extract_icon_cmd` resolves it, so "brave.exe" finds its icon.
///
/// `None` means the page draws a letter disc. That is the ONLY fallback and
/// it is the page's, not this function's: this side never invents a glyph.
pub fn build_entries(
    cfg: &AppConfig,
    profile: &str,
    scope: crate::config::MiddleRingScope,
    extract: &dyn Fn(&str) -> Option<String>,
) -> Vec<RingEntry> {
    use crate::config::MiddleRingScope;
    let Some(p) = cfg.profiles.iter().find(|p| p.name == profile) else {
        return Vec::new();
    };
    let favourites = favourites_for(cfg, profile);
    let mut order: Vec<char> = favourites.clone();
    if scope == MiddleRingScope::All {
        for c in bound_letters(cfg, profile) {
            if !order.contains(&c) {
                order.push(c);
            }
        }
    }
    let specials = ring_specials_for(cfg);
    let mut out = Vec::with_capacity(order.len() + specials.len());
    for c in order {
        let Some(bind) = p.bindings.get(&c.to_string()) else { continue };
        if !bind.is_mapped() {
            continue;
        }
        let (kind, target) = classify(bind);
        // PHASE A — one naming rule for every surface (`specials::binding_name`).
        let name = crate::engine::specials::binding_name(bind);
        let name = crate::browser_profiles::hud_label(&name, bind.browser_profile_name.as_deref());
        let icon = match kind {
            ItemKind::Link => bind.site_icon.clone().filter(|s| !s.is_empty()),
            _ => bind
                .icon_override
                .clone()
                .filter(|s| !s.is_empty())
                .map(|b64| as_png_data_url(&b64))
                .or_else(|| target.as_deref().and_then(extract).map(|b64| as_png_data_url(&b64))),
        };
        out.push(RingEntry {
            key: c.to_ascii_uppercase().to_string(),
            code: c,
            name,
            icon,
            kind,
        });
    }
    if scope == MiddleRingScope::All {
        for (key, name, code) in specials {
            out.push(RingEntry { key, code, name, icon: None, kind: ItemKind::Special });
        }
    }
    out
}

/// `icon_override` and the extractor both hand back bare base64 PNG; the page
/// wants a `data:` URL it can put straight into `<img src>`. A value that is
/// already a data URL (a hand-edited config) passes through untouched.
pub fn as_png_data_url(b64: &str) -> String {
    if b64.starts_with("data:") {
        b64.to_string()
    } else {
        format!("data:image/png;base64,{b64}")
    }
}

/// Lay the entries out in open space (full circles) and assemble the payload
/// with the centre at `(cx, cy)` CSS px. Pure; the tests' short path.
pub fn build_payload(
    entries: Vec<RingEntry>,
    scope: crate::config::MiddleRingScope,
    fun: bool,
    reduced: bool,
    centre_css: (f64, f64),
) -> (MiddleRingPayload, Vec<RingSlot>) {
    let (slots, arcs) = layout_arcs(entries.len(), Room::OPEN, TILE).unwrap_or_default();
    build_payload_shaped(entries, scope, fun, reduced, RingShape::Circle, slots, &arcs, centre_css, None, 1.0)
}

/// `build_payload` with the slots already chosen — `choose_shape` for
/// Favourites, the clamped circles for "All". `centre_css` is the ring centre
/// in the CANVAS page's CSS px (`page_point` of the physical centre), which
/// is where the page anchors the ring; `arcs` are the placement's (empty for
/// a spiral) and `room` the layout room on the page (`page_rect`), both for
/// the round-6 guide arcs and scrim clip.
#[allow(clippy::too_many_arguments)]
pub fn build_payload_shaped(
    entries: Vec<RingEntry>,
    scope: crate::config::MiddleRingScope,
    fun: bool,
    reduced: bool,
    shape: RingShape,
    slots: Vec<RingSlot>,
    arcs: &[ArcInfo],
    centre_css: (f64, f64),
    room: Option<PageRect>,
    scale: f64,
) -> (MiddleRingPayload, Vec<RingSlot>) {
    debug_assert_eq!(slots.len(), entries.len());
    let items = entries
        .into_iter()
        .zip(slots.iter())
        .map(|(e, s)| {
            let (name, account) = split_display_name(&e.name);
            RingItem {
                key: e.key,
                code: e.code.to_string(),
                name,
                account,
                icon: e.icon,
                kind: e.kind,
                ring: s.ring,
                angle_deg: s.angle_deg,
                radius: s.radius,
                tile: s.tile,
                pitch_deg: s.pitch_deg,
            }
        })
        .collect();
    let payload = MiddleRingPayload {
        items,
        scope: match scope {
            crate::config::MiddleRingScope::MyEight => "my_eight".into(),
            crate::config::MiddleRingScope::All => "all".into(),
        },
        cx: centre_css.0,
        cy: centre_css.1,
        scale,
        scrim: scrim_diameter(&slots),
        guides: guide_diameters(&slots),
        guide_arcs: guide_arcs(&slots, arcs),
        room,
        fun,
        reduced,
        pill_max: pill_max_diameter(&slots),
        shape: shape.name(),
    };
    (payload, slots)
}

/// The physical-px hit-test table for a layout drawn at `scale` (the monitor's
/// scale factor): the same slots, radii scaled, angles untouched.
pub fn hits_for(slots: &[RingSlot], scale: f64) -> Vec<RingHit> {
    slots
        .iter()
        .map(|s| RingHit {
            ring: s.ring,
            angle_deg: s.angle_deg,
            radius: s.radius * scale,
            tile: s.tile * scale,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests — house rule: every pure decision above is pinned here.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test_support {
    use super::*;

    /// Per ring: the sorted angles.
    pub fn rings_of(slots: &[RingSlot]) -> Vec<(u8, Vec<&RingSlot>)> {
        let mut rings: Vec<u8> = slots.iter().map(|s| s.ring).collect();
        rings.sort_unstable();
        rings.dedup();
        rings
            .into_iter()
            .map(|r| {
                let mut band: Vec<&RingSlot> = slots.iter().filter(|s| s.ring == r).collect();
                band.sort_by(|a, b| a.angle_deg.partial_cmp(&b.angle_deg).unwrap());
                (r, band)
            })
            .collect()
    }

    /// The angular gaps between consecutive tiles of one ring, including the
    /// wrap-around gap, ascending.
    pub fn gaps(band: &[&RingSlot]) -> Vec<f64> {
        let k = band.len();
        let mut g: Vec<f64> = (0..k)
            .map(|i| (band[(i + 1) % k].angle_deg - band[i].angle_deg).rem_euclid(360.0))
            .collect();
        if k == 1 {
            g = vec![360.0];
        }
        g.sort_by(|a, b| a.partial_cmp(b).unwrap());
        g
    }

    /// THE LAW, asserted: (1) even spacing per ring — the k−1 smallest gaps
    /// agree within 1° (the k-th is the wrap gap, equal on a full ring,
    /// larger on an arc); (2) no overlap — the smallest arc between
    /// neighbours ≥ tile + gap; (3) every tile inside the room; (4) inner
    /// rings fill before outer ones open — a ring has tiles only when the
    /// one inside it holds its arc's capacity.
    pub fn assert_law(n: usize, room: Room, slots: &[RingSlot], what: &str) {
        assert_eq!(slots.len(), n, "{what}: one slot per item");
        assert!(slots_fit(slots, room), "{what}: a tile is outside the room: {slots:?}");
        let rings = rings_of(slots);
        for (r, band) in &rings {
            let g = gaps(band);
            if band.len() >= 3 {
                // k−1 gaps are the pitch; the odd one is the wrap gap of a
                // partial arc, larger (a notch) or smaller (a near-full arc).
                let k = band.len();
                let even = |c: &[f64]| c[c.len() - 1] - c[0] < 1.0;
                assert!(even(&g[..k - 1]) || even(&g[1..]), "{what}: ring {r} unevenly spaced: gaps {g:?}");
            }
            if band.len() >= 2 {
                let arc = band[0].radius * g[0].to_radians();
                assert!(
                    arc + 1e-6 >= band[0].tile,
                    "{what}: ring {r} tiles overlap: arc {arc} < {}",
                    band[0].tile
                );
            }
        }
        for w in rings.windows(2) {
            let (inner_r, inner) = &w[0];
            let tile = inner[0].tile;
            let arc = feasible_arc(room, inner[0].radius, tile).expect("inner ring exists");
            assert_eq!(
                inner.len(),
                arc_capacity(*inner_r, inner[0].radius, tile, arc),
                "{what}: ring {inner_r} must be full before ring {} opens",
                w[1].0
            );
        }
    }
}

#[cfg(test)]
mod layout_tests {
    use super::test_support::*;
    use super::*;

    /// Open space: 1..=6 on one ring at 125, evenly from north; 7..=15 fill
    /// the inner six and spread the rest at 201; two items sit opposite.
    #[test]
    fn open_space_fills_six_then_nine_evenly() {
        for n in 1..=FAVOURITES_MAX {
            let s = layout_ring_slots(n, TILE);
            assert_law(n, Room::OPEN, &s, &format!("open n={n}"));
            assert!(s.iter().all(|x| x.tile == TILE));
            let inner = s.iter().filter(|x| x.ring == 0).count();
            assert_eq!(inner, n.min(RING_CAPS[0]), "n={n}");
            assert!(s.iter().filter(|x| x.ring == 1).all(|x| x.radius == RING_RADII[1]));
            assert!(s.iter().all(|x| x.ring <= 1), "n={n} fits two rings");
        }
        let two = layout_ring_slots(2, TILE);
        assert_eq!((two[0].angle_deg, two[1].angle_deg), (0.0, 180.0));
        let three = layout_ring_slots(3, TILE);
        assert_eq!(three[1].angle_deg, 120.0);
        let five = layout_ring_slots(RING_CAPS[0], TILE);
        assert!(five.iter().all(|x| x.pitch_deg == 360.0 / RING_CAPS[0] as f64));
        let thirteen = layout_ring_slots(FAVOURITES_MAX, TILE);
        assert!(thirteen
            .iter()
            .filter(|x| x.ring == 1)
            .all(|x| x.pitch_deg == 360.0 / RING_CAPS[1] as f64));
        assert!(layout_ring_slots(0, TILE).is_empty());
    }

    /// "All": past fifteen a third ring opens at 277 and takes as many as
    /// fit at spacing; 33 (26 letters + 7 specials) fits on three rings with
    /// normal tiles.
    #[test]
    fn all_opens_a_third_ring_past_fifteen() {
        let third_opens = RING_CAPS[0] + RING_CAPS[1] + 1; // 14: one past 5+8
        let s = layout_ring_slots(third_opens, TILE);
        assert_eq!(s.iter().filter(|x| x.ring == 2).count(), 1);
        assert_eq!(s.iter().find(|x| x.ring == 2).unwrap().radius, RING_RADII[2]);
        for n in third_opens..=40 {
            let s = layout_ring_slots(n, TILE);
            assert_law(n, Room::OPEN, &s, &format!("all n={n}"));
        }
        assert!(layout_ring_slots(33, TILE).iter().all(|x| x.ring <= 3 && x.tile == TILE));
        assert_eq!(ring_capacity(RING_RADII[2], TILE), 25);
    }

    /// The per-ring capacities behind the law: 6 / 9 on the first two full
    /// rings (design caps, well under what spacing allows), spacing-limited
    /// after that; a partial arc holds one tile per spacing step plus one.
    #[test]
    fn arc_capacity_is_the_design_cap_then_spacing() {
        assert_eq!(arc_capacity(0, RING_RADII[0], TILE, (0.0, 360.0)), RING_CAPS[0]);
        assert_eq!(arc_capacity(1, RING_RADII[1], TILE, (0.0, 360.0)), RING_CAPS[1]);
        assert_eq!(arc_capacity(2, RING_RADII[2], TILE, (0.0, 360.0)), RING_CAPS[2]);
        assert!(ring_capacity(RING_RADII[0], TILE) > RING_CAPS[0] && ring_capacity(RING_RADII[1], TILE) > RING_CAPS[1]);
        // 90° at r = 125: 125 × π/2 = 196 px of arc → 2 steps of 76 + the first tile.
        assert_eq!(arc_capacity(0, 125.0, TILE, (180.0, 270.0)), 3);
        assert_eq!(arc_capacity(0, 125.0, TILE, (180.0, 181.0)), 1, "never less than one");
    }

    /// `feasible_arc`: open space is the full circle; a wall closer than a
    /// tile's half + margin cuts a notch; no room at all is `None`.
    #[test]
    fn feasible_arc_finds_the_open_arc() {
        assert_eq!(feasible_arc(Room::OPEN, 125.0, TILE), Some((0.0, 360.0)));
        let bottom_wall = Room { left: 1000.0, right: 1000.0, up: 1000.0, down: 40.0 };
        let (lo, hi) = feasible_arc(bottom_wall, 125.0, TILE).unwrap();
        assert!(hi - lo < 360.0 && hi - lo > 160.0, "{lo}..{hi}");
        assert!(((lo + hi) / 2.0).rem_euclid(360.0) < 1.0, "centred on north: {lo}..{hi}");
        let none = Room { left: 10.0, right: 10.0, up: 10.0, down: 10.0 };
        assert_eq!(feasible_arc(none, 125.0, TILE), None);
        assert!(Room::OPEN.holds(1e6, -1e6, TILE));
        assert!(!none.holds(0.0, 0.0, TILE));
    }

    /// The scrim, guides and pill follow the layout.
    #[test]
    fn scrim_guides_and_pill_follow_the_layout() {
        // A clean single ring: RING_CAPS[0] items, all on ring 0.
        let five = layout_ring_slots(RING_CAPS[0], TILE);
        assert_eq!(scrim_diameter(&five), SCRIM_D_ONE);
        assert_eq!(guide_diameters(&five), vec![2.0 * RING_RADII[0] + 14.0]);
        assert_eq!(pill_max_diameter(&five), 2.0 * (RING_RADII[0] - TILE / 2.0 - PILL_FREE_MARGIN));
        // A clean two rings: RING_CAPS[0] + RING_CAPS[1], filling both exactly.
        let thirteen = layout_ring_slots(RING_CAPS[0] + RING_CAPS[1], TILE);
        assert_eq!(scrim_diameter(&thirteen), SCRIM_D_MULTI);
        assert_eq!(guide_diameters(&thirteen), vec![2.0 * RING_RADII[0] + 14.0, 2.0 * RING_RADII[1] + 14.0]);
        // 33 exceeds the first three rings' combined cap (5+8+13=26) — a
        // fourth ring opens.
        let big = layout_ring_slots(33, TILE);
        assert_eq!(scrim_diameter(&big), 2.0 * ring_extent(&big) + 80.0);
        assert_eq!(guide_diameters(&big).len(), 4);
        assert_eq!(pill_max_diameter(&[]), PILL_D);
    }
}

#[cfg(test)]
mod clamp_tests {
    use super::*;

    const AREA: WorkArea = WorkArea { x: 0.0, y: 0.0, w: 2560.0, h: 1552.0 };
    const EXTENT: f64 = 240.0; // 160 logical × 1.5

    #[test]
    fn an_interior_cursor_is_the_centre() {
        assert_eq!(clamp_ring_center((1280.0, 800.0), AREA, EXTENT), (1280.0, 800.0));
        assert_eq!(clamp_ring_center((240.0, 240.0), AREA, EXTENT), (240.0, 240.0));
    }

    #[test]
    fn the_four_corners_slide_the_centre_inward_by_the_overhang() {
        assert_eq!(clamp_ring_center((10.0, 10.0), AREA, EXTENT), (240.0, 240.0));
        assert_eq!(clamp_ring_center((2550.0, 10.0), AREA, EXTENT), (2320.0, 240.0));
        assert_eq!(clamp_ring_center((10.0, 1545.0), AREA, EXTENT), (240.0, 1312.0));
        assert_eq!(clamp_ring_center((2559.0, 1551.0), AREA, EXTENT), (2320.0, 1312.0));
    }

    #[test]
    fn a_single_edge_moves_one_axis_only() {
        assert_eq!(clamp_ring_center((1280.0, 5.0), AREA, EXTENT), (1280.0, 240.0));
        assert_eq!(clamp_ring_center((3.0, 800.0), AREA, EXTENT), (240.0, 800.0));
    }

    #[test]
    fn the_work_area_origin_is_honoured() {
        let area = WorkArea { x: 100.0, y: 48.0, w: 2000.0, h: 1000.0 };
        assert_eq!(clamp_ring_center((0.0, 0.0), area, EXTENT), (340.0, 288.0));
        assert_eq!(clamp_ring_center((5000.0, 5000.0), area, EXTENT), (1860.0, 808.0));
    }

    #[test]
    fn a_too_small_area_centres_the_ring() {
        let tiny = WorkArea { x: 0.0, y: 0.0, w: 300.0, h: 300.0 };
        assert_eq!(clamp_ring_center((5.0, 5.0), tiny, EXTENT), (150.0, 150.0));
    }

    /// "All" with every count on the owner's monitor and on a second monitor
    /// at negative x: every tile of every ring inside THAT monitor's work
    /// area, from each edge and corner.
    #[test]
    fn all_of_them_stays_inside_the_cursors_monitor_from_every_corner() {
        let owner = (WorkArea { x: 0.0, y: 48.0, w: 2560.0, h: 1552.0 }, 1.5);
        let second = (WorkArea { x: -1920.0, y: 0.0, w: 1920.0, h: 1040.0 }, 1.0);
        for (area, scale) in [owner, second] {
            for n in [16usize, 25, 33, 35] {
                let slots = layout_ring_slots(n, TILE);
                let extent = clamp_extent(&slots, scale);
                let pts = [
                    (area.x, area.y),
                    (area.right() - 1.0, area.y),
                    (area.x, area.bottom() - 1.0),
                    (area.right() - 1.0, area.bottom() - 1.0),
                    (area.x + area.w / 2.0, area.y),
                    (area.x + 5.0, area.y + area.h / 2.0),
                    (area.x + area.w / 2.0, area.y + area.h / 2.0),
                ];
                for p in pts {
                    let (cx, cy) = clamp_ring_center(p, area, extent);
                    for s in &slots {
                        let (dx, dy) = s.offset();
                        let half = s.tile / 2.0 * scale;
                        let (tx, ty) = (cx + dx * scale, cy + dy * scale);
                        assert!(tx - half >= area.x - 1e-6 && tx + half <= area.right() + 1e-6, "n={n} {p:?} x");
                        assert!(ty - half >= area.y - 1e-6 && ty + half <= area.bottom() + 1e-6, "n={n} {p:?} y");
                    }
                }
            }
        }
    }
}

/// The canvas — the round-3 window — and the page-centre arithmetic, with
/// the 1.0.110 log's numbers (the owner's 2560×1600 @ 1.5, work area
/// x 0..2560, y 48..1600).
#[cfg(test)]
mod canvas_tests {
    use super::*;

    const MON: WorkArea = WorkArea { x: 0.0, y: 0.0, w: 2560.0, h: 1600.0 };
    const WORK: WorkArea = WorkArea { x: 0.0, y: 48.0, w: 2560.0, h: 1552.0 };

    #[test]
    fn the_canvas_is_the_work_area_when_a_taskbar_exists() {
        assert_eq!(canvas_rect(WORK, MON), WORK);
    }

    /// An auto-hide taskbar makes the work area the monitor: the canvas is
    /// inset 2 px a side so it is never the exact monitor rectangle.
    #[test]
    fn the_canvas_is_never_the_exact_monitor_bounds() {
        let c = canvas_rect(MON, MON);
        assert_eq!(c, WorkArea { x: 2.0, y: 2.0, w: 2556.0, h: 1596.0 });
        assert!(c.w < MON.w && c.h < MON.h);
        let second = WorkArea { x: -1920.0, y: 0.0, w: 1920.0, h: 1080.0 };
        let c2 = canvas_rect(second, second);
        assert_eq!((c2.x, c2.y, c2.w, c2.h), (-1918.0, 2.0, 1916.0, 1076.0));
    }

    /// Hold #1 of the log: button down at (883,756) physical on the canvas
    /// at (0,48) → the page draws the ring at (588.67, 472) CSS px. The
    /// poller keeps the physical centre; the two describe one point.
    #[test]
    fn the_page_centre_is_the_physical_centre_relative_to_the_canvas() {
        let c = canvas_rect(WORK, MON);
        let (cx, cy) = page_point((883.0, 756.0), c, 1.5);
        assert!((cx - 883.0 / 1.5).abs() < 1e-9 && (cy - (756.0 - 48.0) / 1.5).abs() < 1e-9);
        let second = WorkArea { x: -1920.0, y: 0.0, w: 1920.0, h: 1040.0 };
        assert_eq!(page_point((-1000.0, 500.0), second, 1.0), (920.0, 500.0));
    }

    /// The Space ring's stage: on the owner's monitor (taskbar at the top,
    /// scale 1.5) and on a second monitor at negative x with a bottom
    /// taskbar at scale 1.0, the stage's centre in physical px is the
    /// MONITOR's centre — where the old window was centred — not the
    /// canvas's.
    #[test]
    fn the_space_ring_stage_is_centred_on_the_monitor_on_any_monitor() {
        let (w, h) = (1256.0, 769.0);
        for (canvas, monitor, scale) in [
            (canvas_rect(WORK, MON), MON, 1.5),
            (
                canvas_rect(
                    WorkArea { x: -1920.0, y: 0.0, w: 1920.0, h: 1040.0 },
                    WorkArea { x: -1920.0, y: 0.0, w: 1920.0, h: 1080.0 },
                ),
                WorkArea { x: -1920.0, y: 0.0, w: 1920.0, h: 1080.0 },
                1.0,
            ),
            (
                canvas_rect(
                    WorkArea { x: 2560.0, y: 0.0, w: 3840.0, h: 2100.0 },
                    WorkArea { x: 2560.0, y: 0.0, w: 3840.0, h: 2160.0 },
                ),
                WorkArea { x: 2560.0, y: 0.0, w: 3840.0, h: 2160.0 },
                2.0,
            ),
        ] {
            let centre = (monitor.x + monitor.w / 2.0, monitor.y + monitor.h / 2.0);
            let (sx, sy) = stage_box(canvas, centre, scale, w, h);
            let back = (canvas.x + (sx + w / 2.0) * scale, canvas.y + (sy + h / 2.0) * scale);
            assert!((back.0 - centre.0).abs() < 1e-6 && (back.1 - centre.1).abs() < 1e-6, "{back:?} vs {centre:?}");
            // The stage lies inside the canvas page on every monitor.
            assert!(sx >= 0.0 && sy >= -h && sx + w <= canvas.w / scale + 1e-6, "sx {sx} sy {sy}");
        }
        // The owner's numbers: canvas y starts at 48, so the stage's top is
        // (800 − 48)/1.5 − 384.5 = 116.8 css.
        let (sx, sy) = stage_box(canvas_rect(WORK, MON), (1280.0, 800.0), 1.5, w, h);
        assert!((sx - (1280.0 / 1.5 - 628.0)).abs() < 1e-9);
        assert!((sy - ((800.0 - 48.0) / 1.5 - 384.5)).abs() < 1e-9);
    }

    /// THE REGRESSION THAT WOULD HAVE CAUGHT THE RIGHT/BOTTOM CLIP
    /// (2026-09-15). The canvas's PHYSICAL right and bottom edges must be
    /// the work area's own, EXACTLY — not "close", not "within rounding" —
    /// on a 1.5-scale and a 1.0-scale monitor, at a positive and at a
    /// NEGATIVE origin. A window anchored at the work area's top-left that
    /// is even one pixel short looks perfect on the left and top and clips
    /// everything reaching toward the far edge, which is precisely the
    /// shape of the owner's report.
    #[test]
    fn the_canvas_edges_are_the_work_areas_edges_to_the_pixel() {
        let cases = [
            // (work area, monitor bounds, scale) — a real taskbar, so no inset.
            (WORK, MON, 1.5),
            (
                WorkArea { x: -1920.0, y: 0.0, w: 1920.0, h: 1040.0 },
                WorkArea { x: -1920.0, y: 0.0, w: 1920.0, h: 1080.0 },
                1.0,
            ),
            (
                WorkArea { x: 2560.0, y: 0.0, w: 3840.0, h: 2100.0 },
                WorkArea { x: 2560.0, y: 0.0, w: 3840.0, h: 2160.0 },
                2.0,
            ),
        ];
        for (work, mon, scale) in cases {
            let c = canvas_rect(work, mon);
            assert_eq!(c.x, work.x, "left edge, scale {scale}");
            assert_eq!(c.y, work.y, "top edge, scale {scale}");
            assert_eq!(c.right(), work.right(), "RIGHT edge, scale {scale}");
            assert_eq!(c.bottom(), work.bottom(), "BOTTOM edge, scale {scale}");
            // …and the page's own width/height in CSS px is the canvas's,
            // so a point at the canvas's far edge is at the page's far edge.
            let (px, py) = page_point((c.right(), c.bottom()), c, scale);
            assert_eq!((px, py), (c.w / scale, c.h / scale), "far corner, scale {scale}");
        }
    }

    /// The inset case (an auto-hidden taskbar makes the work area equal the
    /// bounds) is SYMMETRIC — the same 2 px on every side — and the layout
    /// must be measured against the CANVAS, never the work area: the origin
    /// moves +2, so a room taken from the work area is 2 px outside the
    /// window on the right and the bottom and 2 px further inside on the
    /// left and the top. Right/bottom-only, exactly as reported.
    #[test]
    fn the_layout_room_is_the_canvas_not_the_work_area() {
        let c = canvas_rect(MON, MON);
        assert_eq!(c.x - MON.x, MON.right() - c.right(), "inset is symmetric in x");
        assert_eq!(c.y - MON.y, MON.bottom() - c.bottom(), "inset is symmetric in y");
        // A ring snapped to the WORK AREA's right/bottom edge lands outside
        // the page; snapped to the CANVAS's, it lands on the page's own edge.
        let scale = 1.5;
        let page_w = c.w / scale;
        let (bad, _) = page_point((MON.right(), MON.bottom()), c, scale);
        assert!(bad > page_w, "work-area edge is {bad} css, past the page's {page_w}");
        let (good, _) = page_point((c.right(), c.bottom()), c, scale);
        assert_eq!(good, page_w);
        // `room_rect` with no appbar IS the canvas, so the layout can never
        // place a tile the window cannot show.
        assert_eq!(room_rect(c, (0.0, 0.0, 0.0, 0.0)), c);
    }

    /// The auto-hidden taskbar band. Measured on the owner's machine
    /// 2026-09-15: `Shell_TrayWnd` at (0,1598)-(2560,1670) — 72 px tall,
    /// docked bottom, mostly slid off-screen — while `rcWork` reserved
    /// NOTHING at the bottom. `appbar_reserve` resolves the bar to its
    /// docked edge and claims its FULL height, whether it is slid out or not.
    #[test]
    fn an_autohidden_taskbar_reserves_its_whole_band_on_its_own_edge() {
        let bar = WorkArea { x: 0.0, y: 1598.0, w: 2560.0, h: 72.0 };
        assert_eq!(appbar_reserve(MON, bar), (0.0, 0.0, 0.0, 2.0));
        // Slid OUT (the state that actually covers the ring): the whole 72.
        let out = WorkArea { x: 0.0, y: 1528.0, w: 2560.0, h: 72.0 };
        assert_eq!(appbar_reserve(MON, out), (0.0, 0.0, 0.0, 72.0));
        // Docked left / right / top, and a bar on another monitor entirely.
        assert_eq!(
            appbar_reserve(MON, WorkArea { x: 0.0, y: 0.0, w: 60.0, h: 1600.0 }),
            (60.0, 0.0, 0.0, 0.0)
        );
        assert_eq!(
            appbar_reserve(MON, WorkArea { x: 2500.0, y: 0.0, w: 60.0, h: 1600.0 }),
            (0.0, 0.0, 60.0, 0.0)
        );
        assert_eq!(
            appbar_reserve(MON, WorkArea { x: 0.0, y: 0.0, w: 2560.0, h: 48.0 }),
            (0.0, 48.0, 0.0, 0.0)
        );
        assert_eq!(
            appbar_reserve(MON, WorkArea { x: -1920.0, y: 1000.0, w: 1920.0, h: 80.0 }),
            (0.0, 0.0, 0.0, 0.0),
            "a bar on a different monitor takes nothing from this one"
        );
        // And the room shrinks on that edge only — never below 1 px, and a
        // reserve that would swallow the screen is refused outright.
        let room = room_rect(MON, (0.0, 0.0, 0.0, 72.0));
        assert_eq!((room.x, room.y, room.w, room.h), (0.0, 0.0, 2560.0, 1528.0));
        assert_eq!(room_rect(MON, (0.0, 0.0, 0.0, 9999.0)), MON);
    }

    /// With the auto-hide band reserved, NOTHING the layout places can land
    /// in it — including the snapped centre the cursor is warped to, which
    /// is the gesture that makes the taskbar slide out in the first place.
    #[test]
    fn a_bottom_press_never_snaps_the_centre_into_the_taskbars_band() {
        let room = room_rect(canvas_rect(MON, MON), (0.0, 0.0, 0.0, 72.0));
        let scale = 1.5;
        // The owner's hold #21: pressed at y = 1407 physical, 13 items.
        let cursor = (1482.0, 1407.0);
        let p = choose_shape(13, Room::at(cursor, room, scale));
        let centre = (cursor.0 + p.offset.0 * scale, cursor.1 + p.offset.1 * scale);
        assert!(
            centre.1 <= room.bottom() + 1e-6,
            "the centre snapped to {} — inside the taskbar band below {}",
            centre.1,
            room.bottom()
        );
        for s in &p.slots {
            let (dx, dy) = s.offset();
            let (x, y) = (centre.0 + dx * scale, centre.1 + dy * scale);
            let h = (s.tile / 2.0 + CLAMP_MARGIN) * scale;
            assert!(
                x - h >= room.x - 1e-6
                    && x + h <= room.right() + 1e-6
                    && y - h >= room.y - 1e-6
                    && y + h <= room.bottom() + 1e-6,
                "tile at ({x},{y}) leaves the room {room:?}"
            );
        }
    }
}

/// The shape at the press point — the owner's monitor (2560×1600 @ 1.5,
/// work area y 48..1600) and the log's press points, plus the round-2
/// hardware case that was BUNCHED: (2485,175).
#[cfg(test)]
mod shape_tests {
    use super::test_support::*;
    use super::*;

    const SCALE: f64 = 1.5;
    const AREA: WorkArea = WorkArea { x: 0.0, y: 48.0, w: 2560.0, h: 1552.0 };

    fn room_at(cursor: (f64, f64)) -> Room {
        Room::at(cursor, AREA, SCALE)
    }

    fn assert_anchored(n: usize, cursor: (f64, f64)) -> (RingShape, Vec<RingSlot>) {
        let room = room_at(cursor);
        let Placement { shape, offset, slots, .. } = choose_shape(n, room);
        assert!(shape.anchored(), "{cursor:?} n={n} → {}", shape.name());
        // ROUND 6: on a real monitor the centre NEVER moves — only a
        // `Nudged` shape (a sliver of a room) carries an offset, and none
        // of these press points is one.
        assert_eq!(offset, (0.0, 0.0), "{cursor:?} n={n} {}: the centre moved", shape.name());
        let seen = room.shifted(offset.0, offset.1);
        assert_law(n, seen, &slots, &format!("{cursor:?} n={n} {}", shape.name()));
        // Reachable: every tile picks itself from its own centre.
        let hits = hits_for(&slots, 1.0);
        let dead = dead_zone_for(&slots);
        for (i, s) in slots.iter().enumerate() {
            let (dx, dy) = s.offset();
            assert_eq!(ring_pick(&hits, dead, dx, dy, None), Some(i), "{} tile {i}", shape.name());
        }
        (shape, slots)
    }

    /// Hold #1 of the log, (883,756): open space → the circle, six at 60°.
    #[test]
    fn open_space_is_the_circle() {
        let (shape, slots) = assert_anchored(6, (883.0, 756.0));
        assert_eq!(shape, RingShape::Circle);
        assert_eq!(slots, layout_ring_slots(6, TILE));
        let (shape, _) = assert_anchored(15, (883.0, 756.0));
        assert_eq!(shape, RingShape::Circle);
    }

    /// THE ROUND-2 HARDWARE CASE: (2485,175) physical — 50 logical from the
    /// right edge, 85 below the work-area top. Round 2 packed two arcs of
    /// four at a fixed 20° pitch over ~60° with 56 px tiles. Now: normal
    /// 70 px tiles, the inner arc filled with as many as fit evenly over its
    /// whole available arc (four over ~105°), the overflow evenly over the
    /// outer arc, every tile inside, the centre on the press point (the
    /// pill may overhang the edge — allowed).
    #[test]
    fn the_bunched_corner_case_spreads_over_the_whole_arc() {
        // ROUND 6: the centre stays ON the press point (50 logical from the
        // right wall, 85 below the top), so the inner ring's arc is the
        // ~122° between the two walls at r = 108 — `dx ≤ 12` cuts the east,
        // `dy ≥ −47` cuts the north — and holds 4 (floor(108·2.13/71)+1).
        // Round 4/5 snapped the centre onto the corner point instead and
        // got a 49° arc of 2; this pins the restored numbers.
        let cursor = (2485.0, 175.0);
        let (shape, slots) = assert_anchored(8, cursor);
        assert_eq!(shape, RingShape::Quarter(Dir::SW), "{}", shape.name());
        assert!(slots.iter().all(|s| s.tile == TILE), "normal tiles: {slots:?}");
        let by_ring = |k: u8| slots.iter().filter(|s| s.ring == k).count();
        assert_eq!(by_ring(0), 4, "the inner arc holds as many as fit evenly at spacing");
        assert_eq!(by_ring(1), 4);
        assert_eq!(slots.iter().find(|s| s.ring == 0).unwrap().radius, RING_RADII[0]);
        assert_eq!(slots.iter().find(|s| s.ring == 1).unwrap().radius, RING_RADII[1]);
        // Every tile stays inside the 50 px to the right (centre ≤ +12) and
        // the 85 px above (centre ≥ −47).
        for s in &slots {
            let (dx, dy) = s.offset();
            assert!(dx <= 12.0 + 1e-6, "tile at {}° is too far right", s.angle_deg);
            assert!(dy >= -47.0 - 1e-6, "tile at {}° is too high", s.angle_deg);
        }
        // The pill overhangs: 50 px of room, 57 px of pill — and the centre
        // did not move, by law.
        assert!(room_at(cursor).right < PILL_D / 2.0);
    }

    /// Hold #4 of the log, (2461,299): the top-right corner. The room above
    /// is 167 logical, so the available arc is wide (over 150°) — a "half"
    /// facing west, not a bunched fan.
    #[test]
    fn the_top_right_corner_uses_every_degree_it_has() {
        // ROUND 6: 66 logical to the right, 167 above. The INNER ring
        // (r = 108, reach 108 + 38 = 146 < 167) is cut by the right wall
        // only, so the first partial arc opens WEST — a half, named by the
        // arc it is, not by which axes a full circle would have failed.
        let (shape, slots) = assert_anchored(8, (2461.0, 299.0));
        assert!(slots.iter().all(|s| s.tile == TILE));
        assert_eq!(shape, RingShape::Half(Dir::W), "{}", shape.name());
        assert!(slots.iter().all(|s| s.offset().0 <= 66.0 - 38.0 + 1e-6), "nothing past the 66 px of room to the right");
    }

    /// The bottom edge with room either side, (1280,1465): an arc facing
    /// NORTH — nothing below the cursor, tiles evenly over the arc.
    #[test]
    fn near_the_bottom_edge_is_a_half_ring_facing_north() {
        let (shape, slots) = assert_anchored(8, (1280.0, 1465.0));
        assert_eq!(shape, RingShape::Half(Dir::N), "{}", shape.name());
        // 90 logical of room below (ROUND 6: the centre IS the press point):
        // a tile may sit up to 90 − 22 − 16 = 52 px below the cursor line —
        // its halo still inside — and none lower. Computed from the law,
        // not typed, so the margin and the tile stay the single source.
        let lowest = room_at((1280.0, 1465.0)).down - TILE / 2.0 - CLAMP_MARGIN;
        assert!(slots.iter().all(|s| s.offset().1 <= lowest + 1e-6), "a tile is too low");
        assert!(slots.iter().any(|s| s.offset().1 > 0.0), "the arc uses the room below the cursor too");
        let arc = feasible_arc(room_at((1280.0, 1465.0)), RING_RADII[0], TILE).unwrap();
        assert!(arc.1 - arc.0 > 180.0, "the whole available arc is used: {arc:?}");
    }

    /// The right edge, mid-height, 13 logical from the edge: an arc facing
    /// WEST with nothing to the right — the pill overhangs and the centre
    /// stays put.
    #[test]
    fn the_right_edge_faces_west_and_never_moves() {
        let (shape, slots) = assert_anchored(8, (2540.0, 800.0));
        assert_eq!(shape, RingShape::Half(Dir::W), "{}", shape.name());
        assert!(slots.iter().all(|s| s.offset().0 <= 1e-9), "nothing to the right");
        assert!(slots.iter().all(|s| s.tile == TILE));
    }

    /// The left edge and the top edge, for the other two facings.
    #[test]
    fn the_left_and_top_edges_face_east_and_south() {
        let (shape, _) = assert_anchored(8, (12.0, 800.0));
        assert_eq!(shape, RingShape::Half(Dir::E));
        let (shape, _) = assert_anchored(8, (1280.0, 60.0));
        assert_eq!(shape, RingShape::Half(Dir::S));
    }

    /// The very corner pixel still gets an anchored fan, tiles inside.
    #[test]
    fn the_corner_pixel_itself_still_fits() {
        for &c in &[(0.0, 48.0), (2559.0, 48.0), (0.0, 1599.0), (2559.0, 1599.0)] {
            let (shape, _) = assert_anchored(8, c);
            assert!(matches!(shape, RingShape::Quarter(_)), "{:?} → {}", c, shape.name());
            let (_, _) = assert_anchored(15, c);
        }
    }

    /// A work area too small for any fan falls back to the clamped circle —
    /// the only case in which the centre may move.
    #[test]
    fn a_tiny_work_area_falls_back_to_the_clamped_circle() {
        let tiny = WorkArea { x: 0.0, y: 0.0, w: 120.0, h: 120.0 };
        let Placement { shape, slots, .. } = choose_shape(8, Room::at((5.0, 5.0), tiny, 1.0));
        assert_eq!(shape, RingShape::CircleClamped);
        assert!(!shape.anchored());
        assert_eq!(slots, layout_ring_slots(8, TILE));
    }

    /// Tiles shrink ONLY when the room forces it (round 6, the round-3
    /// law's "44 px tiles unless the room forces smaller"). A 2000×150
    /// sliver pressed in its middle: 75 px above and below, so a tile
    /// centre may sit only 37 px off the centre line — each ring gets a
    /// ~40°/24°/15°/9° arc east and west and holds 2, i.e. 8 at FULL size.
    /// Eight therefore stays at 44 px; twelve cannot fit at 44 and the
    /// ladder descends until it does — never further than it must (the
    /// next rung up is proven not to fit), never past `TILE_MIN`, and the
    /// centre never moves.
    #[test]
    fn tiles_shrink_only_when_the_room_forces_it() {
        let sliver = WorkArea { x: 0.0, y: 0.0, w: 2000.0, h: 150.0 };
        let room = Room::at((1000.0, 75.0), sliver, 1.0);
        // Not forced: full size.
        let eight = choose_shape(8, room);
        assert!(eight.shape.anchored() && eight.offset == (0.0, 0.0), "{}", eight.shape.name());
        assert!(eight.slots.iter().all(|s| s.tile == TILE), "eight fit at full size: {:?}", eight.slots);
        assert!(layout_arcs(9, room, TILE).is_none(), "nine do NOT fit at full size in this sliver");
        // Forced: the first rung that fits, and no lower.
        let twelve = choose_shape(12, room);
        assert!(twelve.shape.anchored(), "{}", twelve.shape.name());
        assert_eq!(twelve.offset, (0.0, 0.0), "the centre never moves to shrink");
        assert!(!matches!(twelve.shape, RingShape::Nudged(_)));
        let tile = twelve.slots[0].tile;
        assert!(twelve.slots.iter().all(|s| s.tile == tile), "one tile size per layout");
        assert!(tile < TILE && tile >= TILE_MIN - 1e-9, "tile {tile}");
        assert!(slots_fit(&twelve.slots, room));
        assert_law(12, room, &twelve.slots, "sliver n=12");
        let ladder = tile_ladder();
        let k = ladder.iter().position(|t| (t - tile).abs() < 1e-9).expect("a ladder rung");
        assert!(k > 0);
        assert!(layout_arcs(12, room, ladder[k - 1]).is_none(), "the rung above ({}) must not fit", ladder[k - 1]);
        // The radii scaled with the tile.
        for s in &twelve.slots {
            assert!((s.radius - ring_radius(s.ring, tile)).abs() < 1e-9);
        }
        println!("sliver 2000x150, n=12 → {} tile {tile:.1} rings {:?}", twelve.shape.name(),
            twelve.arcs.iter().map(|a| (a.ring, a.count, a.radius.round())).collect::<Vec<_>>());
    }

    /// The shape name is what the marker line prints.
    #[test]
    fn shape_names_are_greppable() {
        assert_eq!(RingShape::Circle.name(), "circle");
        assert_eq!(RingShape::Half(Dir::W).name(), "half-W");
        assert_eq!(RingShape::Quarter(Dir::SW).name(), "quarter-SW");
        assert_eq!(RingShape::CircleClamped.name(), "circle-clamped");
        assert_eq!(RingShape::Nudged(Dir::E).name(), "nudged-E");
        assert_eq!(Dir::nearest(233.0), Dir::SW);
        assert_eq!(Dir::nearest(359.0), Dir::N);
    }

    /// A sweep across the whole work area at 1.5 for 8 and 13 favourites:
    /// every press point gets an anchored, lawful, pickable layout WITH THE
    /// CENTRE ON THE PRESS POINT AND FULL-SIZE TILES (round 6 — the corner
    /// point itself holds 2/3/6/10 = 21 at 44 px, so nothing on a real
    /// monitor ever needs the ladder or the nudge); and the same on a
    /// second monitor at negative x, scale 1.0.
    #[test]
    fn every_press_point_on_every_monitor_gets_an_anchored_lawful_layout() {
        let second = WorkArea { x: -1920.0, y: 0.0, w: 1920.0, h: 1040.0 };
        for (area, scale) in [(AREA, SCALE), (second, 1.0)] {
            let mut x = area.x;
            while x < area.right() {
                let mut y = area.y;
                while y < area.bottom() {
                    for n in [8usize, FAVOURITES_MAX] {
                        let room = Room::at((x, y), area, scale);
                        let Placement { shape, offset, slots, arcs } = choose_shape(n, room);
                        assert!(shape.anchored(), "({x},{y}) n={n} → {}", shape.name());
                        assert_eq!(offset, (0.0, 0.0), "({x},{y}) n={n} {}: the centre moved", shape.name());
                        assert!(slots.iter().all(|s| s.tile == TILE), "({x},{y}) n={n}: shrunk");
                        assert_eq!(shape, shape_of_arcs(&arcs));
                        let seen = room.shifted(offset.0, offset.1);
                        assert_law(n, seen, &slots, &format!("({x},{y}) n={n}"));
                        // Every tile picks itself from its own centre.
                        let hits = hits_for(&slots, scale);
                        let dead = dead_zone_for(&slots) * scale;
                        for (i, s) in slots.iter().enumerate() {
                            let (dx, dy) = s.offset();
                            assert_eq!(ring_pick(&hits, dead, dx * scale, dy * scale, None), Some(i), "({x},{y}) n={n} tile {i}");
                        }
                    }
                    y += 97.0;
                }
                x += 101.0;
            }
        }
    }
}

/// The layout law as PROPERTIES over n = 1..=15 × nine rooms: even angular
/// spacing, no overlap, all inside, inner-before-outer fill order.
#[cfg(test)]
mod layout_law_tests {
    use super::test_support::*;
    use super::*;

    fn rooms() -> Vec<(&'static str, Room)> {
        let big = 1000.0;
        let near = 40.0; // under a tile's half + margin: no tile on that side
        vec![
            ("circle", Room { left: big, right: big, up: big, down: big }),
            ("half-N", Room { left: big, right: big, up: big, down: near }),
            ("half-E", Room { left: near, right: big, up: big, down: big }),
            ("half-S", Room { left: big, right: big, up: near, down: big }),
            ("half-W", Room { left: big, right: near, up: big, down: big }),
            ("quarter-NE", Room { left: near, right: big, up: big, down: near }),
            ("quarter-SE", Room { left: near, right: big, up: near, down: big }),
            ("quarter-SW", Room { left: big, right: near, up: near, down: big }),
            ("quarter-NW", Room { left: big, right: near, up: big, down: near }),
        ]
    }

    #[test]
    fn every_count_in_every_room_obeys_the_law() {
        for (name, room) in rooms() {
            for n in 1..=FAVOURITES_MAX {
                let Placement { shape, offset, slots, .. } = choose_shape(n, room);
                assert!(shape.anchored(), "{name} n={n}");
                assert_eq!(offset, (0.0, 0.0), "{name} n={n}: the centre moved");
                let seen = room.shifted(offset.0, offset.1);
                assert_law(n, seen, &slots, &format!("{name} n={n}"));
                assert!(slots.iter().all(|s| s.tile == TILE), "{name} n={n}: normal tiles");
                // ROUND 6: the name IS the room's — a wall on one side is a
                // half opening away from it, walls on two sides a quarter
                // opening into the free corner, for every count.
                assert_eq!(shape.name(), name, "n={n}");
            }
        }
    }

    /// The direction a fan opens is the open side. ROUND 6: the centre is
    /// the press point, so a tile MAY sit a little toward a near wall (up
    /// to the room less its halo — `slots_fit` guards that); what the law
    /// promises is that the fan as a whole leans AWAY from every near wall
    /// and that no tile reaches into the wall's margin.
    #[test]
    fn a_fan_opens_away_from_the_near_edges() {
        for (name, room) in rooms() {
            let Placement { slots, .. } = choose_shape(15, room);
            assert!(slots_fit(&slots, room), "{name}: a tile is in the margin");
            let n = slots.len() as f64;
            let (mx, my) = slots.iter().fold((0.0, 0.0), |(x, y), s| {
                let (dx, dy) = s.offset();
                (x + dx / n, y + dy / n)
            });
            if room.down < 100.0 { assert!(my < 0.0, "{name}: the fan leans down ({my})") }
            if room.up < 100.0 { assert!(my > 0.0, "{name}: the fan leans up ({my})") }
            if room.left < 100.0 { assert!(mx > 0.0, "{name}: the fan leans left ({mx})") }
            if room.right < 100.0 { assert!(mx < 0.0, "{name}: the fan leans right ({mx})") }
        }
    }
}

/// THE GOLDEN-ANGLE STAGGER (owner, 2026-09-15). The property, not the
/// numbers: no tile on an outer ring may sit at (or next to) the bearing of
/// a tile on the ring inside it, for every count the ring can hold.
#[cfg(test)]
mod golden_stagger_tests {
    use super::test_support::*;
    use super::*;

    /// The worst inner-vs-outer angular gap in a layout, degrees.
    fn worst_gap(slots: &[RingSlot]) -> f64 {
        let rings = rings_of(slots);
        let mut worst = f64::INFINITY;
        for w in rings.windows(2) {
            for inner in &w[0].1 {
                for outer in &w[1].1 {
                    worst = worst.min(compass_dist(inner.angle_deg, outer.angle_deg));
                }
            }
        }
        worst
    }

    /// The old rule, `first_k = first_(k−1) + pitch_k / 2`, reproduced so
    /// the test can show what it did rather than assert against a memory.
    fn half_pitch_layout(n: usize) -> Vec<RingSlot> {
        let mut out: Vec<RingSlot> = Vec::new();
        let mut remaining = n;
        let mut ring: u8 = 0;
        let mut first = 0.0;
        while remaining > 0 && (ring as usize) < RING_RADII.len() {
            let take = remaining.min(RING_CAPS[ring as usize]);
            let pitch = 360.0 / take as f64;
            first = if ring == 0 { 0.0 } else { (first + pitch / 2.0).rem_euclid(360.0) };
            for i in 0..take {
                out.push(RingSlot {
                    ring,
                    angle_deg: (first + i as f64 * pitch).rem_euclid(360.0),
                    radius: RING_RADII[ring as usize],
                    tile: TILE,
                    pitch_deg: pitch,
                });
            }
            remaining -= take;
            ring += 1;
        }
        out
    }

    /// THE CASE THE OLD RULE GOT EXACTLY WRONG. "All" fills rings of 5, 8
    /// and 13; 360/lcm(8,13) = 3.4615° is the lattice the outer two share,
    /// and half of ring 2's pitch is 13.846° = four lattice steps exactly —
    /// so four of ring 2's tiles sat at EXACTLY a ring-1 tile's bearing,
    /// one app hidden directly behind another. Not a near miss: zero.
    #[test]
    fn the_old_half_pitch_stagger_hid_tiles_exactly_behind_each_other() {
        let old = half_pitch_layout(26);
        assert!(
            worst_gap(&old) < 1e-6,
            "the old rule's worst gap was {}°, expected an exact 0",
            worst_gap(&old)
        );
    }

    /// And the golden angle removes it, for every count — measured across
    /// n = 1..=26 (one letter to the whole alphabet).
    #[test]
    fn no_outer_tile_hides_behind_an_inner_one() {
        let mut worst_overall = f64::INFINITY;
        for n in 1..=26usize {
            let slots = layout_ring_slots(n, TILE);
            assert_eq!(slots.len(), n, "n={n} must lay out");
            let g = worst_gap(&slots);
            if g.is_finite() {
                assert!(g > 0.5, "n={n}: an outer tile is only {g}° from an inner one");
                worst_overall = worst_overall.min(g);
            }
        }
        assert!(worst_overall.is_finite() && worst_overall > 0.5);
        // The constant is the real one, not an approximation of it.
        assert!((GOLDEN_ANGLE_DEG - 137.5).abs() < 0.1, "{GOLDEN_ANGLE_DEG}");
        assert!((stagger(0.0, 1, 45.0) - GOLDEN_ANGLE_DEG).abs() < 1e-9);
        assert_eq!(stagger(123.0, 0, 45.0), 0.0, "ring 0 always starts at north");
    }

    /// PARTIAL ARCS — a half ring and a quarter fan with two or more rings
    /// of tiles. Under the old rule every arc started at its own `arc.0`
    /// and the inner and outer runs shared a phase; the outer run is now
    /// shifted inward by `arc_phase`, so the phases differ. Containment is
    /// re-checked against the same room the layout used — the shift is
    /// INWARD, so it can only ever improve it.
    #[test]
    fn a_fans_outer_arc_does_not_repeat_the_inner_arcs_phase() {
        // Two real shapes from the owner's log: the right edge (half-W) and
        // the top-right corner (quarter-SW), 13 favourites, his monitor.
        let scale = 1.5;
        let work = WorkArea { x: 0.0, y: 48.0, w: 2560.0, h: 1552.0 };
        for cursor in [(2559.0, 658.0), (2559.0, 60.0), (1482.0, 1407.0), (4.0, 700.0)] {
            let room = Room::at(cursor, work, scale);
            let p = choose_shape(13, room);
            let seen = room.shifted(p.offset.0, p.offset.1);
            assert!(slots_fit(&p.slots, seen), "{cursor:?}: containment must not regress");
            let rings = rings_of(&p.slots);
            for w in rings.windows(2) {
                if w[0].1.len() < 2 || w[1].1.len() < 2 {
                    continue; // too narrow to separate — best effort, by design
                }
                let mut worst = f64::INFINITY;
                for inner in &w[0].1 {
                    for outer in &w[1].1 {
                        worst = worst.min(compass_dist(inner.angle_deg, outer.angle_deg));
                    }
                }
                assert!(
                    worst > 0.5,
                    "{cursor:?} ({}): ring {} and ring {} share a bearing to within {worst}°",
                    p.shape.name(),
                    w[0].0,
                    w[1].0
                );
            }
        }
    }

    /// The phase never widens an arc and never runs away on a narrow one.
    #[test]
    fn the_arc_phase_stays_inside_its_own_arc() {
        assert_eq!(arc_phase(0, 120.0, 4), 0.0, "ring 0 keeps the arc's own start");
        assert_eq!(arc_phase(1, 120.0, 1), 0.0, "a lone tile has no run to shift");
        for span in [10.0, 45.0, 90.0, 180.0, 359.0] {
            for count in 2..=13usize {
                let p = arc_phase(1, span, count);
                assert!(p >= 0.0 && p <= span * 0.25 + 1e-9, "span {span} count {count}: {p}");
                // The run still ends on `hi`, so every tile is inside.
                let pitch = (span - p) / (count - 1) as f64;
                assert!(pitch > 0.0);
                assert!((p + pitch * (count - 1) as f64 - span).abs() < 1e-9);
            }
        }
    }
}

/// THE SPIRAL — "All"'s other layout (owner, 2026-09-15). The same three
/// laws the rings obey: nothing overlaps, everything fits the room, and the
/// cursor on a tile's own centre picks that tile.
#[cfg(test)]
mod spiral_tests {
    use super::*;

    fn xy(s: &RingSlot) -> (f64, f64) {
        s.offset()
    }

    /// No two tiles overlap, and the nearest-neighbour spacing is the same
    /// tile-derived spacing the rings use (`ARC_STEP`) — which is what
    /// `spiral_step`'s area argument is for. n = 1..=40 (every letter plus
    /// the specials, and then some).
    #[test]
    fn the_spiral_never_overlaps_and_matches_the_rings_spacing() {
        for n in 1..=40usize {
            let slots = spiral_slots(n, TILE);
            assert_eq!(slots.len(), n);
            let mut worst = f64::INFINITY;
            for i in 0..n {
                let a = xy(&slots[i]);
                // Nothing may sit under the centre disc.
                assert!(
                    slots[i].radius >= RING_R1 - 1e-9,
                    "n={n} tile {i} at r={} is inside the disc",
                    slots[i].radius
                );
                for j in (i + 1)..n {
                    let b = xy(&slots[j]);
                    let d = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
                    worst = worst.min(d);
                    assert!(d >= TILE, "n={n}: tiles {i} and {j} are {d} apart, tile is {TILE}");
                }
            }
            if n > 8 {
                // The measured nearest-neighbour distance sits around
                // ARC_STEP; allow a fifth either way (the spiral's first
                // few turns are necessarily looser than the asymptote).
                assert!(
                    worst > ARC_STEP * 0.8 && worst < ARC_STEP * 1.35,
                    "n={n}: nearest neighbours {worst}px, ARC_STEP is {ARC_STEP}"
                );
            }
        }
        // The step is derived, not tuned: k = s·sqrt(√3 / 2π).
        let k = spiral_step();
        assert!((k - ARC_STEP * (3.0f64.sqrt() / (2.0 * std::f64::consts::PI)).sqrt()).abs() < 1e-12);
        assert!(k > 37.0 && k < 37.6, "{k}");
    }

    /// Every tile fits a representative room — the owner's work area — once
    /// "All"'s own clamp has placed the centre, which is the containment law
    /// a spiral inherits by being a variant of "All".
    #[test]
    fn a_clamped_spiral_keeps_every_tile_inside_the_room() {
        let scale = 1.5;
        let work = WorkArea { x: 0.0, y: 48.0, w: 2560.0, h: 1552.0 };
        for n in [1usize, 8, 26, 33, 40] {
            let slots = spiral_slots(n, TILE);
            for cursor in [
                (10.0, 60.0),
                (2550.0, 60.0),
                (2550.0, 1590.0),
                (10.0, 1590.0),
                (1280.0, 800.0),
            ] {
                let c = clamp_ring_center(cursor, work, clamp_extent(&slots, scale));
                for (i, s) in slots.iter().enumerate() {
                    let (dx, dy) = s.offset();
                    let (x, y) = (c.0 + dx * scale, c.1 + dy * scale);
                    let h = (s.tile / 2.0 + CLAMP_MARGIN) * scale;
                    assert!(
                        x - h >= work.x - 1e-6
                            && x + h <= work.right() + 1e-6
                            && y - h >= work.y - 1e-6
                            && y + h <= work.bottom() + 1e-6,
                        "n={n} from {cursor:?}: tile {i} at ({x},{y}) leaves the work area"
                    );
                }
            }
        }
    }

    /// The cursor on any tile's own centre picks that tile — the only pick
    /// rule a layout with no bands and no sectors can have.
    #[test]
    fn a_point_on_a_spiral_tile_picks_that_tile() {
        for n in 1..=40usize {
            let slots = spiral_slots(n, TILE);
            let hits = hits_for(&slots, 1.0);
            let dead = dead_zone_for(&slots);
            for (i, s) in slots.iter().enumerate() {
                let (dx, dy) = s.offset();
                assert_eq!(spiral_pick(&hits, dead, dx, dy, None), Some(i), "n={n} tile {i}");
                // …and with a completely unrelated tile armed, it still does:
                // the hysteresis slack is a fraction of a tile, not a licence.
                let other = (i + n / 2) % n;
                assert_eq!(
                    spiral_pick(&hits, dead, dx, dy, Some(other)),
                    Some(i),
                    "n={n} tile {i} with {other} armed"
                );
            }
            // The dead zone still refuses, and an empty table picks nothing.
            assert_eq!(spiral_pick(&hits, dead, 0.0, 0.0, None), None);
            assert_eq!(spiral_pick(&[], dead, 500.0, 0.0, None), None);
        }
    }

    /// A spiral draws no dashed guide circles — it has no rings to draw —
    /// while the ring layouts still draw one per ring.
    #[test]
    fn a_spiral_has_no_guide_circles_and_the_rings_still_do() {
        assert!(guide_diameters(&spiral_slots(26, TILE)).is_empty());
        assert_eq!(guide_diameters(&layout_ring_slots(26, TILE)).len(), 3);
        assert_eq!(guide_diameters(&layout_ring_slots(3, TILE)).len(), 1);
    }
}

#[cfg(test)]
mod name_split_tests {
    use super::*;

    #[test]
    fn a_browser_profile_splits_into_app_and_account() {
        assert_eq!(split_display_name("Google Chrome — Arpon"), ("Chrome".into(), Some("Arpon".into())));
        assert_eq!(split_display_name("Brave — Work"), ("Brave".into(), Some("Work".into())));
        assert_eq!(split_display_name("Microsoft Edge — arpon@gmail.com"), ("Edge".into(), Some("arpon".into())));
        assert_eq!(split_display_name("Mozilla Firefox — dev"), ("Firefox".into(), Some("dev".into())));
    }

    #[test]
    fn a_plain_name_has_no_account_and_keeps_its_words() {
        assert_eq!(split_display_name("Terminal"), ("Terminal".into(), None));
        assert_eq!(split_display_name("Google Drive"), ("Drive".into(), None));
        assert_eq!(split_display_name("Google"), ("Google".into(), None));
        assert_eq!(split_display_name("Boss Key"), ("Boss Key".into(), None));
        assert_eq!(split_display_name("https://github.com"), ("https://github.com".into(), None));
        assert_eq!(split_display_name("  Notes  "), ("Notes".into(), None));
    }

    #[test]
    fn an_empty_account_part_is_none() {
        assert_eq!(split_display_name("Chrome — "), ("Chrome".into(), None));
        assert_eq!(split_display_name("Chrome — @"), ("Chrome".into(), None));
        assert_eq!(short_app_name("microsoft edge"), "Edge");
    }
}

#[cfg(test)]
mod pick_tests {
    use super::*;

    fn hits(n: usize) -> (Vec<RingHit>, Vec<RingSlot>) {
        let slots = layout_ring_slots(n, TILE);
        (hits_for(&slots, 1.0), slots)
    }

    /// The index of the tile on `ring` whose bearing is nearest `deg`. The
    /// ring-to-ring stagger is the golden angle, so "the first tile of the
    /// outer ring" and "the outer tile nearest north" are different tiles —
    /// a band test wants the second and must not hard-code the first.
    fn nearest_in_ring(items: &[RingHit], ring: u8, deg: f64) -> usize {
        items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.ring == ring)
            .min_by(|(_, a), (_, b)| {
                compass_dist(deg, a.angle_deg)
                    .partial_cmp(&compass_dist(deg, b.angle_deg))
                    .unwrap()
            })
            .map(|(i, _)| i)
            .expect("the ring must have at least one tile")
    }

    /// Every tile is reachable: the cursor at a tile's own centre picks it,
    /// for one, two and three rings.
    #[test]
    fn every_item_is_reachable_from_its_own_centre() {
        for n in [1usize, 3, 6, 8, 12, 15, 26, 33, 45] {
            let (h, slots) = hits(n);
            let dead = dead_zone_for(&slots);
            for (i, s) in slots.iter().enumerate() {
                let (dx, dy) = s.offset();
                assert_eq!(
                    ring_pick(&h, dead, dx, dy, None),
                    Some(i),
                    "n={n}: tile {i} (ring {}, {}°) must pick itself",
                    s.ring, s.angle_deg
                );
            }
        }
    }

    /// Adjacent tiles never share a sector: a point a hair past the midpoint
    /// between two neighbours picks the one it is nearer to, on both sides.
    #[test]
    fn adjacent_tiles_never_share_a_sector() {
        for n in [6usize, 15, 33] {
            let (h, slots) = hits(n);
            let dead = dead_zone_for(&slots);
            let mut rings: Vec<u8> = slots.iter().map(|s| s.ring).collect();
            rings.sort_unstable();
            rings.dedup();
            for r in rings {
                let idx: Vec<usize> = (0..slots.len()).filter(|&i| slots[i].ring == r).collect();
                if idx.len() < 2 {
                    continue;
                }
                for k in 0..idx.len() {
                    let a = idx[k];
                    let b = idx[(k + 1) % idx.len()];
                    let step = 360.0 / idx.len() as f64;
                    let mid = slots[a].angle_deg + step / 2.0;
                    let rad = slots[a].radius;
                    let at = |deg: f64| {
                        let t = deg.to_radians();
                        (rad * t.sin(), -rad * t.cos())
                    };
                    let (x1, y1) = at(mid - 0.5);
                    let (x2, y2) = at(mid + 0.5);
                    assert_eq!(ring_pick(&h, dead, x1, y1, None), Some(a), "n={n} ring {r}: left of midpoint");
                    assert_eq!(ring_pick(&h, dead, x2, y2, None), Some(b), "n={n} ring {r}: right of midpoint");
                }
            }
        }
    }

    /// The dead zone: nothing inside it, whichever way the cursor points.
    #[test]
    fn the_dead_zone_picks_nothing() {
        let (h, slots) = hits(6);
        let dead = dead_zone_for(&slots);
        assert!(dead >= PILL_D / 2.0);
        for deg in (0..360).step_by(15) {
            let t = (deg as f64).to_radians();
            let r = dead - 1.0;
            assert_eq!(ring_pick(&h, dead, r * t.sin(), -r * t.cos(), None), None);
        }
        assert_eq!(ring_pick(&h, dead, 0.0, 0.0, None), None);
        assert_eq!(ring_pick(&[], dead, 500.0, 0.0, None), None);
    }

    /// Bands are picked by distance: the same bearing chooses the inner tile
    /// near the inner radius and the outer tile near the outer radius, and
    /// beyond the outer ring it stays with the outer band.
    #[test]
    fn the_band_follows_the_cursors_distance() {
        // 5 inner at RING_RADII[0] + 8 outer at RING_RADII[1] — exactly two
        // full rings (RING_CAPS[0] + RING_CAPS[1]), both from north.
        let (h, slots) = hits(RING_CAPS[0] + RING_CAPS[1]);
        let dead = dead_zone_for(&slots);
        // WHICH outer tile is nearest north is no longer index RING_CAPS[0]:
        // the ring-to-ring stagger is the GOLDEN ANGLE since 2026-09-15, so
        // the outer ring's tile 0 sits at 137.5°, not at half a pitch. The
        // test asks the layout rather than assuming a starting index — the
        // property under test is the BAND, not the ordering.
        let outer_north = nearest_in_ring(&h, 1, 0.0);
        assert_eq!(ring_pick(&h, dead, 0.0, -RING_RADII[0], None), Some(0));
        assert_eq!(ring_pick(&h, dead, 0.0, -RING_RADII[1], None), Some(outer_north));
        assert_eq!(
            ring_pick(&h, dead, 0.0, -600.0, None),
            Some(outer_north),
            "beyond the outer ring stays outer"
        );
        // The midpoint between the bands belongs to whichever is nearer; a
        // few px either side flips it.
        let mid = (RING_RADII[0] + RING_RADII[1]) / 2.0;
        assert_eq!(ring_pick(&h, dead, 0.0, -(mid - 2.0), None), Some(0));
        assert_eq!(ring_pick(&h, dead, 0.0, -(mid + 2.0), None), Some(outer_north));
    }

    /// Hysteresis holds the armed tile through a small overshoot inside its
    /// own band, and never across a band boundary.
    #[test]
    fn hysteresis_holds_within_a_band_and_not_across_bands() {
        let (h, slots) = hits(RING_CAPS[0] + RING_CAPS[1]);
        let dead = dead_zone_for(&slots);
        let at = |deg: f64, r: f64| {
            let t = deg.to_radians();
            (r * t.sin(), -r * t.cos())
        };
        let pitch = 360.0 / RING_CAPS[0] as f64; // 72°, the inner ring's spacing
        let mid = pitch / 2.0; // 36°, the midpoint to tile 1
        // Tile 0 (0°) is armed; the cursor drifts 1.5° past the midpoint
        // toward tile 1. Held (within 2×HYSTERESIS_DEG of the midpoint).
        let (x, y) = at(mid + 1.5, RING_RADII[0]);
        assert_eq!(ring_pick(&h, dead, x, y, Some(0)), Some(0));
        // Past the hysteresis it lets go.
        let (x, y) = at(mid + 2.0 * HYSTERESIS_DEG + 1.0, RING_RADII[0]);
        assert_eq!(ring_pick(&h, dead, x, y, Some(0)), Some(1));
        // An armed INNER tile does not hold the cursor at the OUTER radius.
        let (x, y) = at(0.0, RING_RADII[1]);
        assert_eq!(ring_pick(&h, dead, x, y, Some(0)), Some(nearest_in_ring(&h, 1, 0.0)));
    }

    /* ---- THE PROXIMITY OVERRIDE (owner, 2026-09-15) -------------------- */

    /// The cursor sitting on a tile's own centre picks THAT tile, whatever
    /// the band-and-angle test would have said — including the adversarial
    /// case built here, which the angle math gets demonstrably wrong.
    ///
    /// THE ADVERSARIAL CASE, built deliberately, and the failure mode it
    /// exercises is the BAND — the one input to the angle test that has
    /// nothing to do with where a tile is DRAWN.
    ///
    /// Two bands 37 px apart (less than one tile), which is legal for any
    /// layout whose rings are close or whose tiles have been shrunk: ring 0
    /// at r = 283 with tiles at 0° and 40°, ring 1 at r = 320 with one at
    /// 20°. The cursor is put 20 px inside the ring-1 tile — comfortably
    /// within its 44 px box, visibly on that icon — and its distance from
    /// the centre is then 300, which is NEARER ring 0's radius (17) than
    /// ring 1's (20). The band test therefore goes to ring 0 and the angle
    /// test picks a tile ~100 px away from the pointer.
    /// `naive_band_and_angle` reproduces that old answer, so the test
    /// proves the override CHANGED the verdict rather than agreeing with it.
    #[test]
    fn a_cursor_on_a_tiles_own_box_beats_the_angle() {
        let items = vec![
            RingHit { ring: 0, angle_deg: 0.0, radius: 283.0, tile: 44.0 },
            RingHit { ring: 0, angle_deg: 40.0, radius: 283.0, tile: 44.0 },
            RingHit { ring: 1, angle_deg: 20.0, radius: 320.0, tile: 44.0 },
        ];
        let a = 20.0_f64.to_radians();
        let (tx, ty) = (320.0 * a.sin(), -320.0 * a.cos());
        let r = 300.0; // 20 px inside the tile, and nearer 283 than 320
        let (px, py) = (r * a.sin(), -r * a.cos());
        assert!(
            ((px - tx).powi(2) + (py - ty).powi(2)).sqrt() < 22.0,
            "the probe must be inside the ring-1 tile's box"
        );
        assert_eq!(
            naive_band_and_angle(&items, px, py),
            Some(0),
            "the angle test alone picks a ring-0 tile ~100px away — the bug"
        );
        assert_eq!(
            ring_pick(&items, 60.0, px, py, None),
            Some(2),
            "proximity wins: the cursor is on the 20° tile's own box"
        );
        // And on every tile's exact centre, in every layout, for both rings.
        for n in [1usize, 5, 8, 13, 26] {
            let (h, slots) = hits(n);
            let dead = dead_zone_for(&slots);
            for (i, s) in slots.iter().enumerate() {
                let (dx, dy) = s.offset();
                assert_eq!(ring_pick(&h, dead, dx, dy, None), Some(i), "n={n} tile {i}");
            }
        }
    }

    /// The override is an ADDITION: far from every tile, and between two of
    /// them, it stands aside and the angle test answers exactly as before.
    #[test]
    fn proximity_declines_when_it_is_not_sure() {
        let (h, slots) = hits(5);
        let dead = dead_zone_for(&slots);
        // A flick far outside the ring — nothing is within reach, so the
        // answer is the angular one (the tile nearest that bearing).
        let far = 900.0;
        let a = 40.0_f64.to_radians();
        assert_eq!(proximity_pick(&h, far * a.sin(), -far * a.cos(), None), None);
        assert_eq!(
            ring_pick(&h, dead, far * a.sin(), -far * a.cos(), None),
            Some(nearest_in_ring(&h, 0, 40.0))
        );
        // Exactly between two neighbours on one ring: both are the same
        // distance away, so proximity refuses and the angle decides.
        let mid = 36.0_f64.to_radians(); // halfway between 0° and 72°
        let (mx, my) = (108.0 * mid.sin(), -108.0 * mid.cos());
        assert_eq!(proximity_pick(&h, mx, my, None), None, "a tie is not an override");
        assert!(ring_pick(&h, dead, mx, my, None).is_some());
        // Hysteresis survives: an armed tile the cursor is still on holds
        // the pick even when a neighbour has crept marginally closer.
        let near1 = 30.0_f64.to_radians();
        let (hx, hy) = (108.0 * near1.sin(), -108.0 * near1.cos());
        assert_eq!(ring_pick(&h, dead, hx, hy, Some(0)), Some(0), "armed tile 0 holds");
    }

    /// The band-and-angle answer, with no proximity override — the code as
    /// it stood before 2026-09-15, kept here so the adversarial test above
    /// can prove the override changes the verdict rather than merely
    /// agreeing with it.
    fn naive_band_and_angle(items: &[RingHit], dx: f64, dy: f64) -> Option<usize> {
        let dist = (dx * dx + dy * dy).sqrt();
        let bearing = dx.atan2(-dy).to_degrees().rem_euclid(360.0);
        let band = items
            .iter()
            .map(|i| i.ring)
            .min_by(|a, b| {
                let ra = items.iter().find(|i| i.ring == *a).unwrap().radius;
                let rb = items.iter().find(|i| i.ring == *b).unwrap().radius;
                (ra - dist).abs().partial_cmp(&(rb - dist).abs()).unwrap()
            })?;
        items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.ring == band && compass_dist(bearing, it.angle_deg) < 90.0)
            .min_by(|(_, a), (_, b)| {
                compass_dist(bearing, a.angle_deg)
                    .partial_cmp(&compass_dist(bearing, b.angle_deg))
                    .unwrap()
            })
            .map(|(i, _)| i)
    }

    /// A band whose every tile is more than 90° from the bearing picks
    /// nothing — releasing there is "close, no action".
    #[test]
    fn pointing_away_from_a_sparse_band_picks_nothing() {
        let (h, slots) = hits(2); // two items: 0° (north) and 180° (south)
        let dead = dead_zone_for(&slots);
        assert_eq!(ring_pick(&h, dead, 125.0, 0.0, None), None);
        assert_eq!(ring_pick(&h, dead, 125.0, -5.0, None), Some(0));
        let sparse = vec![
            RingHit { ring: 0, angle_deg: 10.0, radius: 125.0, tile: 44.0 },
            RingHit { ring: 0, angle_deg: 40.0, radius: 125.0, tile: 44.0 },
        ];
        assert_eq!(ring_pick(&sparse, 60.0, 0.0, 125.0, None), None, "straight down: nothing");
        assert_eq!(ring_pick(&sparse, 60.0, 30.0, -120.0, None), Some(0));
    }

    #[test]
    fn compass_distance_folds_across_north() {
        assert!((compass_dist(359.0, 1.0) - 2.0).abs() < 1e-9);
        assert!((compass_dist(0.0, 180.0) - 180.0).abs() < 1e-9);
        assert!((compass_dist(90.0, 45.0) - 45.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod route_and_special_tests {
    use super::*;
    use crate::config::MiddleRingStyle;

    /// The owner's switch: each value takes its own path, and the default
    /// (`IconRing`) is the new ring.
    #[test]
    fn the_style_setting_picks_the_path() {
        assert_eq!(middle_down_route(MiddleRingStyle::GuideHud), MiddleRoute::GuideHud);
        assert_eq!(middle_down_route(MiddleRingStyle::IconRing), MiddleRoute::IconRing);
        assert_eq!(middle_down_route(MiddleRingStyle::default()), MiddleRoute::IconRing);
    }

    /// Every ring special maps to a real KeyCombo, no code collides with a
    /// letter, and a letter is never mistaken for a special. PHASE A: the
    /// list is derived from a SEEDED profile and the combo is the key's `Vk`.
    #[test]
    fn every_ring_special_has_a_combo_and_a_private_code() {
        let cfg = crate::engine::specials::seeded_cfg();
        let specials = ring_specials_for(&cfg);
        assert_eq!(specials.len(), 10, "ten tiles: the twelve seeded specials minus the two scroll ones");
        for (_, _, code) in &specials {
            assert!(is_special_code(*code));
            assert!(special_combo_for(*code).is_some());
            assert!(!code.is_ascii_lowercase());
        }
        assert!(!is_special_code('m'));
        assert!(special_combo_for('m').is_none());
        // Esc's tile fires Space+Esc's VK; `.`'s fires VK_OEM_PERIOD.
        assert!(matches!(special_combo_for('\u{E000}'), Some(crate::hook::KeyCombo::Vk(0x1B))));
        assert!(matches!(special_combo_for('\u{E006}'), Some(crate::hook::KeyCombo::Vk(0xBE))));
        // A removed special leaves the ring; the others keep their codes.
        let mut cfg2 = cfg.clone();
        cfg2.profiles[0].bindings.remove("esc");
        let after = ring_specials_for(&cfg2);
        assert_eq!(after.len(), 9);
        assert_eq!(after[0].2, '\u{E001}', "the backtick keeps U+E001");
    }
}

#[cfg(test)]
mod favourites_and_payload_tests {
    use super::*;
    use crate::config::{KeyBinding, MiddleRingScope, Profile};

    fn cfg_with(bindings: &[(&str, KeyBinding)]) -> AppConfig {
        let mut cfg = AppConfig::default();
        let mut map = crate::config::BindingMap::new();
        for (k, b) in bindings {
            map.insert((*k).to_string(), b.clone());
        }
        cfg.profiles = vec![Profile { name: "Ring".into(), bindings: map, emoji: None, specials_seeded: false }];
        cfg.active_profile = "Ring".into();
        cfg
    }

    fn app(label: &str, exe: &str) -> KeyBinding {
        KeyBinding { label: Some(label.into()), app: Some(exe.into()), ..Default::default() }
    }

    /// The default favourites are the first `FAVOURITES_DEFAULT` bound
    /// letters, sorted; an unbound stored favourite is dropped; a stored list
    /// wins when present and may hold up to fifteen.
    #[test]
    fn favourites_default_to_the_first_six_bound_letters_and_cap_at_fifteen() {
        let mut binds: Vec<(String, KeyBinding)> = Vec::new();
        for c in "zyxwvutsrq".chars() {
            binds.push((c.to_string(), app(&c.to_string(), "x.exe")));
        }
        let refs: Vec<(&str, KeyBinding)> = binds.iter().map(|(k, b)| (k.as_str(), b.clone())).collect();
        let mut cfg = cfg_with(&refs);
        assert_eq!(favourites_for(&cfg, "Ring"), vec!['q', 'r', 's', 't', 'u']);
        // Stored favourites, including one unbound letter and a duplicate.
        cfg.middle_ring_favourites = vec!["z".into(), "a".into(), "Q".into(), "z".into()];
        assert_eq!(favourites_for(&cfg, "Ring"), vec!['z', 'q']);
        // A stored list that is entirely unbound falls back to the default.
        cfg.middle_ring_favourites = vec!["a".into(), "b".into()];
        assert_eq!(favourites_for(&cfg, "Ring").len(), FAVOURITES_DEFAULT);
        assert!(favourites_for(&cfg, "No Such Profile").is_empty());
        // Ten stored and bound: all ten (the eight-cap is gone), in order.
        cfg.middle_ring_favourites = "zyxwvutsrq".chars().map(|c| c.to_string()).collect();
        assert_eq!(favourites_for(&cfg, "Ring").len(), 10);
        assert_eq!(favourites_for(&cfg, "Ring")[0], 'z');
    }

    /// THE OWNER'S PRIORITY — "ensure real app icons get shown". A payload
    /// built from a profile with an exe, a folder and a link binding has a
    /// non-empty icon for each. The extractor is a stub so this runs on CI;
    /// the link's icon comes from the binding's own `site_icon`, never from
    /// the extractor.
    #[test]
    fn a_payload_carries_a_real_icon_for_an_exe_a_folder_and_a_link() {
        let folder = std::env::temp_dir();
        let folder_s = folder.to_string_lossy().into_owned();
        let link = KeyBinding {
            label: Some("GitHub".into()),
            web_url: Some("https://github.com".into()),
            site_icon: Some("data:image/x-icon;base64,AAEC".into()),
            ..Default::default()
        };
        let cfg = cfg_with(&[
            ("b", app("Brave", "brave.exe")),
            ("d", app("Downloads", &folder_s)),
            ("g", link),
        ]);
        let asked = std::cell::RefCell::new(Vec::new());
        let extract = |target: &str| -> Option<String> {
            asked.borrow_mut().push(target.to_string());
            Some("iVBORw0KGgo=".to_string())
        };
        let entries = build_entries(&cfg, "Ring", MiddleRingScope::MyEight, &extract);
        assert_eq!(entries.len(), 3);
        for e in &entries {
            assert!(e.icon.as_deref().is_some_and(|s| !s.is_empty()), "{} must carry an icon", e.name);
        }
        let by_key = |k: &str| entries.iter().find(|e| e.key == k).unwrap();
        assert_eq!(by_key("B").kind, ItemKind::App);
        assert_eq!(by_key("B").icon.as_deref(), Some("data:image/png;base64,iVBORw0KGgo="));
        assert_eq!(by_key("D").kind, ItemKind::Folder, "an absolute directory is a folder");
        assert_eq!(by_key("G").kind, ItemKind::Link);
        assert_eq!(by_key("G").icon.as_deref(), Some("data:image/x-icon;base64,AAEC"));
        // The extractor was asked about the exe and the folder, and NOT the link.
        let asked = asked.borrow();
        assert!(asked.iter().any(|t| t == "brave.exe"));
        assert!(asked.iter().any(|t| t == &folder_s));
        assert!(!asked.iter().any(|t| t.contains("github")));

        let (payload, slots) =
            build_payload(entries, MiddleRingScope::MyEight, true, false, (588.0, 472.0));
        assert_eq!(payload.items.len(), 3);
        assert_eq!(slots.len(), 3);
        assert_eq!(payload.scope, "my_eight");
        assert_eq!((payload.cx, payload.cy), (588.0, 472.0));
        assert_eq!(payload.pill_max, 2.0 * (RING_RADII[0] - TILE / 2.0 - PILL_FREE_MARGIN));
        assert!(payload.items.iter().all(|i| i.icon.is_some()));
    }

    /// The picker's PNG (`icon_override`) is used before the extractor is
    /// asked; a link with no `site_icon` gets `None` (letter disc), and the
    /// extractor is never asked to fetch anything for it.
    #[test]
    fn icon_override_wins_and_a_link_without_a_site_icon_falls_to_the_disc() {
        let mut pinned = app("Pinned", "pinned.exe");
        pinned.icon_override = Some("QUJD".into());
        let link = KeyBinding {
            label: Some("Reddit".into()),
            web_url: Some("https://reddit.com".into()),
            ..Default::default()
        };
        let cfg = cfg_with(&[("p", pinned), ("r", link)]);
        let extract = |_: &str| -> Option<String> { panic!("the extractor must not be asked") };
        let entries = build_entries(&cfg, "Ring", MiddleRingScope::MyEight, &extract);
        assert_eq!(entries.iter().find(|e| e.key == "P").unwrap().icon.as_deref(), Some("data:image/png;base64,QUJD"));
        assert_eq!(entries.iter().find(|e| e.key == "R").unwrap().icon, None);
    }

    /// "All" = the favourites first, then every other bound letter, then the
    /// specials — and Favourites is the favourites alone.
    #[test]
    fn all_of_them_appends_the_rest_and_the_specials_in_order() {
        let mut binds: Vec<(String, KeyBinding)> = Vec::new();
        for c in "abcdefghijk".chars() {
            binds.push((c.to_string(), app(&c.to_string(), "x.exe")));
        }
        let refs: Vec<(&str, KeyBinding)> = binds.iter().map(|(k, b)| (k.as_str(), b.clone())).collect();
        let mut cfg = cfg_with(&refs);
        // PHASE A — the specials are the profile's own now; seed them so the
        // "All" scope has its ten tiles as before.
        assert!(crate::config::seed_specials(&mut cfg));
        cfg.middle_ring_favourites = vec!["k".into(), "a".into()];
        let extract = |_: &str| -> Option<String> { None };
        let eight = build_entries(&cfg, "Ring", MiddleRingScope::MyEight, &extract);
        assert_eq!(eight.iter().map(|e| e.code).collect::<Vec<_>>(), vec!['k', 'a']);
        let all = build_entries(&cfg, "Ring", MiddleRingScope::All, &extract);
        let codes: Vec<char> = all.iter().map(|e| e.code).collect();
        assert_eq!(&codes[..2], &['k', 'a']);
        assert_eq!(&codes[2..11], &['b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j']);
        assert_eq!(codes.len(), 11 + ring_specials_for(&cfg).len());
        assert_eq!(codes.len(), 21);
        assert!(all[11..].iter().all(|e| e.kind == ItemKind::Special && is_special_code(e.code)));
        let (payload, _) = build_payload(all, MiddleRingScope::All, false, true, (0.0, 0.0));
        assert_eq!(payload.scope, "all");
        assert!(!payload.fun && payload.reduced);
        assert_eq!(payload.guides.len(), 3, "18 items: three rings, three guide circles");
        // The pill's two lines ride in the payload, split at display time.
        let mut cfg2 = cfg_with(&[("c", KeyBinding {
            label: Some("Google Chrome".into()), app: Some("chrome.exe".into()),
            browser_profile_name: Some("Arpon".into()), ..Default::default()
        })]);
        cfg2.middle_ring_favourites = vec!["c".into()];
        let e = build_entries(&cfg2, "Ring", MiddleRingScope::MyEight, &extract);
        assert_eq!(e[0].name, "Google Chrome — Arpon");
        let (p, _) = build_payload(e, MiddleRingScope::MyEight, true, false, (0.0, 0.0));
        assert_eq!(p.items[0].name, "Chrome");
        assert_eq!(p.items[0].account.as_deref(), Some("Arpon"));
    }
}

/// ROUND 6 (2026-09-17): the owner's round-3 law restored — the centre is
/// the press point; shrink before move; guides are arcs that stay on-screen.
#[cfg(test)]
mod round6_tests {
    use super::*;

    const SCALE: f64 = 1.5;

    /// The owner's own edge and corner presses from debug.log (2026-09-16
    /// 10:08-10:09 and 2026-09-17 05:16), reconstructed as press point =
    /// logged centre − clamp delta, with the ROOM each line printed. Every
    /// one of them used to move the centre onto the screen edge; now none
    /// does, and every tile — halo included — stays inside the room.
    fn owner_presses() -> Vec<(&'static str, (f64, f64), WorkArea)> {
        let room_a = WorkArea { x: 0.0, y: 48.0, w: 2560.0, h: 1550.0 };
        let room_b = WorkArea { x: 0.0, y: 48.0, w: 2560.0, h: 1480.0 };
        let room_c = WorkArea { x: 2.0, y: 2.0, w: 2556.0, h: 1594.0 };
        vec![
            ("right edge, was half-W delta (206,0)", (2560.0 - 206.0, 489.0), room_a),
            ("top-left corner, was quarter-SE delta (-4,-44)", (4.0, 48.0 + 44.0), room_a),
            ("bottom-left corner, was quarter-NE delta (-12,-71)", (12.0, 1528.0 + 71.0), room_b),
            ("left edge, was half-E delta (0,0)", (0.0, 1060.0), room_a),
            ("top-left, was quarter-SE delta (-123,-278)", (2.0 + 123.0, 2.0 + 278.0), room_c),
            ("top-right, was quarter-SW delta (268,1)", (2558.0 - 268.0, 2.0 - 1.0 + 1.0), room_c),
            ("top edge, was half-S delta (0,-94)", (1237.0, 2.0 + 94.0), room_c),
            ("top edge (05:16), was half-S delta (0,-31)", (1490.0, 32.0 + 31.0), WorkArea { x: 0.0, y: 32.0, w: 1920.0, h: 1000.0 }),
        ]
    }

    #[test]
    fn the_owners_edge_presses_keep_the_centre_on_the_press_point() {
        for (what, press, area) in owner_presses() {
            let scale = if area.w > 2000.0 { SCALE } else { 1.0 };
            let room = Room::at(press, area, scale);
            let Placement { shape, offset, slots, arcs } = choose_shape(13, room);
            let seen = room.shifted(offset.0, offset.1);
            let tile = slots.iter().map(|s| s.tile).fold(f64::INFINITY, f64::min);
            eprintln!(
                "{what}: press {press:?} → {} tile {tile:.1} offset {offset:?} rings {:?}",
                shape.name(),
                arcs.iter().map(|a| (a.ring, a.count, a.span().round())).collect::<Vec<_>>()
            );
            assert!(shape.anchored(), "{what}: {}", shape.name());
            assert_eq!(offset, (0.0, 0.0), "{what}: the centre moved");
            assert!(slots_fit(&slots, seen), "{what}: a tile left the room");
            assert_eq!(slots.len(), 13, "{what}: every favourite is placed");
            assert!(!matches!(shape, RingShape::Nudged(_) | RingShape::CircleClamped), "{what}");
        }
    }

    /// Every point of every guide arc (the dashed line the page strokes)
    /// lies inside the room, for every owner press and for a sweep of the
    /// whole panel — the round-6 promise that nothing but the pill may
    /// cross the screen edge.
    #[test]
    fn guide_arcs_never_leave_the_room() {
        let area = WorkArea { x: 0.0, y: 48.0, w: 2560.0, h: 1552.0 };
        let mut presses: Vec<(f64, f64)> = owner_presses().into_iter().map(|p| p.1).collect();
        let mut x = area.x + 20.0;
        while x < area.right() {
            let mut y = area.y + 20.0;
            while y < area.bottom() {
                presses.push((x, y));
                y += 160.0;
            }
            x += 160.0;
        }
        for press in presses {
            let room = Room::at(press, area, SCALE);
            let Placement { shape, offset, slots, arcs } = choose_shape(13, room);
            if !shape.anchored() {
                continue;
            }
            let seen = room.shifted(offset.0, offset.1);
            for g in guide_arcs(&slots, &arcs) {
                let r = g.d / 2.0;
                let mut a = g.lo;
                while a <= g.hi + 1e-9 {
                    let (dx, dy) = (r * a.to_radians().sin(), -r * a.to_radians().cos());
                    assert!(
                        -dx <= seen.left + 1e-6 && dx <= seen.right + 1e-6 && -dy <= seen.up + 1e-6 && dy <= seen.down + 1e-6,
                        "press {press:?} {}: guide d={} point at {a:.1}° ({dx:.0},{dy:.0}) is off-screen",
                        shape.name(),
                        g.d
                    );
                    a += 0.5;
                }
            }
        }
    }

    /// The ladder descends monotonically from `TILE` to exactly `TILE_MIN`,
    /// and every rung scales the radii and the spacing with it.
    #[test]
    fn the_tile_ladder_scales_the_whole_number_system() {
        let ladder = tile_ladder();
        assert_eq!(ladder[0], TILE);
        assert_eq!(*ladder.last().unwrap(), TILE_MIN);
        assert!(ladder.windows(2).all(|w| w[1] < w[0]));
        for &t in &ladder {
            let k = t / TILE;
            assert!((ring_radius(0, t) - RING_RADII[0] * k).abs() < 1e-9);
            assert!((arc_step(t) - ARC_STEP * k).abs() < 1e-9);
        }
    }

    /// A sliver too small for the floor forces a NUDGE, and the nudge is
    /// the shortest grid vector that works: no shorter candidate fits.
    #[test]
    fn a_nudge_is_the_shortest_vector_that_fits() {
        let area = WorkArea { x: 0.0, y: 0.0, w: 2000.0, h: 70.0 };
        let press = (30.0, 35.0);
        let room = Room::at(press, area, 1.0);
        let Placement { shape, offset, slots, .. } = choose_shape(13, room);
        match shape {
            RingShape::Nudged(_) => {
                let seen = room.shifted(offset.0, offset.1);
                assert!(slots_fit(&slots, seen));
                let len = (offset.0.powi(2) + offset.1.powi(2)).sqrt();
                for (dx, dy) in nudge_candidates(room) {
                    if (dx * dx + dy * dy).sqrt() < len - 1e-9 {
                        assert!(layout_arcs(13, room.shifted(dx, dy), TILE_MIN).is_none(), "a shorter nudge {:?} fits", (dx, dy));
                    }
                }
            }
            RingShape::CircleClamped => {}
            other => panic!("a 70 px sliver produced {}", other.name()),
        }
    }
}
