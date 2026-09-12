/// hook/pointer.rs — pointer activation on the Guide HUD (PROBLEM 206,
/// amended by PROBLEM 209).
///
/// While Space is held and the ring is up, POINTING the cursor in a binding's
/// DIRECTION arms it; releasing Space (gesture A) or left-clicking (gesture B)
/// launches that binding. The owner's request, verbatim: *"if someone moves
/// the cursor towards the listed [letter] and then leaves the cursor, then
/// that app opens up... if they move the cursor to click on that app name,
/// then that app opens up. So either click or leave space after moving cursor
/// to that app."*
///
/// ### PROBLEM 209 — DIRECTIONAL SECTORS REPLACE CONTAINMENT. OWNER DECISION.
///
/// 1.0.87 shipped guard 4 as CONTAINMENT: the cursor had to be INSIDE a chip's
/// rect plus a 12px halo. That was a safety decision taken AGAINST the owner's
/// "towards" wording, and on 2026-08-27 he rejected it in as many words:
/// *"A person shouldn't have to physically move on top of the name of the app
/// to launch. It's 360 degrees, right? In different degrees there are
/// different apps, and depending on which place the cursor is, if the
/// direction from the Space to the app is there, it should launch that app."*
///
/// **So the hit-test is now ANGULAR.** The angle from the HUD centre to the
/// cursor picks the chip whose own centre-angle sector contains that
/// direction; the cursor never has to reach the chip. He has reaffirmed this
/// model twice. **DO NOT "fix" it back to containment** — that would be
/// re-litigating a decision the owner has already made and reversed once.
///
/// The safety that containment used to provide is now carried entirely by the
/// DEAD ZONE (guard 4 below). The direction test all but always has an answer,
/// so without a region where the answer is "nothing", every Space release
/// after any mouse twitch would launch something. The dead zone is the
/// load-bearing guard of this feature; do not weaken it without replacing it.
///
/// **1.0.89 — the hit-test scores by PERPENDICULAR DISTANCE TO THE AIM RAY,
/// not by nearest angle.** `sector_pick` carries the full derivation. The
/// short version: with TWO app bands (`hud_band_count`), two chips at a
/// similar angle on different radii tie under an angle-only comparison, and
/// perpendicular distance resolves to the nearer one. For a SINGLE band the
/// two rules are provably identical, which is what
/// `single_band_scoring_is_exactly_todays_nearest_angle` asserts against a
/// copy of the 1.0.88 algorithm. Still angle-driven: the score uses the CHIP's
/// radius, never the cursor's, so reaching further out never changes the pick.
///
/// ARCHITECTURE — three parts, strictly separated by thread:
///
///   1. THE HOOK CALLBACK (`ms_hook_proc`) stores the cursor position into
///      three atomics and nothing else. The overlay window is
///      `WS_EX_TRANSPARENT` + click-through BY DESIGN (`lib.rs`
///      `configure_overlay_window` fails closed on it), so the page can never
///      see a mousemove and all cursor knowledge must come from `WH_MOUSE_LL`.
///      This machine's hook budget is already failing — 32 DEAF events and 65
///      watchdog re-hooks in one 6h54m log window — and `LowLevelHooksTimeout`
///      is wall-clock, so the callback does lock-free atomics ONLY: no
///      geometry, no scanning, no logging, no win32k calls (PROBLEM
///      58/134/184).
///
///   2. CHIP GEOMETRY is published once per HUD show: the overlay page calls
///      `publish_hud_chips` (commands.rs) with the chips' boxes in CSS px +
///      dpr — the exact convention `overlay_shape` already uses — and it is
///      converted to PHYSICAL screen px here using the overlay window's
///      ACTUAL read-back position. Stored in fixed-size static arrays of
///      atomics, never a Vec, so the snapshot shared with the poller is
///      heap-free and lock-free. Since PROBLEM 209 the HUD CENTRE and the
///      DEAD-ZONE RADIUS are derived at publish time and stored beside it —
///      both are pure functions of the snapshot, so the 60Hz poller never
///      recomputes them and the hook callback never sees them at all.
///
///      **ONLY THE OUTER (apps) RING IS IN THE SNAPSHOT.** `publishHudChips`
///      in `toast.ts` selects `.st-chip.ap` and nothing else, so the eight
///      inner-ring SPECIALS are not published, are not indexed, and cannot be
///      armed. That is deliberate and it is what makes the angular model
///      well-defined: the two rings share one angular space, so a special and
///      an app can point in the SAME direction and a sector would be
///      ambiguous. The specials stay reachable by their keys, which is how
///      they have always been reached, and the owner's description ("the
///      direction from the space to the app") is about apps. The dead zone
///      independently guarantees it: every special sits at an inner-ring
///      radius, which is inside the dead zone by construction.
///
///   3. THE `st-hud-pointer` POLLER (same shape as `hook/exclusions.rs` and
///      `hook/fullscreen.rs`, the blessed pattern for "work the callback must
///      not do") reads the cursor atomics + the chip snapshot at ~60Hz while
///      Space is held, decides the armed chip, and ON CHANGE ONLY stores it
///      to `ARMED_INDEX` and emits the global `hud-pointer` event (`{ index }`
///      into the apps ring) for the page's highlight. Global `emit` + one
///      listener in the overlay page is the only event arrangement that has
///      ever worked here; `emit_to` never has.
///
/// THE `SPACE_ABORTED` MECHANIC. Arming sets `SPACE_ABORTED = true` — exactly
/// what the wheel does — so the Space-up path in `kb_hook_proc` stays
/// byte-identical to before: armed + release injects no space, not-armed +
/// release types a space precisely as today. The structural guarantee ("a tap
/// always types a space") is preserved rather than re-argued. DISARMING MUST
/// CLEAR IT BACK, or a user who drifts across the ring and out again loses
/// their space — but ONLY when the disarm is the user drifting out of a live,
/// visible, unblocked hold (`Verdict::DisarmClear`). A disarm caused by the
/// HUD hiding, the hold ending, or the wheel firing keeps the flag
/// (`Verdict::DisarmKeep`): in those cases another mechanism owns the abort
/// (a combo's dispatch, the wheel's opacity gesture) and clearing it would
/// type a space behind a launched action. That distinction is the subtle bug
/// this file exists to get right; `apply_to` has its own tests for it.
///
/// SIX GUARDS, all required, because "hold Space, move the mouse, release"
/// must stay a typed space unless the user meant otherwise:
///   1. `guide_hud::is_visible()` — arming is impossible before the ring is
///      actually up (`guide_hud_delay_ms`, 300ms default / 500 on this
///      machine), so nothing can be selected that cannot be seen.
///   2. Minimum travel (`MIN_TRAVEL_PHYS_PX`) from the cursor's position at
///      Space-down — a thumb on Space with a hand resting on the mouse
///      jitters a few px, never 24. UNCHANGED by PROBLEM 209.
///   3. Dwell (`DWELL_MS`) — the direction must SETTLE; a sweep across the
///      ring is a non-event. PROBLEM 209 cut this from 150ms to 60ms: a
///      direction is a coarser and more deliberate signal than landing on a
///      60x30 rect, and 150ms of it reads as lag on a flick.
///   4. THE DEAD ZONE — a central circle, radius = the apps ring's inscribed
///      radius (its nearest chip edge), floored at
///      `DEAD_ZONE_MIN_PHYS_PX`. Inside it NOTHING is armed,
///      `SPACE_ABORTED` is cleared, and releasing Space types a space exactly
///      as it always has. This is what containment used to do and it is the
///      only thing standing between "hold Space and jog the mouse" and a
///      launched app; the cursor has to LEAVE the middle of the ring before
///      any direction counts. Plus angular hysteresis
///      (`HYSTERESIS_RAD`) so a cursor sitting on a sector boundary cannot
///      flicker between two neighbours.
///   5. Visible arming — the poller emits `hud-pointer` for the page's
///      highlight; a gesture the user cannot see arm is a lottery.
///   6. Disarm on wheel — Space+scroll already aborts the space and changes
///      opacity; `block_for_hold()` makes sure it cannot ALSO launch.
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// Tuning — every number here is a guard threshold; justify before changing.
// ---------------------------------------------------------------------------

/// Chip capacity. 26 letters is the real maximum (only alpha keys can carry
/// bindings); 40 leaves headroom without making the static tables large.
pub(crate) const MAX_CHIPS: usize = 40;

/// Guard 2: physical px the cursor must move from its Space-down position
/// before anything can arm. Resting-hand jitter is a few px; a deliberate
/// reach toward the ring is tens to hundreds. 24 physical px (16 logical at
/// this machine's 1.5 scale) is comfortably above one and below the other.
/// PROBLEM 209 kept this number: it measures the same thing it always did.
const MIN_TRAVEL_PHYS_PX: i64 = 24;

/// Guard 3: how long the DIRECTION must hold steady before it arms.
///
/// PROBLEM 209 cut this from 150ms to 60ms (~4 poller ticks). 150ms was
/// calibrated for containment, where the target was a 60x30 rect and a
/// fly-through crossed it in a couple of frames. A sector is tens of degrees
/// wide and the cursor can be anywhere along it, so the same 150ms is spent
/// staring at a direction the user already chose — it reads as lag on a
/// flick. 60ms is still ~4 ticks, which a sweep across the ring cannot
/// accumulate in any one sector at plausible mouse speeds (see
/// `a_sweep_across_the_ring_is_a_non_event`).
const DWELL_MS: u64 = 60;

/// Guard 4, half of it: how far PAST the boundary between two sectors the
/// cursor's direction must go before the armed chip gives way to its
/// neighbour. ~3°, which at a 34-chip ring (10.6° sectors) is about a quarter
/// of a sector — enough to kill boundary flicker, small enough that nobody
/// can feel it. Applied as `d(armed) - d(nearest) >= 2 * HYSTERESIS_RAD`,
/// because moving `h` past the midpoint changes that difference by `2h`.
const HYSTERESIS_RAD: f64 = 3.0 * std::f64::consts::PI / 180.0;

/// Guard 4, the load-bearing half: the smallest dead zone we will ever use,
/// in physical px, and the value used when no chips are published at all.
///
/// The dead zone is normally DERIVED — the largest circle around the HUD
/// centre that touches no app chip (`dead_zone_radius`). This floor exists
/// for the degenerate snapshots: an empty ring, or geometry so small that the
/// derived radius would let a resting hand point at something. Erring LARGE
/// errs toward "nothing arms, and releasing Space types a space", which is
/// the failure this whole file is arranged around.
///
/// 140 physical px is ~93 logical px at this machine's 1.5 scale — well
/// inside the real apps ring, whose nearest edge measures ~250-270 physical
/// px even with the specials ring hidden (`hud_show_specials` off), so the
/// floor never bites on real geometry. Verified by
/// `dead_zone_radius_*` in the tests, not by assertion here.
const DEAD_ZONE_MIN_PHYS_PX: f64 = 140.0;

/// Poller cadence while Space is held (~60Hz) and while idle. The idle tick
/// only has to catch the START of a hold, and the HUD cannot appear sooner
/// than `guide_hud_delay_ms` (>= 300ms), so 40ms is generous.
const TICK_HELD_MS: u64 = 16;
const TICK_IDLE_MS: u64 = 40;

// ---------------------------------------------------------------------------
// Shared atomics — hook callback writes, poller reads (or vice versa)
// ---------------------------------------------------------------------------

/// Cursor position in PHYSICAL screen px (`MSLLHOOKSTRUCT.pt` is physical —
/// this process is PerMonitorV2), plus the tick it was stamped. The stamp is
/// what lets the poller tell "the mouse moved during THIS hold" from a stale
/// position left by the previous hold: valid iff `CURSOR_STAMP >=
/// SPACE_DOWN_TS`, both on the same GetTickCount64 clock.
static CURSOR_X: AtomicI32 = AtomicI32::new(0);
static CURSOR_Y: AtomicI32 = AtomicI32::new(0);
static CURSOR_STAMP: AtomicU64 = AtomicU64::new(0);

/// Chip rects in PHYSICAL screen px, x0/y0/x1/y1 per chip. Fixed-size static
/// atomics — the shared snapshot is heap-free by construction.
static CHIP_RECTS: [AtomicI32; MAX_CHIPS * 4] = [const { AtomicI32::new(0) }; MAX_CHIPS * 4];
/// The key char (lowercase ASCII, as u32) each chip launches. 0 = invalid.
static CHIP_KEYS: [AtomicU32; MAX_CHIPS] = [const { AtomicU32::new(0) }; MAX_CHIPS];
/// How many rects / keys are valid. They are written by DIFFERENT callers
/// (the page's `publish_hud_chips` vs `show_guide_hud`), so both counts exist
/// and every consumer bounds itself by the smaller.
static CHIP_GEOM_COUNT: AtomicUsize = AtomicUsize::new(0);
static CHIP_KEY_COUNT: AtomicUsize = AtomicUsize::new(0);

/// PROBLEM 209 — the HUD centre in PHYSICAL screen px: the origin every
/// direction is measured from. Derived at publish time from the overlay
/// window's read-back position + its inner size, because `#st-hud` is
/// `position: fixed; inset: 0` and every chip is placed with
/// `calc(50% + …)` — the ring's centre IS the client-area centre, whatever
/// size Rust clamped the window to.
///
/// `HUD_CENTER_OK` is a separate flag rather than a sentinel coordinate:
/// (0,0) is a perfectly legal physical screen position on the primary
/// monitor, so there is no coordinate that can mean "unknown". Published
/// BEFORE `CHIP_GEOM_COUNT` and cleared with it, so a poller tick can never
/// pair live chips with a stale or absent centre.
static HUD_CENTER_X: AtomicI32 = AtomicI32::new(0);
static HUD_CENTER_Y: AtomicI32 = AtomicI32::new(0);
static HUD_CENTER_OK: AtomicBool = AtomicBool::new(false);

/// Guard 4's dead-zone radius in physical px, derived from the snapshot.
static DEAD_ZONE_PHYS: AtomicI32 = AtomicI32::new(0);

/// The armed chip (index into the apps ring), -1 = none. Written by the
/// poller on change and consumed (reset to -1) by the hook on activation.
pub(crate) static ARMED_INDEX: AtomicI32 = AtomicI32::new(-1);

/// Set when Space+wheel fires (guard 6) or an activation is consumed:
/// pointer activation stands down for the REST of this hold. Cleared at the
/// next Space-down.
static HOLD_BLOCKED: AtomicBool = AtomicBool::new(false);

/// PAIRED CLICK SUPPRESSION. Gesture B swallows a WM_LBUTTONDOWN; letting
/// the matching WM_LBUTTONUP through would hand the app underneath an
/// unbalanced button state — a stuck drag, a phantom selection. Same shape
/// and same rule as `SPACE_INTERCEPTED` (hook/mod.rs): whoever eats the down
/// owes the up. Only the hook thread touches this, so plain load/store is
/// race-free.
static CLICK_EATEN: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Hook-path helpers — relaxed atomics ONLY (PROBLEM 58). No logging, no
// allocation, no locks, no win32k. Every one of these runs inside a LL hook
// callback.
// ---------------------------------------------------------------------------

/// WM_MOUSEMOVE while Space is held: remember where the cursor is. Three
/// relaxed stores; the third (the stamp) is what makes a stale position from
/// a previous hold distinguishable — without it, a hold with the mouse never
/// touched could dwell-arm on wherever the cursor happened to be left.
#[inline(always)]
pub(crate) fn note_cursor(x: i32, y: i32) {
    CURSOR_X.store(x, Ordering::Relaxed);
    CURSOR_Y.store(y, Ordering::Relaxed);
    CURSOR_STAMP.store(super::tick_count(), Ordering::Relaxed);
}

/// Space-down: a fresh hold must not inherit the last hold's armed chip or
/// its wheel-block. Two relaxed stores on the Space-down branch only.
#[inline(always)]
pub(crate) fn on_space_down() {
    ARMED_INDEX.store(-1, Ordering::Relaxed);
    HOLD_BLOCKED.store(false, Ordering::Relaxed);
}

/// Guard 6 — Space+wheel: disarm and stand down for the rest of this hold.
#[inline(always)]
pub(crate) fn block_for_hold() {
    ARMED_INDEX.store(-1, Ordering::Relaxed);
    HOLD_BLOCKED.store(true, Ordering::Relaxed);
}

/// If a chip is armed (and the feature is on), consume the arm and return
/// its key char. Called from the Space-up path (gesture A) and the
/// WM_LBUTTONDOWN path (gesture B). Consuming also blocks the rest of the
/// hold, so a click-then-release cannot activate twice.
#[inline(always)]
pub(crate) fn take_armed_key() -> Option<char> {
    if !super::POINTER_HUD_ACTIVATION.load(Ordering::Relaxed) {
        return None;
    }
    let i = ARMED_INDEX.load(Ordering::Relaxed);
    if i < 0 {
        return None;
    }
    let i = i as usize;
    ARMED_INDEX.store(-1, Ordering::Relaxed);
    HOLD_BLOCKED.store(true, Ordering::Relaxed);
    if i >= CHIP_KEY_COUNT.load(Ordering::Relaxed).min(MAX_CHIPS) {
        return None;
    }
    char::from_u32(CHIP_KEYS[i].load(Ordering::Relaxed)).filter(|c| c.is_ascii_lowercase())
}

/// Gesture B ate a WM_LBUTTONDOWN — owe the matching up.
#[inline(always)]
pub(crate) fn latch_click_eaten() {
    CLICK_EATEN.store(true, Ordering::Relaxed);
}

/// The matching WM_LBUTTONUP arrived: eat it too, once. One relaxed load on
/// every mouse-up system-wide; the store runs only when the latch was set.
#[inline(always)]
pub(crate) fn eat_click_up() -> bool {
    if CLICK_EATEN.load(Ordering::Relaxed) {
        CLICK_EATEN.store(false, Ordering::Relaxed);
        true
    } else {
        false
    }
}

/// The watchdog re-hooked after an eviction: every latch that a lost event
/// could have stranded is reset, exactly as it already resets the Space
/// latches beside this call.
pub(crate) fn reset_on_eviction() {
    ARMED_INDEX.store(-1, Ordering::Relaxed);
    HOLD_BLOCKED.store(false, Ordering::Relaxed);
    CLICK_EATEN.store(false, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Publishing — called from the engine thread (show) and the IPC thread
// (the page's publish). Logging is legal here.
// ---------------------------------------------------------------------------

/// One chip box from the overlay page, CSS px relative to the overlay
/// window's client area — the exact `overlay_shape` convention.
#[derive(serde::Deserialize, Clone, Copy)]
pub struct ChipRectIn {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Record which key each chip launches, in apps-ring order. Called by
/// `show_guide_hud` with the SAME `apps` list the page builds its chips from,
/// so index i here is chip i there by construction.
///
/// Geometry from the previous layout is invalidated FIRST: a poller tick
/// landing mid-publish must never pair new keys with old rects.
pub fn publish_keys(apps: &[(String, String)]) {
    CHIP_GEOM_COUNT.store(0, Ordering::Relaxed);
    let n = apps.len().min(MAX_CHIPS);
    for (i, (key, _)) in apps.iter().take(n).enumerate() {
        let ch = key
            .chars()
            .next()
            .map(|c| c.to_ascii_lowercase())
            .filter(|c| c.is_ascii_lowercase())
            .map(|c| c as u32)
            .unwrap_or(0);
        CHIP_KEYS[i].store(ch, Ordering::Relaxed);
    }
    CHIP_KEY_COUNT.store(n, Ordering::Relaxed);
}

/// CSS px (window-relative) → PHYSICAL screen px, floor/ceil OUTWARD — the
/// same convention `commands::apply_region` established (`(r.x * dpr).floor()`
/// / `ceil` on the far edge), so the hit-test rect always fully covers the
/// painted chip. `win_x`/`win_y` are the overlay window's ACTUAL read-back
/// position in physical px (the log shows asked (202,247) vs GOT (203,247) —
/// a 1px logical rounding artefact — so the requested position must never be
/// used here).
pub(crate) fn css_rect_to_phys(
    win_x: i32,
    win_y: i32,
    dpr: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> (i32, i32, i32, i32) {
    let x0 = win_x + (x * dpr).floor() as i32;
    let y0 = win_y + (y * dpr).floor() as i32;
    let x1 = win_x + ((x + w) * dpr).ceil() as i32;
    let y1 = win_y + ((y + h) * dpr).ceil() as i32;
    (x0, y0, x1, y1)
}

/// Store the page's chip boxes, converted to physical screen px, and derive
/// the two things guard 4 needs from them: the HUD centre and the dead-zone
/// radius. Count is zeroed while the table is rewritten so a concurrent
/// poller tick reads either the old snapshot or the new one, never a
/// half-written mix — and the centre/dead-zone are published BEFORE the
/// count for the same reason.
///
/// `win_w`/`win_h` are the overlay window's INNER size in physical px.
/// Zero on either axis means the centre cannot be trusted, and without a
/// centre there are no directions: the snapshot is still published (the key
/// table stays consistent) but `HUD_CENTER_OK` stays false and the poller
/// arms nothing. Fails toward a typed space, like every other unknown here.
pub fn publish_chips(
    win_x: i32,
    win_y: i32,
    win_w: u32,
    win_h: u32,
    chips: &[ChipRectIn],
    dpr: f64,
) {
    let dpr = if dpr.is_finite() && dpr > 0.0 { dpr } else { 1.0 };
    CHIP_GEOM_COUNT.store(0, Ordering::Relaxed);
    HUD_CENTER_OK.store(false, Ordering::Relaxed);
    let n = chips.len().min(MAX_CHIPS);
    let mut rects = [(0i32, 0i32, 0i32, 0i32); MAX_CHIPS];
    for (i, c) in chips.iter().take(n).enumerate() {
        let r = css_rect_to_phys(win_x, win_y, dpr, c.x, c.y, c.w, c.h);
        rects[i] = r;
        CHIP_RECTS[i * 4].store(r.0, Ordering::Relaxed);
        CHIP_RECTS[i * 4 + 1].store(r.1, Ordering::Relaxed);
        CHIP_RECTS[i * 4 + 2].store(r.2, Ordering::Relaxed);
        CHIP_RECTS[i * 4 + 3].store(r.3, Ordering::Relaxed);
    }
    let cx = win_x + (win_w / 2) as i32;
    let cy = win_y + (win_h / 2) as i32;
    let dead = dead_zone_radius(cx, cy, &rects[..n]);
    HUD_CENTER_X.store(cx, Ordering::Relaxed);
    HUD_CENTER_Y.store(cy, Ordering::Relaxed);
    DEAD_ZONE_PHYS.store(dead.round() as i32, Ordering::Relaxed);
    HUD_CENTER_OK.store(win_w > 0 && win_h > 0, Ordering::Relaxed);
    CHIP_GEOM_COUNT.store(n, Ordering::Relaxed);
    let keys = CHIP_KEY_COUNT.load(Ordering::Relaxed);
    if n != keys {
        // A rebuild race (the page re-published for a layout whose keys have
        // moved on, or vice versa). Arming is bounded by the SMALLER count,
        // so the mismatch cannot mis-fire — the next publish heals it.
        log::info!(
            "hud-pointer: {n} chip rect(s) published but {keys} key(s) known — arming is \
             bounded to the smaller until the next publish"
        );
    } else {
        // The dead-zone radius is in this line on purpose: it is the ONLY
        // number that decides whether a given hold could have armed anything,
        // and "it launched something I didn't pick" is unanswerable without
        // it. Centre + radius + the cursor position from the ARMED line are
        // enough to replay any report by hand.
        log::debug!(
            "hud-pointer: {n} chip rect(s) published at dpr {dpr}, centre ({cx},{cy}), \
             dead zone {dead:.0}px"
        );
    }
}

/// The HUD hid — nothing on screen means nothing to hit. Called from every
/// hide path (it is cheap enough to run on the typed-space path).
pub fn clear_chips() {
    CHIP_GEOM_COUNT.store(0, Ordering::Relaxed);
    CHIP_KEY_COUNT.store(0, Ordering::Relaxed);
    HUD_CENTER_OK.store(false, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Pure decision logic — everything below here is testable without a mouse.
// ---------------------------------------------------------------------------

/// A chip's own direction from the HUD centre, in radians, `atan2`'s range
/// `(-π, π]`. Screen Y grows DOWNWARD, so this angle runs clockwise from
/// "east" — which is irrelevant as long as the cursor's angle is measured the
/// same way, and it is (`sector_pick`). Centre is computed in f64: the
/// integer midpoint of a 61px-wide rect is half a pixel off, and half a pixel
/// at ring radius is ~0.07° — small, but there is no reason to spend it.
pub(crate) fn chip_angle(cx: i32, cy: i32, r: (i32, i32, i32, i32)) -> f64 {
    let ccx = (r.0 as f64 + r.2 as f64) / 2.0;
    let ccy = (r.1 as f64 + r.3 as f64) / 2.0;
    (ccy - cy as f64).atan2(ccx - cx as f64)
}

/// A chip's distance from the HUD centre, in physical px — the other half of
/// its polar coordinates (`chip_angle` is the first). Only the SCORING rule in
/// `sector_pick` needs it, and only because there can now be two app bands:
/// two chips at the same angle on different radii are indistinguishable to an
/// angle-only comparison. Same f64 centre as `chip_angle`, for the same reason.
pub(crate) fn chip_radius(cx: i32, cy: i32, r: (i32, i32, i32, i32)) -> f64 {
    let ccx = (r.0 as f64 + r.2 as f64) / 2.0;
    let ccy = (r.1 as f64 + r.3 as f64) / 2.0;
    ((ccx - cx as f64).powi(2) + (ccy - cy as f64).powi(2)).sqrt()
}

/// The unsigned angular distance between two directions, in `[0, π]`. This
/// is the one place wraparound is handled, and every sector decision goes
/// through it: an angle of `179°` and one of `-179°` are 2° apart, not 358°.
pub(crate) fn ang_dist(a: f64, b: f64) -> f64 {
    let two_pi = std::f64::consts::PI * 2.0;
    let mut d = (a - b).abs() % two_pi;
    if d > std::f64::consts::PI {
        d = two_pi - d;
    }
    d
}

/// Guard 4's dead zone: the largest circle around the HUD centre that touches
/// NO app chip — i.e. the apps ring's inscribed radius, measured to the
/// nearest chip EDGE (not centre), so it never overlaps a chip.
///
/// The ring is an ELLIPSE, so this radius is set by whichever chip comes
/// closest — in practice one on the short (vertical) axis. A circle is the
/// right shape anyway: it is the region where the cursor has no meaningful
/// direction, and that region is round regardless of how the chips are
/// arranged around it.
///
/// **This is derived from the APPS ring alone**, because that is all
/// `publishHudChips` sends (`.st-chip.ap`). When `hud_show_specials` is off
/// the inner ring is not drawn at all and the apps ring moves inward; the
/// derived radius follows it, which is the correct behaviour and not the
/// fallback path — the fallback fires only for an EMPTY snapshot, where
/// there is nothing to arm in the first place.
///
/// Floored at `DEAD_ZONE_MIN_PHYS_PX`, which is also the value for an empty
/// or degenerate snapshot. Erring large errs toward a typed space.
pub(crate) fn dead_zone_radius(cx: i32, cy: i32, rects: &[(i32, i32, i32, i32)]) -> f64 {
    let mut best = f64::INFINITY;
    for &(x0, y0, x1, y1) in rects {
        // Distance from the centre point to the rect: zero on an axis the
        // centre already lies within, otherwise the gap to the nearer edge.
        let dx = (x0 - cx).max(cx - x1).max(0) as f64;
        let dy = (y0 - cy).max(cy - y1).max(0) as f64;
        let d = (dx * dx + dy * dy).sqrt();
        if d < best {
            best = d;
        }
    }
    if !best.is_finite() {
        DEAD_ZONE_MIN_PHYS_PX
    } else {
        best.max(DEAD_ZONE_MIN_PHYS_PX)
    }
}

/// GUARD 4 — the directional hit-test that replaced containment (PROBLEM
/// 209). Which chip is the cursor POINTING at, from the HUD centre?
///
/// Two rules and nothing else:
///
///   * **Dead zone first.** Inside `dead_r` of the centre the answer is
///     `None`, always. There is no direction there worth acting on, and this
///     is the only region that can produce a "nothing armed" answer now that
///     containment is gone — it is what keeps a Space-hold with a twitching
///     mouse a typed space.
///
///   * **Least perpendicular offset from the aim ray wins.** For each chip,
///     with `d = ang_dist(aim, chipAngle)`:
///
///     ```text
///       if cos(d) <= 0 { skip }          // the chip is BEHIND the aim
///       score = chipRadius * sin(d)      // px offset from the ray
///     ```
///
///     Lowest score wins. `ang_dist` returns `[0, π]`, so `cos(d) <= 0` is
///     exactly `d >= π/2` and `sin(d)` is already the unsigned `|sin|` the
///     design handoff writes — which is also why wraparound at ±π is still
///     handled in the one place it has always been handled.
///
/// **WHY IT IS NO LONGER A PURE NEAREST-ANGLE SEARCH (1.0.89).** With TWO app
/// bands, two chips can sit at nearly the same angle on different radii and
/// tie under an angle-only comparison — the ring would pick by array order,
/// which is arbitrary. Perpendicular distance breaks that tie toward the
/// NEARER chip, which is the one the user is looking at. It is still purely
/// angle-driven: `score` depends on the chip's radius, never on the cursor's,
/// so pushing the cursor further out along the same bearing can never change
/// the choice. That property is what the owner's "it's 360 degrees, right?"
/// model rests on and it survives intact.
///
/// **AND WHY IT DOES NOT DISTURB A SINGLE BAND.** With every chip at one
/// radius `R`, `score = R * sin(d)` and `sin` is strictly increasing on
/// `[0, π/2)`, so ordering by score IS ordering by `d` — the same winner as
/// 1.0.88, and the same `d_nearest` fed to the hysteresis below.
/// `single_band_scoring_is_exactly_todays_nearest_angle` proves that by
/// sweeping every whole degree against a copy of the old algorithm rather than
/// by argument.
///
/// The one deliberate behaviour CHANGE is the skip: when every chip is more
/// than 90° away from the aim, this now returns `None` where 1.0.88 armed the
/// least-wrong chip behind the cursor. A ring of `n` evenly spread chips is
/// never more than `π/n` from the nearest, so a normal HUD cannot reach it;
/// a two-binding profile with both chips on one side can, and there the
/// honest answer to "point away from both" is nothing, which releases as a
/// typed space.
///
/// `armed` adds hysteresis, UNCHANGED and still angular: while a chip is armed
/// it keeps the sector until the cursor's direction is `HYSTERESIS_RAD` PAST
/// the midpoint toward the neighbour. Moving `h` past a midpoint widens the
/// gap between the two angular distances by `2h`, hence the factor of two.
/// Without this a cursor resting exactly on a boundary — which is where a user
/// aiming between two apps naturally leaves it — flickers between two chips at
/// 60Hz, emitting `hud-pointer` every tick and arming whichever one the
/// release lands on. Kept in the ANGULAR domain on purpose: expressing it in
/// score px would make the comparison `sin(d_armed) - sin(d_nearest) < 2h`,
/// which is not the same number as `d_armed - d_nearest < 2h` and would have
/// changed a threshold the owner has already hand-tested. An armed chip that
/// is itself behind the aim cannot be held.
pub(crate) fn sector_pick(
    rects: &[(i32, i32, i32, i32)],
    cx: i32,
    cy: i32,
    dead_r: f64,
    px: i32,
    py: i32,
    armed: Option<usize>,
) -> Option<usize> {
    if rects.is_empty() {
        return None;
    }
    let dx = (px - cx) as f64;
    let dy = (py - cy) as f64;
    if dx * dx + dy * dy < dead_r * dead_r {
        return None;
    }
    let theta = dy.atan2(dx);
    // (index, score in px, angular distance). The angle is carried alongside
    // the score because the hysteresis below is angular and must compare the
    // same quantity 1.0.88 compared.
    let mut best: Option<(usize, f64, f64)> = None;
    for (i, &r) in rects.iter().enumerate() {
        let d = ang_dist(theta, chip_angle(cx, cy, r));
        // cos(d) <= 0 — the chip is behind the direction the cursor points.
        // `ang_dist` is already folded into [0, π], so this is the whole test.
        if d >= std::f64::consts::FRAC_PI_2 {
            continue;
        }
        let score = chip_radius(cx, cy, r) * d.sin();
        if best.map_or(true, |(_, bs, _)| score < bs) {
            best = Some((i, score, d));
        }
    }
    // Every chip is behind the aim: nothing to arm. Erring toward "nothing"
    // is the same direction the dead zone errs in — toward a typed space.
    let (nearest, _, d_nearest) = best?;
    if let Some(a) = armed {
        // `a` can be out of range for one tick after a rebuild shrank the
        // ring; the nearest chip is then simply the right answer.
        if a != nearest && a < rects.len() {
            let d_armed = ang_dist(theta, chip_angle(cx, cy, rects[a]));
            // An armed chip that has fallen behind the aim is not a candidate
            // at all, so hysteresis must not resurrect it.
            if d_armed < std::f64::consts::FRAC_PI_2
                && d_armed - d_nearest < 2.0 * HYSTERESIS_RAD
            {
                return Some(a);
            }
        }
    }
    Some(nearest)
}

/// Everything one poller tick needs to know, gathered by the impure side.
pub(crate) struct TickIn<'a> {
    /// The user's setting (`POINTER_HUD_ACTIVATION`).
    pub enabled: bool,
    /// Space physically held (`MODIFIER_ACTIVE`).
    pub modifier_active: bool,
    /// Guard 1 (`guide_hud::is_visible()`).
    pub hud_visible: bool,
    /// Guard 6 / consumed activation (`HOLD_BLOCKED`).
    pub blocked: bool,
    /// `SPACE_DOWN_TS` — identifies WHICH hold this tick belongs to.
    pub hold_ts: u64,
    /// GetCursorPos latched by the poller when it first saw this hold —
    /// guard 2's reference point.
    pub hold_start_cursor: Option<(i32, i32)>,
    /// Cursor position, `None` until the mouse has moved during THIS hold
    /// (`CURSOR_STAMP >= hold_ts`).
    pub cursor: Option<(i32, i32)>,
    /// `ARMED_INDEX` read back as < 0 — a gesture consumed the arm under us.
    pub externally_disarmed: bool,
    /// The chip snapshot (physical px), already bounded by the smaller of the
    /// two counts. APPS RING ONLY — see the file header.
    pub chips: &'a [(i32, i32, i32, i32)],
    /// The HUD centre in physical px — the origin every direction is measured
    /// from. `None` when no snapshot has published one, and then nothing can
    /// arm at all.
    pub centre: Option<(i32, i32)>,
    /// Guard 4's dead-zone radius in physical px, derived at publish time.
    pub dead_r: f64,
    /// How much wall time this tick represents (drives dwell).
    pub tick_ms: u64,
}

/// What one tick decided. The impure side applies it to the shared atomics
/// via `apply_to` and emits `hud-pointer` on anything but `NoChange`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    NoChange,
    /// A gesture consumed the arm — resync the local state, emit null,
    /// touch nothing else.
    SyncLost,
    /// none → armed(i). `apply_to` claims `SPACE_ABORTED` via CAS; if the
    /// hold was already aborted (wheel/combo won a race) the arm is REFUSED.
    Arm(usize),
    /// armed(a) → armed(b) without passing through none. Only reachable if
    /// dwell can complete in a single tick; kept for completeness.
    Shift(usize),
    /// armed → none because the cursor came back INSIDE THE DEAD ZONE while
    /// the hold was live and visible: CLEAR `SPACE_ABORTED` so release types
    /// a space exactly as today. THE bug to get right (see file header).
    /// Before PROBLEM 209 this meant "drifted out of every halo"; the dead
    /// zone is the only way back to "nothing armed" now.
    DisarmClear,
    /// armed → none for any other reason (HUD hid, hold ended, wheel
    /// blocked, setting turned off): the abort flag belongs to someone else
    /// now — leave it alone.
    DisarmKeep,
}

/// Per-hold pointer state. Pure: no atomics, no Win32 — the tests drive it
/// tick by tick.
pub(crate) struct HoldTracker {
    hold_ts: u64,
    start: Option<(i32, i32)>,
    traveled: bool,
    dwell_on: Option<usize>,
    dwell_ms: u64,
    armed: Option<usize>,
}

impl HoldTracker {
    pub fn new() -> Self {
        HoldTracker {
            hold_ts: 0,
            start: None,
            traveled: false,
            dwell_on: None,
            dwell_ms: 0,
            armed: None,
        }
    }

    /// The CAS in `apply_to` refused the arm (the hold was already aborted
    /// by the wheel or a combo): roll the local state back.
    pub fn cancel_arm(&mut self) {
        self.armed = None;
    }

    pub fn tick(&mut self, i: &TickIn) -> Verdict {
        if i.hold_ts != self.hold_ts {
            // A new hold: every per-hold latch resets. The hook already reset
            // ARMED_INDEX at Space-down, so an armed chip cannot carry over.
            *self = HoldTracker {
                hold_ts: i.hold_ts,
                start: i.hold_start_cursor,
                traveled: false,
                dwell_on: None,
                dwell_ms: 0,
                armed: None,
            };
        }

        // A gesture fired and consumed the arm between our ticks.
        if self.armed.is_some() && i.externally_disarmed {
            self.armed = None;
            return Verdict::SyncLost;
        }

        let live = i.enabled && i.modifier_active && i.hud_visible && !i.blocked;
        let hit: Option<usize> = if !live {
            None
        } else {
            match i.cursor {
                None => None,
                Some(cur) => {
                    // Guard 2 — minimum travel from the Space-down position.
                    // If GetCursorPos failed at hold start, the first observed
                    // position becomes the reference (fails toward NOT arming).
                    if self.start.is_none() {
                        self.start = Some(cur);
                    }
                    if !self.traveled {
                        let s = self.start.unwrap();
                        let dx = (cur.0 - s.0) as i64;
                        let dy = (cur.1 - s.1) as i64;
                        if dx * dx + dy * dy >= MIN_TRAVEL_PHYS_PX * MIN_TRAVEL_PHYS_PX {
                            self.traveled = true;
                        }
                    }
                    if !self.traveled {
                        None
                    } else {
                        // Guard 4 — direction, not containment (PROBLEM 209).
                        // No centre means no directions: fail toward NOT
                        // arming, same as a failed GetCursorPos above.
                        match i.centre {
                            None => None,
                            Some((cx, cy)) => sector_pick(
                                i.chips, cx, cy, i.dead_r, cur.0, cur.1, self.armed,
                            ),
                        }
                    }
                }
            }
        };

        // Guard 3 — dwell bookkeeping on whatever the cursor points at now.
        match hit {
            Some(c) if self.dwell_on == Some(c) => {
                self.dwell_ms = self.dwell_ms.saturating_add(i.tick_ms);
            }
            Some(c) => {
                self.dwell_on = Some(c);
                self.dwell_ms = i.tick_ms;
            }
            None => {
                self.dwell_on = None;
                self.dwell_ms = 0;
            }
        }

        let desired = match hit {
            None => None,
            // Already armed on this chip: stay armed while the cursor keeps
            // pointing at it (hysteresis is inside `sector_pick`).
            Some(c) if self.armed == Some(c) => Some(c),
            Some(c) if self.dwell_ms >= DWELL_MS => Some(c),
            Some(_) => None, // pointing at a NEW sector, dwell still running
        };

        match (self.armed, desired) {
            (None, None) => Verdict::NoChange,
            (Some(a), Some(d)) if a == d => Verdict::NoChange,
            (None, Some(d)) => {
                self.armed = Some(d);
                Verdict::Arm(d)
            }
            (Some(_), Some(d)) => {
                self.armed = Some(d);
                Verdict::Shift(d)
            }
            (Some(_), None) => {
                self.armed = None;
                if live {
                    Verdict::DisarmClear
                } else {
                    Verdict::DisarmKeep
                }
            }
        }
    }
}

/// Apply a verdict to the shared flags. Parameterised over the two atomics so
/// the tests can use their own — the production caller passes
/// `hook::SPACE_ABORTED` and `ARMED_INDEX`.
///
/// Returns `(what to emit, arm_refused)`. `Some(Some(i))` = arm the chip,
/// `Some(None)` = clear the highlight, `None` = emit nothing.
///
/// ORDERING IS LOAD-BEARING, both ways, against a concurrent Space-up on the
/// hook thread:
///   * Arm: claim `SPACE_ABORTED` FIRST, then publish `ARMED_INDEX`. A
///     release landing between the two sees aborted-but-not-armed: no space,
///     no launch — a lost space in a microsecond window. The other order
///     would give armed-but-not-aborted: a LAUNCH plus a typed space.
///   * DisarmClear: retract `ARMED_INDEX` FIRST, then clear `SPACE_ABORTED`.
///     A release in between sees aborted-but-not-armed again (no space, no
///     launch). The other order would type a space AND launch.
/// In both races the failure chosen is the quiet one.
pub(crate) fn apply_to(
    v: Verdict,
    aborted: &AtomicBool,
    armed_idx: &AtomicI32,
) -> (Option<Option<usize>>, bool) {
    match v {
        Verdict::NoChange => (None, false),
        Verdict::SyncLost => (Some(None), false),
        Verdict::Arm(i) => {
            match aborted.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => {
                    armed_idx.store(i as i32, Ordering::SeqCst);
                    (Some(Some(i)), false)
                }
                // The hold was already aborted (wheel or combo won a race
                // the guards usually prevent): arming now would let a
                // release launch on top of that gesture. Refuse.
                Err(_) => (None, true),
            }
        }
        Verdict::Shift(i) => {
            armed_idx.store(i as i32, Ordering::SeqCst);
            (Some(Some(i)), false)
        }
        Verdict::DisarmClear => {
            armed_idx.store(-1, Ordering::SeqCst);
            aborted.store(false, Ordering::SeqCst);
            (Some(None), false)
        }
        Verdict::DisarmKeep => {
            armed_idx.store(-1, Ordering::SeqCst);
            (Some(None), false)
        }
    }
}

// ---------------------------------------------------------------------------
// The poller thread
// ---------------------------------------------------------------------------

/// Spawn `st-hud-pointer`. Same shape as `exclusions::start_exclusion_watcher`
/// and for the same reasons: named thread, `catch_unwind` around the Win32
/// probe, non-panicking spawn (PROBLEM 124), one-line logs on CHANGE only.
pub fn start_pointer_watcher() {
    std::thread::Builder::new()
        .name("st-hud-pointer".into())
        .spawn(move || {
            log::debug!("hud-pointer watcher thread started");
            #[cfg(windows)]
            {
                let mut tracker = HoldTracker::new();
                let mut seen_hold: u64 = u64::MAX;
                let mut hold_start_cursor: Option<(i32, i32)> = None;
                let mut chips = [(0i32, 0i32, 0i32, 0i32); MAX_CHIPS];
                // PROBLEM 261 — when the fallback reaper last probed the
                // foreground. `0` is "never", and `tick_count()` never returns
                // 0 in practice, so the first probe happens on the first tick
                // of the first fallback hold.
                let mut last_fg_probe: u64 = 0;
                loop {
                    let enabled = super::POINTER_HUD_ACTIVATION.load(Ordering::Relaxed);
                    // PROBLEM 261 — BOTH witnesses. A fallback hold ticks at
                    // the held cadence too: it has a ring on screen and a
                    // cursor to follow, so polling it at the idle 40 ms rate
                    // would make aiming inside the dashboard visibly coarser
                    // than aiming anywhere else.
                    let held = super::any_hold_latched();
                    let tick_ms = if !enabled {
                        250
                    } else if held {
                        TICK_HELD_MS
                    } else {
                        TICK_IDLE_MS
                    };
                    std::thread::sleep(std::time::Duration::from_millis(tick_ms));

                    // PROBLEM 218 — the stale-hold reaper, and it runs BEFORE
                    // the `enabled` bail-out below on purpose.
                    //
                    // What it repairs is not a pointer-activation feature, it
                    // is a Guide HUD stranded on screen by a Space-UP the hook
                    // never received. That happens whether or not the user has
                    // pointer activation switched on, so gating it on
                    // `enabled` would leave the stuck HUD — the failure the
                    // owner actually reported — unfixed for anyone with the
                    // setting off.
                    //
                    // THIS THREAD, because it is not the hook thread. The whole
                    // point is to still be running when the hook thread's own
                    // hooks have been evicted or its pump is starved, which is
                    // precisely when the Space-UP goes missing. Two relaxed
                    // loads on a tick that already runs; `reap_stale_hold`
                    // does nothing further unless the hold is provably dead.
                    if super::reap_stale_hold() {
                        // The hold is gone, so this tracker's per-hold state
                        // is describing something that no longer exists.
                        tracker.cancel_arm();
                    }

                    // PROBLEM 261 — and the FALLBACK's reaper, immediately
                    // beside it and above the same `enabled` bail-out, for
                    // exactly the reason written above: what it repairs is a
                    // ring stranded on screen, which happens whether or not
                    // the user has pointer activation switched on.
                    //
                    // The two are separate functions on purpose. This one's
                    // liveness signal is the FOREGROUND, because guard 1
                    // admitted the hold on that condition and it is observable
                    // from outside the page; `reap_stale_hold`'s is the
                    // keyboard hook's auto-repeat, which a fallback hold
                    // produces none of. Merging them would mean one of the two
                    // shapes losing its evidence.
                    let now = super::tick_count();
                    let probe_fg = now.saturating_sub(last_fg_probe)
                        >= super::OWN_HOLD_FG_CHECK_MS;
                    if probe_fg {
                        last_fg_probe = now;
                    }
                    if super::reap_own_window_hold(probe_fg) {
                        tracker.cancel_arm();
                    }

                    // PROBLEM 263 — and the MIDDLE BUTTON's reaper, third in
                    // the row, above the same `enabled` bail-out and for the
                    // same reason: what it repairs is a ring stranded on
                    // screen, which happens whether or not the user has pointer
                    // activation switched on.
                    //
                    // A THIRD separate function, not a merge, because its
                    // liveness signal is a third thing again: this one asks
                    // whether the MOUSE callback is still being called at all,
                    // because the WM_MBUTTONUP that ends the hold is delivered
                    // by that hook and by nothing else. `reap_stale_hold` reads
                    // keyboard auto-repeat; `reap_own_window_hold` reads the
                    // foreground window. Merging any two of them means one
                    // shape losing its evidence.
                    //
                    // It shares `probe_fg`'s throttle rather than adding a
                    // fourth clock: both are 250 ms, both guard a handful of
                    // Win32 calls that are free four times a second and are not
                    // free at 62 Hz.
                    if super::reap_middle_hold(probe_fg) {
                        tracker.cancel_arm();
                    }

                    if !enabled && tracker.armed.is_none() {
                        continue;
                    }

                    // PROBLEM 261 — the hold's identity across both witnesses.
                    let hold_ts = super::current_hold_ts();
                    if hold_ts != seen_hold {
                        seen_hold = hold_ts;
                        // Guard 2's reference point. A failing/panicking
                        // GetCursorPos fails toward NOT arming.
                        hold_start_cursor =
                            std::panic::catch_unwind(cursor_pos).ok().flatten();
                        // Belt-and-braces against the one µs-scale race the
                        // atomics allow: an Arm applied from the previous
                        // hold's snapshot landing AFTER the hook's Space-down
                        // reset. The hook already reset ARMED_INDEX; doing it
                        // again here wipes any such straggler. SPACE_ABORTED
                        // is deliberately NOT touched — an early wheel abort
                        // in the new hold must survive.
                        ARMED_INDEX.store(-1, Ordering::Relaxed);
                    }

                    let stamp = CURSOR_STAMP.load(Ordering::Relaxed);
                    let cursor = if stamp >= hold_ts {
                        Some((
                            CURSOR_X.load(Ordering::Relaxed),
                            CURSOR_Y.load(Ordering::Relaxed),
                        ))
                    } else {
                        None
                    };

                    let n = CHIP_GEOM_COUNT
                        .load(Ordering::Relaxed)
                        .min(CHIP_KEY_COUNT.load(Ordering::Relaxed))
                        .min(MAX_CHIPS);
                    for (i, slot) in chips.iter_mut().take(n).enumerate() {
                        *slot = (
                            CHIP_RECTS[i * 4].load(Ordering::Relaxed),
                            CHIP_RECTS[i * 4 + 1].load(Ordering::Relaxed),
                            CHIP_RECTS[i * 4 + 2].load(Ordering::Relaxed),
                            CHIP_RECTS[i * 4 + 3].load(Ordering::Relaxed),
                        );
                    }

                    // Guard 4's two derived numbers, read AFTER the count so
                    // they belong to the snapshot just copied out.
                    let centre = if HUD_CENTER_OK.load(Ordering::Relaxed) {
                        Some((
                            HUD_CENTER_X.load(Ordering::Relaxed),
                            HUD_CENTER_Y.load(Ordering::Relaxed),
                        ))
                    } else {
                        None
                    };
                    let dead_r = DEAD_ZONE_PHYS.load(Ordering::Relaxed) as f64;

                    let input = TickIn {
                        enabled,
                        // PROBLEM 261 — `live` in `HoldTracker::tick` is
                        // `enabled && modifier_active && hud_visible &&
                        // !blocked`. Reading `MODIFIER_ACTIVE` alone here is
                        // the third of the three gates that shut pointer
                        // activation out of a fallback hold; the field keeps
                        // its name because its MEANING is unchanged — "a Space
                        // is held right now" — only the set of witnesses that
                        // can say so has grown.
                        modifier_active: super::any_hold_latched(),
                        // Guard 1 AND guard 5: `HUD_VISIBLE` is published even
                        // when OVERLAY_DISABLED (click-through failed at
                        // startup and the window is never shown), and a page
                        // in a hidden window can still lay out and publish
                        // chips. Arming against a ring nobody can see is a
                        // lottery, so a disabled overlay disables this too.
                        hud_visible: crate::guide_hud::is_visible()
                            && !crate::guide_hud::OVERLAY_DISABLED.load(Ordering::Relaxed),
                        blocked: HOLD_BLOCKED.load(Ordering::Relaxed),
                        hold_ts,
                        hold_start_cursor,
                        cursor,
                        externally_disarmed: ARMED_INDEX.load(Ordering::Relaxed) < 0,
                        chips: &chips[..n],
                        centre,
                        dead_r,
                        tick_ms,
                    };
                    let verdict = tracker.tick(&input);
                    let (emit, refused) =
                        apply_to(verdict, &super::SPACE_ABORTED, &ARMED_INDEX);
                    if refused {
                        tracker.cancel_arm();
                    }
                    if let Some(index) = emit {
                        // ON CHANGE ONLY, by construction: verdicts other
                        // than NoChange are transitions. A typical hold emits
                        // a handful of these, not hundreds.
                        match index {
                            Some(i) => log::info!(
                                "hud-pointer: ARMED chip {i} (key '{}') — release or click \
                                 launches it",
                                char::from_u32(CHIP_KEYS[i].load(Ordering::Relaxed))
                                    .unwrap_or('?')
                            ),
                            None => log::debug!("hud-pointer: disarmed"),
                        }
                        emit_pointer(index);
                    }
                }
            }
        })
        .map(|_| ())
        // PROBLEM 124 — never `.expect()` a spawn during setup.
        .unwrap_or_else(|e| {
            log::error!(
                "hud-pointer: could not spawn the watcher thread ({e}) — continuing WITHOUT \
                 pointer activation. Keyboard shortcuts are unaffected."
            );
        });
}

/// Emit the armed index to the overlay page's highlight (guard 5). Global
/// `emit` + the single listener in `initToastListener` — the only event
/// arrangement that has ever delivered in this app.
fn emit_pointer(index: Option<usize>) {
    #[derive(serde::Serialize, Clone)]
    struct HudPointer {
        index: Option<u32>,
    }
    let Some(handle) = crate::guide_hud::app_handle() else { return };
    use tauri::Emitter;
    if let Err(e) = handle.emit(
        "hud-pointer",
        HudPointer {
            index: index.map(|i| i as u32),
        },
    ) {
        // Guard 5's failure mode: if the highlight cannot be delivered the
        // user cannot see what is armed. Say so — this is the line to grep
        // when "it launched something I didn't pick" is reported.
        log::warn!("hud-pointer: emit failed ({e}) — the armed highlight may be missing");
    }
}

/// The cursor in physical screen px, from the poller thread (never the hook
/// callback — GetCursorPos is a win32k call, PROBLEM 58/134's territory).
#[cfg(windows)]
fn cursor_pos() -> Option<(i32, i32)> {
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut pt = windows::Win32::Foundation::POINT::default();
    unsafe { GetCursorPos(&mut pt).ok().map(|_| (pt.x, pt.y)) }
}

// ---------------------------------------------------------------------------
// Tests — house rule: pure logic, especially the branches a user only
// reaches after something has already gone sideways.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    // -- Test geometry helpers ---------------------------------------------

    const DEG: f64 = std::f64::consts::PI / 180.0;

    /// A 60x30 chip whose CENTRE sits at `(r, deg)` polar from `(cx, cy)`.
    /// Built from the centre outward so `chip_angle` reads back exactly the
    /// angle asked for (integer rounding of the centre, not of an edge).
    fn chip_at(cx: i32, cy: i32, r: f64, deg: f64) -> (i32, i32, i32, i32) {
        let x = cx + (r * (deg * DEG).cos()).round() as i32;
        let y = cy + (r * (deg * DEG).sin()).round() as i32;
        (x - 30, y - 15, x + 30, y + 15)
    }

    /// A cursor point at `(r, deg)` polar from `(cx, cy)`.
    fn at(cx: i32, cy: i32, r: f64, deg: f64) -> (i32, i32) {
        (
            cx + (r * (deg * DEG).cos()).round() as i32,
            cy + (r * (deg * DEG).sin()).round() as i32,
        )
    }

    // -- Guard 4a: sector assignment, including wraparound at ±π -----------

    #[test]
    fn each_chip_owns_the_directions_nearest_its_own_angle() {
        // Four chips at the compass points of a ring; the cursor is nowhere
        // near any of them — that is the entire point of PROBLEM 209.
        let (cx, cy) = (1000, 1000);
        let chips = [
            chip_at(cx, cy, 400.0, 0.0),    // 0: east
            chip_at(cx, cy, 400.0, 90.0),   // 1: south (screen Y grows down)
            chip_at(cx, cy, 400.0, 180.0),  // 2: west
            chip_at(cx, cy, 400.0, -90.0),  // 3: north
        ];
        let dead = 140.0;
        let pick = |deg: f64, r: f64| {
            let (px, py) = at(cx, cy, r, deg);
            sector_pick(&chips, cx, cy, dead, px, py, None)
        };
        // Dead centre of each sector, at a radius NOWHERE NEAR the chips —
        // 160px out of a 400px ring. Containment would have said None here.
        assert_eq!(pick(0.0, 160.0), Some(0));
        assert_eq!(pick(90.0, 160.0), Some(1));
        assert_eq!(pick(180.0, 160.0), Some(2));
        assert_eq!(pick(-90.0, 160.0), Some(3));
        // And far BEYOND the ring: a direction is a direction.
        assert_eq!(pick(0.0, 4000.0), Some(0));
        assert_eq!(pick(-90.0, 4000.0), Some(3));
        // Just inside each boundary (midpoints at ±45°, ±135°).
        assert_eq!(pick(44.0, 300.0), Some(0));
        assert_eq!(pick(46.0, 300.0), Some(1));
        assert_eq!(pick(134.0, 300.0), Some(1));
        assert_eq!(pick(136.0, 300.0), Some(2));
        assert_eq!(pick(-44.0, 300.0), Some(0));
        assert_eq!(pick(-46.0, 300.0), Some(3));
    }

    #[test]
    fn sectors_wrap_correctly_across_the_plus_minus_pi_seam() {
        // atan2's discontinuity. Two chips STRADDLING it: 170° and -170° are
        // 20° apart, not 340°, and a cursor at 175° must pick the 170° chip
        // even though the raw numbers differ by 5 vs 345.
        let (cx, cy) = (0, 0);
        let chips = [
            chip_at(cx, cy, 400.0, 170.0),  // 0
            chip_at(cx, cy, 400.0, -170.0), // 1
            chip_at(cx, cy, 400.0, 0.0),    // 2, the far side
        ];
        let dead = 140.0;
        let pick = |deg: f64| {
            let (px, py) = at(cx, cy, 300.0, deg);
            sector_pick(&chips, cx, cy, dead, px, py, None)
        };
        assert_eq!(pick(170.0), Some(0));
        assert_eq!(pick(175.0), Some(0), "5° from chip 0, 15° from chip 1");
        assert_eq!(pick(179.0), Some(0), "9° vs 11° — still chip 0");
        assert_eq!(pick(-179.0), Some(1), "11° vs 9° — the seam is not a wall");
        assert_eq!(pick(-175.0), Some(1));
        assert_eq!(pick(-170.0), Some(1));
        // ang_dist itself, the one function that owns wraparound.
        assert!((ang_dist(179.0 * DEG, -179.0 * DEG) - 2.0 * DEG).abs() < 1e-9);
        assert!((ang_dist(-3.0 * DEG, 3.0 * DEG) - 6.0 * DEG).abs() < 1e-9);
        assert!((ang_dist(0.0, std::f64::consts::PI) - std::f64::consts::PI).abs() < 1e-9);
    }

    // -- Guard 4a': the scoring change, and the proof it changed nothing ---

    /// 1.0.88's rule, copied verbatim so the new one can be compared against
    /// it rather than against an argument. Deliberately NOT refactored to
    /// share code with `sector_pick`: a reference implementation that calls
    /// the thing it is checking proves nothing.
    fn legacy_nearest_angle(
        rects: &[(i32, i32, i32, i32)],
        cx: i32,
        cy: i32,
        dead_r: f64,
        px: i32,
        py: i32,
        armed: Option<usize>,
    ) -> Option<usize> {
        if rects.is_empty() {
            return None;
        }
        let dx = (px - cx) as f64;
        let dy = (py - cy) as f64;
        if dx * dx + dy * dy < dead_r * dead_r {
            return None;
        }
        let theta = dy.atan2(dx);
        let mut best: Option<(usize, f64)> = None;
        for (i, &r) in rects.iter().enumerate() {
            let d = ang_dist(theta, chip_angle(cx, cy, r));
            if best.map_or(true, |(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        let (nearest, d_nearest) = best?;
        if let Some(a) = armed {
            if a != nearest && a < rects.len() {
                let d_armed = ang_dist(theta, chip_angle(cx, cy, rects[a]));
                if d_armed - d_nearest < 2.0 * HYSTERESIS_RAD {
                    return Some(a);
                }
            }
        }
        Some(nearest)
    }

    /// THE NO-REGRESSION PROOF, and it is a measurement, not an argument.
    ///
    /// The owner hand-tested 1.0.88 on 2026-08-27 and the log shows a clean
    /// sweep — `ARMED chip 10 → 11 → 18 → 19`, no oscillation. That is a
    /// SINGLE-band ring, which is what every build so far has drawn, so the
    /// new scoring must not move a single pick there.
    ///
    /// It cannot, and here is why: with all chips at one radius `R`,
    /// `score = R * sin(d)`, and `sin` is strictly increasing on `[0, π/2)`,
    /// so ordering by score is ordering by `d`. The only chips excluded are
    /// those at `d >= π/2`, and one of those can only WIN if every chip is
    /// behind the aim — the one case the two rules deliberately disagree on.
    ///
    /// So the assertion is exact: for every whole degree of cursor bearing, on
    /// rings of 2/4/13/26 chips, unarmed and armed on every index, the two
    /// implementations agree — with exactly two carve-outs, both of them the
    /// "behind the aim" rule and neither of them the SELECTION rule:
    ///
    ///   * every chip more than 90° from the aim → the new rule returns
    ///     `None` where the legacy one armed the least-wrong chip behind the
    ///     cursor. Unreachable on an evenly spread ring (see the `skips`
    ///     assertion at the end) and covered by
    ///     `a_chip_behind_the_aim_is_never_picked`.
    ///   * the ARMED chip is behind the aim while some other chip is in front
    ///     → legacy hysteresis could pin the chip the user has just pointed
    ///     away from; the new rule falls to the nearest chip in front. Counted
    ///     as `armed_behind` below rather than waved away, because it is a
    ///     real difference and it should be visible in this file.
    #[test]
    fn single_band_scoring_is_exactly_todays_nearest_angle() {
        let (cx, cy) = (1000, 1000);
        let dead = 140.0;
        let mut compared = 0usize;
        let mut skips = 0usize;
        let mut armed_behind = 0usize;

        for n in [2usize, 4, 13, 26] {
            // An even ring, plus a deliberate rotation so the chips never land
            // on the same round numbers the cursor sweep uses.
            let chips: Vec<_> = (0..n)
                .map(|k| chip_at(cx, cy, 400.0, 7.5 + 360.0 * k as f64 / n as f64))
                .collect();

            let armed_cases: Vec<Option<usize>> =
                std::iter::once(None).chain((0..n).map(Some)).collect();

            for deg in 0..360 {
                // 300px out: past the dead zone, nowhere near the 400px ring,
                // which is exactly the regime PROBLEM 209 exists for.
                let (px, py) = at(cx, cy, 300.0, deg as f64);
                for &armed in &armed_cases {
                    let old = legacy_nearest_angle(&chips, cx, cy, dead, px, py, armed);
                    let new = sector_pick(&chips, cx, cy, dead, px, py, armed);

                    let theta = ((py - cy) as f64).atan2((px - cx) as f64);
                    let d_of = |i: usize| ang_dist(theta, chip_angle(cx, cy, chips[i]));
                    let any_in_front =
                        (0..n).any(|i| d_of(i) < std::f64::consts::FRAC_PI_2);
                    let armed_is_behind = armed
                        .is_some_and(|a| d_of(a) >= std::f64::consts::FRAC_PI_2);

                    if !any_in_front {
                        assert_eq!(
                            new, None,
                            "n={n} deg={deg} armed={armed:?}: every chip is behind \
                             the aim, so nothing may arm"
                        );
                        skips += 1;
                    } else if armed_is_behind && old == armed {
                        // The second carve-out, and it is narrow ON PURPOSE:
                        // it fires only where legacy hysteresis ACTUALLY
                        // pinned a chip the user has pointed away from. An
                        // armed chip that is behind but was not held changes
                        // nothing, and is compared like everything else.
                        assert_ne!(new, armed, "n={n} deg={deg}: a chip behind the aim was held");
                        assert_eq!(
                            new,
                            sector_pick(&chips, cx, cy, dead, px, py, None),
                            "n={n} deg={deg}: dropping a behind-the-aim armed chip must \
                             fall through to the plain pick"
                        );
                        armed_behind += 1;
                    } else {
                        assert_eq!(
                            new, old,
                            "n={n} deg={deg} armed={armed:?}: single-band scoring \
                             must reproduce 1.0.88's nearest-angle pick exactly"
                        );
                        compared += 1;
                    }
                }
            }
        }

        // A test that silently compared nothing would pass. It must not be
        // able to.
        assert!(compared > 10_000, "the sweep barely ran: {compared} comparisons");
        // An evenly spread ring of n chips is never more than 180/n degrees
        // from its nearest, so "every chip behind the aim" is unreachable
        // here — the case exists only for a lopsided profile, which
        // `a_chip_behind_the_aim_is_never_picked` builds on purpose.
        assert_eq!(
            skips, 0,
            "an evenly spread ring can never put every chip behind the aim — \
             {skips} says the geometry drifted"
        );
        // The armed-chip carve-out IS reachable on a 2-chip ring (the armed
        // chip can sit 92° off the aim while its partner sits 88° off), and
        // stating the count keeps it from quietly becoming the whole test.
        assert!(
            armed_behind < compared / 20,
            "the carve-out swallowed the comparison: {armed_behind} vs {compared}"
        );
    }

    /// The one deliberate behaviour change: a chip BEHIND the cursor's
    /// direction is not a candidate, however little competition it has.
    ///
    /// Reachable in real life only from a lopsided profile — two bindings whose
    /// chips both sit on one side of the ring — and the honest answer to
    /// "point away from both" is nothing, which releases as a typed space.
    /// 1.0.88 would have armed the least-wrong chip behind the cursor.
    #[test]
    fn a_chip_behind_the_aim_is_never_picked() {
        let (cx, cy) = (0, 0);
        // Both chips on the east side, 60° apart.
        let chips = [chip_at(cx, cy, 400.0, -30.0), chip_at(cx, cy, 400.0, 30.0)];
        let dead = 140.0;
        let pick = |deg: f64, armed: Option<usize>| {
            let (px, py) = at(cx, cy, 300.0, deg);
            sector_pick(&chips, cx, cy, dead, px, py, armed)
        };

        // In front: unchanged behaviour.
        assert_eq!(pick(-30.0, None), Some(0));
        assert_eq!(pick(30.0, None), Some(1));
        assert_eq!(pick(0.0, None), Some(0), "a tie goes to the lower index, as before");
        // 89.9° from chip 1 — still just in front.
        assert_eq!(pick(119.0, None), Some(1));
        // 90° and past: BOTH chips are behind the aim now.
        assert_eq!(pick(121.0, None), None, "91° from the nearer chip");
        assert_eq!(pick(180.0, None), None, "pointing due west, away from both");
        assert_eq!(pick(-121.0, None), None);
        // And hysteresis cannot resurrect an armed chip that has fallen
        // behind — the same rule the dead zone already enforces.
        assert_eq!(pick(180.0, Some(0)), None);
        assert_eq!(pick(180.0, Some(1)), None);
    }

    /// THE REASON THE RULE CHANGED, as a measurement rather than a claim.
    ///
    /// Two bands. The OUTER chip is 2° off the aim, the INNER one 3° off — so
    /// nearest-angle takes the outer one, by a margin of one degree that means
    /// nothing to a user. In px off the aim ray the outer chip is 18.0 away
    /// and the inner one 15.7: the inner chip is the one the cursor is
    /// actually reaching past, and that is what the new rule returns.
    ///
    /// This is the case a single band cannot produce, which is why the change
    /// arrives with `hud_band_count` and not before it.
    #[test]
    fn two_bands_resolve_to_the_chip_nearer_the_aim_ray_not_the_nearer_angle() {
        let (cx, cy) = (0, 0);
        let chips = [
            chip_at(cx, cy, 520.0, 2.0), // 0 — outer band, 2° off
            chip_at(cx, cy, 300.0, 3.0), // 1 — inner band, 3° off
        ];
        let dead = 140.0;
        let (px, py) = at(cx, cy, 600.0, 0.0);

        assert_eq!(
            legacy_nearest_angle(&chips, cx, cy, dead, px, py, None),
            Some(0),
            "1.0.88 compared angles alone and took the outer chip",
        );
        assert_eq!(
            sector_pick(&chips, cx, cy, dead, px, py, None),
            Some(1),
            "perpendicular distance takes the inner chip — 15.7px off the ray \
             against the outer chip's 18.0px",
        );

        // The two scores, spelled out, so a future change that moves either
        // one shows up here as an arithmetic difference and not a mystery.
        let s_outer = chip_radius(cx, cy, chips[0]) * ang_dist(0.0, chip_angle(cx, cy, chips[0])).sin();
        let s_inner = chip_radius(cx, cy, chips[1]) * ang_dist(0.0, chip_angle(cx, cy, chips[1])).sin();
        assert!((s_outer - 18.0).abs() < 0.5, "outer score {s_outer}");
        assert!((s_inner - 16.0).abs() < 0.5, "inner score {s_inner}");
        assert!(s_inner < s_outer);
    }

    /// And the property the owner's model depends on: reaching FURTHER OUT
    /// along one bearing must never change the answer. The score uses the
    /// CHIP's radius, never the cursor's, so distance is irrelevant by
    /// construction — asserted here so nobody "improves" it into a
    /// cursor-to-chip distance later.
    #[test]
    fn pushing_the_cursor_further_out_never_changes_the_pick() {
        let (cx, cy) = (0, 0);
        let chips = [
            chip_at(cx, cy, 520.0, 40.0),
            chip_at(cx, cy, 300.0, 40.0),
            chip_at(cx, cy, 520.0, -80.0),
            chip_at(cx, cy, 300.0, 150.0),
        ];
        let dead = 140.0;
        // Bearings deliberately kept CLEAR of the chips' own angles. A ray
        // that passes through a chip's centre scores 0 for it, and whether it
        // does is decided by the integer rounding of the cursor position, not
        // by the rule — that ambiguity is real but it is sub-pixel, and
        // testing it would be testing `round()`.
        for deg in [0.0, 20.0, 60.0, 110.0, -30.0, -140.0, -179.0] {
            let (px0, py0) = at(cx, cy, 200.0, deg);
            let first = sector_pick(&chips, cx, cy, dead, px0, py0, None);
            for r in [260.0, 400.0, 900.0, 4000.0] {
                let (px, py) = at(cx, cy, r, deg);
                assert_eq!(
                    sector_pick(&chips, cx, cy, dead, px, py, None),
                    first,
                    "bearing {deg}° must give one answer at every radius"
                );
            }
        }
    }

    // -- Guard 4b: angular hysteresis --------------------------------------

    #[test]
    fn hysteresis_holds_the_armed_chip_until_three_degrees_past_the_boundary() {
        // Two neighbours 30° apart → boundary at 15°.
        let (cx, cy) = (0, 0);
        let chips = [chip_at(cx, cy, 400.0, 0.0), chip_at(cx, cy, 400.0, 30.0)];
        let dead = 140.0;
        let pick = |deg: f64, armed: Option<usize>| {
            let (px, py) = at(cx, cy, 300.0, deg);
            sector_pick(&chips, cx, cy, dead, px, py, armed)
        };
        // Nothing armed: the plain midpoint rule applies.
        assert_eq!(pick(14.0, None), Some(0));
        assert_eq!(pick(16.0, None), Some(1));
        // Armed on chip 0, cursor 2° past the boundary: NOT enough.
        assert_eq!(pick(17.0, Some(0)), Some(0));
        // 4° past: the switch happens.
        assert_eq!(pick(19.0, Some(0)), Some(1));
        // Symmetric — armed on chip 1, drifting back toward chip 0.
        assert_eq!(pick(13.0, Some(1)), Some(1), "2° past, the other way");
        assert_eq!(pick(11.0, Some(1)), Some(0), "4° past, the other way");
        // Hysteresis never pins a chip that is no longer in the snapshot
        // (a rebuild shrank the ring under us).
        assert_eq!(pick(0.0, Some(7)), Some(0));
    }

    #[test]
    fn hysteresis_cannot_hold_a_chip_across_the_dead_zone() {
        // The dead zone outranks everything: it is the only source of "None"
        // and an armed chip must not survive a return to the middle.
        let (cx, cy) = (0, 0);
        let chips = [chip_at(cx, cy, 400.0, 0.0), chip_at(cx, cy, 400.0, 90.0)];
        assert_eq!(sector_pick(&chips, cx, cy, 140.0, 300, 0, Some(0)), Some(0));
        assert_eq!(sector_pick(&chips, cx, cy, 140.0, 139, 0, Some(0)), None);
        assert_eq!(sector_pick(&chips, cx, cy, 140.0, 0, 0, Some(0)), None);
    }

    // -- Guard 4c: the dead zone, and the two-ring exclusion ---------------

    #[test]
    fn dead_zone_radius_is_the_apps_ring_inscribed_circle() {
        // The ring is an ELLIPSE, so the radius is set by the chip that comes
        // closest — here the one on the short (vertical) axis. Measured to
        // the chip's near EDGE, never its centre.
        let (cx, cy) = (0, 0);
        let chips = [
            (370, -15, 430, 15),    // east, near edge at 370
            (-30, -315, 30, -285),  // north, near edge at 285
            (-430, -15, -370, 15),  // west
            (-30, 285, 30, 315),    // south
        ];
        assert!((dead_zone_radius(cx, cy, &chips) - 285.0).abs() < 1e-9);
        // Nothing arms inside it, in ANY direction…
        for deg in [0.0, 45.0, 90.0, 180.0, -120.0] {
            let (px, py) = at(cx, cy, 284.0, deg);
            assert_eq!(sector_pick(&chips, cx, cy, 285.0, px, py, None), None);
        }
        // …and one px outside it, the sector answers.
        let (px, py) = at(cx, cy, 286.0, 0.0);
        assert_eq!(sector_pick(&chips, cx, cy, 285.0, px, py, None), Some(0));
    }

    #[test]
    fn an_inner_ring_chip_can_never_be_selected_because_it_lies_inside_the_dead_zone() {
        // THE TWO-RING EXCLUSION, both halves.
        //
        // First half: the specials never reach this file at all —
        // `publishHudChips` (toast.ts) selects `.st-chip.ap` and nothing else,
        // so the snapshot is the APPS ring by construction. Not testable from
        // Rust; asserted by the second half instead.
        //
        // Second half, which IS testable: even if an inner-ring chip somehow
        // arrived, it sits at a specials-ring radius, and the dead zone
        // derived from the APPS ring swallows that radius whole. A special and
        // an app can share a direction — that ambiguity is exactly why only
        // one ring may participate.
        //
        // Real geometry, this machine (dpr 1.5): specials ring outer edge
        // ~135 CSS px → ~202 physical; apps ring near edge ~181 CSS px →
        // ~271 physical.
        let (cx, cy) = (0, 0);
        let apps = [
            chip_at(cx, cy, 300.0, 0.0),
            chip_at(cx, cy, 271.0 + 15.0, 90.0),
            chip_at(cx, cy, 300.0, 180.0),
            chip_at(cx, cy, 271.0 + 15.0, -90.0),
        ];
        let dead = dead_zone_radius(cx, cy, &apps);
        assert!(dead >= 270.0, "the apps ring's inner edge, not its centres: {dead}");
        // A cursor pointed at a SPECIAL — same direction as an app, but at the
        // inner ring's radius — arms nothing.
        for deg in [0.0, 45.0, 90.0, 135.0, 180.0, -90.0] {
            let (px, py) = at(cx, cy, 202.0, deg);
            assert_eq!(
                sector_pick(&apps, cx, cy, dead, px, py, None),
                None,
                "an inner-ring direction at {deg}° must arm nothing"
            );
        }
    }

    #[test]
    fn dead_zone_without_an_inner_ring_still_tracks_the_apps_ring() {
        // `hud_show_specials` OFF: the inner ring is not drawn, so the apps
        // ring moves INWARD (toast.ts: `rout0 = gi.rx + innerHalf + outerHalf
        // + RING_CLEAR`, and `innerHalf` is 0 with no specials). The derived
        // radius follows it — this is NOT the fallback path, and it must not
        // be: the fallback fires only for an EMPTY snapshot.
        //
        // Specials-hidden geometry, this machine: apps ring near edge ~167
        // CSS px horizontally / ~181 vertically → ~250 / ~271 physical.
        let (cx, cy) = (0, 0);
        let apps = [
            (250, -15, 310, 15),
            (-30, -301, 30, -271),
            (-310, -15, -250, 15),
            (-30, 271, 30, 301),
        ];
        let dead = dead_zone_radius(cx, cy, &apps);
        assert!((dead - 250.0).abs() < 1e-9, "inscribed radius, not the floor: {dead}");
        assert!(
            dead > DEAD_ZONE_MIN_PHYS_PX,
            "the floor must not bite on real specials-hidden geometry"
        );
        // The floor and the empty-snapshot fallback are the same number, and
        // both err LARGE — toward nothing arming, toward a typed space.
        assert_eq!(dead_zone_radius(cx, cy, &[]), DEAD_ZONE_MIN_PHYS_PX);
        assert_eq!(
            dead_zone_radius(cx, cy, &[(-10, -10, 10, 10)]),
            DEAD_ZONE_MIN_PHYS_PX,
            "a degenerate chip over the centre must not open the dead zone"
        );
        // An empty snapshot arms nothing whatever the radius says.
        assert_eq!(sector_pick(&[], cx, cy, 140.0, 9999, 9999, None), None);
    }

    // -- The CSS px → physical px conversion, round-tripped ----------------

    #[test]
    fn css_to_phys_round_trips_within_one_physical_px() {
        // This machine: 1.5 scale; the overlay read back at a physical
        // position the requested one was rounded to (asked 202 → got 203
        // logical is the recorded artefact — hence positions are READ BACK).
        let (win_x, win_y, dpr) = (305, 371, 1.5);
        for &(x, y, w, h) in &[
            (0.0, 0.0, 118.0, 28.0),
            (100.0, 200.0, 118.0, 28.0),
            (10.33, 7.77, 96.5, 26.25), // fractional CSS positions are real
            (1234.5, 999.0, 40.0, 40.0),
        ] {
            let (x0, y0, x1, y1) = css_rect_to_phys(win_x, win_y, dpr, x, y, w, h);
            // Outward rounding: the physical rect fully covers the CSS rect…
            assert!(x0 as f64 <= win_x as f64 + x * dpr);
            assert!(y0 as f64 <= win_y as f64 + y * dpr);
            assert!(x1 as f64 >= win_x as f64 + (x + w) * dpr);
            assert!(y1 as f64 >= win_y as f64 + (y + h) * dpr);
            // …and round-trips back to CSS within one physical px per edge.
            assert!(((x0 - win_x) as f64 / dpr - x).abs() < 1.0 / dpr + 1e-9);
            assert!(((y0 - win_y) as f64 / dpr - y).abs() < 1.0 / dpr + 1e-9);
            assert!(((x1 - win_x) as f64 / dpr - (x + w)).abs() < 1.0 / dpr + 1e-9);
            assert!(((y1 - win_y) as f64 / dpr - (y + h)).abs() < 1.0 / dpr + 1e-9);
        }
        // dpr 1.0 must be exact.
        assert_eq!(css_rect_to_phys(10, 20, 1.0, 5.0, 6.0, 7.0, 8.0), (15, 26, 22, 34));
    }

    // -- The disarm path and SPACE_ABORTED ---------------------------------

    #[test]
    fn arm_sets_abort_and_drift_out_disarm_clears_it() {
        let aborted = AtomicBool::new(false);
        let armed = AtomicI32::new(-1);
        // Arming claims the abort flag — the wheel's exact mechanic.
        let (emit, refused) = apply_to(Verdict::Arm(3), &aborted, &armed);
        assert!(!refused);
        assert_eq!(emit, Some(Some(3)));
        assert!(aborted.load(Ordering::SeqCst), "arming must set SPACE_ABORTED");
        assert_eq!(armed.load(Ordering::SeqCst), 3);
        // Drifting back out MUST return the user's space: this is the bug the
        // feature ships or dies on — armed, drift out, release must type a
        // space exactly as a plain hold always has.
        let (emit, refused) = apply_to(Verdict::DisarmClear, &aborted, &armed);
        assert!(!refused);
        assert_eq!(emit, Some(None));
        assert!(
            !aborted.load(Ordering::SeqCst),
            "drift-out disarm must CLEAR SPACE_ABORTED or the user loses their space"
        );
        assert_eq!(armed.load(Ordering::SeqCst), -1);
    }

    #[test]
    fn disarm_keep_leaves_someone_elses_abort_standing() {
        // The other half of the ordering problem: when the disarm is caused
        // by the HUD hiding (a combo fired) or the wheel, the abort flag
        // belongs to THAT gesture — clearing it would type a space behind a
        // launched action.
        let aborted = AtomicBool::new(true);
        let armed = AtomicI32::new(2);
        let (emit, _) = apply_to(Verdict::DisarmKeep, &aborted, &armed);
        assert_eq!(emit, Some(None));
        assert!(
            aborted.load(Ordering::SeqCst),
            "a non-drift disarm must NOT clear an abort it does not own"
        );
        assert_eq!(armed.load(Ordering::SeqCst), -1);
    }

    #[test]
    fn arming_into_an_already_aborted_hold_is_refused() {
        // Wheel (or a combo) aborted this hold first. Arming now would let
        // the release launch on top of that gesture.
        let aborted = AtomicBool::new(true);
        let armed = AtomicI32::new(-1);
        let (emit, refused) = apply_to(Verdict::Arm(0), &aborted, &armed);
        assert!(refused, "an already-aborted hold must refuse to arm");
        assert_eq!(emit, None);
        assert_eq!(armed.load(Ordering::SeqCst), -1, "a refused arm must not publish");
        assert!(aborted.load(Ordering::SeqCst));
    }

    // -- Guards 2 and 3 at their boundaries, via the tracker ---------------

    /// A live hold with the HUD up, centred on the origin, dead zone 140.
    /// `hold_start_cursor` is deliberately OUTSIDE the dead zone (500,0) so
    /// that guard 2 (travel) and guard 4 (direction) can be exercised one at
    /// a time — a start point in the middle of the ring would conflate them.
    fn base_in<'a>(chips: &'a [(i32, i32, i32, i32)], hold: u64) -> TickIn<'a> {
        TickIn {
            enabled: true,
            modifier_active: true,
            hud_visible: true,
            blocked: false,
            hold_ts: hold,
            hold_start_cursor: Some((500, 0)),
            cursor: None,
            externally_disarmed: false,
            chips,
            centre: Some((0, 0)),
            dead_r: 140.0,
            tick_ms: TICK_HELD_MS,
        }
    }

    /// The four-chip compass ring every tracker test below points into.
    fn ring() -> [(i32, i32, i32, i32); 4] {
        [
            chip_at(0, 0, 400.0, 0.0),   // 0: east
            chip_at(0, 0, 400.0, 90.0),  // 1: south
            chip_at(0, 0, 400.0, 180.0), // 2: west
            chip_at(0, 0, 400.0, -90.0), // 3: north
        ]
    }

    #[test]
    fn travel_threshold_boundary_one_px_short_never_arms() {
        let chips = ring();
        let mut t = HoldTracker::new();
        let mut i = base_in(&chips, 1000);
        // Start at (500,0) — already OUTSIDE the dead zone and already
        // pointing due east at chip 0. Only guard 2 stands in the way, which
        // is the point: this is the resting-hand case, a hand on the mouse
        // that happens to sit somewhere on the desk.
        i.cursor = Some((523, 0)); // 23px — one short of MIN_TRAVEL_PHYS_PX
        for _ in 0..100 {
            assert_eq!(t.tick(&i), Verdict::NoChange);
        }
        // Exactly 24px: traveled, and dwell starts counting.
        i.cursor = Some((524, 0));
        let mut armed = false;
        for _ in 0..100 {
            if t.tick(&i) == Verdict::Arm(0) {
                armed = true;
                break;
            }
        }
        assert!(armed, "exactly MIN_TRAVEL px of movement must satisfy guard 2");
    }

    #[test]
    fn dwell_boundary_arms_on_the_tick_it_is_reached_not_before() {
        let chips = ring();
        let mut t = HoldTracker::new();
        let mut i = base_in(&chips, 2000);
        // Due east, well past min travel, well outside the dead zone — and
        // nowhere near the chip itself (the chip's near edge is at 370).
        i.cursor = Some((200, 0));
        i.tick_ms = 20; // DWELL_MS=60 → exactly 3 ticks
        assert_eq!(t.tick(&i), Verdict::NoChange); // 20ms
        assert_eq!(t.tick(&i), Verdict::NoChange); // 40ms — one tick short
        assert_eq!(t.tick(&i), Verdict::Arm(0)); // 60ms — arms exactly here
        // And 57ms of accumulated dwell must NOT arm: 3 ticks of 19ms.
        let mut t2 = HoldTracker::new();
        let mut j = base_in(&chips, 3000);
        j.cursor = Some((200, 0));
        j.tick_ms = 19;
        assert_eq!(t2.tick(&j), Verdict::NoChange); // 19
        assert_eq!(t2.tick(&j), Verdict::NoChange); // 38
        assert_eq!(t2.tick(&j), Verdict::NoChange); // 57 < 60
        assert_eq!(t2.tick(&j), Verdict::Arm(0)); // 76 ≥ 60
    }

    #[test]
    fn a_sweep_across_the_ring_is_a_non_event() {
        // Guard 3's purpose, restated for sectors: swinging the cursor around
        // the ring on the way to somewhere else must not arm anything. Three
        // 16ms ticks per sector is 48ms — under DWELL_MS, and a real sweep is
        // far faster than that.
        let chips = ring();
        let mut t = HoldTracker::new();
        let mut i = base_in(&chips, 4000);
        for deg in [0.0, 90.0, 180.0, -90.0, 0.0, 90.0] {
            let (x, y) = at(0, 0, 300.0, deg);
            i.cursor = Some((x, y));
            for _ in 0..3 {
                assert_eq!(t.tick(&i), Verdict::NoChange, "{deg}° for 48ms must not arm");
            }
        }
    }

    #[test]
    fn a_settled_direction_shifts_to_the_neighbour_it_settles_on() {
        // The other half of the sweep: STOP in a new sector and it takes over
        // — but only after its own dwell, and Shift, not a second Arm.
        let chips = ring();
        let mut t = HoldTracker::new();
        let mut i = base_in(&chips, 4500);
        i.cursor = Some((300, 0)); // east
        i.tick_ms = DWELL_MS;
        assert_eq!(t.tick(&i), Verdict::Arm(0));
        i.cursor = Some((0, 300)); // south, one full sector away
        assert_eq!(t.tick(&i), Verdict::Shift(1));
    }

    #[test]
    fn back_into_the_dead_zone_clears_and_hud_hide_keeps() {
        let chips = ring();
        // Arm by pointing east and settling.
        let mut t = HoldTracker::new();
        let mut i = base_in(&chips, 5000);
        i.cursor = Some((300, 0));
        i.tick_ms = DWELL_MS; // dwell completes on the first tick
        assert_eq!(t.tick(&i), Verdict::Arm(0));
        // Back into the dead zone while the hold is live: DisarmClear —
        // release must type a space again. This is the ONLY way back to
        // "nothing armed" since PROBLEM 209 replaced containment.
        i.cursor = Some((60, 20));
        assert_eq!(t.tick(&i), Verdict::DisarmClear);

        // Re-arm, then the HUD hides (a combo fired and cancel_hud ran):
        // DisarmKeep — the combo's abort must survive.
        i.cursor = Some((300, 0));
        assert_eq!(t.tick(&i), Verdict::Arm(0));
        i.hud_visible = false;
        assert_eq!(t.tick(&i), Verdict::DisarmKeep);

        // Re-arm, then the wheel blocks the hold (guard 6): DisarmKeep.
        i.hud_visible = true;
        i.cursor = Some((300, 0));
        assert_eq!(t.tick(&i), Verdict::Arm(0));
        i.blocked = true;
        assert_eq!(t.tick(&i), Verdict::DisarmKeep);
    }

    #[test]
    fn a_new_hold_resets_everything_and_a_consumed_arm_syncs_quietly() {
        let chips = ring();
        let mut t = HoldTracker::new();
        let mut i = base_in(&chips, 6000);
        i.cursor = Some((300, 0));
        i.tick_ms = DWELL_MS;
        assert_eq!(t.tick(&i), Verdict::Arm(0));
        // The hook consumed the arm (click or release fired the activation).
        i.externally_disarmed = true;
        assert_eq!(t.tick(&i), Verdict::SyncLost);
        // A NEW hold: nothing carries over — same cursor, but travel and
        // dwell must be earned again from the new start point.
        let mut j = base_in(&chips, 7000);
        j.hold_start_cursor = Some((300, 0)); // hand already pointing east
        j.cursor = Some((300, 0));
        j.tick_ms = DWELL_MS;
        for _ in 0..20 {
            assert_eq!(t.tick(&j), Verdict::NoChange, "no travel → never arms");
        }
    }

    #[test]
    fn no_mouse_movement_this_hold_never_arms_even_pointing_at_a_chip() {
        // The stale-cursor trap: the LAST hold left the cursor atomics parked
        // pointing straight at a chip. This hold, the mouse never moves →
        // cursor is None → nothing arms, however long the dwell.
        let chips = ring();
        let mut t = HoldTracker::new();
        let i = base_in(&chips, 8000); // cursor: None
        for _ in 0..200 {
            assert_eq!(t.tick(&i), Verdict::NoChange);
        }
    }

    #[test]
    fn without_a_published_centre_nothing_can_arm() {
        // No centre means no directions. `publish_chips` refuses to vouch for
        // one when the overlay's inner size reads back zero, and the poller
        // must then fail toward a typed space rather than measure angles from
        // (0,0) — which is a real screen position, not a sentinel.
        let chips = ring();
        let mut t = HoldTracker::new();
        let mut i = base_in(&chips, 9000);
        i.centre = None;
        i.cursor = Some((300, 0));
        i.tick_ms = DWELL_MS;
        for _ in 0..50 {
            assert_eq!(t.tick(&i), Verdict::NoChange);
        }
    }

    #[test]
    fn take_helpers_shape_is_publish_bounded() {
        // publish_keys bounds and lowercases; an empty key slot reads invalid.
        publish_keys(&[
            ("A".into(), "App".into()),
            ("Z".into(), "Zed".into()),
            ("".into(), "broken".into()),
        ]);
        assert_eq!(CHIP_KEY_COUNT.load(Ordering::Relaxed), 3);
        assert_eq!(CHIP_KEYS[0].load(Ordering::Relaxed), 'a' as u32);
        assert_eq!(CHIP_KEYS[1].load(Ordering::Relaxed), 'z' as u32);
        assert_eq!(CHIP_KEYS[2].load(Ordering::Relaxed), 0, "an empty key must read invalid");
        // publish_keys invalidates geometry from the previous layout.
        assert_eq!(CHIP_GEOM_COUNT.load(Ordering::Relaxed), 0);
        clear_chips();
        assert_eq!(CHIP_KEY_COUNT.load(Ordering::Relaxed), 0);
    }
}
