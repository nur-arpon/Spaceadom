/**
 * DROP-IN REPLACEMENT for `src/components/toast.ts` in a V13 fork.
 * This is the EXACT file that produced the working radial Guide HUD and the
 * island-pill toasts. Copy it verbatim; it needs `overlay-earthy.css`
 * (same folder here → put it at `src/styles/overlay-earthy.css`) and the
 * Rust/HTML edits listed in RUST_AND_HTML_CHANGES.md.
 *
 * Backend events consumed (unchanged from V13):
 *   "toast-notification" (string) · "guide-hud-show" ({profile,apps,specials})
 *   "guide-hud-hide" · plus "theme-changed"/"sound-changed" (bool, optional)
 */
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
// The band count lives in its own LEAF module (nothing app-level imports
// into it), seeded and updated by overlay.ts. Read via getBandCount() at
// layout time — never cached at import, where it is still the default.
import { getBandCount } from "./hud-band-count";
// PROBLEM 255 — the ONE "auto" theme rule, shared with the DASHBOARD bundle.
// A leaf module (it imports nothing at all), so taking it here adds nothing
// to the overlay's bundle and cannot drag `main.ts` in behind it — which is
// the reason the overlay carried its own, wrong, copy of the rule until now.
import { resolveTheme } from "../theme-resolve";

const LIFE = 2800;   // toast lifetime; the progress ring drains over this
const OPEN_AT = 200; // dot → open
const LEAVE_MS = 380;

/* ---- PROBLEM 112: the warp handover ---------------------------------------
   Toast pills and the SPACE pill are ONE object. Every pill on screen warps
   into SPACE when Space is held and warps back out to its exact slot on
   release. The overlay window move is instant and un-animatable, so the flight
   happens in CSS inside the window and the move is cancelled arithmetically. */
/* ---------------------------------------------------------------------------
   WARP — the toast <-> SPACE handover flights (PROBLEM 112/114), built across
   1.0.29-1.0.32 and switched OFF for 1.0.33 at the owner's request: he tried
   1.0.32, preferred 1.0.27's plain behaviour, and asked for the 1.0.33 fixes
   without it.
   Switched off rather than deleted, deliberately. All three flight paths were
   written as enhancements over a plain path that is still present and still
   correct — `absorbIntoSpace` even documents the fallback as "always correct,
   just less pretty". Setting this to false takes every one of them, leaving
   1.0.27's behaviour: toasts fade in bottom-centre, the HUD opens and closes on
   its own. Nothing is reconstructed from memory, so nothing can be lost.
   Set to true to work on the transition again; there are exactly three call
   sites and each is marked `if (WARP`.
   --------------------------------------------------------------------------- */
/* RE-ENABLED 2026-08-18 at the owner's request, WITH NEW FLIGHTS. He rejected
   1.0.32's motion, not the machinery: the staging, freezing, parking and rect
   arithmetic below are unchanged, but the flights inside absorbIntoSpace and
   hideGuideHud are now flightThruster / flightSlingDown from THRUSTER_SLING.md
   - the pairing he chose and supplied himself. flightWarp remains only for the
   §4.3 grace-window ejection. */
/* PROBLEM 174 — WARP and SLING are now the USER'S choice, not a build-time
   constant, and they default to OFF.

   The owner, 2026-08-24, relaying his testers: *"some people gave me feedback
   that they found it disturbing, too much time consuming and doesn't add much
   to the functionality… just have an on off switch for this specific space hud
   to toast, off by default, if off then it has toasts and space hud like
   before, but ensure no twitches or buggy feel."*

   ONE switch drives BOTH, deliberately. Asked whether "off" should keep the
   inbound absorb, he chose "Everything — full 1.0.27". That is also the safer
   half of the answer: WARP and SLING share `_stageMode`, `_slingStaged` and
   `_hudBusy`, so leaving one on leaves the whole state machine live — and that
   state machine is what latched `hudBusy=true` for three minutes in his log
   (PROBLEM 175). Off means none of it runs.

   Nothing is reconstructed from memory. The plain path both flags fall back to
   has been present and correct all along — `absorbIntoSpace` documents its own
   fallback as "always correct, just less pretty" — so OFF is the code that
   shipped in 1.0.27, not a re-implementation of it.

   Seeded from `get_config` on load and updated by the "flight-changed" event,
   the same two-way arrangement the theme and sound settings use: an event that
   only fires on CHANGE leaves a freshly-opened overlay with the wrong value
   (the rule CLAUDE.md records for the theme).

   `let`, not `const`. Read at the call sites, never captured. */
let WARP = false;

/* ---------------------------------------------------------------------------
   SLINGSHOT — the HUD -> toast arrival, and ONLY that direction.
   Owner's request 2026-08-17: he kept the ring's entrance (he likes the ripple)
   and asked for the handover INTO the toast to become a real, visible move. He
   also said explicitly: do NOT bring back 1.0.33's warp, implement this fresh.
   So this is gated on its own flag and does not turn WARP on: the toast->SPACE
   absorb and the return-on-release stay off, exactly as he left them.
   He is writing the opposite direction (toast -> HUD) himself.
   --------------------------------------------------------------------------- */
let SLING = false;
const SLING_MS = 940;      // door to door - slow ON PURPOSE, this is the set piece
const SLING_BOW = 150;     // px the arc swings out past the chord
const SLING_SAMPLES = 22;  // bezier keyframes; 22 is smooth, more buys nothing
const SLING_STRETCH = 1.9; // nose-first stretch at mid-flight (warp uses 2.5)
const SLING_MID = 0.5;     // fraction at which the box is smallest / trail longest
const SLING_T0 = 0.16;     // stretch ramps from here - AFTER the face fade (0.17)
/* Long tail - orbital capture. Fast off the chip, then a long deceleration into
   the slot. Do NOT reuse WARP_EASE: its late snap fights the arc. */
const CAPTURE_EASE = "cubic-bezier(.3,0,.08,1)";

/* ---- thruster convoy (absorb: toasts -> SPACE) - THRUSTER_SLING.md ---- */
const THRUST_MS = 640;        // one lift-off, door to door
const THRUST_STAGGER = 120;   // convoy spacing
const THRUST_DIP = 14;        // squat px before lift-off - the "compress" beat
const THRUST_STRETCH = 2.35;  // shear along travel at mid-flight
const EXHAUST_EVERY = 64;     // ms between shed rings/sparks

/* ---- slingshot down (release: SPACE -> slots) ---- */
const SLING_DOWN_MS = 820;    // one continuous arc, no pause
const SLING_DOWN_BOW = 170;   // px the arc bows out; alternates sides per pill
const SLING_DOWN_STRETCH = 1.9;
/* The ring must OUTLIVE the flight. The engine cancels the HUD before it
   dispatches the action, so without this the chips are torn down ~240ms after
   release while the flight needs 940ms - which is exactly the owner's
   complaint: "as soon as I left the space key the guide disappeared... there
   was no time". Set when a slingshot launches; hideGuideHud waits for it. */
let _slingUntil = 0;
/* How long the ring waits, after a COMBO cancels the HUD, for the toast to
   arrive and claim its chip. Only applies when the engine says an action is
   pending - a plain release collapses on the normal schedule. 1200 because the
   gap is real launch latency, MEASURED: Brave ~500ms, VLC ~1000ms from combo to
   toast. The 380 this replaced lost the race to every cold launch, and the ring
   was gone before the flight began (PROBLEM 135). */
const SLING_HANDOVER_MS = 1200;
let _slingHeld = false;
/* PROBLEM 136 - a slingshot LANDED on the staged stack, so the toast already
   sits exactly where the pill touched down. Re-fitting after that re-anchors
   the stack (setStageAnchor(false): top:50%+239px -> bottom:74px) AND moves the
   window itself, so the toast leaps out from under the landing. That is the
   owner's "pausing in the middle before jumping to toast". While this is set,
   the handover leaves the geometry alone. */
let _slingStaged = false;

const WARP_MS = 560;      // one flight, door to door
const STAGGER = 95;       // between pills when several fly at once
/* HUD entrance / exit, and the reason they are no longer the same number.
   ------------------------------------------------------------------------
   `#st-hud` used to carry ONE `transition: … 220ms cubic-bezier(.4,0,1,1)`
   for both directions, so the HUD arrived and left at the same speed AND on
   the same ease-IN curve — an accelerating curve on an ENTRANCE, which reads
   as the ring being yanked on rather than blooming. CLAUDE.md's design rules
   are explicit: "exits run at ~65% of entrance time with --ease-in".

   So the two directions are now separate, in the CSS and here:
     entrance  HUD_IN_MS  220ms  ease-OUT  (cubic-bezier(.2,.8,.2,1),
                                            the same curve .pulse uses)
     exit      HUD_OUT_MS 143ms  ease-IN   (cubic-bezier(.4,0,1,1))  = 65%

   HUD_OUT_MS is depended on by exactly two teardown timers, both in
   hideGuideHud, and both are "wait for the fade, then tear down":
     · the WARP branch:  HUD_OUT_MS + SLING_DOWN_MS + STAGGER*back.length
     · the plain branch: HUD_OUT_MS + 20
   Neither is the handover grace. PROBLEM 135's grace is SLING_HANDOVER_MS
   (1200ms) and SPACE_GRACE_MS (420ms), and both SLING branches `return`
   before reaching these timers — so shortening this by 77ms shortens only
   the tail AFTER every deferral has already run. `overlay_toasts_done`
   (the single terminal path that hides the window) therefore fires 77ms
   earlier on a plain release, and 77ms earlier after a flight has landed;
   nothing waits on it.
   BOTH NUMBERS MUST MATCH #st-hud's transitions in overlay-earthy.css. */
const HUD_IN_MS  = 220;
const HUD_OUT_MS = 143;   // 65% of HUD_IN_MS — the design language's exit ratio
const SPACE_W = 230;      // #st-hud .space
const SPACE_H = 60;
/* Brief wind-up, very fast middle, soft landing. Do NOT use a curve with a long
   flat start (e.g. cubic-bezier(.85,0,.12,1)) — it reads as the pill pausing
   mid-flight rather than accelerating. */
const WARP_EASE = "cubic-bezier(.42,0,.16,1)";
const SETTLE_AT = 0.18;   // fraction of the flight spent finishing the entrance
const SQUEEZE_AT = 0.40;  // fraction at which the pill is a circle, mid-warp
/** Reads the .reduced-motion class that main.ts / overlay.ts put on <html>
 *  from the user's "Visual effects" setting, falling back to the OS query if
 *  neither has run yet. Never query the media query alone — that ignores the
 *  in-app override (PROBLEM 47). */
const REDUCED = () =>
  document.documentElement.classList.contains("reduced-motion");

/* ---------------- sound ticks (OFF by default) ---------------- */
let _soundOn = false;
let _ac: AudioContext | null = null;
function beep(f: number): void {
  if (!_soundOn) return;
  try {
    _ac = _ac || new AudioContext();
    const o = _ac.createOscillator(), g = _ac.createGain();
    o.type = "sine";
    o.frequency.value = f;
    g.gain.setValueAtTime(0.05, _ac.currentTime);
    g.gain.exponentialRampToValueAtTime(0.0001, _ac.currentTime + 0.09);
    o.connect(g); g.connect(_ac.destination);
    o.start(); o.stop(_ac.currentTime + 0.1);
  } catch { /* never break the overlay for a tick */ }
}

/**
 * Pitch sweep — the Guide HUD's open/close transition sound.
 *
 * A single tick reads as a click; a swept tone reads as something ARRIVING,
 * which is what the HUD blooming outward actually is. Rising on show,
 * falling on hide, so the two are distinguishable with your eyes shut.
 *
 * Gain ramps from and to near-silence rather than starting at full: an
 * abrupt gain step is an audible click at the start of the note. Exponential
 * ramps cannot touch exactly 0, hence 0.0001.
 *
 * Follows the same "Sound ticks" setting as beep() — OFF by default.
 */
function sweep(from: number, to: number, ms: number): void {
  if (!_soundOn) return;
  try {
    _ac = _ac || new AudioContext();
    const t = _ac.currentTime, dur = ms / 1000;
    const o = _ac.createOscillator(), g = _ac.createGain();
    o.type = "sine";
    o.frequency.setValueAtTime(from, t);
    o.frequency.exponentialRampToValueAtTime(to, t + dur);
    g.gain.setValueAtTime(0.0001, t);
    g.gain.exponentialRampToValueAtTime(0.055, t + 0.025);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    o.connect(g); g.connect(_ac.destination);
    o.start(t); o.stop(t + dur + 0.02);
  } catch { /* never break the overlay for a sound */ }
}

let _hudActive = false;
/** True from hide() until the 220ms HUD exit finishes — blocks window fits. */
let _hudBusy = false;
/** PROBLEM 175 — deadline timer that unsticks `_hudBusy` if nothing else does. */
let _hudBusyGuard: number | undefined;

/**
 * PROBLEM 267 — ANOTHER component owns the overlay window right now: the
 * middle button's cursor-anchored icon ring (`middle-ring.ts`). It is not the
 * Space HUD, it has none of the HUD's DOM or choreography, and it must not
 * touch `_hudActive` — but it shares this window, and everything in this
 * file that resizes, hides or paints INTO that window has to know it is
 * spoken for. So: while `_extHud` is true no fit runs (`fitToStack`,
 * `requestFit`), no `overlay_toasts_done` is sent (`retire`), and the toast
 * layer is parked hidden exactly as it is under the Space HUD, so a toast
 * that arrives mid-ring does not paint into the ring's window. `endExternalHud`
 * releases it and does the ONE fit or the ONE hide the stack then needs.
 *
 * Exported as a pair rather than as a setter so the release step cannot be
 * forgotten: the ring's hide path calls `endExternalHud` from a bounded timer.
 */
let _extHud = false;
export function beginExternalHud(): void {
  _extHud = true;
  setToastLayerHidden(true);
  hideToastGlow();
}
export function endExternalHud(): void {
  if (!_extHud) return;
  _extHud = false;
  setToastLayerHidden(false);
  if (_toasts.length > 0) {
    const glow = document.getElementById("st-toastglow");
    if (glow) showToastGlow(glow);
    anchorGlow("toast");
    relayout();
  } else if (_isOverlay && !_hudActive && !_hudBusy) {
    invoke("overlay_toasts_done").catch(() => {});
  }
}

/* ---- PROBLEM 112 state ---- */
type Rect = { x: number; y: number; w: number; h: number; stage?: Rect | null };

/* =======================================================================
   THE STAGE — PROBLEM 267 round 3.

   The overlay window is now ONE BIG CANVAS: Rust's `overlay_fit_hud` sizes
   it to the whole work area of the cursor's monitor (never the exact monitor
   bounds) and hands back `stage`, the ring's OLD box — the size this file
   asked for, centred on the monitor exactly where the old window was. `#st-hud`
   is placed at that box, so every chip, the SPACE pill, the pulse and the
   beam (all `calc(50% + …)` inside it) land where they always did; only the
   window around the stage grew. A bloomed pill that runs past the stage now
   has canvas to grow into — the round-2 "cut at the window edge" — and the
   crop guard in `paintBloom` measures the WINDOW around the stage centre,
   not the stage. With no stage (an older Rust, or a refused fit) `#st-hud`
   stays `inset: 0` and everything behaves as before.
   ======================================================================= */
let _stage: Rect | null = null;

/** Put `#st-hud` on its stage, or back to `inset: 0` when there is none. */
function applyStage(stage: Rect | null): void {
  _stage = stage;
  if (!_hudEl) return;
  if (stage) {
    _hudEl.style.left = `${stage.x}px`;
    _hudEl.style.top = `${stage.y}px`;
    _hudEl.style.width = `${stage.w}px`;
    _hudEl.style.height = `${stage.h}px`;
    _hudEl.style.right = "auto";
    _hudEl.style.bottom = "auto";
  } else {
    for (const k of ["left", "top", "width", "height", "right", "bottom"] as const) {
      _hudEl.style[k] = "";
    }
  }
}

/** The stage's TOP-LEFT in window CSS px — the origin `#st-hud`'s children
 *  measure their `offsetLeft`/`offsetTop` from, since `applyStage` makes
 *  `#st-hud` the offset parent at that point. `(0, 0)` with no stage, which
 *  is the `inset: 0` case this file assumed everywhere before round 3. */
function stageOrigin(): { x: number; y: number } {
  return _stage ? { x: _stage.x, y: _stage.y } : { x: 0, y: 0 };
}

/** 1.0.119 (brief 4 §1) — the ratio this page ACTUALLY runs at, sent with
 *  every fit so Rust expresses the stage in THIS page's css px. It is not
 *  always the monitor's scale: Windows' accessibility "Text size" (109 % on
 *  the owner's machine) is folded into WebView2's devicePixelRatio, and a
 *  stage handed over in Windows logical px then lands 9 % too far right and
 *  down — the whole cloud, pill included, because `#st-hud` sits on that
 *  rectangle. The icon ring has had the same correction since round 6
 *  (`pageRescale`); this is the Space ring's half. `1` when the browser
 *  cannot say (the harness), which is also "no correction". */
function pageDpr(): number {
  const d = window.devicePixelRatio;
  return typeof d === "number" && isFinite(d) && d > 0 ? d : 1;
}

/** The stage's centre in window CSS px — the ring's centre. Falls back to
 *  the window's centre when there is no stage. */
function stageCentre(): { x: number; y: number } {
  if (_stage) return { x: _stage.x + _stage.w / 2, y: _stage.y + _stage.h / 2 };
  return { x: window.innerWidth / 2, y: window.innerHeight / 2 };
}

/** PROBLEM 137's handover pin: the ring must not move a pixel while the
 *  window's bottom edge is moved. On a stage the box is already explicit
 *  and the window's top edge is kept by Rust, so there is nothing to pin;
 *  without one, pin `#st-hud` to the window box it had (today's rule). */
function pinStage(ringH: number): void {
  if (!_hudEl || _stage) return;
  _hudEl.style.top = "0px";
  _hudEl.style.bottom = "auto";
  _hudEl.style.height = `${ringH}px`;
}
/** Last known logical screen rect of the overlay window. */
let _rect: Rect | null = null;
/** Pills currently wearing the SPACE identity. They do not age. */
let _absorbed: ToastEntry[] = [];
/** Flights in the air. While > 0 the window must not be hidden or refitted. */
let _flying = 0;
/** The stack is living inside the HUD's window — no fit may run. */
let _stageMode = false;

/**
 * PROBLEM 114 — where the SPACE pill was, and when it stopped being it.
 *
 * The engine cancels the HUD BEFORE it runs the action (engine/mod.rs,
 * HookEvent::KeyCombo calls `s.cancel_hud()` and only then dispatches), so by
 * the time the toast arrives `_hudActive` is already false. The "peel the toast
 * out of the SPACE pill" branch in showToast was therefore UNREACHABLE for the
 * one case it exists for: firing a shortcut while the HUD is up.
 *
 * Rather than reorder the engine — the cancel-first ordering is deliberate, so
 * a slow action cannot leave the HUD on screen — the overlay remembers the
 * SPACE geometry for a short grace period. A toast arriving inside it is the
 * SAME gesture continuing, and launches from where SPACE just was.
 */
let _spaceExit: { x: number; y: number; w: number; h: number; s: number } | null = null;
let _spaceExitAt = 0;
/** Long enough to cover the hide→action→toast round trip, short enough that an
 *  unrelated toast seconds later is never mistaken for part of the gesture. */
const SPACE_GRACE_MS = 420;

/**
 * True only in the OVERLAY window. The dashboard renders toasts too (its own
 * #toast-container — PROBLEM 45), but it must NEVER call overlay_fit /
 * overlay_toasts_done: those resize and hide the SEPARATE always-on-top
 * overlay window, so a "Settings saved" toast in the dashboard would yank the
 * HUD's window around. The dashboard is a normal fixed-size window; its
 * toasts just sit in it.
 *
 * Set by overlay.ts before initToastListener(). Defaults to false so any new
 * caller is safe by default.
 */
let _isOverlay = false;
export function markOverlayWindow(): void {
  _isOverlay = true;
}

const GLOW_ANIMATION = "st-toast-glow 4s ease-in-out infinite";

/**
 * `st-toast-glow`'s keyframes set `opacity` on every frame (.5↔.9, never 0)
 * — a running CSS animation overrides an element's own inline `opacity` on
 * the properties it animates, so setting `style.opacity = "0"` alone is
 * cosmetically defeated for as long as the animation keeps running. The
 * animation was started once at element creation and never stopped, so the
 * glow has kept faintly pulsing at wherever it was last anchored (toast
 * bottom-centre, or HUD centre) ever since the first toast of the session —
 * including under the icon ring, which shares this one glow element but
 * never asked for it. `hideToastGlow` must stop the animation, not just the
 * opacity; the three call sites that relight the glow restore it.
 */
function hideToastGlow(): void {
  const g = document.getElementById("st-toastglow");
  if (g) {
    g.style.opacity = "0";
    g.style.animation = "none";
  }
}

function showToastGlow(g: HTMLElement | null): void {
  if (!g) return;
  g.style.opacity = "1";
  g.style.animation = GLOW_ANIMATION;
}

/**
 * The ONE glow element serves both surfaces, re-anchored per mode.
 *
 * "toast" → bottom-centre, behind the pill stack (its original home).
 * "hud"   → centred, behind the SPACE pill.
 *
 * CRITICAL — do not enlarge it or raise its blur in either mode. This is the
 * 340x150 / blur(22px) element that is PROVEN to composite on this machine.
 * A separate, bigger HUD glow (560x320 / blur(34px)) made the entire
 * transparent overlay window compose ZERO pixels — see PROBLEM 37 and the
 * removal note in overlay-earthy.css. Re-anchoring costs nothing; resizing
 * risks everything.
 */
function anchorGlow(mode: "toast" | "hud"): void {
  const g = document.getElementById("st-toastglow");
  if (!g) return;
  if (mode === "hud") {
    g.style.top = "50%";
    g.style.bottom = "auto";
    g.style.transform = "translate(-50%, -50%)";
  } else {
    g.style.top = "auto";
    g.style.bottom = "8px";
    g.style.transform = "translateX(-50%)";
  }
}

/**
 * Toast pills are hidden while the HUD owns the window.
 *
 * WHY: every Space+key emits a toast, so pressing a shortcut WHILE STILL
 * HOLDING Space rendered the pill (and its glow) at the bottom of the big
 * centred HUD window — the "glow sitting under Contextual Search" report —
 * and then the window visibly jumped when the HUD hid and the stack re-fit.
 * Toasts still arrive and age normally; they are simply not painted until
 * the HUD lets go, at which point one clean fit runs. (2026-08-11)
 */
function setToastLayerHidden(hidden: boolean): void {
  const c = document.getElementById("toast-container");
  if (c) c.style.visibility = hidden ? "hidden" : "visible";
}

/* ---- PROBLEM 112: geometry for the warp ---------------------------------- */

/**
 * Fractional border-box size + the scale currently applied to it.
 *
 * NOT offsetWidth/offsetHeight: those round to whole pixels, which puts the
 * derived scale out by up to 1% and leaves the flying copy ~2px narrower than
 * the pill it is supposed to be sitting on.
 */
function boxOf(el: HTMLElement): { w: number; h: number; s: number; cx: number; cy: number } {
  const cs = getComputedStyle(el);
  const r = el.getBoundingClientRect();
  const n = (v: string) => parseFloat(v) || 0;
  const w = n(cs.width) + n(cs.paddingLeft) + n(cs.paddingRight)
          + n(cs.borderLeftWidth) + n(cs.borderRightWidth);
  const h = n(cs.height) + n(cs.paddingTop) + n(cs.paddingBottom)
          + n(cs.borderTopWidth) + n(cs.borderBottomWidth);
  return { w, h, s: w ? r.width / w : 1, cx: r.left + r.width / 2, cy: r.top + r.height / 2 };
}

function flightHost(): HTMLDivElement {
  let f = document.getElementById("st-flight") as HTMLDivElement | null;
  if (!f) {
    f = document.createElement("div");
    f.id = "st-flight";
    document.body.appendChild(f);
  }
  return f;
}

/** Geometry relative to the flight origin (= the window centre = SPACE). */
function boxRel(el: HTMLElement) {
  const host = flightHost().getBoundingClientRect();
  const b = boxOf(el);
  return { x: b.cx - host.left, y: b.cy - host.top, w: b.w, h: b.h, s: b.s };
}

/** The geometry a pill is ANIMATING TOWARDS, measured with transitions off.
 *  Only ever call this on a parked pill — the snap must not be visible. */
function settledBox(el: HTMLElement) {
  el.classList.add("settle");
  void el.offsetWidth;
  return boxRel(el);
}

function spaceBox() {
  const sp = _hudEl?.querySelector(".space") as HTMLElement | null;
  if (!sp) return { x: 0, y: 0, w: SPACE_W, h: SPACE_H, s: 1 };
  return boxRel(sp);
}

/** Park a pill while its copy flies. Also pins it at its settled geometry, so
 *  when it is revealed on arrival nothing resizes afterwards. */
function park(t: ToastEntry, on: boolean): void {
  t.el.classList.toggle("parked", on);
  t.el.classList.toggle("settle", on);
}

/**
 * Re-anchor the stack (and its glow) inside the HUD's own window.
 *
 * Inline, not a CSS class: toastLayer() writes the container's whole cssText, so
 * a class rule on #toast-container would lose to it.
 */
function setStageAnchor(on: boolean): void {
  const c = document.getElementById("toast-container");
  const g = document.getElementById("st-toastglow");
  // Clear the outer ring but stay inside the HUD window on short displays,
  // where the ring clamp shrinks the window too. Measured from the STAGE
  // (the ring's box), not the window: since round 3 the window is the whole
  // work area and its centre is half a taskbar off the ring's.
  const sc = stageCentre();
  const half = _stage ? _stage.h / 2 : window.innerHeight / 2;
  const y = Math.min(250, Math.max(120, half - 44));
  for (const el of [c, g]) {
    if (!(el instanceof HTMLElement)) continue;
    const isGlow = el.id === "st-toastglow";
    if (on) {
      el.style.bottom = "auto";
      el.style.top = `${sc.y}px`;
      el.style.left = `${sc.x}px`;
      el.style.transform = `translate(-50%, ${isGlow ? y + 46 : y}px)`;
    } else {
      el.style.top = "";
      el.style.left = "50%";
      el.style.bottom = isGlow ? "8px" : "74px";
      el.style.transform = "translateX(-50%)";
    }
  }
}

/* =======================================================================
   TOAST — island pill, bottom-centre, stack of 3
   ======================================================================= */
interface ToastOptions { duration?: number; accent?: string }
interface ToastEntry {
  el: HTMLDivElement;
  phase: "dot" | "open" | "leave";
  duration: number;
  /** Its own timers, so its life clock can be PAUSED while it is the SPACE key. */
  h: number[];
  leaveIn: number;
  dieIn: number;
  armedAt: number;
  /** LIVE TOAST (1.0.123): a keyed pill that is updated in place and has no
   *  clock until `endLiveToast` arms one. Sits out of the depth count so the
   *  normal stack above it is laid out exactly as if it were not there. */
  live?: boolean;
}
const _toasts: ToastEntry[] = [];

function toastLayer(): HTMLDivElement | null {
  const c = document.getElementById("toast-container") as HTMLDivElement | null;
  if (!c) return null;
  if (c.dataset.stStyled !== "1") {
    c.dataset.stStyled = "1";
    // Set via CSSOM, not the HTML style attribute: attribute styles were seen
    // silently not applying in one live run.
    // bottom:74px lifts the stack clear of the window edge so the glow fits.
    c.style.cssText = `
      position: fixed; bottom: 74px; left: 50%; transform: translateX(-50%);
      display: flex; flex-direction: column; align-items: center; gap: 10px;
      width: max-content; pointer-events: none; z-index: 30;`;
    // Breathing glow behind the stack. bottom:8px (NOT -34px): at -34 with a
    // 22px blur it bled past the frame and rendered as a straight cut.
    const glow = document.createElement("div");
    glow.id = "st-toastglow";
    glow.style.cssText = `
      position: fixed; bottom: 8px; left: 50%; transform: translateX(-50%);
      width: 340px; height: 150px; border-radius: 50%; pointer-events: none;
      z-index: 29; filter: blur(22px);
      background: radial-gradient(ellipse, rgba(var(--st-glow-rgb),.30) 0%,
                  rgba(var(--st-glow-rgb),.12) 45%, transparent 70%);
      animation: none; opacity: 0; transition: opacity .3s;`;
    document.body.appendChild(glow);
  }
  return c;
}

/** THE STACK RULE: a pill's depth = the number of NEWER toasts currently in
 *  the "open" phase — NEVER its array index. Index-based depth breaks while
 *  another toast is mid-enter/mid-exit; this keeps sizes monotonic
 *  newest→oldest at all times. Depth drives scale, opacity AND max-width
 *  (560/300/128px — those live in overlay-earthy.css via [data-depth]). */
function relayout(): void {
  for (let i = 0; i < _toasts.length; i++) {
    const t = _toasts[i];
    if (t.phase !== "open") continue;
    // A live pill is always full size and never counts as "newer" for the
    // pills above it (1.0.123) — with no live pill present this line is the
    // same computation as before.
    if (t.live) { t.el.dataset.depth = "0"; continue; }
    const depth = _toasts.slice(i + 1).filter((x) => x.phase === "open" && !x.live).length;
    t.el.dataset.depth = String(Math.min(depth, 2));
  }
  requestFit();
}

/* ---- PROBLEM 112: a pausable life clock ---------------------------------- */

function setDrain(t: ToastEntry, run: boolean): void {
  const c = t.el.querySelector("svg circle:last-child") as SVGElement | null;
  if (c) c.style.animationPlayState = run ? "running" : "paused";
}

/** (Re)start a toast's clock. `leaveIn`/`dieIn` are ms from now. */
function armEntry(t: ToastEntry, leaveIn: number, dieIn: number): void {
  t.h.forEach((h) => window.clearTimeout(h));
  t.h = [];
  t.armedAt = performance.now();
  t.leaveIn = leaveIn;
  t.dieIn = dieIn;
  if (leaveIn > 0) {
    t.h.push(window.setTimeout(() => {
      t.phase = "leave";
      t.el.classList.remove("open");
      t.el.classList.add("leave");
      relayout();
    }, leaveIn));
  }
  t.h.push(window.setTimeout(() => retire(t), dieIn));
  setDrain(t, true);
}

/**
 * Stop the clock while the pill IS the SPACE key.
 *
 * Without this the toast keeps ageing during the hold, expires mid-return-flight
 * and takes the window down with it — the morph then plays inside an already
 * hidden window and the user sees SPACE simply vanish. It is also the correct
 * behaviour: a confirmation should not burn its lifetime while it is a modifier.
 */
function freezeEntry(t: ToastEntry): void {
  t.h.forEach((h) => window.clearTimeout(h));
  t.h = [];
  const gone = performance.now() - (t.armedAt || performance.now());
  t.leaveIn = Math.max(0, t.leaveIn - gone);
  t.dieIn = Math.max(0, t.dieIn - gone);
  setDrain(t, false);
}

function thawEntry(t: ToastEntry): void {
  armEntry(t, t.leaveIn, t.dieIn);
}

function retire(t: ToastEntry): void {
  // Never retire a pill that is airborne or currently wearing SPACE.
  if (_flying > 0 || _absorbed.includes(t)) {
    t.h.push(window.setTimeout(() => retire(t), 200));
    return;
  }
  t.el.remove();
  const i = _toasts.indexOf(t);
  if (i >= 0) _toasts.splice(i, 1);
  relayout();
  if (_toasts.length === 0) {
    hideToastGlow();
    if (_stageMode) { _stageMode = false; setStageAnchor(false); }
    _slingStaged = false;          // PROBLEM 136 - landing pad is gone
    // PROBLEM 267 — and not while the icon ring owns the window either.
    if (_isOverlay && !_hudActive && !_extHud) invoke("overlay_toasts_done").catch(() => {});
  }
}

/** Timestamp of the last window fit, for the coalescing below. */
let _lastFitAt = 0;
let _fitTimer: number | undefined;

function fitToStack(): void {
  // Dashboard toasts must not touch the overlay window (see _isOverlay).
  if (!_isOverlay) return;
  // Never resize while the HUD is up OR still fading: a toast arriving then
  // used to shrink the window mid-fade — a violent visual jump. PROBLEM 267 —
  // nor while the icon ring owns the window.
  if (_hudActive || _hudBusy || _stageMode || _flying > 0 || _extHud) return;
  const c = document.getElementById("toast-container");
  if (!c || _toasts.length === 0) return;
  // Room for the 340x150 blurred glow on every side, so it is never cut.
  const w = Math.max(Math.ceil(c.offsetWidth) + 420, 520);
  const h = Math.ceil(c.offsetHeight) + 240;
  invoke<Rect | null>("overlay_fit", { width: w, height: h })
    .then((r) => { if (r) _rect = r; })
    .catch(() => {});
  _lastFitAt = performance.now();
}

/**
 * Leading-edge-immediate, trailing-edge-coalesced window fit.
 *
 * Tapping a shortcut twice quickly (Space+Y to open, again to minimise) fires
 * two toasts ~1s apart, and each phase change calls relayout(). Resizing an
 * OS window several times in a few frames reads as a stutter — the motion
 * reference is explicit that window bounds want ONE jump, never a per-frame
 * animation. So: the first fit runs instantly (no added latency before a
 * toast appears), and any fit requested within COALESCE_MS of the last one is
 * deferred and merged into a single trailing resize.
 */
const COALESCE_MS = 90;
function requestFit(): void {
  if (_hudActive || _hudBusy || _stageMode || _flying > 0 || _extHud) return;
  const since = performance.now() - _lastFitAt;
  if (since >= COALESCE_MS) { fitToStack(); return; }
  window.clearTimeout(_fitTimer);
  _fitTimer = window.setTimeout(() => fitToStack(), COALESCE_MS - since);
}

export function showToast(message: string, options: ToastOptions = {}): void {
  const { duration = LIFE, accent = "#c67139" } = options;
  const layer = toastLayer();
  if (!layer) return;

  // toastLayer() writes the container's whole cssText on first use, which
  // would clear the "parked" visibility set at HUD-show time. Re-apply it
  // whenever the HUD is up — the first toast of a session is often fired by
  // the very shortcut the user pressed while holding Space.
  if (_hudActive || _extHud) setToastLayerHidden(true);

  const glow = document.getElementById("st-toastglow");
  // PROBLEM 267 — the glow stays dark under the icon ring too; endExternalHud
  // lights it when the stack gets the window back.
  if (glow && !_extHud) showToastGlow(glow);

  // Leading glyph from the engine (⚡ ⚠️ ❌ ↩) becomes the icon disc letter.
  const first = Array.from(message)[0] ?? "•";
  const isGlyph = !/[a-z0-9]/i.test(first);
  const letter = isGlyph ? first : first.toUpperCase();
  const text = isGlyph ? message.slice(first.length).trim() : message;

  const el = document.createElement("div");
  el.className = "st-toast";
  el.setAttribute("role", "status");
  el.innerHTML =
    `<div class="ico" style="background:${accent}">${letter}</div>` +
    `<span class="msg"></span>` +
    `<span class="dot" style="background:${accent}"></span>` +
    `<svg width="20" height="20" viewBox="0 0 20 20" aria-hidden="true">` +
    `<circle cx="10" cy="10" r="8" fill="none" stroke="var(--st-track)" stroke-width="2"/>` +
    `<circle cx="10" cy="10" r="8" fill="none" stroke="${accent}" stroke-width="2" ` +
    `stroke-linecap="round" stroke-dasharray="50.27" transform="rotate(-90 10 10)" ` +
    `style="animation:st-ring-drain ${duration}ms linear forwards"/></svg>`;
  // textContent, never innerHTML — app names are arbitrary user data.
  (el.querySelector(".msg") as HTMLSpanElement).textContent = text;

  layer.appendChild(el);
  const entry: ToastEntry = {
    el, phase: "dot", duration, h: [], leaveIn: 0, dieIn: 0, armedAt: 0,
  };
  _toasts.push(entry);
  while (_toasts.length > 3) {
    const gone = _toasts.shift();
    if (gone) {
      gone.h.forEach((h) => window.clearTimeout(h));
      const k = _absorbed.indexOf(gone);
      if (k >= 0) _absorbed.splice(k, 1);
      gone.el.remove();
    }
  }
  beep(520);

  const LEAVE_AT = OPEN_AT + duration;
  const DIE_AT = LEAVE_AT + LEAVE_MS;

  /* ---- PROBLEM 112: fired WHILE Space is held — peel it off the SPACE pill.
     The stack is staged INSIDE the HUD's window, so no window move happens at
     all; the pill flies out of SPACE to its slot. ---- */
  // PROBLEM 114 — the shortcut-fired-during-a-hold case. `_hudActive` is
  // already false here (the engine cancels the HUD before dispatching the
  // action), so this ALSO accepts a toast arriving within the grace window
  // after SPACE left. Without the second condition the warp never ran for the
  // exact gesture it was written for.
  const fromSpaceExit =
    !_hudActive && _isOverlay && _spaceExit !== null &&
    performance.now() - _spaceExitAt < SPACE_GRACE_MS;

  /* ---- SLINGSHOT: fired WHILE Space is held, and the launched app HAS a chip
     on the ring. The toast tears out of that chip, arcs around the outside of
     the ring, and lands in its slot. Gated on SLING, NOT on WARP: the owner
     asked for this direction only and asked explicitly that 1.0.33's warp not
     come back with it. ---- */
  // PROBLEM 114's ordering, which this MUST respect: the engine calls
  // cancel_hud() BEFORE it dispatches the action, so by the time the toast
  // arrives `_hudActive` is already false. Gating on _hudActive alone would
  // make this branch unreachable for the exact gesture it exists for. The ring
  // is still on screen here because hideGuideHud defers its collapse (below),
  // so chipFor() can still find the chip and tear it out.
  if (SLING && (_hudActive || _hudBusy) && _isOverlay && !REDUCED() && _hudEl) {
    const c = chipFor(text);
    // One line, so "no animation happened" is never again a matter of opinion:
    // it says whether the branch ran and whether the app's chip was found.
    invoke("overlay_log", {
      msg: `sling: text="${text}" chip=${c ? "FOUND" : "none"} ` +
           `hudActive=${_hudActive} hudBusy=${_hudBusy} chips=${
             _hudEl.querySelectorAll("[data-st-app]").length}`,
    }).catch(() => {});
    if (c) {
      _stageMode = true;
      setStageAnchor(true);
      anchorGlow("hud");
      setToastLayerHidden(false);   // container visible; the PILL is parked
      park(entry, true);
      entry.phase = "open";
      el.classList.add("open");
      relayout();                   // depth attrs only - the stage guard blocks the fit
      const to = settledBox(el);    // its real slot and real width, measured now

      // Tear-out, measure and the copy's first frame are ONE synchronous block,
      // so there is never a frame showing both the chip and its copy.
      const from = boxRel(c.chip);
      const chipHtml = c.chip.innerHTML;

      _slingStaged = true;   // PROBLEM 136 - this stack is a landing pad now
      const chipHtml2 = chipHtml;

      /* PROBLEM 137 - fly to the REAL bottom-centre slot, not a midpoint.
         The toast's final home is below the HUD window's bottom edge, so grow
         the window DOWNWARD first (top edge fixed, bottom edge = overlay_fit's
         own bottom), pin #st-hud to its original height so the ring does not
         move a pixel, then un-stage the stack so it sits at its NORMAL
         bottom:74px anchor - which is now the true final position. Everything
         is measured AFTER that, so the flight lands where the toast lives and
         nothing moves afterwards. */
      const ringH = _stage ? _stage.h : window.innerHeight;
      invoke<Rect | null>("overlay_fit_handover", {
        width: _stage ? _stage.w : window.innerWidth, height: ringH, dpr: pageDpr(),
      }).then(() => {
        pinStage(ringH);                    // ring keeps its old box, so it stays put
        _stageMode = false;                 // normal anchor = the real slot
        setStageAnchor(false);
        _stageMode = true;                  // but still no window fits during the flight
        void document.body.offsetWidth;     // flush before measuring

        const to2 = settledBox(el);
        const from2 = boxRel(c.chip);
        tearOut(c);
        const D2 = flightSling({
          from: from2, to: to2, chipHtml: chipHtml2, toastHtml: el.innerHTML,
          cell: c.cell,
          onArrive: () => { park(entry, false); beep(640); },
        });
        armEntry(entry, D2 + duration, D2 + duration + LEAVE_MS);
      }).catch(() => {
        // Window did not grow: fall back to the mid-screen landing rather than
        // no animation at all.
        tearOut(c);
        const D3 = flightSling({
          from, to, chipHtml: chipHtml2, toastHtml: el.innerHTML, cell: c.cell,
          onArrive: () => { park(entry, false); beep(640); },
        });
        armEntry(entry, D3 + duration, D3 + duration + LEAVE_MS);
      });
      return;
    }
    else {
      /* No chip on the ring (volume, clipboard, an unlisted app): the pill
         flies out of the SPACE key itself - flightSlingDown, the same descent
         the release uses - straight to the true bottom slot. Same handover
         window, same no-jump guarantee. */
      _stageMode = true;
      setStageAnchor(true);
      anchorGlow("hud");
      setToastLayerHidden(false);
      park(entry, true);
      entry.phase = "open";
      el.classList.add("open");
      relayout();
      _slingStaged = true;
      const ringH2 = _stage ? _stage.h : window.innerHeight;
      invoke<Rect | null>("overlay_fit_handover", {
        width: _stage ? _stage.w : window.innerWidth, height: ringH2, dpr: pageDpr(),
      }).then(() => {
        pinStage(ringH2);
        const from3 = spaceBox();        // ring pinned, so SPACE has not moved
        _stageMode = false;
        setStageAnchor(false);
        _stageMode = true;
        void document.body.offsetWidth;
        const to3 = settledBox(el);
        const D4 = flightSlingDown({
          from: from3, to: to3, html: el.innerHTML, bow: SLING_DOWN_BOW,
          onArrive: () => { park(entry, false); beep(640); },
        });
        armEntry(entry, D4 + duration, D4 + duration + LEAVE_MS);
      }).catch(() => {
        // The grow failed: show the pill plainly rather than fly wrong.
        park(entry, false);
        armEntry(entry, LEAVE_AT, DIE_AT);
        relayout();
      });
      return;
    }
  }

  if (WARP && (_hudActive || fromSpaceExit) && _isOverlay && !REDUCED()) {
    _stageMode = true;
    setStageAnchor(true);
    anchorGlow("hud");
    setToastLayerHidden(false);      // the container is visible; the PILL is parked
    park(entry, true);
    entry.phase = "open";
    el.classList.add("open");
    relayout();                      // depth attrs only — the stage guard blocks the fit
    const to = settledBox(el);       // its real slot and real width, measured now
    // Live SPACE while the HUD is up; its remembered position if it has just
    // left. `spaceBox()` would fall back to a hard-coded centre once the HUD
    // element is torn down, which is what put an earlier flight in the wrong
    // place.
    const from = _hudActive ? spaceBox() : (_spaceExit ?? spaceBox());
    // The flight now owns SPACE's identity — drop the real pill this frame so
    // it is never on screen at the same time as its copy. The ring and chips
    // are untouched and keep collapsing behind the flight, which is what the
    // handover should look like.
    _hudEl?.classList.add("space-gone");
    const D = flightWarp({
      from,
      to,
      html: el.innerHTML,
      toSpace: false,
      onArrive: () => { park(entry, false); beep(640); },
    });
    // One gesture, one launch: consume the grace so a later unrelated toast
    // does not also come flying out of a SPACE pill that is long gone.
    if (!_hudActive) { _spaceExit = null; _spaceExitAt = 0; }
    armEntry(entry, D + duration, D + duration + LEAVE_MS);
    return;
  }

  // PROBLEM 113 — a toast arriving OUTSIDE a hold must never be blocked by a
  // leftover handover flag. `_stageMode` legitimately suppresses fits while the
  // pills live inside the HUD window, but if it is still set here the HUD is
  // gone and the flag is stale — and a suppressed fit means a hidden window,
  // i.e. a toast the user never sees. Clearing it is always safe at this point.
  if (!_hudActive && !_hudBusy && _stageMode) {
    _stageMode = false;
    setStageAnchor(false);
    _slingStaged = false;
    anchorGlow("toast");
  }

  const open = () => { entry.phase = "open"; el.classList.add("open"); beep(640); relayout(); };
  if (REDUCED()) open(); else window.setTimeout(open, OPEN_AT);
  armEntry(entry, LEAVE_AT, DIE_AT);
  relayout();
}

/* =======================================================================
   LIVE TOAST — one keyed pill, updated IN PLACE (owner, 2026-09-19, 1.0.123)
   =======================================================================
   A touchpad edge slide reports its value up to 8× a second. Pushing each
   report through `showToast` would stack eight "Volume 62%" pills; instead
   Rust sends `toast-live` {key, text} and this side keeps ONE pill per key:
   the first call builds it (same markup, same entrance, same slot, no life
   clock), every later call with the same key finds it and swaps the text
   node — nothing else restarts, no fit, no beep — and `toast-live-end`
   {key} arms the normal leave/retire clock so it lingers LIVE_LINGER_MS and
   plays the ordinary exit. `retire` is the same terminal path as every
   other toast, so `overlay_toasts_done` still fires when the stack empties.
   A live pill and a normal pill coexist: `order:1` keeps the live one in the
   bottom slot and `relayout` leaves it out of the depth count, so the normal
   stack above it behaves as if it were alone. `showToast` is untouched. */
const LIVE_LINGER_MS = 600;
const _live = new Map<string, ToastEntry>();

/** The leading-glyph rule `showToast` applies, as a function, for the live
 *  path only (`showToast`'s own inline copy is left exactly as it was). */
function splitGlyph(message: string): { letter: string; text: string } {
  const first = Array.from(message)[0] ?? "•";
  const isGlyph = !/[a-z0-9]/i.test(first);
  return {
    letter: isGlyph ? first : first.toUpperCase(),
    text: isGlyph ? message.slice(first.length).trim() : message,
  };
}

/** Show — or, when a pill for `key` is already up, UPDATE IN PLACE — the live
 *  toast. Idempotent per key: a second call never adds a second element. */
export function showLiveToast(key: string, message: string, options: ToastOptions = {}): void {
  const { letter, text } = splitGlyph(message);
  const cur = _live.get(key);
  if (cur && cur.phase !== "leave" && _toasts.includes(cur)) {
    const m = cur.el.querySelector(".msg") as HTMLSpanElement | null;
    if (m && m.textContent !== text) m.textContent = text;
    const ico = cur.el.querySelector(".ico") as HTMLDivElement | null;
    if (ico && ico.textContent !== letter) ico.textContent = letter;
    return;
  }
  const { accent = "#c67139" } = options;
  const layer = toastLayer();
  if (!layer) return;
  if (_hudActive || _extHud) setToastLayerHidden(true);
  const glow = document.getElementById("st-toastglow");
  if (glow && !_extHud) showToastGlow(glow);

  const el = document.createElement("div");
  el.className = "st-toast";
  el.setAttribute("role", "status");
  el.dataset.liveKey = key;
  el.style.order = "1";
  // Same markup as a normal pill, minus the drain: a live pill has no
  // lifetime to show, so its ring stays full until `endLiveToast`.
  el.innerHTML =
    `<div class="ico" style="background:${accent}"></div>` +
    `<span class="msg"></span>` +
    `<span class="dot" style="background:${accent}"></span>` +
    `<svg width="20" height="20" viewBox="0 0 20 20" aria-hidden="true">` +
    `<circle cx="10" cy="10" r="8" fill="none" stroke="var(--st-track)" stroke-width="2"/>` +
    `<circle cx="10" cy="10" r="8" fill="none" stroke="${accent}" stroke-width="2" ` +
    `stroke-linecap="round" stroke-dasharray="50.27" transform="rotate(-90 10 10)"/></svg>`;
  // textContent, never innerHTML — the text is built from live values.
  (el.querySelector(".ico") as HTMLDivElement).textContent = letter;
  (el.querySelector(".msg") as HTMLSpanElement).textContent = text;
  layer.appendChild(el);

  const entry: ToastEntry = {
    el, phase: "dot", duration: 0, h: [], leaveIn: 0, dieIn: 0, armedAt: 0, live: true,
  };
  _toasts.push(entry);
  _live.set(key, entry);
  beep(520);

  // PROBLEM 113's rule, verbatim: a stale handover flag would suppress the
  // fit, and a suppressed fit is a hidden window.
  if (!_hudActive && !_hudBusy && _stageMode) {
    _stageMode = false;
    setStageAnchor(false);
    _slingStaged = false;
    anchorGlow("toast");
  }

  const open = () => {
    if (!_toasts.includes(entry) || entry.phase === "leave") return;
    entry.phase = "open"; el.classList.add("open"); beep(640); relayout();
  };
  if (REDUCED()) open(); else entry.h.push(window.setTimeout(open, OPEN_AT));
  relayout();
}

/** The pill for `key` lingers LIVE_LINGER_MS, then leaves the way every toast
 *  leaves. A key with no pill is a no-op. */
export function endLiveToast(key: string): void {
  const cur = _live.get(key);
  _live.delete(key);
  if (!cur || !_toasts.includes(cur)) return;
  const wasArmed = cur.h.length > 0 && cur.phase === "dot";
  // Keep a pending dot→open timer: armEntry clears every timer on the entry,
  // and a pill that ends before OPEN_AT must still open before it leaves.
  const leaveIn = LIVE_LINGER_MS + (wasArmed ? OPEN_AT : 0);
  armEntry(cur, leaveIn, leaveIn + LEAVE_MS);
  if (wasArmed) {
    cur.h.push(window.setTimeout(() => {
      if (cur.phase === "dot") { cur.phase = "open"; cur.el.classList.add("open"); relayout(); }
    }, OPEN_AT));
  }
}

/* =======================================================================
   GUIDE HUD — radial bloom, centred on screen, viewport-aware
   ======================================================================= */
interface GuideHudPayload {
  profile: string;
  apps: [string, string][];      // [key, label] — ONLY assigned letters
  /** PROBLEM 267 round 3 — one per `apps` row, same order: the app's icon as
   *  a `data:` URL (the icon ring's sources, from Rust's cache — never
   *  fetched at raise time), or null for the letter disc. Absent on an
   *  older payload → every chip keeps its disc. */
  app_icons?: (string | null)[];
  specials: [string, string][];
  /** The active profile's emoji, for the glyph beside the SPACE pill.
   *  `null`/absent is the NORMAL state and must draw the pill exactly as every
   *  build before this one did — see `paintSpacePill`. */
  profile_emoji?: string | null;
  /** 1.0.119 (brief 4 §2) — the FOCUSED APP for the centre pill: a short
   *  name (already cut to ~14 chars by Rust) and its icon as a `data:` URL.
   *  `null`/absent keeps the word SPACE — the desktop, the lock screen, our
   *  own windows, an unreadable process, a preview. The pill's 230×60 box is
   *  fixed in CSS and the name is ellipsized inside it, so the cloud never
   *  moves whatever the name (`spacePillHtml`). */
  focus?: { name: string; icon?: string | null } | null;
  /** Present ONLY for a Settings preview (`preview_hud_layout`). A real
   *  Space-hold sends `null` and the ring reads the user's own saved settings,
   *  exactly as it always has. See `previewOverride`. */
  preview?: { layout?: string; bands?: string } | null;
}
let _hudEl: HTMLDivElement | null = null;
let _lastPayload: GuideHudPayload | null = null;

/* ===========================================================================
   THE SETTINGS PREVIEW — the page's half. 1.0.96.

   A preview is the REAL ring, with the REAL bindings, drawn in a layout the
   user has not chosen yet. Two things and ONLY two things differ from an
   ordinary hold:

     1. `buildHud` takes the layout and band count from the PAYLOAD instead of
        from the saved settings, for that one show. Nothing is written and
        nothing is cached — `getHudLayout()` / `getBandCount()` are untouched,
        so the very next real hold is back on the user's own choice with no
        cleanup step that could be missed.

     2. `publishHudChips` does not run, so Rust never learns where the chips
        are. Together with Rust declining to publish the chip KEYS for the same
        show, both of `pointer.rs`'s counts stay at zero and there is nothing
        for the pointer to arm — no beam, no armed chip, nothing a release or a
        click could fire. That is the whole of the "preview cannot launch
        anything" guarantee on this side, and it is one early return rather
        than a new mode threaded through the aiming code.

   Deliberately NOT a module-level "preview mode" that something has to turn
   off. It is derived from the payload every time it is needed, so the only way
   to be left in preview state is to still be showing the preview. */
/** The layout override for THIS payload, normalised, or `null` for a real
 *  hold. Normalisation matches `hud-layout.ts` / `hud-band-count.ts` exactly —
 *  the strings come from Rust, but a renamed constant on either side must
 *  degrade to the shipped default rather than to something nobody chose. */
function previewOverride(
  p: GuideHudPayload | null,
): { mode: HudLayoutMode; bands: "auto" | "one" | "two" } | null {
  const pv = p?.preview;
  if (!pv) return null;
  return {
    mode: pv.layout === "classic" ? "classic" : "magnetic",
    bands: pv.bands === "one" ? "one" : pv.bands === "two" ? "two" : "auto",
  };
}

/** Estimated chip width — used ONLY to pick the ring radii before the real
 *  chips exist. Placement itself uses MEASURED widths (PROBLEM 77). */
function estW(label: string, special: boolean): number {
  return Math.min(label.length * 6.8, 118) + (special ? 64 : 56);
}

/* ===========================================================================
   RING SIZING — PROBLEM 208. The ellipse must adapt to the CONTENT.

   WHAT WAS WRONG (and what was NOT):
   PROBLEM 77 already distributes chips along the rim by MEASURED ARC LENGTH,
   and that part is correct — do not re-solve it. The bug was one layer up:
   the ring's SIZE was decoupled from the item COUNT. `rin`/`rout` were derived
   from the SINGLE WIDEST chip's half-width (`Math.max(...)`), and `ryi`/`ryo`
   were the hard constants 118 / 196. Nothing anywhere grew the ellipse as more
   keys were bound, so the ring for 8 apps and the ring for 26 apps was the
   same ring.

   The owner's real config binds ALL 26 LETTERS: 26 outer chips + 8 inner
   specials = 34 chips. Measured on his own label set (see the harness numbers
   in the report), the outer ring needed ~3.0-3.1k px of rim against a
   circumference of ~2.0k px — about 50% more content than rim. `arcAngles`
   went on distributing proportionally, which is the only honest thing it can
   do with an impossible budget: every chip got a share SMALLER than its own
   width, so neighbours were placed closer together than they are wide. That
   is the overlap he reported.

   THE FIX, in the owner's stated order of preference. Each step is only
   reached if the one before it could not make the ring fit:
     (a) GROW the ring until the rim can hold the content, bounded by the
         screen budget (94%, matching Rust's own clamp in overlay_fit_hud).
     (b) TIGHTEN the inter-chip gap from RING_GAP_PREF toward RING_GAP_MIN.
     (c) SHRINK chip padding/font by a small bounded amount (`.dense-chips`,
         one step, ~1px of padding and ~0.75px of font — see overlay-earthy.css).
     (d) TIGHTEN the label truncation cap last (`.tight-chips`, 118px → 92px).

   Growth is UNIFORM in x and y, which preserves the ellipse's existing
   eccentricity exactly rather than forcing a nominal ratio — at 8 chips
   nothing grows at all and the geometry is byte-identical to what shipped.
   Uniform scaling also leaves `arcAngles`' parameter mapping unchanged, so
   the screen clamp below can still be applied AFTER the angles are solved.

   AND THE RESULT IS MEASURED, NOT REASONED. `overlapCount()` runs the real
   rectangles against each other after every rung of the ladder; a rung is
   only accepted at ZERO intersections. Overlap is the failure this exists to
   fix, so nothing is taken on trust.
   =========================================================================== */

/** Preferred clearance between two neighbours on the rim. */
const RING_GAP_PREF = 16;
/** Floor for step (b). Below this, chips start touching in the diagonal
 *  quadrants where a chip's HEIGHT stops protecting it. */
const RING_GAP_MIN = 10;
/** Fraction of the monitor the window may occupy. MUST match Rust's clamp in
 *  `overlay_fit_hud` — if the request exceeds it Rust silently shrinks the
 *  window and the outermost chips get cropped. */
const SCREEN_BUDGET = 0.94;
/** Breathing room around the bloom box. Clears the 340px ring-pulse; without
 *  it the circle looks "cut off at the back". */
const HUD_PAD = 180;
/** The pad we refuse to give up even at maximum ring size — the chip shadow
 *  is `0 10px 28px`, so it needs ~38px to land inside the window. */
const HUD_PAD_MIN = 64;
/** Clears the 230x60 SPACE pill. */
const RIN_CLEAR = 115;
/** SPACE↔inner and inner↔outer clearance. */
const RING_CLEAR = 26;
const RYI_BASE = 118;
/** The OUTER ring's vertical floor. CLASSIC ONLY — Magnetic has no outer ring
 *  to floor, because every app band's ry is `rx · BAND_RATIO` and its rx comes
 *  from the band packer. Left where it was, and used only from
 *  `layoutClassic`, so that branch stays a byte-for-byte no-op against
 *  1.0.88. */
const RYO_BASE = 196;
const ARC_N = 720;
/** The roundest the ellipse may ever get: `ry = 0.55 · rx`, the ratio the
 *  design brief names. Growing uniformly preserves whatever eccentricity the
 *  base constants give (about 0.42-0.50 depending on chip widths), which is
 *  FLATTER than the brief — so on a screen too narrow to grow sideways there
 *  is legitimate room to round the ellipse OUT toward 0.55 and buy rim length
 *  from the vertical budget. Hard-capped here so it can never become a
 *  circle: at 0.55 it is still unmistakably an ellipse. */
const RING_ASPECT_MAX = 0.55;

/* ===========================================================================
   MAGNETIC SECTOR — v1.0.89. The design handoff's geometry, with the owner's
   two overrides folded in.

   WHAT CHANGED AND WHY. PROBLEM 208's ring is sized to its CONTENT: 26 apps
   need ~3.0k px of rim, so the ellipse grows until it has that much, and the
   result is a ~950x800 ring with a large empty middle that the user has to
   sweep their eyes across. The rim is bought with SCREEN. Magnetic Sector buys
   it from the LABELS instead: every chip stays inside a fixed glance radius,
   and when the rim runs out a second BAND opens inside the ceiling rather than
   the ellipse pushing outward.

   THE OWNER'S THREE DECISIONS, which override the handoff where they differ:

   1. LABELS CLIP BY WORD, NOT BY PIXEL. The handoff's `restCap` was a px cap
      producing ~6 characters ("Discor…"). Rejected: "clip to the first word,
      let the first word complete for most stuff, clip if the name is too long,
      and full label at aim." A whole word is recognisable and a severed one is
      not — that is the whole argument. `REST_CAP_BACKSTOP` survives only for a
      first "word" that is itself too long, which is real: a binding with no
      `label` falls back to `bind.app` or `bind.web_url` (engine/mod.rs ~221),
      and those are single tokens with no spaces at all.

   2. ONE GEOMETRY FOR BOTH MODES. The handoff scoped this to `hasSp === false`
      and left the old layout in place when specials are shown. Overruled:
      "same geometry for both." Specials, when shown, are simply the INNERMOST
      band and the apps fill outward from the next one. The user-facing band
      count counts APP bands only.

   3. SPECIALS KEEP THEIR FULL LABELS AT REST. They sit inside the dead zone
      and can never be aimed at, so they never bloom — word-clipping them would
      destroy information with no recovery path ("Force" for "Force Close App"
      is worse forever). So the rest cap below is scoped to `.st-chip.ap` and
      `fitInner`'s tight 1.0.88 baseline is untouched.
   =========================================================================== */

/** Ceiling on a chip's OUTER EDGE, and NOT on the band radius — applying it to
 *  the radius alone let chips reach ~448px in the design prototype. The band
 *  ceiling is therefore `GLANCE_R - maxChipWidth / 2`.
 *
 *  It is a ceiling the geometry meets whenever it can, not a law it can always
 *  obey: see `layout`, where an inner SPECIALS band can push the app band
 *  outside it. That case is measured and logged rather than silently clipped. */
const GLANCE_R = 400;
/**
 * The same ceiling WHEN THE SPECIALS BAND IS ON SCREEN.
 *
 * 400 IS A SOFT TARGET, NOT A HARD LINE. The owner's ruling, in his words:
 * *"The 400px glance radius holds with specials hidden — a little bit here and
 * there is okay, it's not a hard line."* Specials-hidden is therefore held to
 * GLANCE_R exactly (measured: 399.2px worst case over 1..26 apps, zero
 * violations), and this constant is the allowance for the one configuration
 * that provably cannot meet it.
 *
 * WHY IT EXISTS, and it is not a comfort measure. The specials sit at their
 * kept 1.0.88 baseline, whose own outer edge is ~340px; an app band must clear
 * that by RING_CLEAR plus half a chip, so the app band's INNER edge starts
 * beyond 400 before a single app is placed. With the ceiling pinned at 400 the
 * band could not grow, one band could not hold 26 chips at the acceptance bar,
 * and the page fell through to DROPPING the specials — on the owner's own
 * config, where all 26 letters are bound, that meant switching "show special
 * keys on the ring" ON changed nothing at all. A control that does nothing is
 * worse than a missing control; CLAUDE.md says so in as many words.
 *
 * THE VALUE IS MEASURED, NOT CHOSEN. 26 of his real labels at the preferred
 * 16px gap need ~2.8k px of rim, and a 0.62 ellipse gives 5.16 * rx of rim, so
 * one band outside the specials must reach rx ~= 550 and a furthest outer edge
 * of ~620px. 640 is that requirement rounded up to the next tidy figure, and
 * it is a CEILING, not a size: `fitRx` still returns the SMALLEST radius that
 * fits, so eight apps with specials on stay exactly as tight as they are today
 * and only a full alphabet ever approaches this number.
 *
 * It is not the last guard. The screen budget below bounds the ring again on
 * both axes, because `overlay_fit_hud` clamps the window to 94% of the monitor
 * and a clamped window crops chips rather than shrinking them.
 */
const GLANCE_R_WITH_SPECIALS = 640;
/** `ry = rx · BAND_RATIO` for every APP band. Never 1.0 — a circle was
 *  explicitly rejected by the design. (The specials band is not on this ratio:
 *  it keeps the 1.0.88 baseline shape, decision 3 above.) */
const BAND_RATIO = 0.62;
/** Vertical breathing room between two stacked bands, on top of a chip height. */
const BAND_PAD = 12;
/** Fallback chip height if a measurement comes back zero. 33, not 30: 5px
 *  padding x2 + a 20px `kbd` + 1.5px border x2, MEASURED. The prototype used 30
 *  and every clearance test came out optimistic while the shipped ring
 *  collided. The real value is measured once per build — see `bandStepFor`. */
const CHIP_H_FALLBACK = 33;
/** Innermost band's clearance over `RIN_CLEAR` + half a chip. */
const BAND_LO_PAD = 24;
/** Minimum aim wedge a chip must own, and the furthest the relax pass may drag
 *  any chip from where arc-length placement put it. */
const RELAX_FLOOR = 5 * Math.PI / 180;
const RELAX_LIMIT = 12 * Math.PI / 180;
/** Outward push, in px, for the armed chip and for each of its two angular
 *  neighbours. PRESENTATION ONLY — see `paintBloom`. */
const BLOOM_PUSH_ARMED = 40;
const BLOOM_PUSH_NEIGHBOUR = 24;
/** The at-rest label cap for APP chips, published as `--chip-cap`.
 *
 *  This is a BACKSTOP, not the clipping rule — the clipping rule is "first
 *  whole word" (`restWord`). It exists for a first word that is itself too
 *  long to be a word: a URL or a path, which arrive as ONE token with no
 *  spaces in them ("notebooklm.google.com", "C:\\Program Files\\...\\chrome.exe").
 *
 *  88px is measured, not chosen. Across every profile in the owner's live
 *  config the widest FIRST WORD is "Afterburner"/"Illustrator" at 11
 *  characters; both render 80px wide at the shipped `600 11.5px Outfit`, and
 *  "PowerPoint"/"Battle.net" come in at 74-77. 88 clears the widest real word
 *  with ~8px of headroom for a font fallback, and still truncates any token
 *  long enough to have been a URL. Raising it costs ring compactness; lowering
 *  it starts severing real words, which is the thing decision 1 forbids. */
const REST_CAP_BACKSTOP = 88;
/** The full-label cap: 118px, the value `.st-chip span` has always carried.
 *  An opened label is therefore never wider than what 1.0.88 already drew.
 *  The BLOOM rule in the stylesheet spells the same 118 out literally rather
 *  than reading this — a `--chip-cap-full` handover would be a third copy of a
 *  number that has exactly one meaning. This constant exists so `--chip-cap`
 *  can be published as a no-op in CLASSIC mode, where the cap must be the
 *  shipped value and nothing else. */
const FULL_CAP = 118;

/** A radial step of S in x buys only `BAND_RATIO · S` in y, and bands stack at
 *  the TOP and BOTTOM of the ellipse where it is y that has to clear. A naive
 *  46px step leaves 28.5px vertically and the bands touch. */
function bandStepFor(chipH: number): number {
  return Math.round((chipH + BAND_PAD) / BAND_RATIO);
}

/** The at-rest text of an app label: its FIRST WHOLE WORD.
 *  "Google Chrome" -> "Google"; "Samsung Browser" -> "Samsung";
 *  "WhatsApp" -> "WhatsApp" (already one word, so nothing is lost).
 *  A single long token comes back whole and is bounded by `--chip-cap`
 *  instead, with a CSS ellipsis. */
function restWord(label: string): string {
  const m = /^\S+/.exec(label.trim());
  return m ? m[0] : label;
}

/** Write an app chip's visible label. No-op on a browser-profile chip, which
 *  has no `.st-chip-label` span — see the comment where those are built. */
function setChipLabel(cell: HTMLElement, text: string): void {
  const lab = cell.querySelector<HTMLElement>(".st-chip-label");
  if (lab && lab.textContent !== text) lab.textContent = text;
}

/** One app chip's RESTING geometry and boxes, in ring-local px.
 *
 *  THE SINGLE SOURCE OF TRUTH FOR EVERYTHING DOWNSTREAM OF THE LAYOUT, and
 *  deliberately not the live DOM: bloom moves chips and widens labels, and
 *  anything that measured the moved DOM would feed that movement back into the
 *  decision that caused it. `w`/`h` are the resting box; `full` is the width
 *  the same chip has with its whole label showing, which is what clamps the
 *  bloom push against GLANCE_R. */
interface RestChip {
  x: number; y: number; w: number; h: number; full: number;
  geo: number; r: number;
}
let _apRest: RestChip[] = [];
/** Is the CURRENT ring a Magnetic one, and therefore allowed to bloom?
 *  Set once per build from the layout mode, and read by `paintBloom` — which
 *  runs on every `hud-pointer` event and must not re-read a setting on that
 *  path. Classic never blooms: 1.0.88 had no bloom, and the option only has
 *  value if choosing it gives back exactly the ring the owner already knows. */
let _bloomOn = true;

/* ===========================================================================
   THE BLOOM PUSH'S TWO BOUNDS. Both are set once per build, in `buildHud`,
   and read by `paintBloom` on every pointer event — that path may not compute
   geometry, only look it up.

   THE HANDOFF'S FORMULA WAS `min(BLOOM_PUSH, GLANCE_R - fullWidth/2 - r)`,
   AND IT IS CHANGED HERE. MEASURED REASON, and it falls straight out of the
   owner's decision 2 rather than out of anything the handoff got wrong:

   the handoff scoped Magnetic to "specials hidden", where the whole ring fits
   inside GLANCE_R and that clamp costs almost nothing — measured on the
   owner's 26 real labels, 26 of 26 chips still got a push, 22 of them the
   full 40px. The owner overruled that scoping ("same geometry for both"), and
   with the specials shown at their kept 1.0.88 baseline the app band is
   pushed to ~413px, i.e. ALREADY outside a 400px ceiling. `GLANCE_R - ... - r`
   is then negative for every chip near the rim: measured at 12 apps with
   specials on, FIVE OF TWELVE chips got a push of exactly ZERO. The armed
   chip would move for some letters and not for others, which is precisely
   the inconsistency the owner asked to be rid of.

   So the ceiling that governs bloom is the one ACTUALLY IN FORCE for this
   build, not the nominal one. `GLANCE_R` still governs the RESTING ring —
   that is the promise it exists to make, and bloom is not rest. The invariant
   below is the honest version of the same idea:

     a bloomed chip never reaches more than BLOOM_PUSH_ARMED beyond the
     ring's own outermost full-label edge, and never leaves the window.

   If the owner would rather keep the literal 400px bound and accept an armed
   chip that sometimes does not move, this is the one line to change back. */
/** Radial ceiling for a BLOOMED chip's outer edge. */
let _bloomCap = GLANCE_R + BLOOM_PUSH_ARMED;
/** Half-extents of the fitted overlay window, for the crop guard. A scalar
 *  radius cannot do this job: the window is an ellipse's bounding box, so a
 *  chip on the wide flank has room a chip at the top does not. */
let _bloomHalf = { w: 0, h: 0 };

/** THE ARC_N SAMPLE ANGLES' cos/sin, BUILT ONCE.
 *
 *  `arcTable`'s loop used to call `Math.cos` and `Math.sin` 1,440 times per
 *  table for arguments that never change — `(i / ARC_N) · 2π` is a property of
 *  ARC_N, not of the ellipse. Hoisting them is BIT-IDENTICAL by construction
 *  (same function, same argument, evaluated once instead of N times) and it is
 *  half the cost of a table: measured 43µs -> 20µs per build.
 *
 *  Deliberately NOT a `Math.hypot` -> `Math.sqrt(dx*dx + dy*dy)` swap as well.
 *  That is faster again and it is NOT bit-identical — `hypot` is the more
 *  accurate of the two — and `arcAngles` inverts this table with a binary
 *  search, so a last-bit change can flip a chip to the adjacent sample, which
 *  is 1/720 of the ring: up to 4px of movement at the radii this file reaches.
 *  Every gain here has to be exact, or it is a layout change wearing a
 *  performance hat. */
const _arcCos = new Float64Array(ARC_N + 1);
const _arcSin = new Float64Array(ARC_N + 1);
for (let i = 0; i <= ARC_N; i++) {
  const t = (i / ARC_N) * Math.PI * 2;
  _arcCos[i] = Math.cos(t);
  _arcSin[i] = Math.sin(t);
}

/** PER-BUILD MEMO FOR `arcTable`.
 *
 *  A HUD build asks for a table hundreds of times over: the ceiling ladder
 *  re-solves the whole band layout at up to 12 ceilings (CEIL_STEPS 6 +
 *  CEIL_BISECT 5, plus the first solve), each of those tries up to `maxBands`
 *  band counts, and every count is measured TWICE — loose and tightened. And
 *  `packBands` puts the innermost band on `lo` and the outermost on `hiR` for
 *  every one of those counts, so those two ellipses alone recur once per count
 *  per measurement. Measured at 40 chips, 1707x1067: 723 tables per rung before
 *  any of this. 583 of those were `ellipsePerimeter` wanting one scalar and are
 *  gone entirely (closed form, below); the 140 that remain are the real
 *  `arcAngles` calls, and they name only **54 distinct (rx, ry) pairs** — which
 *  is what this Map is for.
 *
 *  KEYED ON THE EXACT (rx, ry) PAIR, not on a rounded one. The repeats this is
 *  here to collect are EXACT repeats — `lo`, `hiR` and
 *  `lo + b·(hiR − lo)/(n − 1)` recomputed from the same operands — so an exact
 *  key catches all of them, and it makes the memo provably a no-op on the
 *  layout: a hit returns the table the miss would have built, bit for bit.
 *  A tolerance key (0.5px was tried) buys nothing on top of that and reopens
 *  the binary-search flip described above, because two radii a quarter-pixel
 *  apart do NOT produce the same table.
 *
 *  Nothing mutates a table it gets back, so sharing one is safe.
 *
 *  CLEARED AT THE TOP OF `buildHud` (see `clearArcCache`) so the Map cannot
 *  grow across a session of display changes and label sets. */
const _arcCache = new Map<string, number[]>();
/** Drop the per-build `arcTable` memo. Called once, at the top of `buildHud`. */
function clearArcCache(): void { _arcCache.clear(); }

/** Cumulative arc length of an ellipse, sampled ARC_N times. `arcAngles`
 *  inverts it to turn a chip's share of the rim into a parameter angle, and it
 *  is now the ONLY caller — `ellipsePerimeter` used to build a whole table to
 *  read `cum[ARC_N]` out of it and answers in closed form instead. */
function arcTable(rx: number, ry: number): number[] {
  const key = `${rx}|${ry}`;
  const hit = _arcCache.get(key);
  if (hit) return hit;
  const cum = new Array<number>(ARC_N + 1);
  cum[0] = 0;
  let px = rx, py = 0;                       // t = 0
  for (let i = 1; i <= ARC_N; i++) {
    const x = rx * _arcCos[i], y = ry * _arcSin[i];
    cum[i] = cum[i - 1] + Math.hypot(x - px, y - py);
    px = x; py = y;
  }
  _arcCache.set(key, cum);
  return cum;
}

/**
 * THE RIM LENGTH, IN CLOSED FORM — Ramanujan's second approximation.
 *
 *     P ≈ π(a + b)(1 + 3h / (10 + √(4 − 3h))),   h = ((a − b)/(a + b))²
 *
 * WHY IT IS NOT `arcTable(rx, ry)[ARC_N]` ANY MORE. That built a 721-entry
 * cos/sin/hypot table in full and then read exactly one number out of it.
 * Harmless when the ring was solved once; not harmless since PROBLEM 240 gave
 * rung (a) a ceiling ladder. `CEIL_STEPS` 6 + `CEIL_BISECT` 5 ceilings × up to
 * 19 band counts × two measurements each is 2,400-3,400 table builds behind
 * ONE Space hold, ~81% of them for a single scalar, every one of them
 * synchronous on the main thread while the user is holding the key — and twice
 * that on a display-change rebuild. Measured on the standalone transcription
 * of this path at 1707x1067, 212px chips, per HOLD (the full (a)-(d) walk) —
 * table counts are per rung:
 *
 *     chips   before                    after (this + the memo + hoisted trig)
 *      8      126 tables /  4.2 ms      9 tables /  0.3 ms
 *     26      659 tables  / 84.7 ms     55 tables /  8.2-11.8 ms
 *     34      737 tables  / 92.2 ms     56 tables / 11.4-14.8 ms
 *     40      723 tables  / 97.0 ms     54 tables / 14.3-15.5 ms
 *
 * ACCURACY, AND WHY THE SWAP IS NOT A GEOMETRY CHANGE. Ramanujan II is exact
 * to ~1e-10 relative over every eccentricity this file can produce (`ry/rx`
 * runs 0.42-0.62 in Magnetic and up to `RING_ASPECT_MAX` in classic). The
 * 720-gon it replaces is INSCRIBED, so it was SHORT by ~π²/(6·ARC_N²) ≈ 3.2e-6
 * relative — 0.006px on a 2,000px rim. The new value is that much larger, so
 * `fitRx` answers with a radius ~0.002px smaller. `arcAngles` still normalises
 * by its own table's `cum[N]`, so placement is unchanged in shape; the two
 * lengths differ only by that 3.2e-6, which is four orders of magnitude below
 * the 1px quantisation `offsetWidth` already imposes on every chip width.
 * Measured end-to-end, this change alone: 93 of 105 display × band-mode ×
 * label-set cells BIT-IDENTICAL, the other 12 at 0.0008-0.0015px. Max
 * chip-position delta 0.0015px, against a 0.5px budget (see PROBLEM 240).
 */
function ellipsePerimeter(rx: number, ry: number): number {
  const a = Math.max(rx, ry), b = Math.min(rx, ry);
  const s = a + b;
  if (!(s > 0)) return 0;
  const d = (a - b) / s;
  const h = d * d;
  return Math.PI * s * (1 + (3 * h) / (10 + Math.sqrt(4 - 3 * h)));
}

/**
 * Step (a): grow an ellipse until its rim can hold `ws` plus one `gap` each,
 * bounded by `maxRx`/`maxRy`.
 *
 * TWO PHASES, and the order is the point.
 *
 * Phase 1 is UNIFORM. The perimeter of an ellipse is linear in a uniform
 * scale, so the factor is solved in one shot rather than iterated:
 * k = required / available. Uniform scaling preserves the ring's existing
 * eccentricity exactly, which is why an 8-chip HUD comes out byte-identical
 * to what shipped before this change — it needs no growth, so it gets none.
 *
 * Phase 2 only runs when phase 1 hit a bound and the rim is STILL short. It
 * raises `ry` alone, toward `RING_ASPECT_MAX * rx` and no further, buying rim
 * length out of vertical budget that phase 1 could not reach (a screen is
 * usually wider than it is tall, so `maxRx` binds first and leaves height on
 * the table). Perimeter is monotone in `ry`, so a bisection is exact enough
 * and cheap — and it only ever makes the ellipse ROUNDER, never rounder than
 * the design's own 0.55 ratio.
 *
 * Never shrinks. The base radii are already the collision-safe minimum (they
 * clear the SPACE pill and the ring inside), so shrinking below them trades
 * one overlap for a worse one.
 */
function growRing(
  ws: number[], rx0: number, ry0: number, gap: number,
  maxRx: number, maxRy: number,
): { rx: number; ry: number; need: number; have: number } {
  const have0 = ellipsePerimeter(rx0, ry0);
  const need = ws.reduce((s, w) => s + w, 0) + ws.length * gap;
  let k = ws.length > 1 && need > have0 ? need / have0 : 1;
  k = Math.min(k, maxRx / rx0, maxRy / ry0);
  if (!(k > 1) || !isFinite(k)) k = 1;
  const rx = rx0 * k;
  let ry = ry0 * k;
  let have = have0 * k;

  if (ws.length > 1 && need > have) {
    const ryTop = Math.min(maxRy, RING_ASPECT_MAX * rx);
    if (ryTop > ry) {
      // Default to the roundest allowed — correct when even that is short.
      let best = ryTop;
      if (ellipsePerimeter(rx, ryTop) > need) {
        // Otherwise take the SMALLEST ry that clears `need`: keep the ellipse
        // as close to its original shape as the content permits.
        let lo = ry, hi = ryTop;
        for (let i = 0; i < 20; i++) {             // ~1e-4 relative, 20 evals
          const mid = (lo + hi) / 2;
          if (ellipsePerimeter(rx, mid) < need) lo = mid; else hi = mid;
        }
        best = hi;
      }
      ry = best;
      have = ellipsePerimeter(rx, ry);
    }
  }
  return { rx, ry, need, have };
}

/** Axis-aligned boxes, centre + size, in ring-local coordinates. */
interface ChipBox { x: number; y: number; w: number; h: number }

/**
 * How many pairs of chips actually intersect. This is the acceptance test for
 * every rung of the ladder — the chips are axis-aligned rectangles, so an
 * exact answer costs one O(n²) pass over 34 items (561 comparisons, once per
 * HUD show, microseconds).
 *
 * The 0.5px slack leans TOWARD reporting an overlap, not away from it.
 * Widths come from `offsetWidth`, which rounds to whole pixels and can
 * therefore under-report a chip by up to half a pixel; without this the check
 * would pass a pair that really is touching. Measured: against a strict
 * fractional-rect test in the browser it was the difference between "2 pairs"
 * and "4 pairs" on a deliberately-undersized screen.
 */
function overlapCount(boxes: ChipBox[]): number {
  let n = 0;
  for (let i = 0; i < boxes.length; i++) {
    for (let j = i + 1; j < boxes.length; j++) {
      const a = boxes[i], b = boxes[j];
      if (Math.abs(a.x - b.x) < (a.w + b.w) / 2 + 0.5 &&
          Math.abs(a.y - b.y) < (a.h + b.h) / 2 + 0.5) n++;
    }
  }
  return n;
}

/**
 * The SMALLEST axis-aligned clearance between any pair of boxes, in px.
 * Negative means an overlap that deep; `Infinity` for fewer than two boxes.
 *
 * `overlapCount` answers "do any two touch?", which is the right question for
 * the OUTER ring — it is grown to fit its content, so it arrives at zero
 * overlaps with sensible spacing on the way. It is NOT sufficient for the
 * inner ring's ladder, which starts from a fixed tight baseline and is allowed
 * to accept whatever fits there. Measured: 12 specials on that baseline pass
 * `overlapCount === 0` with 2px between two neighbours — technically apart,
 * and exactly the "a little bit overlapping" the owner reported. So the inner
 * ring is judged on DISTANCE, not on a boolean.
 *
 * `max(gx, gy)` and not `min`: two rectangles are separated if they clear on
 * EITHER axis, so the pair's real clearance is the better of the two.
 */
function minClearance(boxes: ChipBox[]): number {
  let m = Infinity;
  for (let i = 0; i < boxes.length; i++) {
    for (let j = i + 1; j < boxes.length; j++) {
      const a = boxes[i], b = boxes[j];
      const g = Math.max(
        Math.abs(a.x - b.x) - (a.w + b.w) / 2,
        Math.abs(a.y - b.y) - (a.h + b.h) / 2,
      );
      if (g < m) m = g;
    }
  }
  return m;
}

/**
 * THE HOLLOW — how far the nearest chip sits from the SPACE pill, in px.
 *
 * The number the owner was describing when he asked to "make sure the compact
 * actually shows always compact and there's not too much hollow space". It is
 * the empty band between the pill and the ring, and until 1.0.96 NOTHING
 * measured it: the ladder's whole acceptance test is about chips hitting each
 * other, and a ring flung far away from the pill passes that test perfectly.
 * Measured on the owner's own labels before the fix, specials hidden:
 *
 *     apps   4     8    12    14    16    18    26
 *     hollow 16    20    51    75   101    34    32     (px)
 *
 * The spike at 16 is the bug (see the `overflow` note in `trial`), and it is
 * only visible because it is measured. Reported on `HudFit` so a bug report
 * can carry it.
 *
 * Same convention as `minClearance` — two rectangles are apart if they clear on
 * EITHER axis, so a pair's real gap is the better of the two. The pill is
 * centred, hence `|x|`/`|y|` against half of `SPACE_W`/`SPACE_H`.
 */
function hollowOf(boxes: ChipBox[]): number {
  let m = Infinity;
  for (const b of boxes) {
    const g = Math.max(
      Math.abs(b.x) - (b.w + SPACE_W) / 2,
      Math.abs(b.y) - (b.h + SPACE_H) / 2,
    );
    if (g < m) m = g;
  }
  return m;
}

/** Which rung of the (a)-(d) ladder the last layout settled on. Exported for
 *  the harness and worth having in a bug report: "step d at 34 chips" says
 *  the ring ran out of screen, not that the maths is wrong. */
/** Rungs of the INNER ring's ladder. s0-s3 condense the specials in place;
 *  s4 is the only rung that grows the ring. See `fitInner`. */
export type SpStep = "s0" | "s1" | "s2" | "s3" | "s4";

export interface HudFit {
  step: "a" | "b" | "c" | "d";
  /** Which rung of the INNER ring's own (inverted) ladder was accepted.
   *  s0-s3 condense the specials; s4 is the only one that grows the ring. */
  spStep: SpStep;
  /** The inter-chip gap the INNER ring settled on (the outer ring's is `gap`). */
  spGap: number;
  /** Smallest measured clearance between two specials at the accepted rung.
   *  `Infinity` when there are fewer than two of them (specials switched off).
   *  In a bug report this is the number that says whether the inner ring is
   *  merely legal or actually readable. */
  spClear: number;
  gap: number;
  rin: number; ryi: number;
  rout: number; ryo: number;
  /** Rim length the outer ring HAS, and the length its content REQUIRES. */
  outerRim: number; outerNeed: number;
  innerRim: number; innerNeed: number;
  overlaps: number;
  win: { w: number; h: number };
  screen: { w: number; h: number };

  /* --- v1.0.89. All measured, all reportable. ----------------------------- */
  /** WHICH GEOMETRY drew this. First thing to read in a bug report: the two
   *  modes fail in completely different ways, and "which ring am I looking
   *  at" is not answerable from a screenshot once both ship. */
  layout: "magnetic" | "classic";
  /** How many APP bands were used, and each band's rx (inside-out).
   *  Classic reports 1 — its single app ring IS one band. */
  bands: number;
  bandRx: number[];
  /** Which band count the user asked for. `"n/a"` in CLASSIC, which has its
   *  own fixed one-app-ring shape and ignores the setting entirely. */
  bandMode: "auto" | "one" | "two" | "n/a";
  /** Smallest measured clearance between two APP chips, in px. The companion
   *  to `overlaps`: zero overlaps alone once passed a pair sitting 2px apart,
   *  which is why the ladder below is judged on BOTH. */
  clear: number;
  /** Furthest chip OUTER EDGE from SPACE, in px — the number `GLANCE_R` caps.
   *  Reported rather than assumed: an inner specials band can legitimately
   *  push it past the ceiling (see `layout`). */
  reach: number;
  /** THE EMPTY MIDDLE, in px: the gap between the SPACE pill and the nearest
   *  chip (`hollowOf`). Small is compact. It is REPORTED, not enforced —
   *  see `trial`'s `overflow` for the thing that is actually decided on. */
  hollow: number;
  /** How much rim the bands are SHORT of what their own chips need, summed
   *  per band, in px. Zero means every band can genuinely hold what it was
   *  given; anything above zero means a band is carrying more content than it
   *  has rim for, which is `fitRx` having saturated at its ceiling.
   *
   *  MAGNETIC ONLY in spirit — classic reports its single ring's shortfall the
   *  same way, from the numbers it already had, so a bug report can compare. */
  overflow: number;
  /** True when the page dropped a specials list Rust DID send, because the
   *  apps turned out to need a second band. That decision is the page's alone
   *  and only in `auto` — see `engine::specials_for_hud`'s truth table. */
  spDropped: boolean;
  /** The at-rest app-label cap published as `--chip-cap`. */
  restCap: number;
}
let _hudFit: HudFit | null = null;
export function lastHudFit(): HudFit | null { return _hudFit; }

/** PROBLEM 77 — chips overlapped ("Up/Dn ×2 Scroll Top/Bottom" over
 *  "Esc Boss Key…", user report). Two causes, both fixed here:
 *
 *  1. Angles were distributed from WIDTH ESTIMATES that capped long labels at
 *     118px and ignored the key badge entirely — "Up/Dn ×2" alone is ~70px.
 *     Wide chips got arc shares far smaller than their real footprint.
 *     → distribute from the chips' MEASURED DOM widths instead.
 *
 *  2. Shares were proportional in ANGLE, but the rings are ellipses
 *     (ry ≈ 0.55·rx): equal angle steps cover very unequal DISTANCE along the
 *     rim, pinching chips together near the top and bottom.
 *     → distribute along the ellipse's ARC LENGTH, sampled numerically, so a
 *     chip's share of the rim is proportional to its real width everywhere.
 *
 *  `gap` adds clearance between neighbours; `off` rotates the ring. */
function arcAngles(
  ws: number[], rx: number, ry: number, gap: number, off: number,
): number[] {
  const N = ARC_N;
  const cum = arcTable(rx, ry);
  const total = cum[N];
  const shares = ws.map((w) => w + gap);
  const tot = shares.reduce((s, w) => s + w, 0) || 1;
  // arc-length position of each chip centre → invert to the parameter angle
  let acc = 0;
  return shares.map((w) => {
    const target = ((acc + w / 2) / tot) * total;
    acc += w;
    let lo = 0, hi = N;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (cum[mid] < target) lo = mid + 1; else hi = mid;
    }
    return (lo / N) * Math.PI * 2 - Math.PI / 2 + off;
  });
}

/* ===========================================================================
   BAND PACKING — the Magnetic Sector core.

   TWO ANGLES LIVE HERE AND THEY ARE NOT THE SAME NUMBER. This caused a real
   bug in the design prototype and it will cause it again if the distinction is
   lost: `arcAngles` returns the ellipse PARAMETER `t`, not the on-screen angle.
   The point it names is `(rx·cos t, ry·sin t)`; the angle a user actually
   points at is `atan2(y, x)`, and on a 0.62 ellipse the two diverge by up to
   ~25°. So: PLACE with `t`, and RELAX / HIT-TEST / AIM THE PLUME with
   `atan2(y, x)`. `setGeoAngle` converts back the other way,
   `t = atan2(sin(geo)·rx, cos(geo)·ry)`.
   =========================================================================== */

/** One chip's input to the packer: its index in the apps payload and its
 *  MEASURED resting box. */
interface BandItem { i: number; w: number; h: number }
/** One band: an ellipse plus the items assigned to it, inside-out. */
interface Band { rx: number; ry: number; cap: number; items: BandItem[] }
/** A placed chip in ring-local coordinates, carrying the band it belongs to so
 *  the relax pass can move it along its OWN ellipse and no other. */
interface Placed {
  i: number; band: number; brx: number; bry: number;
  x: number; y: number; w: number; h: number;
  /** On-screen angle and radius — `atan2(y, x)` and `hypot(y, x)`. */
  geo: number; r: number;
  /** Where arc-length placement originally put it; `RELAX_LIMIT` is measured
   *  from here so ten relax passes cannot cumulatively scramble the alphabet. */
  geo0: number;
}

/** The smallest single-band radius in `[lo, hiR]` whose rim holds `need`.
 *  Perimeter is monotone in rx at a fixed ratio, so a bisection is exact
 *  enough and cheap. This is what lets a 3-chip HUD sit tight against SPACE
 *  instead of being flung out to the ceiling. */
function fitRx(need: number, lo: number, hiR: number): number {
  if (ellipsePerimeter(lo, lo * BAND_RATIO) >= need) return lo;
  if (ellipsePerimeter(hiR, hiR * BAND_RATIO) < need) return hiR;
  let a = lo, b = hiR;
  for (let i = 0; i < 20; i++) {
    const m = (a + b) / 2;
    if (ellipsePerimeter(m, m * BAND_RATIO) < need) a = m; else b = m;
  }
  return b;
}

/**
 * Split `items` across `want` elliptical bands between `lo` and `hiR`.
 *
 * FILL IS INSIDE-OUT, A→Z, AND BY REAL RIM CAPACITY — never by swept angle.
 * The prototype's first attempt broke to a new band after sweeping 2π, and
 * because a band's capacity is arc length rather than angle, each band's tail
 * landed on its own head. A band opens only when the one inside it is
 * genuinely full.
 *
 * `want === 0` means AUTO, and auto PREFERS ONE BAND: it returns the smallest
 * band count whose summed rim capacity covers the content. The caller then
 * MEASURES that answer and only widens it if the real boxes say so — see
 * `layout`. Do not "optimise" this into balancing chips evenly across bands;
 * the owner's instruction is explicit ("auto should prefer one band unless it
 * doesn't fit"), and a half-empty outer band is worse to read than a full
 * inner one.
 */
function packBands(
  items: BandItem[], gap: number, lo: number, hiR: number,
  bandStep: number, want: number,
): Band[] {
  const total = items.reduce((s, it) => s + it.w + gap, 0);
  const maxBands = Math.max(1, Math.floor((hiR - lo) / bandStep) + 1);
  let n = want > 0 ? want : 0;
  if (!n) {
    n = 1;
    while (n < maxBands) {
      let capSum = 0;
      for (let b = 0; b < n; b++) {
        const rx = n === 1 ? hiR : Math.min(hiR, lo + b * ((hiR - lo) / (n - 1)));
        capSum += ellipsePerimeter(rx, rx * BAND_RATIO);
      }
      if (capSum >= total) break;
      n++;
    }
  }
  n = Math.max(1, Math.min(n, maxBands));

  const bands: Band[] = [];
  for (let b = 0; b < n; b++) {
    const rx = n === 1
      ? fitRx(total, lo, hiR)
      : Math.min(hiR, lo + b * ((hiR - lo) / (n - 1)));
    bands.push({ rx, ry: rx * BAND_RATIO,
                 cap: ellipsePerimeter(rx, rx * BAND_RATIO), items: [] });
  }
  let bi = 0, used = 0;
  for (const it of items) {
    while (bi < bands.length - 1 && used + it.w + gap > bands[bi].cap) { bi++; used = 0; }
    bands[bi].items.push(it);
    used += it.w + gap;
  }
  return bands.filter((b) => b.items.length > 0);
}

/* ===========================================================================
   TIGHTENING — v1.0.96, and the whole of "make it actually compact".

   `packBands` sizes a SINGLE band to its content (`fitRx` returns the smallest
   radius whose rim holds it), and that is why three chips sit tight against
   SPACE instead of being flung out to the ceiling. But for n > 1 it does
   something else entirely: it SPREADS the bands evenly from `lo` to `hiR`,
   `rx = lo + b·(hiR − lo)/(n − 1)`. So the outermost band of every multi-band
   ring lands on the CEILING regardless of how little it is carrying, and the
   inner band's floor is computed from the widest chip in the whole payload
   even when that chip is out on the band above it.

   Measured on the owner's 26 real labels, specials hidden, before this pass:

     apps   bands (rx)     furthest chip edge     hollow
     16     332            383                    101      (one band, at the ceiling)
     18     208 / 332      385                     34
     26     208 / 331      384                     32

   The 18-app ring is 385px wide carrying content that fits inside 349, and its
   inner band sits 12px further out than the chips ON it require. Neither costs
   a collision, so nothing in the ladder ever noticed.

   THE PASS. Once the packer has decided WHICH chips go on WHICH band — which
   is the part that has to be done against generous radii, or the inside-out
   fill would break to a new band too early — every band is pulled IN to the
   smallest radius that still holds its own chips and still clears whatever is
   inside it. Inside-out, so each band's floor is the tightened radius of the
   one below plus one `bandStep`.

   IT CAN ONLY EVER MOVE A BAND INWARD (`fitRx`'s ceiling is the band's own
   current radius). The caller MEASURES the result and keeps the untightened
   bands if it does not come out clean — see `trial`.

   THAT SENTENCE USED TO BE A CLAIM AND IS NOW ENFORCED (2026-09-04 review).
   It shipped as `fitRx(need, min(lo, src.rx), MAX(lo, src.rx))`, and the
   `max` was there for a case the header waved away as degenerate — "the floor
   itself is already outside the band, nothing to do". It is not degenerate and
   there was plenty to do: `solveAt` computes the packer's floor as
   `Math.min(hiCeil, RIN_CLEAR + outerHalf + BAND_LO_PAD)`, so on a display too
   small to hold the dead zone the CEILING clamps `lo` down, while `floor0`
   here is NOT clamped. `lo > src.rx` then, `max` becomes the floor, and the
   band is PUSHED OUT past `hiScreen` — the very bound `hiScreen` exists to
   respect. Measured on the standalone transcription of this path, one band,
   `bandMode` auto/one/two alike:

     display    chips        loose rx -> tight rx      outer edge past the
                                                       94% window (pre-scale)
     640x480    5 @212px     162.8 -> 220.9  (+58.1)   19.4px
     640x480    6..40 @212   162.8 -> 245.0  (+82.2)   up to 12.1px after the
                                                       final uniform clamp
     800x600    6..40 @212   238.0 -> 245.0  (+7.0)    0px

   And nothing caught it, because a ring pushed OUTWARD has more rim than it
   needs: `trial` prefers the tightened bands on `overlaps === 0 && clear >=
   RING_GAP_MIN`, both of which a too-large ring passes trivially. The cost
   lands where the page cannot see it — `overlay_fit_hud` clamps the window to
   94% of the monitor and CROPS whatever sticks out, silently.

   THE FIX IS THE INVARIANT, SPELLED OUT: the ceiling handed to `fitRx` is
   `src.rx` and nothing else. When `lo > src.rx` both ends collapse onto
   `src.rx` and the band is returned exactly where `packBands` put it, which is
   the honest answer — the floor cannot be honoured at this ceiling, so
   tightening has nothing to offer and must not pretend otherwise. When
   `lo <= src.rx` (every case on a normal display) `min(lo, src.rx)` is `lo`
   and `src.rx` is `max(lo, src.rx)`, i.e. byte-identical to what shipped.

   A SINGLE-band ring therefore genuinely cannot move now: with one band its
   own widest chip IS the payload's widest chip and its own content IS the
   whole content, so floor and radius come out where `packBands` already put
   them — and where they used to diverge (the clamped-ceiling case above) the
   band is pinned instead of flung. Every 1..~15-app ring is byte-identical to
   1.0.95.
   =========================================================================== */

/** The smallest radius the innermost band may take, given the widest chip
 *  ACTUALLY on it. This is the "dead zone": inside it there is nothing to aim
 *  at, so every pixel of it is empty middle. */
type Floor0 = (halfWidest: number) => number;

/** Pull each band in to the smallest radius that holds its own chips.
 *
 *  NEVER OUTWARD — `src.rx` is the ceiling, unconditionally, and that is the
 *  one property the caller relies on (see the header above for what it cost
 *  when it was only a comment). Never inside `floor0` (band 0) or one
 *  `bandStep` of the band below EITHER, except in the one case where those two
 *  bounds cannot both hold: a floor already outside the band. There the
 *  no-push rule wins and the band is left exactly where `packBands` put it.
 *  Returns a NEW array — the caller keeps the original to fall back to. */
function tightenBands(
  bands: Band[], gap: number, bandStep: number, floor0: Floor0,
): Band[] {
  const out: Band[] = [];
  for (let b = 0; b < bands.length; b++) {
    const src = bands[b];
    const need = src.items.reduce((s, it) => s + it.w + gap, 0);
    const half = src.items.length
      ? Math.max(...src.items.map((it) => it.w)) / 2 : 0;
    const lo = b === 0 ? floor0(half) : out[b - 1].rx + bandStep;
    /* `src.rx` IS THE CEILING, ALWAYS — that is what makes this a pull and
       never a push, and it is the whole of the header's invariant. `lo` can
       legitimately land OUTSIDE the band (`solveAt` clamps the packer's floor
       to the ceiling on a small display and `floor0` is not clamped); when it
       does, `min` and the ceiling collapse onto `src.rx` and the band comes
       back untouched. Do not restore a `Math.max(lo, src.rx)` here: it reads
       like a guard and is a licence to grow past `hiScreen`, where the window
       clamp crops the chips and nothing in the page can observe it. */
    const rx = fitRx(need, Math.min(lo, src.rx), src.rx);
    out.push({
      rx, ry: rx * BAND_RATIO,
      cap: ellipsePerimeter(rx, rx * BAND_RATIO), items: src.items,
    });
  }
  return out;
}

/** Rim a band is SHORT of what its own chips need, summed over the bands.
 *
 *  THE MEASUREMENT THE AUTO SEARCH WAS MISSING. `fitRx` SATURATES: asked for a
 *  radius whose rim holds `need` and given a ceiling that cannot, it returns
 *  the ceiling and says nothing. `packBands` then hands back a band carrying
 *  more content than it has rim for, and `arcAngles` distributes that content
 *  proportionally — which is the only honest thing it can do with an impossible
 *  budget, and it means the chips come out spaced by LESS than their own
 *  widths. Usually that shows up as an overlap and the ladder catches it. At 16
 *  of the owner's labels it did not: 1734px of content on a 1711px rim missed
 *  by 23px, no pair intersected, `clear` measured 19px — and `auto` accepted a
 *  ring pinned to the ceiling with a 101px hole in the middle.
 *
 *  So "one band does not fit" is now measured instead of being inferred from a
 *  collision that may or may not happen. That IS the owner's rule — "auto
 *  should prefer one band unless it doesn't fit" — made true.
 *
 *  UNITS: `b.cap` comes from `b.rx`, and `it.w` is a chip's `offsetWidth`.
 *  BOTH are pre-scale — the ring's `scale` is a CSS transform on `#st-hud`, and
 *  a transform does not touch `offsetWidth`. So this comparison is already in
 *  one consistent space and must stay that way. Multiplying only the cap by
 *  `scale` (tried, 1.0.96) inflates the result into fiction: at scale .93 the
 *  26-app/no-specials set reported a 158px SHORTFALL against bands that
 *  actually had ~97px of SLACK. */
function bandOverflow(bands: Band[], gap: number): number {
  let over = 0;
  for (const b of bands) {
    const need = b.items.reduce((s, it) => s + it.w + gap, 0);
    if (need > b.cap) over += need - b.cap;
  }
  return over;
}

/** Lay each band's items out along its rim by ARC LENGTH (`arcAngles`), band
 *  `i` staggered by `i · π / count_i` so an outer chip does not sit directly
 *  over an inner one. */
function placeBands(bands: Band[], gap: number): Placed[] {
  const out: Placed[] = [];
  bands.forEach((b, bi) => {
    const off = bi * (Math.PI / Math.max(1, b.items.length));
    const ts = arcAngles(b.items.map((it) => it.w), b.rx, b.ry, gap, off);
    ts.forEach((t, k) => {
      const it = b.items[k];
      const x = Math.cos(t) * b.rx, y = Math.sin(t) * b.ry;
      const geo = Math.atan2(y, x);
      out.push({ i: it.i, band: bi, brx: b.rx, bry: b.ry,
                 x, y, w: it.w, h: it.h, geo, r: Math.hypot(x, y), geo0: geo });
    });
  });
  return out;
}

/** Move a placed chip to a GEOMETRIC angle, staying on its own ellipse. */
function setGeoAngle(c: Placed, ang: number): void {
  const t = Math.atan2(Math.sin(ang) * c.brx, Math.cos(ang) * c.bry);
  c.x = Math.cos(t) * c.brx;
  c.y = Math.sin(t) * c.bry;
  c.geo = Math.atan2(c.y, c.x);
  c.r = Math.hypot(c.x, c.y);
}

/** Nudge chips on DIFFERENT bands apart until each owns at least `RELAX_FLOOR`
 *  of angle. Same-band neighbours are left alone: arc-length placement already
 *  spaced them by their real widths, and pushing them would undo that.
 *  Movement is capped at `RELAX_LIMIT` from where each chip started so ten
 *  passes cannot scramble alphabetical order. */
function relaxAngles(chips: Placed[]): void {
  if (chips.length < 2) return;
  for (let pass = 0; pass < 10; pass++) {
    const order = chips.map((c, k) => ({ k, g: c.geo })).sort((a, b) => a.g - b.g);
    let moved = 0;
    for (let k = 0; k < order.length; k++) {
      const A = chips[order[k].k];
      const B = chips[order[(k + 1) % order.length].k];
      if (A === B || A.band === B.band) continue;
      const d = Math.atan2(Math.sin(B.geo - A.geo), Math.cos(B.geo - A.geo));
      if (Math.abs(d) >= RELAX_FLOOR) continue;
      const push = ((RELAX_FLOOR - Math.abs(d)) / 2) * (d >= 0 ? 1 : -1);
      const na = A.geo - push, nb = B.geo + push;
      if (Math.abs(Math.atan2(Math.sin(na - A.geo0), Math.cos(na - A.geo0))) < RELAX_LIMIT) {
        setGeoAngle(A, na); moved++;
      }
      if (Math.abs(Math.atan2(Math.sin(nb - B.geo0), Math.cos(nb - B.geo0))) < RELAX_LIMIT) {
        setGeoAngle(B, nb); moved++;
      }
    }
    if (!moved) break;
  }
}

/* ===========================================================================
   THE TWO SETTINGS THIS FILE READS AT LAYOUT TIME.

   `hud_layout`: "magnetic" | "classic"  — WHICH GEOMETRY.
     "classic" is the ring that shipped in 1.0.88, unchanged: two rings, the
     outer one grown to fit its content, the 0.55 aspect cap on the
     no-specials path, full labels everywhere, no bloom. "magnetic" is the
     v1.0.89 Magnetic Sector geometry. Both are SHIPPING options, not a new
     one and a legacy one — the owner asked for the choice explicitly, and the
     value of the choice is that "classic" is a true no-op: flip one switch
     and you are back on the thing you have been using all week.

   `hud_band_count`: "auto" | "one" | "two" — HOW MANY APP BANDS, and it counts
     APP bands only. A MAGNETIC concept: classic has its own fixed one-app-ring
     shape and ignores this entirely. Specials-on + "one" means a specials band
     PLUS one app band; specials-on + "two" is impossible and Rust already
     sends an empty specials vec for it (engine::specials_for_hud), so this
     file never has to handle that combination.

   BOTH ARE READ AT THE MOMENT THE RING IS LAID OUT, never at import: the
   values arrive asynchronously from `get_config`, so an import-time read gets
   a stale default. `hud-band-count.ts` owns the band count and its
   normalisation; `hud-layout.ts` owns the layout choice on the same pattern.

   RESOLVED ONCE PER HUD SHOW. `buildHud` reads both at build time and nothing
   re-reads them while Space is held, so neither the layout nor the band count
   can flicker mid-hold — they can only differ between holds, and only if the
   user changed a setting or their bindings. Do not move either read onto the
   pointer path. */
type HudLayoutMode = "magnetic" | "classic";
/**
 * Which geometry to draw.
 *
 * Read through the `window.__stHudLayout` MIRROR rather than by importing
 * `hud-layout.ts`, deliberately and temporarily: that module is being created
 * by another agent in parallel with this change, and an import of a file that
 * does not exist yet fails the build for everybody. The mirror is part of the
 * module's published contract and is written by the same setter that backs
 * `getHudLayout()`, so the value is identical.
 *
 * WHEN `src/components/hud-layout.ts` EXISTS, replace the body with
 * `return getHudLayout();` and add the import — that is the whole change, and
 * the normalisation below can go with it because the module does it.
 *
 * Normalisation, until then: only the exact string "classic" means classic.
 * `undefined` (every config written before 1.0.89), "", a typo, a number — all
 * of it lands on "magnetic", which is the shipped default. Erring toward the
 * default is right here: a value that fell through to "classic" would quietly
 * give a user the OLD ring with nothing on screen to explain it.
 */
function hudLayoutMode(): HudLayoutMode {
  try {
    return (window as unknown as { __stHudLayout?: string }).__stHudLayout === "classic"
      ? "classic" : "magnetic";
  } catch {
    return "magnetic";
  }
}

/* ===========================================================================
   THE PROFILE EMOJI ON THE SPACE PILL. 1.0.96.

   The owner wanted the active profile's emoji visible where the ring already
   draws his attention, and the SPACE pill is the one element that is always
   there and always in the middle. Three constraints shaped what it became:

   1. **It must not fight the wordmark.** "SPACE" is the pill — 800 weight,
      .22em tracking, dead centre — and it is what tells a new user what the
      ring is for. So the emoji is pinned ABSOLUTELY to the pill's left inner
      edge instead of joining the flex row. Adding it as a sibling flex item
      would push "SPACE" off-centre by half the glyph, which is visible on a
      230px pill and would make the whole HUD look mis-hung.

   2. **Absent means BYTE-IDENTICAL, not "similar".** With no emoji this
      appends no element at all — not a hidden one, not an empty one. The pill's
      box, its centring and its `st-space-pop` are exactly what they were in
      every previous build, which is the contract `Profile::emoji` states for
      every surface that renders it.

   3. **It is TEXT, set with textContent, and that is not a style choice.**
      `emoji_is_valid` (schema.rs) is deliberately permissive — "anything that
      renders as one glyph", so kaomoji and symbols pass, and so does a bare
      "<". Interpolating that into `innerHTML` would be a live injection path
      from a hand-edited config.json into the overlay document. A text node
      cannot be one.

   Palettes: the glyph carries its own colour, so all four (earthy, nocturne,
   warcry, starry) get the same treatment with no per-theme rule — which is
   also why it has no disc or plate behind it. Reduced motion: its one
   animation is switched off by a `:root.reduced-motion` guard in the
   stylesheet, alongside the pill's own.
   =========================================================================== */
/** 1.0.119 (brief 4 §2) — the centre pill's markup: the focused app's icon
 *  and short name, or the wordmark when there is no app to name. The class
 *  stays `.space` so every rule, measurement (`spaceBox`) and animation that
 *  targets the pill is untouched; `.st-focus` only restyles the contents.
 *  Only a `data:` icon is accepted — the overlay's CSP allows no host. */
function spacePillHtml(focus: GuideHudPayload["focus"]): string {
  const name = focus && typeof focus.name === "string" ? focus.name.trim() : "";
  if (!name) return '<div class="space">SPACE</div>';
  const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/"/g, "&quot;");
  const icon = typeof focus?.icon === "string" && focus.icon.startsWith("data:image/")
    ? `<img class="st-focus-ico" src="${focus.icon}" alt="" draggable="false">`
    : "";
  return `<div class="space st-focus" title="${esc(name)}">${icon}<span class="st-focus-name">${esc(name)}</span></div>`;
}

function paintSpacePill(emoji: string | null | undefined): void {
  if (!_hudEl) return;
  const pill = _hudEl.querySelector<HTMLElement>(".space");
  if (!pill) return;
  const text = typeof emoji === "string" ? emoji.trim() : "";
  // Nothing to show → nothing in the DOM. `buildHud` has just rewritten
  // `innerHTML`, so there is never a stale one to remove; this is the
  // "byte-identical" branch and it does nothing on purpose.
  if (!text) return;
  const el = document.createElement("span");
  el.className = "st-space-emoji";
  // Decorative: the pill already reads "SPACE" to a screen reader, and an
  // emoji's own name read aloud beside it would be noise.
  el.setAttribute("aria-hidden", "true");
  el.textContent = text;
  pill.appendChild(el);
}

function buildHud(payload: GuideHudPayload, entranceDelay = 0): Promise<Rect | null> {
  if (!_hudEl) return Promise.resolve(null);
  /* ONE BUILD, ONE ARC-TABLE MEMO. Cleared here rather than at the end so an
     early return further down can never leave a stale table behind, and so the
     memo's lifetime is exactly the thing it is keyed against — the radii of
     THIS build, on THIS display, for THIS label set. */
  clearArcCache();
  const specials = payload.specials ?? [];
  const apps = payload.apps ?? [];   // already only assigned letters

  /* BOTH SETTINGS ARE READ EXACTLY ONCE, HERE, and held for the whole build.
     Read at layout time and not at import (the values arrive asynchronously
     from `get_config`), but ALSO not re-read per rung or per pointer event:
     a band count that could change mid-hold would let the ring re-shape under
     a stationary cursor, and a layout mode that could change mid-hold would
     be worse. They can only differ BETWEEN holds.

     A PREVIEW SUBSTITUTES BOTH, FOR THIS BUILD ONLY. The override arrives in
     the payload and is read exactly where the settings are read, so every rung
     of the ladder below, the specials drop, the bloom flag and the reported
     `fit` all see one consistent answer — the same guarantee the settings
     themselves get. Nothing is written and nothing is remembered: a resize
     rebuild re-reads `_lastPayload`, which still carries the override while the
     preview is up and stops carrying it the moment a real hold replaces it. */
  const preview = previewOverride(payload);
  const mode = preview ? preview.mode : hudLayoutMode();
  const bandMode = preview ? preview.bands : getBandCount();

  // NO glow element here — a large blurred surface makes this transparent
  // window compose zero pixels. See the removal note at the top of
  // overlay-earthy.css before adding one back.
  //
  // `.st-beam` is the THRUSTER PLUME (see paintArmedBeam below). It is written
  // BEFORE `.space` and before every chip on purpose: all four are positioned
  // with `z-index: auto`/`0` inside #st-hud's own stacking context, so paint
  // order IS tree order — the plume passes BEHIND the SPACE pill and behind
  // every label instead of lying across them. Its three children are gradient
  // layers, never filters (PROBLEM 37); the stylesheet block spells that out.
  _hudEl.innerHTML =
    '<div class="pulse"></div>' +
    '<div class="st-beam">' +
      '<div class="jet"></div><div class="lick"></div><div class="core"></div>' +
    '</div>' +
    spacePillHtml(payload.focus);
  paintSpacePill(payload.profile_emoji);
  // Same handover mechanism as --hud-in/--hud-out: the nominal beam box is
  // owned HERE, in TS, and the stylesheet reads it. paintArmedBeam divides by
  // BEAM_NOM to get its scaleX, so a literal in the CSS would be a second,
  // silently-diverging source of truth for the same number.
  _hudEl.style.setProperty("--beam-nom", `${BEAM_NOM}px`);
  /* THE REST CAP IS A CUSTOM PROPERTY, NOT AN INLINE `max-width`, AND THAT IS
     NOT A STYLE PREFERENCE. An inline `max-width` on the span would beat
     `#st-hud.tight-chips .st-chip.ap span` outright and silently disable the
     (d) concession rung — a whole rung of the ladder gone with nothing to see.
     Handed over as a variable, the STYLESHEET decides precedence and (d) can
     still `min()` its way under it. Same handover mechanism as --hud-in /
     --hud-out / --beam-nom: the number is owned here and read there. */
  _hudEl.style.setProperty(
    "--chip-cap", `${mode === "classic" ? FULL_CAP : REST_CAP_BACKSTOP}px`);
  /* THE BLOOM CAPS START UNSET ON EVERY BUILD, and that is not tidiness.
     `_hudEl` survives between holds, so a HUD that once hit the degenerate
     branch below and had `--bloom-pill` tightened would keep that tightened
     pill for every later hold — the same "wears its shrunken chips forever"
     failure the (a)-(d) ladder resets its classes to avoid. Unset means the
     stylesheet's `none` applies and an armed label opens completely. */
  _hudEl.style.removeProperty("--bloom-pill");
  _hudEl.style.removeProperty("--bloom-cap");
  // The chips below are brand new, so the resting snapshot that described the
  // PREVIOUS set is now a lie. Cleared here rather than at the end, so nothing
  // between here and the layout can read a stale box.
  _apRest = [];
  // The element above is BRAND NEW and therefore carries no `.on`. Saying so
  // makes the next arming take the JUMP path rather than sweeping in from
  // 0rad — see paintArmedBeam.
  _beamOn = false;
  if (entranceDelay > 0 && !REDUCED()) {
    const pulseEl = _hudEl.querySelector(".pulse") as HTMLElement | null;
    if (pulseEl) pulseEl.style.animationDelay = `${entranceDelay}ms`;
  }

  // PROBLEM 77 — build ALL chips first (unpositioned), measure their REAL
  // rendered widths, and only then compute the ring geometry and angles.
  // Estimates remain solely as a fallback for a zero measurement.
  const make = (a: [string, string], special: boolean, icon: string | null = null): HTMLDivElement => {
    const c = document.createElement("div");
    c.className = "st-chip " + (special ? "sp" : "ap");
    const inner = document.createElement("i");
    const kbd = document.createElement("kbd");
    kbd.textContent = a[0];
    /* PROBLEM 267 round 3 — the app's real icon IN PLACE OF the letter disc,
       with the letter as a small badge at the icon's top-right, exactly as
       a ring tile wears it. Same 20 px box as the disc, so the chip's
       measured width (below) and everything laid out from it are the same
       as with a disc; the badge is absolute and adds no width. A broken
       data URL falls back to the disc, never to a broken-image glyph. */
    if (icon) {
      const ico = document.createElement("span");
      ico.className = "st-ico";
      const img = document.createElement("img");
      img.alt = "";
      img.draggable = false;
      img.src = icon;
      img.addEventListener("error", () => { ico.replaceWith(kbd); });
      const badge = document.createElement("b");
      badge.className = "st-ico-badge";
      badge.textContent = a[0];
      ico.append(img, badge);
      inner.appendChild(ico);
    } else {
      inner.appendChild(kbd);
    }

    // A key pinned to a browser profile arrives as "Brave — STUDIES", built by
    // Rust's `browser_profiles::hud_label` from the binding's stored
    // `browser_profile_name`. (Stored at pick time precisely so the Space-hold
    // path never opens the browser's ~96 KB Local State — that is the entire
    // reason the field exists.)
    //
    // Split into two spans so the BROWSER NAME HOLDS and the PROFILE NAME
    // GIVES: "Chrome — Thesis — Literature Rev…" still tells you it is Chrome,
    // which is the half you needed. One span with a 118px cap truncates
    // whichever half happens to be longer, and "Brave — ST…" on three keys is
    // the situation this feature exists to end.
    //
    // Guarded on the exact separator Rust emits (space, em dash, space) and
    // split ONCE, so a profile whose own name contains an em dash keeps it.
    const cut = a[1].indexOf(" — ");
    if (cut > 0) {
      const browser = document.createElement("span");
      browser.className = "st-chip-browser";
      browser.textContent = a[1].slice(0, cut);
      const sep = document.createElement("span");
      sep.className = "st-chip-sep";
      sep.textContent = "—";
      const profile = document.createElement("span");
      profile.className = "st-chip-profile";
      profile.textContent = a[1].slice(cut + 3);
      inner.append(browser, sep, profile);
    } else {
      // No profile pinned → the browser name alone. No dash, no gap, never a
      // dangling separator.
      //
      // `.st-chip-label` marks the ONE span whose text is swapped between the
      // resting first word and the full label on bloom (`setChipLabel`). The
      // three-span profile form deliberately has no such marker: its browser
      // half already holds and its profile half already gives, and replacing
      // that with "Brave" on three keys would undo the very feature it exists
      // to provide. Those chips clip on `--chip-cap` alone and open to the
      // full 118px on bloom like everything else.
      const span = document.createElement("span");
      span.className = "st-chip-label";
      span.textContent = a[1];
      inner.appendChild(span);
    }
    c.appendChild(inner);
    // SLINGSHOT - the flight has to find the launched app's chip. The label is
    // the only thing a toast ("Brave launched") and a chip ("Brave") share.
    c.dataset.stApp = a[1].toLowerCase();
    _hudEl!.appendChild(c);
    return c;
  };
  // `let`, not `const`: in `auto` the page may DROP a specials list Rust did
  // send, once the measured labels prove the apps need a second band. See the
  // specials-drop block below `layout`.
  let spChips = specials.map((a) => make(a, true));
  const icons = Array.isArray(payload.app_icons) ? payload.app_icons : [];
  const apChips = apps.map((a, i) => make(a, false, icons[i] ?? null));
  /** True once this build dropped the specials itself. Read by `layout`. */
  let _spDropped = false;

  /* WORD CLIPPING — the owner's decision 1, and the reason it is a WORD.
     "clip to the first word, let the first word complete for most stuff, clip
     if the name is too long, and full label at aim." A whole word is
     recognisable ("Google", "Samsung", "Intellij"); a severed one is not
     ("Discor…"), which is what the design handoff's px-based restCap produced
     and what was rejected. `--chip-cap` survives only as a BACKSTOP for a
     first word that is itself a URL or a path — see REST_CAP_BACKSTOP.

     APP CHIPS ONLY. Specials sit inside the dead zone, can never be aimed at
     and therefore never bloom, so a clipped special would be clipped forever
     ("Force" for "Force Close App"). They keep their full labels at rest. */
  const apRest: [string, string][] = apps.map(
    (a) => [a[0], mode === "classic" ? a[1] : restWord(a[1])]);
  apChips.forEach((c, i) => {
    c.dataset.stFull = apps[i][1];
    c.dataset.stRest = apRest[i][1];
    setChipLabel(c, apRest[i][1]);
  });
  /* CLASSIC DOES NOT BLOOM. 1.0.88 had no bloom, so switching to classic must
     not introduce one — and with `apRest === apps` above there would be
     nothing to reveal anyway. The flag is what `paintBloom` reads, so the
     armed chip in classic gets exactly the treatment it has always had: the
     accent fill, the inverted badge, the halo, the plume, and no movement. */
  _bloomOn = mode === "magnetic";

  const width = (c: HTMLDivElement, a: [string, string], special: boolean) => {
    const w = (c.firstElementChild as HTMLElement | null)?.offsetWidth ?? 0;
    return w > 0 ? w : estW(a[1], special);
  };
  /** Chip HEIGHT matters as much as width: on the LEFT and RIGHT flanks of the
   *  ellipse neighbours stack vertically, and there it is the height, not the
   *  width, that keeps them apart. The overlap test needs both. */
  const height = (c: HTMLDivElement, special: boolean) => {
    const h = (c.firstElementChild as HTMLElement | null)?.offsetHeight ?? 0;
    return h > 0 ? h : (special ? 34 : 30);
  };

  const vw = window.screen.availWidth || window.innerWidth;
  const vh = window.screen.availHeight || window.innerHeight;
  const budgetW = SCREEN_BUDGET * vw;
  const budgetH = SCREEN_BUDGET * vh;

  /* =====================================================================
     THE INNER RING'S OWN LADDER — CONDENSE FIRST, GROW LAST.  v1.0.88.

     PROBLEM 208 gave BOTH rings the same ladder, whose first rung is GROW.
     For the outer ring that is right and it is untouched. For the inner ring
     it produced exactly the complaint the owner then filed: the specials'
     rim went 1156px (overlapping) -> 1495px (exact fit), which is a 29%
     larger ellipse, and the specials ended up at arm's length from SPACE.
     His words:

       "Special keys can be a little more condensed towards the Space. The
        special keys are not doing anything for the moving-cursor-to-launch
        thing, so it's taking up too much space... Like how it was before —
        the size was good, but it was a little bit overlapping."

     So the priority is INVERTED here, and only here. The ring starts at the
     old tight baseline (RIN_CLEAR + half the widest special + RING_CLEAR,
     RYI_BASE) — the geometry he liked — and every rung spends the CONTENT
     rather than the radius:

       s0  tight baseline, preferred gap                (nothing conceded)
       s1  gap -> RING_GAP_MIN
       s2  + `.dense-sp`  — special padding/font/badge, one bounded step
       s3  + `.tight-sp`  — the special LABEL truncation cap, hardest step
       s4  grow, minimally, and only because s3 still collides

     s3 is a bigger concession than the outer ring's equivalent on purpose:
     an app chip's label is the thing you are choosing between, while a
     special's key BADGE is the actionable half and its label is glanced.
     Owner, same message: "they are glanced, not read".

     "Grows along with the growing name in ratio" is not a special case — it
     already falls out of the baseline: `rx0` is measured from the WIDEST
     special at the current rung, so a longer name moves the ring out
     proportionally and a condensing rung pulls it straight back in.

     ZERO overlaps is the acceptance test, measured with the same
     `overlapCount` the outer ring uses, over the inner boxes only. (Inner
     against OUTER cannot collide by construction: `rout0` below is built
     from this ring's accepted radius plus both half-widths plus RING_CLEAR.)
     ===================================================================== */
  const SP_LADDER: {
    rung: SpStep; gap: number; dense: boolean; tight: boolean; grow: boolean;
  }[] = [
    { rung: "s0", gap: RING_GAP_PREF, dense: false, tight: false, grow: false },
    { rung: "s1", gap: RING_GAP_MIN,  dense: false, tight: false, grow: false },
    { rung: "s2", gap: RING_GAP_MIN,  dense: true,  tight: false, grow: false },
    { rung: "s3", gap: RING_GAP_MIN,  dense: true,  tight: true,  grow: false },
    { rung: "s4", gap: RING_GAP_MIN,  dense: true,  tight: true,  grow: true  },
  ];

  interface InnerFit {
    rung: SpStep; gap: number;
    rx: number; ry: number;
    /** Angles + measured widths/heights AT THE ACCEPTED RUNG. The caller must
     *  not re-derive these: the widths only exist while that rung's classes
     *  are on the element, which is the state this function exits in. */
    as: number[]; w: number[]; h: number[];
    half: number; tall: number;
    rim: number; need: number; overlaps: number;
    /** Smallest measured clearance between two specials, in px. */
    clear: number;
  }

  /** Walk SP_LADDER and return the first rung with zero measured overlaps.
   *  `outerGap` is the OUTER ring's current gap — used only as the floor for
   *  s0, so the two rings still start from the same preference. */
  const fitInner = (
    outerGap: number, maxRoutBudget: number, maxRyoBudget: number,
    outerHalf: number, outerTall: number,
  ): InnerFit => {
    // No specials at all — the setting is off and Rust sent an empty list.
    // Return a ring that exists only as a number, take no measurements, and
    // above all do not call Math.max() on an empty array (that is -Infinity,
    // and -Infinity propagates into every radius downstream as NaN, which
    // lays every chip out at `calc(50% + NaNpx)` — i.e. nowhere).
    if (spChips.length === 0) {
      _hudEl!.classList.remove("dense-sp", "tight-sp");
      return {
        rung: "s0", gap: outerGap, rx: RIN_CLEAR, ry: RYI_BASE,
        as: [], w: [], h: [], half: 0, tall: 0,
        rim: 0, need: 0, overlaps: 0, clear: Infinity,
      };
    }
    let last: InnerFit | null = null;
    for (const r of SP_LADDER) {
      _hudEl!.classList.toggle("dense-sp", r.dense);
      _hudEl!.classList.toggle("tight-sp", r.tight);
      // Re-measured every rung: `.dense-sp`/`.tight-sp` change the real
      // rendered widths, and a rung judged on the previous rung's numbers is
      // the PROBLEM 77 estimate bug wearing a new hat.
      const gap = r.rung === "s0" ? Math.max(outerGap, RING_GAP_MIN) : r.gap;
      const w = spChips.map((c, i) => width(c, specials[i], true));
      const h = spChips.map((c) => height(c, true));
      const half = Math.max(...w) / 2;
      const tall = Math.max(...h);
      const rx0 = RIN_CLEAR + half + RING_CLEAR;   // the old tight baseline
      let rx = rx0, ry = RYI_BASE;
      let rim = ellipsePerimeter(rx, ry);
      const need = w.reduce((s, v) => s + v, 0) + w.length * gap;
      if (r.grow) {
        const g = growRing(
          w, rx0, RYI_BASE, gap,
          Math.max(rx0, maxRoutBudget - half - outerHalf - RING_CLEAR),
          Math.max(RYI_BASE, maxRyoBudget - (tall + outerTall) / 2 - RING_CLEAR),
        );
        rx = g.rx; ry = g.ry; rim = g.have;
      }
      const as = arcAngles(w, rx, ry, gap, 0);
      const boxes: ChipBox[] = as.map((a, i) => ({
        x: Math.round(Math.cos(a) * rx), y: Math.round(Math.sin(a) * ry),
        w: w[i], h: h[i],
      }));
      last = {
        rung: r.rung, gap, rx, ry, as, w, h, half, tall,
        rim, need, overlaps: overlapCount(boxes), clear: minClearance(boxes),
      };
      // TWO conditions, and the second is the one the owner's complaint is
      // actually about. "Zero overlaps" alone accepted 12 specials at the
      // tight baseline with 2px between two of them (measured) — apart, and
      // unreadable. RING_GAP_MIN is reused as the bar rather than a fresh
      // number because that constant already carries exactly this meaning:
      // "below this, chips start touching in the diagonal quadrants".
      // s4 is the last rung and is accepted whatever it measures — growing is
      // the concession of last resort, and there is nothing after it.
      if (last.overlaps === 0 && last.clear >= RING_GAP_MIN) break;
    }
    const f = last!;
    // Leave the DOM wearing the ACCEPTED rung's classes. It already does when
    // the loop broke on that rung — this is here so that a future edit which
    // reorders or short-circuits the loop cannot ship chips rendered at one
    // rung and placed at another. Toggling a class to the value it already
    // holds costs no reflow.
    const acc = SP_LADDER.find((r) => r.rung === f.rung)!;
    _hudEl!.classList.toggle("dense-sp", acc.dense);
    _hudEl!.classList.toggle("tight-sp", acc.tight);
    return f;
  };

  /* =====================================================================
     THE SHARED RESULT SHAPE. Both geometries produce exactly this, so
     everything downstream of the ladder — `put`, the resting snapshot, the
     window fit, the exhausted-log — is written once and works for either.

     `spPos`/`apPos` are ring-local CENTRES in px, already screen-clamped.
     Positions and not angles: classic places on two ellipses and magnetic
     places on N bands with a relax pass, and a shared "angle" would have to
     mean two different things.
     ===================================================================== */
  interface LayoutOut {
    fit: HudFit;
    spPos: { x: number; y: number }[];
    apPos: { x: number; y: number }[];
    apW: number[]; apH: number[];
  }

  /* =====================================================================
     ONE RUNG OF THE LADDER — CLASSIC (the ring that shipped in 1.0.88).

     THIS IS NOT LEGACY CODE AND IT IS NOT DEAD. The owner asked for the
     choice — "give an option to use this new HUD layout or old layout" — and
     the entire value of that option is that this branch is a TRUE NO-OP
     against 1.0.88: same radii, same ladder, same aspect cap, same full
     labels, no bloom. If Magnetic turns out to feel wrong he flips one
     switch and is back on the ring he has been using all week. So: measure
     any change to this function against the 1.0.88 numbers before making it,
     and if you cannot, do not make it.

     The body below is the 1.0.88 `layout()` verbatim; only its RETURN was
     adapted, from `{ fit, asIn, asOut }` (two rings, two angle arrays) to the
     shared `LayoutOut` above, so that the shared `put` can place either
     geometry. The angle -> position conversion here is exactly the arithmetic
     `put` used to do inline, `cos(a)*rx` / `sin(a)*ry`, with the SAME already-
     scaled radii — so every chip lands on the same pixel it did before.

     PROBLEM 208's ladder, in the owner's stated order of preference. Each
     step is only reached if the one before it could not make the ring fit:
       (a) GROW the ring until the rim can hold the content, bounded by the
           screen budget (94%, matching Rust's own clamp in overlay_fit_hud).
       (b) TIGHTEN the inter-chip gap from RING_GAP_PREF toward RING_GAP_MIN.
       (c) SHRINK chip padding/font by a small bounded amount (`.dense-chips`).
       (d) TIGHTEN the label truncation cap last (`.tight-chips`, 118 -> 92).
     ===================================================================== */
  const layoutClassic = (step: HudFit["step"], gap: number): LayoutOut => {
    const apW = apChips.map((c, i) => width(c, apps[i], false));
    const apH = apChips.map((c) => height(c, false));
    const outerHalf = apW.length ? Math.max(...apW) / 2 : 0;
    const outerTall = apH.length ? Math.max(...apH) : 0;

    // How far the OUTER ring may reach before the window it implies would be
    // clamped by Rust. Written as a window budget, not a viewport heuristic:
    // the number that actually matters is the one `overlay_fit_hud` asks for.
    const maxRoutBudget = (budgetW - HUD_PAD_MIN) / 2 - outerHalf;
    const maxRyoBudget = (budgetH - HUD_PAD_MIN) / 2;

    // --- inner ring (specials) — ITS OWN, INVERTED LADDER -----------------
    // See fitInner. Condense the specials down four rungs before conceding a
    // single pixel of radius; the outer ring's ladder is untouched.
    const gi = fitInner(gap, maxRoutBudget, maxRyoBudget, outerHalf, outerTall);
    const spW = gi.w, spH = gi.h;
    const innerHalf = gi.half, innerTall = gi.tall;
    const hasSp = spChips.length > 0;

    // --- outer ring (apps) ------------------------------------------------
    // Its floor is the collision-safe minimum against the inner ring — on
    // BOTH axes. `ryo` used to be the constant 196 no matter what `ryi` was;
    // once the inner ring can grow, the outer one has to clear it vertically
    // as well as horizontally or the two rings meet at the top and bottom.
    //
    // WITH NO SPECIALS AT ALL (the "show special keys on the ring" setting is
    // off, and Rust sends an EMPTY `specials`) there is no inner ring to
    // clear, so the outer ring's floor is the SPACE pill itself. Written as an
    // explicit branch rather than left to fall out of `innerHalf === 0`: that
    // arithmetic would still add the phantom ring's own radius and push every
    // app chip ~26px further out than it needs to be, for a ring that is not
    // on screen.
    const rout0 = hasSp
      ? gi.rx + innerHalf + outerHalf + RING_CLEAR
      : RIN_CLEAR + outerHalf + RING_CLEAR;
    /* The WITH-specials branch is the shipped 1.0.87 expression, untouched.
       The empty-specials branch needs its own, and MEASURED why:

       `RYO_BASE` (196) is a vertical floor chosen for a ring that has an inner
       ring beneath it — paired with a `rout0` of ~437 it gives the ellipse its
       0.45 eccentricity. Take the inner ring away and `rout0` collapses to
       ~213 while the floor stays 196, so the BASE ellipse is 213x196 — already
       a circle. growRing's phase 1 is uniform, so it preserves that: measured
       on the owner's 26 apps with specials off, the ring came out
       rout 477.6 / ryo 438.4, an aspect of 0.918, and a window 1002px tall
       against a 1003px budget. Not an overlap — the ladder was right to accept
       it — but a screen-filling circle instead of the design's ellipse, and
       one pixel from the clamp that shrinks every chip uniformly.

       So the floor is capped at the roundest shape the design admits.
       RING_ASPECT_MAX already exists for exactly this statement ("it can never
       become a circle: at 0.55 it is still unmistakably an ellipse") — it was
       simply only enforced inside growRing's phase 2, where a ring that starts
       round never reaches. `Math.max` with the collision floor stays outermost
       so the cap can never pull the ring INTO the SPACE pill. */
    const ryo0 = hasSp
      ? Math.max(RYO_BASE, gi.ry + (innerTall + outerTall) / 2 + RING_CLEAR)
      : Math.max(SPACE_H / 2 + outerTall / 2 + RING_CLEAR,
                 Math.min(RYO_BASE, RING_ASPECT_MAX * rout0));
    /* =====================================================================
       GROW UNTIL THE CLEARANCE IS REAL, NOT UNTIL THE ARITHMETIC SAYS SO.
       PROBLEM 235, the CLASSIC half.

       `growRing` sizes the ellipse so its rim holds `sum(w) + n * gap` — i.e.
       it buys each neighbour `gap` px of ARC. What the ring is judged on, and
       what the user sees, is the axis-aligned clearance between two rectangles,
       and arc is not chord: on the flanks and in the diagonals a 16px arc share
       lands as materially less than 16px of box separation. Measured at rung
       (a) with the specials shown: 8px between "Files"/"Youtube" on the owner's
       `sexy_tumar_mexy` profile, and 8px between "Docs"/"Calendar" on
       `Founders`. Both passed classic's acceptance test, because that test is
       `overlaps === 0` and nothing intersected — this is the same "technically
       apart, and unreadable" the inner ring was given `minClearance` for in
       1.0.88, still unfixed on the outer one.

       THE FIX IS RUNG (a) DOING ITS JOB: keep growing while the MEASURED
       clearance is under RING_GAP_MIN. The lever is the gap fed to `growRing`,
       because that is precisely "how much room per neighbour the rim must buy";
       `arcAngles` keeps the REAL `gap` so the extra rim is distributed by the
       same width-proportional rule and nothing about the placement changes
       shape. Bounded by the same screen budget `growRing` already clamps to,
       and abandoned the moment growing stops moving the radius.

       THE RUNG IS NOT CHANGED, only the radius it produces. `clean()` still
       judges classic on `overlaps === 0` alone — the documented no-op
       guarantee against 1.0.88 is about which CONCESSION a config lands on,
       and this walk can only ever make the ring larger, never push a config
       onto a lower rung with smaller type or shorter labels.
       ===================================================================== */
    interface ClassicFit {
      go: ReturnType<typeof growRing>;
      scale: number; rin: number; ryi: number; rout: number; ryo: number;
      spPos: { x: number; y: number }[]; apPos: { x: number; y: number }[];
      boxes: ChipBox[]; overlaps: number; clear: number;
    }
    /** Size, place and MEASURE the classic ring for one growth gap. */
    const solveClassic = (growGap: number): ClassicFit => {
      const go = growRing(
        apW, rout0, ryo0, growGap,
        Math.max(rout0, maxRoutBudget), Math.max(ryo0, maxRyoBudget),
      );

      // The ORIGINAL screen clamp, kept verbatim as the final safety net. It
      // only bites on a display too small to hold even the un-grown ring,
      // which the budgets above cannot help with. Uniform, so the angles
      // solved below are unaffected by it.
      const scale = Math.min(1, (vw / 2 - outerHalf - 24) / go.rx, (vh / 2 - 40) / go.ry);
      const rin = gi.rx * scale, ryi = gi.ry * scale;
      const rout = go.rx * scale, ryo = go.ry * scale;

      // The inner ring's angles were already solved by `fitInner`, at the rung
      // it accepted and with THAT rung's gap. They are reused, not recomputed:
      // `scale` above is uniform, and a uniform scale leaves `arcAngles`'
      // parameter mapping identical — so recomputing here could only introduce
      // a second, silently-diverging source of truth for the same placement.
      // Outer ring: half-step offset so its chips sit in the inner ring's gaps.
      const asIn = gi.as;
      const asOut = arcAngles(apW, rout, ryo, gap, apps.length ? Math.PI / apps.length : 0);

      const spPos = asIn.map((a) => ({ x: Math.cos(a) * rin, y: Math.sin(a) * ryi }));
      const apPos = asOut.map((a) => ({ x: Math.cos(a) * rout, y: Math.sin(a) * ryo }));

      const boxes: ChipBox[] = [
        ...spPos.map((p, i) => ({
          x: Math.round(p.x), y: Math.round(p.y), w: spW[i], h: spH[i],
        })),
        ...apPos.map((p, i) => ({
          x: Math.round(p.x), y: Math.round(p.y), w: apW[i], h: apH[i],
        })),
      ];
      return {
        go, scale, rin, ryi, rout, ryo, spPos, apPos, boxes,
        overlaps: overlapCount(boxes), clear: minClearance(boxes),
      };
    };
    /** Walk, then give the slack back. Same shape as the magnetic ceiling
     *  ladder, and it only runs for a ring that did not clear the bar. */
    const CLASSIC_GROW = [12, 28, 52, 88, 140];
    const CLASSIC_BISECT = 5;
    let cf = solveClassic(gap);
    const classicOk = (f: ClassicFit) => f.overlaps === 0 && f.clear >= RING_GAP_MIN;
    if (!classicOk(cf)) {
      let lastShort = 0;
      let clean: ClassicFit | null = null;
      let cleanExtra = 0;
      for (const extra of CLASSIC_GROW) {
        const cand = solveClassic(gap + extra);
        // `growRing` clamps at the screen budget and then stops moving. When
        // it has, no further extra can help and the walk is over.
        if (cand.go.rx <= cf.go.rx && cand.go.ry <= cf.go.ry) break;
        if (classicOk(cand)) { clean = cand; cleanExtra = extra; break; }
        lastShort = extra;
        if (cand.clear > cf.clear || cand.overlaps < cf.overlaps) cf = cand;
      }
      if (clean) {
        let lo2 = lastShort, hi2 = cleanExtra;
        for (let i = 0; i < CLASSIC_BISECT; i++) {
          const mid = (lo2 + hi2) / 2;
          const cand = solveClassic(gap + mid);
          if (classicOk(cand)) { clean = cand; hi2 = mid; } else lo2 = mid;
        }
        cf = clean;
      }
    }
    const { go, scale, rin, ryi, rout, ryo, spPos, apPos, boxes } = cf;

    // Size the window to the bloom box + PAD. PAD clears the 340px ring-pulse
    // and its glow — without it the circle looks "cut off at the back".
    // NOTE: deliberately NOT the full work area; a fullscreen transparent
    // window composes ZERO pixels on this machine. Capped at the same budget
    // the ring was grown against, so Rust never has to clamp us and the chips
    // are never cropped: the budget already reserved HUD_PAD_MIN for them.
    // FLOOR, not round, on the budget side: Rust clamps at `ms.width * 0.94`
    // exactly, and rounding UP to it means every maximum-size HUD asks for
    // half a pixel more than it can have and gets silently clamped. Harmless
    // in itself, but it puts a "clamped" line in the log for a fit that was
    // actually correct, which is the kind of noise that costs a diagnostic
    // round trip later.
    const w = Math.min(
      Math.round(Math.max(2 * (rout + outerHalf) + HUD_PAD, 360 + HUD_PAD)),
      Math.floor(budgetW));
    const h = Math.min(
      Math.round(Math.max(2 * ryo + HUD_PAD, 360 + HUD_PAD)),
      Math.floor(budgetH));

    return {
      fit: {
        step, spStep: gi.rung, spGap: gi.gap, spClear: gi.clear,
        gap, rin, ryi, rout, ryo,
        outerRim: go.have * scale, outerNeed: go.need,
        innerRim: gi.rim * scale, innerNeed: gi.need,
        overlaps: overlapCount(boxes),
        win: { w, h }, screen: { w: vw, h: vh },
        /* The v1.0.89 reporting fields, filled honestly for classic rather
           than left undefined: ONE app ring is one band, the band count
           setting does not apply here, nothing is ever dropped by the page,
           and the label cap is the shipped 118px. `clear` and `reach` are
           measured the same way in both modes so a bug report can compare
           them directly — which is the point of having the option at all. */
        layout: "classic",
        bands: 1,
        bandRx: [rout],
        bandMode: "n/a",
        clear: minClearance(boxes),
        reach: apPos.length
          ? Math.max(...apPos.map((p, i) => Math.hypot(p.x, p.y) + apW[i] / 2)) : 0,
        /* Measured on `boxes` — the SAME final, scaled set `clear` and
           `overlaps` are judged on, and it already carries the specials. Not
           re-derived from the ring radii: a reported hollow that disagrees
           with the drawn chips is worse than no number at all, and the
           radii are pre-scale here. */
        hollow: hollowOf(boxes),
        /* Classic has ONE app ring, so its shortfall is the single
           have-vs-need subtraction it already computed — reported the same way
           magnetic sums its per-band shortfalls, so a bug report can put the
           two modes side by side. Clamped at 0: rim exceeding need is a ring
           with room to spare, not negative overflow.
           PRE-SCALE, deliberately, matching magnetic's `t.overflow` and the
           `clear`/`hollow` beside it — `go.have` and `go.need` are already in
           one space and `bandOverflow` records what re-scaling one of them
           did when it was tried. */
        overflow: Math.max(0, go.need - go.have),
        spDropped: false,
        restCap: FULL_CAP,
      },
      spPos, apPos, apW, apH,
    };
  };

  /* =====================================================================
     ONE RUNG OF THE LADDER — MAGNETIC SECTOR (v1.0.89, the default).

     Measure at the CURRENT chip scale, put the specials on the innermost
     band, pack the apps into capacity-checked bands outside it, relax the
     cross-band angles, and count REAL intersections over the REAL boxes.
     Nothing here is accepted on arithmetic alone.

     Kept as its own function rather than folded into `layoutClassic` behind a
     pile of flags. The two genuinely differ — one grows an ellipse to fit its
     content, the other fills a fixed glance budget with bands — and a single
     parameterised version would be one function nobody can read instead of
     two anybody can. What they SHARE they share for real: `arcTable`,
     `arcAngles`, `ellipsePerimeter`, `overlapCount`, `minClearance`, the
     measurement pass, `fitInner`, and the (a)-(d) ladder below are all one
     copy called from both.
     ===================================================================== */
  const layoutMagnetic = (step: HudFit["step"], gap: number): LayoutOut => {
    // The at-rest label is the FIRST WHOLE WORD (decision 1). `apRest` is what
    // the width fallback must estimate from — estimating the FULL label here
    // would size the ring for text that is not on screen, which is PROBLEM
    // 77's estimate bug wearing a third hat.
    const apW = apChips.map((c, i) => width(c, apRest[i], false));
    const apH = apChips.map((c) => height(c, false));
    const outerHalf = apW.length ? Math.max(...apW) / 2 : 0;
    const outerTall = apH.length ? Math.max(...apH) : 0;
    /* CHIP_H IS MEASURED, NOT ASSUMED. The design prototype hard-coded 30 and
       every clearance test came out optimistic while the shipped ring
       collided; the real box is 33 (5px padding x2 + a 20px kbd + 1.5px
       border x2) and `.dense-chips` moves it again. BAND_STEP is derived from
       whatever we actually measured. */
    const chipH = outerTall || CHIP_H_FALLBACK;
    const bandStep = bandStepFor(chipH);

    // How far the ring may reach before the window it implies would be clamped
    // by Rust. Magnetic almost never reaches these — GLANCE_R binds first —
    // but `fitInner`'s s4 rung still needs them, and a 640x480 display exists.
    const maxRoutBudget = (budgetW - HUD_PAD_MIN) / 2 - outerHalf;
    const maxRyoBudget = (budgetH - HUD_PAD_MIN) / 2;

    /* --- BAND 0: THE SPECIALS, when they are shown ------------------------
       The owner's decision 2 makes this ONE geometry: the specials are simply
       the innermost band and the apps fill outward from the next one. Their
       own s0-s4 ladder is UNTOUCHED (decision 3) — they sit inside the dead
       zone, can never be aimed at and therefore never bloom, so word-clipping
       them would destroy information with no recovery path. `fitInner` starts
       from the tight 1.0.88 baseline the owner asked to keep and spends
       CONTENT before radius. */
    const gi = fitInner(gap, maxRoutBudget, maxRyoBudget, outerHalf, outerTall);
    const spW = gi.w, spH = gi.h;
    const hasSp = spChips.length > 0;

    /* --- THE APP BANDS ----------------------------------------------------
       The ceiling caps a chip's OUTER EDGE, not the band radius — applying it
       to the radius alone let chips reach ~448px in the prototype.

       TWO CEILINGS, because there are two genuinely different situations and
       one number cannot describe both. Specials hidden: GLANCE_R, held
       exactly, measured at 399.2px worst case over 1..26 apps. Specials shown:
       their band's outer edge is already ~340px, so the app band's INNER edge
       starts beyond 400 before a single app is placed — pinning it at 400 does
       not make the ring tighter, it makes the ring IMPOSSIBLE, and the page
       then drops the specials to escape. See GLANCE_R_WITH_SPECIALS. */
    const glance = hasSp ? GLANCE_R_WITH_SPECIALS : GLANCE_R;
    /* AND THE SCREEN IS THE LAST WORD, on BOTH axes. `overlay_fit_hud` clamps
       the window to 94% of the monitor, and a clamped window CROPS chips —
       it does not shrink them, and nothing in the page can observe that it
       happened. So the ring may never ask for more than the window can show.
       The y bound is divided by BAND_RATIO because a band's ry is
       `rx * BAND_RATIO`: it is a bound on rx expressed through the axis that
       actually runs out first on a laptop panel. */
    const hiScreen = Math.min(
      maxRoutBudget,
      (maxRyoBudget - chipH / 2) / BAND_RATIO,
    );
    /* The ceiling the glance target implies. It is where the search STARTS,
       not where it must end — see the ceiling ladder below `solveAt`. */
    const hiGlance = Math.min(glance - outerHalf, hiScreen);

    const items: BandItem[] = apW.map((w, i) => ({ i, w, h: apH[i] }));
    /* WITH THE SPECIALS SHOWN THERE IS EXACTLY ONE APP BAND, and that is the
       whole meaning of the setting rather than a limitation of this code: the
       band count counts APP bands, and `hud_band_count`'s truth table says
       specials can only exist when the apps need just the outer one
       (config/schema.rs, engine::specials_for_hud). Two app bands OUTSIDE a
       specials band would be three rings, which Rust already refuses to send
       and which no ceiling could hold. Clamped here rather than left to
       `bandStep` arithmetic, which — now that the ceiling stretches — would
       happily have found room for a second one. */
    const forced = hasSp
      ? 1
      : bandMode === "one" ? 1 : bandMode === "two" ? 2 : 0;

    interface Trial {
      n: number; bands: Band[]; placed: Placed[];
      overlaps: number; clear: number;
      /** Rim the bands are SHORT by (`bandOverflow`) — 0 means they fit. */
      overflow: number;
      /** Gap from the SPACE pill to the nearest chip (`hollowOf`). */
      hollow: number;
    }
    /** Place, relax and MEASURE one set of bands. */
    const measureBands = (bands: Band[]): Trial => {
      const placed = placeBands(bands, gap);
      relaxAngles(placed);
      const boxes: ChipBox[] = placed.map((p) => ({
        x: Math.round(p.x), y: Math.round(p.y), w: p.w, h: p.h,
      }));
      // The SPECIALS band is part of the hollow — with it on screen the middle
      // is not empty, it is occupied by another ring. Measuring the apps alone
      // would report a 162px hole where the user sees a full inner ring.
      const withSp: ChipBox[] = hasSp
        ? boxes.concat(gi.as.map((a, i) => ({
            x: Math.round(Math.cos(a) * gi.rx), y: Math.round(Math.sin(a) * gi.ry),
            w: spW[i], h: spH[i],
          })))
        : boxes;
      return {
        n: bands.length, bands, placed,
        overlaps: overlapCount(boxes), clear: minClearance(boxes),
        overflow: bandOverflow(bands, gap), hollow: hollowOf(withSp),
      };
    };
    /** The innermost band's floor, from the widest chip ACTUALLY on it.
     *  Same two expressions `lo` uses above — the only difference is WHOSE
     *  half-width goes in, and that is the whole of "the dead-zone radius
     *  adapts": with specials the floor is the specials ring, without them it
     *  is the SPACE pill. */
    const floor0: Floor0 = (half) => (hasSp
      ? Math.max(RIN_CLEAR + half + BAND_LO_PAD, gi.rx + gi.half + half + RING_CLEAR)
      : RIN_CLEAR + half + BAND_LO_PAD);

    /** ZERO OVERLAPS, RING_GAP_MIN OF CLEARANCE, AND EVERY BAND HOLDING WHAT
     *  IT WAS GIVEN. One predicate, used by the band-count search AND by the
     *  ceiling ladder below it, so the two can never disagree about what
     *  "fits" means. */
    const meets = (x: Trial) =>
      x.overlaps === 0 && x.clear >= RING_GAP_MIN && x.overflow === 0;
    /** Which of two trials to keep when NEITHER meets the bar. Fewer
     *  collisions first, then less rim shortfall, then more clearance. */
    const better = (a: Trial, b: Trial) =>
      a.overlaps < b.overlaps ||
      (a.overlaps === b.overlaps && a.overflow < b.overflow) ||
      (a.overlaps === b.overlaps && a.overflow === b.overflow && a.clear > b.clear);

    interface Solution { t: Trial; lo: number; hiR: number; ceil: number }

    /**
     * Solve the whole band layout at ONE ceiling.
     *
     * Everything from the dead-zone floor to the accepted band count is a
     * function of the ceiling, so it lives in here and the ladder below is
     * free to try more than one. At `hiGlance` this is byte-for-byte the
     * 1.0.96 body.
     */
    const solveAt = (hiCeil: number): Solution => {
      /* The innermost app band. With no specials it clears the SPACE pill and
         sits tight against it, which is the whole point of Magnetic: three
         chips do NOT get flung out to the ceiling. With specials it must clear
         the specials band instead — the same `+ half + half + RING_CLEAR`
         expression 1.0.87 used, because a band is still a ring and two rings
         still have to miss each other. */
      let lo = Math.min(hiCeil, RIN_CLEAR + outerHalf + BAND_LO_PAD);
      if (hasSp) lo = Math.max(lo, gi.rx + gi.half + outerHalf + RING_CLEAR);
      /* The collision floor wins if the two disagree — a ceiling may never
         pull a band INTO the ring inside it. `fit.reach` reports what actually
         happened either way rather than pretending. */
      const hiR = Math.max(hiCeil, lo);
      const maxBands = hasSp
        ? 1
        : Math.max(1, Math.floor((hiR - lo) / bandStep) + 1);

      /** Pack one candidate band count, then measure it BOTH untightened and
       *  tightened and keep the tightened one only if it comes out clean.
       *
       *  The fallback is not defensive padding: `tightenBands` moves bands
       *  closer together, and two bands one `bandStep` apart are at the
       *  closest the measured chip height allows. If that turns out to collide
       *  on a real label set, the answer is the ring 1.0.95 already shipped —
       *  never a tighter one that overlaps. */
      const trial = (n: number): Trial => {
        const spread = packBands(items, gap, lo, hiR, bandStep, n);
        const loose = measureBands(spread);
        const tight = measureBands(tightenBands(spread, gap, bandStep, floor0));
        return (tight.overlaps === 0 && tight.clear >= RING_GAP_MIN) ? tight : loose;
      };

      /* AUTO PREFERS ONE BAND, AND PROVES IT RATHER THAN ASSUMING IT.
         Owner: "auto should prefer one band unless it doesn't fit." So the
         search starts at ONE and stops the moment a count clears the
         acceptance bar — zero measured overlaps AND at least RING_GAP_MIN of
         clearance. It does NOT balance chips across bands and must not be
         "optimised" into doing so: a half-empty outer band is worse to read
         than a full inner one, and the whole design is about reading ONE name
         at a time. `packBands`' own capacity arithmetic remains the fallback
         for the case where nothing measures clean. */
      let t: Trial;
      if (forced) {
        t = trial(forced);
      } else {
        let best: Trial | null = null;
        for (let n = 1; n <= maxBands; n++) {
          const cand = trial(n);
          if (!best || better(cand, best)) best = cand;
          /* THE ACCEPTANCE BAR, and `overflow === 0` is the 1.0.96 addition.
             Zero overlaps and RING_GAP_MIN of clearance say "nothing
             collides"; they do NOT say "the band can hold what it was given".
             At 16 of the owner's labels those two passed on a band pinned to
             the ceiling with 23px less rim than its content, and `auto`
             stopped there — one lonely ring 383px out with a 101px hole in the
             middle, for exactly the same outer size a second band would have
             cost (382px measured). With the shortfall measured, `auto` walks
             on and opens the band INSIDE, which is what the Magnetic Sector
             header says should happen and what "compact" means to the owner.

             A band count that never clears the bar still falls through to
             `best` above, so a payload that genuinely cannot fit ends up
             exactly where 1.0.95 left it. */
          if (meets(cand)) { best = cand; break; }
        }
        t = best ?? trial(0);
      }
      return { t, lo, hiR, ceil: hiCeil };
    };

    /* =====================================================================
       THE CEILING IS PART OF RUNG (a). PROBLEM 235.

       WHAT WAS WRONG. The (a)-(d) ladder's own header states the owner's order
       of preference: (a) GROW the ring until it can hold its content, and only
       then (b) close the gap, (c) shrink the chips, (d) truncate the labels.
       Under Magnetic, (a) could not actually grow: `hiR` was pinned at
       `glance - outerHalf` for the whole build, so a payload that did not fit
       inside the glance target fell straight past (a) to (c) and (d) — smaller
       type and shorter labels — with the ring still not fitting afterwards.

       MEASURED, on the owner's `sexy_tumar_mexy` profile (16 apps, seven of
       them browser-profile pairs like "Youtube — ARPON'S STUDIES", which
       render 168-212px wide against ~90px for an ordinary first-word chip):

         layout            before                       cause
         Compact, sp off   step (d), clear 7px,         one band pinned at the
                           overflow 231px               400px ceiling
         Double            step (d), 2 OVERLAPS,        two bands 96px apart
                           clear -23px                  carrying 168px chips

       The Double figures are the owner's screenshot: "Youtube — Arpon" over
       "Google Chrome — Arpon" (keys Y/M) and "Claude" under "Youtube — ARPON'S
       ST…" (keys C/U). Both pairs sat at EXACTLY 5.0-5.1 degrees apart, which
       is `RELAX_FLOOR` — the cross-band relax pass had done its job and its
       job was the wrong one: 5 degrees is an ANGLE, and what two chips 96px
       apart on adjacent bands need on the ellipse's LEFT and RIGHT FLANKS is
       (w1 + w2) / 2 of PIXELS, here 122 and 144. Nothing else measures that:
       `bandStepFor` sizes the radial step from chip HEIGHT (correct at the top
       and bottom, where stacked bands separate in y, and the wrong axis on the
       flanks), and `bandOverflow` measures per-band RIM shortfall, which is a
       same-band property and reported a clean 0 for both colliding pairs.

       THE FIX, and it is the documented rung (a) rather than a new idea: when
       the ceiling in force cannot produce geometry that meets the bar, RAISE
       IT — bounded by `hiScreen`, which is already the 94% clamp Rust applies
       — and take the SMALLEST ceiling that measures clean. Growing the ceiling
       spreads the bands apart radially, which is the only lever that can
       deliver flank clearance, and it does it without touching the type size.

       GLANCE_R IS A TARGET, NOT A LAW, and this file already says so twice:
       GLANCE_R's own comment ("a ceiling the geometry meets whenever it can,
       not a law it can always obey") and GLANCE_R_WITH_SPECIALS, which exists
       because one configuration provably could not meet it. The owner's ruling
       is quoted there in his words — *"a little bit here and there is okay,
       it's not a hard line"*. This is the same allowance, granted for the same
       reason, and granted only when the alternative is a ring that overlaps.

       NOTHING THAT ALREADY FITS MOVES. The ladder's first rung IS today's
       ceiling, and it returns immediately when the answer meets the bar — so
       every ring in the owner's other three profiles, and every small ring,
       comes out byte-identical. The search only runs for a payload that was
       already going to be shipped broken.

       AND IT IS BOUNDED AND MEASURED. `hiScreen` caps the walk on both axes;
       every rung is judged by `meets()` on the real boxes; a walk that never
       clears keeps the best it saw, which is exactly what 1.0.96 would have
       shipped. `fit.reach` reports the outcome either way.
       ===================================================================== */
    /** Rungs of the ceiling walk, then a bisection to give back the slack. Both
     *  small on purpose: this only runs for a payload that did not fit, and a
     *  HUD is built while the user is holding Space. */
    const CEIL_STEPS = 6;
    const CEIL_BISECT = 5;
    let sol = solveAt(hiGlance);
    if (!meets(sol.t) && hiScreen > hiGlance + 1) {
      let lastShort = hiGlance;
      let clean: Solution | null = null;
      for (let i = 1; i <= CEIL_STEPS; i++) {
        const cand = solveAt(hiGlance + ((hiScreen - hiGlance) * i) / CEIL_STEPS);
        if (meets(cand.t)) { clean = cand; break; }
        lastShort = cand.ceil;
        if (better(cand.t, sol.t)) sol = cand;
      }
      if (clean) {
        // Give back as much of the growth as the bar allows: the smallest
        // ceiling that still measures clean, not the first one that did.
        let lo2 = lastShort, hi2 = clean.ceil;
        for (let i = 0; i < CEIL_BISECT; i++) {
          const mid = (lo2 + hi2) / 2;
          const cand = solveAt(mid);
          if (meets(cand.t)) { clean = cand; hi2 = mid; } else lo2 = mid;
        }
        sol = clean;
      }
    }
    const t = sol.t;
    const lo = sol.lo;

    /* The ORIGINAL screen clamp, kept as the final safety net for a display
       too small to hold even the un-grown ring. Uniform, so it cannot disturb
       the arc-length mapping the angles were solved under. */
    const reachX = t.placed.length
      ? Math.max(...t.placed.map((p) => Math.abs(p.x) + p.w / 2)) : 0;
    const reachY = t.placed.length
      ? Math.max(...t.placed.map((p) => Math.abs(p.y) + p.h / 2)) : 0;
    const spReachX = hasSp ? Math.max(...gi.as.map((a, i) =>
      Math.abs(Math.cos(a) * gi.rx) + spW[i] / 2)) : 0;
    const spReachY = hasSp ? Math.max(...gi.as.map((a, i) =>
      Math.abs(Math.sin(a) * gi.ry) + spH[i] / 2)) : 0;
    const scale = Math.min(
      1,
      (vw / 2 - 24) / Math.max(1, Math.max(reachX, spReachX)),
      (vh / 2 - 40) / Math.max(1, Math.max(reachY, spReachY)),
    );

    const apPos = new Array<{ x: number; y: number }>(apChips.length);
    for (const p of t.placed) apPos[p.i] = { x: p.x * scale, y: p.y * scale };
    // A chip the packer somehow never placed would leave a hole here, and a
    // hole becomes `calc(50% + NaNpx)` — i.e. nowhere. Cannot happen (every
    // item lands in a band), written so it cannot start happening either.
    for (let i = 0; i < apPos.length; i++) if (!apPos[i]) apPos[i] = { x: 0, y: 0 };

    const rin = gi.rx * scale, ryi = gi.ry * scale;
    const spPos = gi.as.map((a) => ({
      x: Math.cos(a) * rin, y: Math.sin(a) * ryi,
    }));

    const boxes: ChipBox[] = [
      ...spPos.map((p, i) => ({
        x: Math.round(p.x), y: Math.round(p.y), w: spW[i], h: spH[i],
      })),
      ...apPos.map((p, i) => ({
        x: Math.round(p.x), y: Math.round(p.y), w: apW[i], h: apH[i],
      })),
    ];

    const outer = t.bands.length ? t.bands[t.bands.length - 1] : null;
    const rout = (outer ? outer.rx : lo) * scale;
    const ryo = (outer ? outer.ry : lo * BAND_RATIO) * scale;
    const reach = t.placed.length
      ? Math.max(...t.placed.map((p) => Math.hypot(p.x, p.y) * scale + p.w / 2)) : 0;

    /* Size the window to the real footprint + PAD. PAD clears the 340px
       ring-pulse and its glow — without it the circle looks "cut off at the
       back". Deliberately NOT the full work area: a fullscreen transparent
       window composes ZERO pixels on this machine. FLOOR on the budget side,
       because Rust clamps at `ms.width * 0.94` exactly and rounding UP to it
       puts a spurious "clamped" line in the log for a fit that was correct.

       Magnetic makes this MUCH smaller than PROBLEM 208's grown ellipse — the
       ring can no longer reach past GLANCE_R on its own — which is a free win
       for a window this machine composites in software. */
    const w = Math.min(
      Math.round(Math.max(2 * Math.max(reachX, spReachX) * scale + HUD_PAD, 360 + HUD_PAD)),
      Math.floor(budgetW));
    const h = Math.min(
      Math.round(Math.max(2 * Math.max(reachY, spReachY) * scale + HUD_PAD, 360 + HUD_PAD)),
      Math.floor(budgetH));

    return {
      fit: {
        step, spStep: gi.rung, spGap: gi.gap, spClear: gi.clear,
        gap, rin, ryi, rout, ryo,
        outerRim: t.bands.reduce((s, b) => s + b.cap, 0) * scale,
        outerNeed: apW.reduce((s, v) => s + v, 0) + apW.length * gap,
        innerRim: gi.rim * scale, innerNeed: gi.need,
        overlaps: overlapCount(boxes),
        win: { w, h }, screen: { w: vw, h: vh },
        layout: "magnetic",
        bands: t.bands.length,
        bandRx: t.bands.map((b) => b.rx * scale),
        bandMode: bandMode,
        clear: minClearance(boxes),
        reach,
        /* THE NUMBER THE HOLLOW FIX IS JUDGED ON. Measured on `boxes` — the
           same final, scaled set `clear` and `overlaps` are judged on, which
           already carries the specials ring — and NOT taken from `t.hollow`.
           The trial's copy is measured pre-scale, so on any display where
           `scale` is not 1 it would disagree with what the user is actually
           looking at. A reported hollow that does not match the drawn chips
           is worse than no number: it is the number a bug report quotes. */
        hollow: hollowOf(boxes),
        /* THE LADDER'S OWN NUMBER, not a re-measurement — `t` is the accepted
           trial, so this is exactly the shortfall that was decided on, and the
           two can never drift apart. Pre-scale, like `clear` and `hollow`
           beside it: see `bandOverflow` for why re-scaling it produces fiction.
           (`outerRim` IS scaled and `outerNeed` is not — a mixed pair that
           predates this field. Do not "make overflow match" them; the honest
           move is one consistent space, which is this one.) */
        overflow: t.overflow,
        spDropped: _spDropped,
        restCap: REST_CAP_BACKSTOP,
      },
      spPos, apPos, apW, apH,
    };
  };

  /** THE BRANCH. One line, at the top of the ladder rather than sprinkled
   *  through it, so every rung of the (a)-(d) walk below is shared and neither
   *  geometry can drift out from under it. */
  const layout = (step: HudFit["step"], gap: number): LayoutOut =>
    mode === "classic" ? layoutClassic(step, gap) : layoutMagnetic(step, gap);

  /* The ladder. Each rung concedes exactly one thing, in the owner's stated
     order, and is accepted the moment the measured geometry is clean.

     TWO conditions now, not one. `overlapCount === 0` alone once accepted a
     pair sitting 2px apart — technically apart, and unreadable — which is why
     `minClearance` exists and why the inner ring has been judged on distance
     since 1.0.88. The outer ring is now held to the same bar, because Magnetic
     no longer GROWS its way to comfortable spacing: it fills a fixed budget,
     so "did not intersect" stopped being evidence of "is readable".

     Rung (a)'s "grow" means something narrower under Magnetic than it did in
     1.0.88: a band still grows to fit its content (`fitRx`), but only up to
     the ceiling in force — GLANCE_R with the specials hidden, the stretched
     allowance with them shown, and the screen budget over both. So (a) is
     "grow as far as the ceiling allows, at the preferred 16px gap", and (b) is
     the first real concession. */
  const LADDER: { step: HudFit["step"]; gap: number; dense: boolean; tight: boolean }[] = [
    { step: "a", gap: RING_GAP_PREF, dense: false, tight: false }, // preferred gap
    { step: "b", gap: RING_GAP_MIN,  dense: false, tight: false }, // tighten the gap
    { step: "c", gap: RING_GAP_MIN,  dense: true,  tight: false }, // shrink the chips
    { step: "d", gap: RING_GAP_MIN,  dense: true,  tight: true  }, // truncate harder
  ];
  /** Did this rung produce geometry we are willing to ship?
   *
   *  THE BAR IS DIFFERENT IN THE TWO MODES, deliberately. CLASSIC keeps
   *  1.0.88's test EXACTLY — zero measured overlaps and nothing else — because
   *  the whole point of that branch is to be a no-op, and adding a clearance
   *  floor to it would silently push some of the owner's real configurations
   *  onto a lower rung than the build he is running today. MAGNETIC adds the
   *  clearance floor, because it no longer GROWS its way to comfortable
   *  spacing: it fills a fixed budget, so "did not intersect" stopped being
   *  evidence of "is readable". (`overlapCount === 0` alone once accepted a
   *  pair sitting 2px apart, which is why `minClearance` exists at all.) */
  const clean = (f: HudFit) =>
    f.overlaps === 0 && (mode === "classic" || f.clear >= RING_GAP_MIN);

  /** Walk (a) -> (d) and stop at the first rung that measures clean.
   *
   *  A fresh walk always starts from the top of BOTH ladders — otherwise a HUD
   *  that once needed rung (d) or (s3) would wear its shrunken chips forever,
   *  and the second walk below (after a specials drop) would inherit the first
   *  walk's concessions and report a rung it never actually needed. */
  const runLadder = (): LayoutOut => {
    _hudEl!.classList.remove("dense-chips", "tight-chips", "dense-sp", "tight-sp");
    let r = layout("a", RING_GAP_PREF);
    for (let i = 1; i < LADDER.length && !clean(r.fit); i++) {
      const rung = LADDER[i];
      // Only rungs (c) and (d) change the chips themselves; the toggles below
      // force the re-measure that `layout` then reads. `classList.toggle` is
      // safe beside `hidden`/`handoff`/`collapsing`.
      _hudEl!.classList.toggle("dense-chips", rung.dense);
      _hudEl!.classList.toggle("tight-chips", rung.tight);
      r = layout(rung.step, rung.gap);
    }
    return r;
  };

  let out = runLadder();

  /* THE SPECIALS DROP — the page's half of `hud_band_count`'s truth table, and
     now THE LAST RESORT rather than the first move.

     Rust resolves every deterministic row itself (engine::specials_for_hud):
     specials off, or "two" rows, and the vec arrives empty. The ONE row it
     cannot resolve is `auto` + specials ON, because the answer depends on
     MEASURED label widths and those exist nowhere but in this document. Its
     own comment says so: "Rust keeps sending the specials and the page drops
     them if it ends up needing two bands."

     THIS USED TO FIRE AT RUNG (a), AND ON THE OWNER'S OWN CONFIG THAT WAS A
     BUG. With the ceiling pinned at 400 the app band outside the specials
     could not grow, one band could not hold his 26 bound letters at the
     acceptance bar, and the specials were dropped at rung (a) — so toggling
     "show special keys on the ring" ON changed nothing he could see. Measured
     crossover on his real labels before the fix: specials survived to 22 apps
     and vanished at 23.

     Two changes, together: the ceiling stretches (GLANCE_R_WITH_SPECIALS) so
     GROWING is tried properly first, and the drop now runs only after the
     FULL (a)-(d) ladder has failed with the specials kept. So the specials are
     given up only when the ring cannot be made to work with them at all — not
     merely when it would exceed a soft 400px target. When it does fire, the
     second walk starts from a clean slate: `lo` collapses from ~550 back to
     ~190 and two comfortable app bands fit inside GLANCE_R again.

     `auto` only. In `one` the user has overruled the arithmetic himself, so
     the specials stay and the ladder's concessions absorb the pressure —
     exactly as the design handoff specifies for a forced band count. */
  if (mode === "magnetic" && bandMode === "auto" && spChips.length > 0 && !clean(out.fit)) {
    for (const c of spChips) c.remove();
    spChips = [];
    _spDropped = true;
    out = runLadder();
  }

  const geo = out.fit;
  _hudFit = geo;
  /* THE LADDER CAN BE EXHAUSTED, and when it is, SAY SO.
     N chips of readable width need N·width of rim; if the bands inside the
     glance radius have less than that, no arrangement avoids a collision and
     the only remaining answers are unreadable type or a third band. It is
     logged rather than hidden because a silently-overlapping ring is precisely
     the bug this work exists to end — the same rule the window commands
     follow: anything that changes what the user can see must be in the log. */
  if (!clean(geo) && _isOverlay) {
    invoke("overlay_log", {
      msg: `buildHud: ${geo.layout} ring EXHAUSTED at step ${geo.step} — ${geo.overlaps} ` +
        `chip pair(s) intersect, worst clearance ${Math.round(geo.clear)}px, furthest ` +
        `outer edge ${Math.round(geo.reach)}px. ${apps.length} apps + ${spChips.length} ` +
        `specials need ${Math.round(geo.outerNeed)}px of rim and the ring offers ` +
        `${Math.round(geo.outerRim)}px. ` +
        (geo.layout === "magnetic"
          ? `${geo.bands} band(s) inside the ${GLANCE_R}px glance radius ` +
            `(rx ${geo.bandRx.map((r) => Math.round(r)).join("/")}, rows=${geo.bandMode}, ` +
            `specials ${geo.spDropped ? "dropped by the page" : "as sent"}). `
          : `one grown ring, rout=${Math.round(geo.rout)} ryo=${Math.round(geo.ryo)} on a ` +
            `${geo.screen.w}x${geo.screen.h} screen. `) +
        `Not a layout fault: the content does not physically fit at a readable size.`,
    }).catch(() => {});
  }

  /**
   * The per-chip entrance stagger, ADAPTED to the count.
   *
   * It was a flat `i * 26ms`. With 8 chips that is a 182ms sweep — exactly
   * right, and unchanged below. With the owner's 26 bound letters it is a
   * 650ms sweep, so the last chip only STARTS blooming 950ms in and does not
   * settle until ~1.57s — on a HUD you hold for a few hundred milliseconds,
   * the far side of the ring never finished arriving. The step now shrinks so
   * the whole sweep lands inside STAG_SPAN, and is capped at the original 26
   * so nothing about the small-ring case changes.
   *
   * UNCHANGED BY MAGNETIC, deliberately. `st-bloom-in` on the inner <i>, its
   * 620ms cubic-bezier(.34,1.3,.4,1) curve, the --fx/--fy vector that makes
   * the bloom radial, and this stagger are the HUD's entrance and are not part
   * of the geometry change.
   */
  const STAG_SPAN = 340;
  const stagger = (n: number) => (n > 1 ? Math.min(26, STAG_SPAN / (n - 1)) : 26);

  const put = (chips: HTMLDivElement[], pos: { x: number; y: number }[],
               delay0: number) => {
    const step = stagger(chips.length);
    chips.forEach((c, i) => {
      const x = pos[i]?.x ?? 0, y = pos[i]?.y ?? 0;
      c.style.left = `calc(50% + ${Math.round(x)}px)`;
      c.style.top = `calc(50% + ${Math.round(y)}px)`;
      const inner = c.firstElementChild as HTMLElement;
      // --fx/--fy = vector from final position BACK to centre → blooms outward
      inner.style.cssText +=
        `;--fx:${Math.round(-x)}px;--fy:${Math.round(-y)}px;` +
        (REDUCED()
          ? "animation:none;"
          : `animation-delay:${Math.round(entranceDelay + delay0 + i * step)}ms;`);
    });
  };

  // Positions come from the accepted rung — recomputing them here would be a
  // second, silently-diverging source of truth for the same geometry.
  put(spChips, out.spPos, 120);
  put(apChips, out.apPos, 300);

  /* THE RESTING SNAPSHOT — the one thing bloom may never be allowed to touch.

     Everything downstream of the layout reads from here and NOT from the live
     DOM: `publishHudChips` (so Rust hit-tests against boxes that do not move),
     and `paintBloom` (so the push and the neighbour choice are computed from
     where the chips REST, not from where a previous bloom left them). Publish
     bloomed rects and Rust's next hit test runs against moved boxes, arms a
     different chip, moves a different set — a feedback oscillation with the
     cursor sitting perfectly still.

     `full` is each chip's width with its WHOLE label showing, measured at the
     accepted rung, which is what clamps the bloom push against GLANCE_R. It
     costs exactly two reflows, once per HUD show. */
  const apFull: number[] = [];
  for (let i = 0; i < apChips.length; i++) {
    apChips[i].classList.add("bloom");
    setChipLabel(apChips[i], apps[i][1]);
  }
  for (let i = 0; i < apChips.length; i++) apFull[i] = width(apChips[i], apps[i], false);
  for (let i = 0; i < apChips.length; i++) {
    apChips[i].classList.remove("bloom");
    setChipLabel(apChips[i], apRest[i][1]);
  }
  _apRest = out.apPos.map((p, i) => ({
    x: p.x, y: p.y, w: out.apW[i], h: out.apH[i], full: apFull[i] ?? out.apW[i],
    geo: Math.atan2(p.y, p.x), r: Math.hypot(p.x, p.y),
  }));
  /* The bloom bounds, resolved once from the accepted geometry. The radial
     one is measured against the ring's own outermost FULL-LABEL edge — not
     its resting edge — so the outermost chip gets its whole push rather than
     only what its label growth left over. See _bloomCap for the argument. */
  const fullReach = _apRest.length
    ? Math.max(..._apRest.map((c) => c.r + c.full / 2)) : 0;
  _bloomCap = Math.max(GLANCE_R, fullReach) + BLOOM_PUSH_ARMED;

  /* =====================================================================
     THE WINDOW IS SIZED FOR THE BLOOMED RING, NOT THE RESTING ONE. 1.0.90.

     THE BUG THIS FIXES, and it is the half the CSS could not fix on its own.
     `layout()` sizes the window from RESTING boxes, because that is the ring
     it laid out. But blooming makes a chip WIDER — the whole point of it — and
     at the ellipse's horizontal extremes a chip is already at maximum |x|, so
     the extra width has nowhere to go. Lifting the pill cap in the stylesheet
     without this would have traded an ellipsis for a chip that runs off the
     window edge, which is worse: a cropped chip is the one failure this
     overlay cannot show the user, and it cannot be observed from inside the
     page either.

     SYMMETRIC GROWTH, EVERYWHERE, AND THAT IS THE POINT. The obvious cheaper
     fix is to anchor the growth inward for chips near the edge so the window
     never has to change. It is rejected for the same reason the design
     handoff's original bloom clamp was rejected a day earlier: a chip that
     grows symmetrically in the middle of the ring and lopsidedly at the edges
     reads as a bug, and inconsistent motion is exactly what the owner asked to
     be rid of. Buying a slightly larger window is the cheaper of the two
     costs, and the window is transparent — nobody can see it.

     THE ASK MUST STILL FIT WHAT RUST WILL GRANT. `overlay_fit_hud` clamps at
     94% of the monitor and a clamped window CROPS rather than shrinks, which
     rotates every aim sector and breaks selection silently. So the ask is
     capped at the same budget the ring was grown against, and if the bloom
     headroom cannot fit inside it the caps are tightened until it does — with
     a log line, because a silently ellipsized label is the bug we are here to
     fix and doing it quietly in a corner case would just move it. */
  const bloomExtent = () => {
    let bx = 0, by = 0;
    for (const c of _apRest) {
      const push = c.r > 0
        ? Math.min(BLOOM_PUSH_ARMED, Math.max(0, _bloomCap - c.full / 2 - c.r))
        : 0;
      const k = c.r > 0 ? 1 + push / c.r : 1;
      bx = Math.max(bx, Math.abs(c.x * k) + c.full / 2);
      by = Math.max(by, Math.abs(c.y * k) + c.h / 2);
    }
    // HUD_PAD_MIN, not HUD_PAD: the resting ask already bought the 340px
    // pulse its room, and this only has to keep the chip's own
    // `0 10px 28px` shadow inside the window.
    return { w: Math.round(2 * bx + HUD_PAD_MIN), h: Math.round(2 * by + HUD_PAD_MIN) };
  };

  let want = bloomExtent();
  if (want.w > Math.floor(budgetW) || want.h > Math.floor(budgetH)) {
    /* DEGENERATE: even the whole screen budget cannot show this label bloomed.
       Tighten the PILL cap by exactly the overflow and re-measure — the pill
       is the right lever because it is what actually bounds the chip's width,
       and one uniform value keeps every chip growing by the same rule rather
       than singling out the offender. Then say so, loudly: the alternative is
       an ellipsis nobody can explain. */
    const overW = want.w - Math.floor(budgetW);
    const overH = want.h - Math.floor(budgetH);
    const widest = _apRest.length ? Math.max(..._apRest.map((c) => c.full)) : 0;
    /* Only WIDTH can be bought back here. The pill cap bounds a chip's width;
       it does nothing for a ring that is too TALL, and pretending otherwise
       would tighten labels for no gain and then report a cause that is not the
       cause. `Math.min(widest, ...)` keeps the cap a no-op when the overflow
       is vertical — the log below then says so instead of blaming the label. */
    const pill = overW > 0
      ? Math.max(140, Math.min(widest, widest - overW))
      : widest;
    if (overW > 0) _hudEl.style.setProperty("--bloom-pill", `${Math.round(pill)}px`);
    for (let i = 0; i < apChips.length; i++) {
      apChips[i].classList.add("bloom");
      setChipLabel(apChips[i], apps[i][1]);
    }
    for (let i = 0; i < apChips.length; i++) {
      _apRest[i].full = width(apChips[i], apps[i], false);
    }
    for (let i = 0; i < apChips.length; i++) {
      apChips[i].classList.remove("bloom");
      setChipLabel(apChips[i], apRest[i][1]);
    }
    const before = want;
    want = bloomExtent();
    if (_isOverlay) {
      const axis = overW > 0 && overH > 0 ? "WIDTH and HEIGHT"
                 : overW > 0 ? "WIDTH" : "HEIGHT";
      const bit = overW > 0 && pill < widest;
      invoke("overlay_log", {
        msg: `buildHud: BLOOM HEADROOM CLAMPED on ${axis} — the bloomed ring wanted a ` +
          `${before.w}x${before.h} window against a ${Math.floor(budgetW)}x` +
          `${Math.floor(budgetH)} budget (94% of ${geo.screen.w}x${geo.screen.h}); the ` +
          `widest label needs ${Math.round(widest)}px of pill. ` +
          (bit
            ? `--bloom-pill tightened to ${Math.round(pill)}px, so an armed label that long ` +
              `WILL still ellipsize.`
            : `The pill cap was left alone — it bounds width and cannot buy back height, so ` +
              `tightening it would cost readability for nothing.`) +
          ` Now asking ${want.w}x${want.h}; the window will be clamped and the outermost ` +
          `chips may be cropped when armed. Not a layout fault: this ring does not fit this ` +
          `screen at a readable size.`,
      }).catch(() => {});
    }
  }
  geo.win = {
    w: Math.min(Math.max(geo.win.w, want.w), Math.floor(budgetW)),
    h: Math.min(Math.max(geo.win.h, want.h), Math.floor(budgetH)),
  };
  /* The crop guard reads the FINAL granted box. `#st-hud` is
     `position: fixed; inset: 0`, so its own offset box IS the overlay window's
     client area — which means that after the resize lands, `paintBloom` can
     measure the window Rust ACTUALLY gave us rather than the one we asked for.
     Seeded here from the ask so the very first arming (which can land before
     the resize) still has a sane bound. */
  _bloomHalf = { w: geo.win.w / 2, h: geo.win.h / 2 };

  // Re-apply the armed highlight: buildHud has just replaced every chip, so
  // whatever Rust last told us is armed has to be put back onto the NEW
  // element or the highlight silently disappears on a rebuild (resize, or the
  // absorb path building a second time inside one hold).
  paintArmedChip();

  const w = geo.win.w, h = geo.win.h;
  // Captured now, before the invoke below, so the rAF closure can tell THIS
  // show apart from whatever show is active by the time it actually runs.
  const seq = _hudShowSeq;
  // PROBLEM 112 — hand the promise back. The window MOVE is the one thing here
  // that cannot be animated, so callers must be able to wait for it and keep
  // content invisible until it has landed.
  return invoke<Rect | null>("overlay_fit_hud", { width: w, height: h, dpr: pageDpr() })
    .then((r) => {
      // One rAF so the webview has actually re-laid-out at the NEW window
      // size before the chip boxes are read — the chips are placed with
      // `calc(50% + …)`, so every rect depends on the post-resize viewport.
      // RACE GUARD: a preview's buildHud can still have this rAF pending when
      // a REAL Space-hold starts and bumps `_hudShowSeq` before the rAF fires
      // — without the check below, the preview's stale rAF would publish
      // preview geometry for the real ring (and consume `_chipsPubSeq`,
      // skipping the real publish outright).
      requestAnimationFrame(() => { if (seq === _hudShowSeq) publishHudChips(); });
      if (r) {
        _rect = r;
        // Round 3: the window is the canvas; the ring lives on its stage.
        if (seq === _hudShowSeq) applyStage(r.stage ?? null);
        return r;
      }
      // SHOULD-FIX 4 — the overlay_log call that used to live here is
      // DELETED, deliberately. `overlay_fit_hud`'s null return means Rust
      // already refused the fit and logged WHY, at INFO, with MORE detail
      // than this branch had (commands.rs documents it as a legitimate
      // non-fault: Space released inside the ~15ms between show and fit).
      // `overlay_log` is `log::warn!`, so this republished that same benign
      // event into the alarm channel, 9ms later, with less information —
      // proven duplicated live:
      //   07:10:58.828 [INFO] overlay_fit_hud: REFUSED 1144x572 ...
      //   07:10:58.837 [WARN] overlay-js: buildHud: overlay_fit_hud
      //                 returned null ...
      // Rust's copy survives a dead/frozen frontend and is strictly better;
      // this one only added noise to WARN.
      return null;
    })
    .catch((e) => {
      // KEPT: a rejected invoke is a real IPC failure (the command didn't
      // even complete), which Rust has no way to observe on its own — unlike
      // the null branch above, there is no Rust-side log this duplicates.
      // Has never fired in practice, but if it ever does, WARN is correct.
      invoke("overlay_log", { msg: `buildHud: overlay_fit_hud REJECTED: ${e}` }).catch(() => {});
      return null;
    });
}

/* =======================================================================
   ARMED CHIP — "this is the one that will fire when you let go".

   PRESENTATION ONLY. Which chip is armed is decided in Rust and arrives on
   the global `hud-pointer` event as `{ index }`, an index into the OUTER
   (apps) ring in the SAME order Rust sent them in `GuideHudPayload.apps`.
   `null` means nothing is armed. See `initToastListener` for the listener;
   the arrangement there (global `emit` + one listener in the overlay page) is
   the ONLY one that has ever worked in this project — `emit_to` never has.

   The index maps to the DOM by construction: `buildHud` appends the app chips
   in `apps` order and they are the only `.st-chip.ap` elements, so
   `querySelectorAll(".st-chip.ap")[i]` IS `apps[i]`. Nothing else may reorder
   them.
   ======================================================================= */
/** Last index Rust armed. Kept across rebuilds — see `paintArmedChip`. */
let _armed: number | null = null;

/** Put `_armed` back onto the live DOM. Called on every `hud-pointer` event
 *  AND at the end of every `buildHud`, because a rebuild throws the old chip
 *  elements away and the class would go with them. */
function paintArmedChip(): void {
  if (!_hudEl) return;
  const cells = _hudEl.querySelectorAll<HTMLElement>(".st-chip.ap");
  for (let i = 0; i < cells.length; i++) {
    cells[i].classList.toggle("armed", i === _armed);
  }
  /* AIMING — while ANY chip is armed the user is pointing at an app, and the
     specials ring is not part of that decision. Owner, 1.0.88: the ring is
     "too hard to look at the whole screen" mid-gesture. So the whole inner
     ring drops to a ghost for as long as a beam exists, and comes back the
     moment the pointer returns to the dead zone.

     Two things fall out of this for free, and both were open questions:
       · a beam that passes NEAR an inner chip on its way out can no longer
         make that chip look armed — there is nothing lit to confuse;
       · with the "show special keys on the ring" setting OFF there are no
         `.st-chip.sp` elements at all, so this class matches nothing and the
         whole behaviour no-ops without a single guard.
     The class goes on #st-hud, not on each chip, so it is ONE style write
     however many specials there are. */
  _hudEl.classList.toggle("aiming", _armed !== null);
  paintBloom();
  paintArmedBeam();
}

/* =======================================================================
   BLOOM — the armed chip and its two angular neighbours push outward and
   open to their full label. Everything else holds still.

   This is what pays for word clipping. At rest the ring is compact because
   every app chip shows one word; the moment the user aims, the three chips
   their ray is nearest tell them the whole name. They read ONE name at a
   time, which is all they ever needed to read.

   NO PROTOCOL CHANGE. Rust still sends `hud-pointer { index }` and nothing
   else — the neighbours are DERIVED here from the armed index, by geometric
   angle, out of the resting snapshot.

   PRESENTATION ONLY, RESOLVED AGAINST RESTING GEOMETRY. This is the load-
   bearing sentence in the whole feature. Bloom moves chips outward; if the
   moved boxes ever reached Rust, its next hit test would run against them,
   arm a different chip, move a different set, and oscillate with the cursor
   sitting perfectly still. So: `_apRest` is never written here, the push is a
   TRANSFORM (which leaves `offsetLeft`/`offsetTop` alone), and
   `publishHudChips` takes its sizes from `_apRest` rather than from
   `offsetWidth`. Three independent reasons the snapshot cannot be bloomed.

   THE PUSH LIVES ON THE `.st-chip` CELL, NOT ON THE INNER `<i>`. `st-bloom-in`
   runs on the `<i>` with `fill: both`, and a filled animation beats a plain
   transform declaration — a transform written there would silently never
   apply. That trap is documented twice in overlay-earthy.css already; this is
   the third thing to fall into it.
   ======================================================================= */
function paintBloom(): void {
  if (!_hudEl) return;
  const cells = _hudEl.querySelectorAll<HTMLElement>(".st-chip.ap");
  const on = new Set<number>();
  // `_armed` is an index Rust chose and can outrun this DOM for one frame
  // after a rebuild; a mismatched snapshot blooms nothing rather than throwing.
  if (_bloomOn && _armed !== null && _armed >= 0 && _armed < cells.length &&
      cells.length === _apRest.length) {
    on.add(_armed);
    // The two NEAREST ANGULAR NEIGHBOURS — by `atan2(y, x)`, the on-screen
    // angle, NOT by the ellipse parameter and NOT by payload order. On a 0.62
    // ellipse those disagree by up to ~25°, and with two bands the chip beside
    // you on screen is routinely not the next letter of the alphabet.
    const order = _apRest.map((c, i) => ({ i, g: c.geo })).sort((a, b) => a.g - b.g);
    const k = order.findIndex((o) => o.i === _armed);
    if (k >= 0 && order.length > 1) {
      on.add(order[(k - 1 + order.length) % order.length].i);
      on.add(order[(k + 1) % order.length].i);
    }
  }
  /* REDUCED MOTION: no push, but the label STILL OPENS. The armed state must
     never depend on motion to be legible — the same rule the armed chip's own
     block follows. The gate is here in JS and not in CSS because an inline
     custom property beats any stylesheet rule that tried to zero it. */
  const still = REDUCED();
  for (let i = 0; i < cells.length; i++) {
    const cell = cells[i];
    const lit = on.has(i);
    if (cell.classList.contains("bloom") !== lit) {
      cell.classList.toggle("bloom", lit);
      setChipLabel(cell, (lit ? cell.dataset.stFull : cell.dataset.stRest) ?? "");
    }
    const rest = _apRest[i];
    if (lit && !still && rest && rest.r > 0) {
      // Bound 1: the radial ceiling in force for this build. See _bloomCap.
      let push = Math.min(i === _armed ? BLOOM_PUSH_ARMED : BLOOM_PUSH_NEIGHBOUR,
                          Math.max(0, _bloomCap - rest.full / 2 - rest.r));
      // Bound 2: the window. A bloomed chip that leaves it is not "pushed
      // out", it is CROPPED — and a chip cropped by the window edge is the
      // one failure mode this overlay has no way to show the user. Solved per
      // axis along the chip's own radial direction; the small floor on the
      // divisor keeps a chip on a cardinal axis from dividing by zero.
      /* MEASURED FROM THE GRANTED WINDOW, NOT THE REQUESTED ONE. `#st-hud` is
         `position: fixed; inset: 0`, so its own offset box IS the overlay
         window's client area — the size Rust actually granted, after any 94%
         clamp. `_bloomHalf` (the ask) is only the seed for the first arming,
         which can land before the resize has been applied. Reading the ask
         when the two disagree is how a "guarded" chip still gets cropped.
         With the window now sized for the bloomed ring this should never
         bite; it is the net under that, not the mechanism. */
      /* ROUND 3: the window is the canvas and `#st-hud` is its stage, so
         the room a chip has is the WINDOW's edge in the chip's own
         direction, measured from the stage centre — asymmetric, because
         the stage is centred on the monitor and the window is the work
         area. Without a stage, `#st-hud` IS the window and the halves are
         the old symmetric ones. */
      let halfW: number, halfH: number;
      if (_stage) {
        const sc = stageCentre();
        halfW = rest.x >= 0 ? window.innerWidth - sc.x : sc.x;
        halfH = rest.y >= 0 ? window.innerHeight - sc.y : sc.y;
      } else {
        halfW = _hudEl.offsetWidth > 0 ? _hudEl.offsetWidth / 2 : _bloomHalf.w;
        halfH = _hudEl.offsetHeight > 0 ? _hudEl.offsetHeight / 2 : _bloomHalf.h;
      }
      if (halfW > 0) {
        const ux = Math.abs(rest.x) / rest.r, uy = Math.abs(rest.y) / rest.r;
        const roomX = (halfW - rest.full / 2 - Math.abs(rest.x)) / Math.max(0.02, ux);
        const roomY = (halfH - rest.h / 2 - Math.abs(rest.y)) / Math.max(0.02, uy);
        push = Math.max(0, Math.min(push, roomX, roomY));
      }
      const k = push / rest.r;
      cell.style.setProperty("--px", `${Math.round(rest.x * k)}px`);
      cell.style.setProperty("--py", `${Math.round(rest.y * k)}px`);
    } else {
      cell.style.removeProperty("--px");
      cell.style.removeProperty("--py");
    }
  }
}

function setArmedChip(index: number | null): void {
  const n = typeof index === "number" && isFinite(index) ? Math.trunc(index) : null;
  _armed = n !== null && n >= 0 ? n : null;
  paintArmedChip();
}

/* =======================================================================
   THRUSTER PLUME — a beam from the SPACE pill to the armed chip.

   Owner, 1.0.88, on making directional selection visible:

     "There should be a line type of thing going from Space to the app. And
      the line should not look like a plain line. It should look like the
      boost behind a spaceship — from the Space to the app — so a person can
      visually see which app is going to be launched depending on their
      cursor."

   PRESENTATION ONLY, and it invents NO new contract: the `hud-pointer` event
   still carries `{ index }` and nothing else. The two endpoints are read out
   of the DOM this file already owns.

   ONE element, TWO style writes per index change, ZERO per-frame JS. The
   plume is a fixed-size box pinned at the ring centre with its
   transform-origin on its own left edge; aiming it is `rotate()`, and
   reaching the chip is `scaleX()`. Both are composited — no width animation,
   so no layout runs while the beam sweeps. That matters more here than
   usual: this overlay composites in SOFTWARE (--disable-gpu).

   NO `filter: blur()` ANYWHERE IN IT. Every soft edge is a gradient stop —
   see the stylesheet block, which spells out why (PROBLEM 37: one 560x320
   element at blur(34px) made the entire overlay window compose zero pixels
   while every in-page check reported perfect health).
   ======================================================================= */
/** Nominal (unscaled) length of the beam box, in px. THE ONE SOURCE OF TRUTH:
 *  `buildHud` hands it to the stylesheet as `--beam-nom` — the same mechanism
 *  HUD_IN_MS/HUD_OUT_MS use — because this number divides into the scaleX
 *  written below, and a literal repeated in the CSS would be a second copy of
 *  it free to drift. Chosen large enough that the usual beam is a mild
 *  DOWN-scale (crisper gradient) and small enough that the box the compositor
 *  actually rasterises stays well under the toast glow's proven-safe
 *  footprint, at ANY beam length: a chip 1200px out still paints 880x56. */
const BEAM_NOM = 880;
/** How far SHORT of the chip centre the plume stops. The armed chip's own
 *  halo finishes the story; a tip that stabs into the label reads as an arrow
 *  pointing AT a thing rather than as thrust pushing TOWARDS it. */
const BEAM_GAP = 10;
/** Is `.on` currently on the beam? An arming that follows a DISARM must jump
 *  to its new angle, not sweep to it: sweeping is meaningful when the user can
 *  watch the beam travel, and meaningless — but still 190ms long — when the
 *  beam it is travelling from is invisible. */
let _beamOn = false;
/** The last angle WRITTEN, unwrapped (so it may sit outside ±π). See below. */
let _beamAng = 0;

function paintArmedBeam(): void {
  if (!_hudEl) return;
  const beam = _hudEl.querySelector<HTMLElement>(".st-beam");
  if (!beam) return;
  const sp = _hudEl.querySelector<HTMLElement>(".space");
  const cells = _hudEl.querySelectorAll<HTMLElement>(".st-chip.ap");
  // `_armed` is an index Rust chose; it can outrun this DOM for one frame
  // after a rebuild. An out-of-range index disarms rather than throws.
  const chip = _armed !== null ? cells[_armed] : undefined;
  const off = () => { beam.classList.remove("on"); _beamOn = false; };
  if (!sp || !chip) { off(); return; }

  /* offsetLeft/offsetTop, NOT getBoundingClientRect — the same reason
     `publishHudChips` gives, and here it is load-bearing rather than merely
     tidy. THREE transforms are live on these elements at the moment this
     runs: the chip's inner <i> is mid-`st-bloom-in` (which has `fill: both`,
     so it is authoritative even before it starts), `.st-chip.armed` wears a
     scale(1.1), and #st-hud itself may still be at `.hidden`'s scale(.93).
     None of them touches LAYOUT. A client rect would aim the beam at whatever
     the chip's on-screen position happened to be mid-bloom, so the beam would
     visibly chase the chip through its entrance.

     Both `.space` and `.st-chip` are placed at `left/top: 50% + offset` and
     then centred on that point by `translate(-50%, -50%)` — a transform, so
     not layout. Their offsetLeft/offsetTop therefore ARE their centres, and
     the beam's own rotation pivot is that same 50%/50% point (its CSS pins it
     there with `top: 50%` and `margin-top: -h/2`, so the pivot needs no
     JS at all and cannot drift out of sync with the chips). */
  const dx = chip.offsetLeft - sp.offsetLeft;
  const dy = chip.offsetTop - sp.offsetTop;
  const len = Math.hypot(dx, dy) - BEAM_GAP;
  // A chip closer to SPACE than the stop-short distance leaves no beam to
  // draw. Cannot happen with the real geometry (RIN_CLEAR alone is 115px),
  // but a zero/negative scaleX would flip the plume through the origin.
  if (!(len > 0)) { off(); return; }

  /* UNWRAP THE ANGLE. `atan2` returns (-π, π], and CSS interpolates the NUMBER
     inside rotate(), not the direction. So a chip at 3.05rad followed by its
     neighbour at -3.10rad — the two chips either side of the ±π seam, which is
     the LEFT flank of the ring and about as ordinary a place to point as
     exists — animates 6.15rad the long way round, sweeping the beam through
     every other chip in the ring. Rewriting the target as the equivalent angle
     nearest the one already on the element makes every sweep the short one. */
  let ang = Math.atan2(dy, dx);
  ang += Math.round((_beamAng - ang) / (Math.PI * 2)) * Math.PI * 2;
  _beamAng = ang;

  const jump = !_beamOn;
  // `.st-beam-jump` kills the transition for exactly this write. The reflow
  // between add and remove is what makes it a real frame boundary — without
  // it the browser coalesces both class changes and the transition survives.
  if (jump) beam.classList.add("st-beam-jump");
  beam.style.setProperty("--beam-ang", `${ang}rad`);
  beam.style.setProperty("--beam-scale", `${len / BEAM_NOM}`);
  if (jump) { void beam.offsetWidth; beam.classList.remove("st-beam-jump"); }
  beam.classList.add("on");
  _beamOn = true;
}

/* =======================================================================
   CHIP GEOMETRY PUBLISH — the frontend half of a two-part feature.

   *** THE RUST SIDE LANDS SEPARATELY. `publish_hud_chips` DOES NOT EXIST   ***
   *** YET as a Tauri command. Until it is registered this invoke rejects   ***
   *** with "command not found", which is EXPECTED and swallowed below —    ***
   *** a missing command must never be able to break the HUD.               ***

   Convention is deliberately IDENTICAL to `overlay_shape`
   (commands.rs ~1913-1917): rectangles in CSS px relative to the overlay
   window's client area, plus `dpr`, and Rust multiplies the two to reach
   physical pixels. Anything that consumes these — hit-testing the cursor
   against a chip, for instance — then works the same way the window-region
   code already does.

   Boxes are read from `offsetLeft/offsetTop/offsetWidth/offsetHeight`, NOT
   `getBoundingClientRect`, on purpose. The chips' inner `<i>` runs
   `st-bloom-in` with `fill: both`, and `#st-hud` itself may still be wearing
   `.hidden`'s `scale(.93)` at this moment — both of which contaminate a
   client rect and neither of which touches layout. `offsetLeft` is the
   untransformed layout position; the chip's own `translate(-50%, -50%)` is
   undone arithmetically here. `#st-hud` is `position: fixed; inset: 0` with
   no border or padding, so its offset origin IS the window client origin
   (that stays true under the handover pin, which only sets top: 0).

   ONCE PER HUD SHOW, never per frame. `_chipsPubSeq` gates it against
   `_hudShowSeq`, which `showGuideHud` bumps — so the absorb path building a
   second time inside one hold publishes once, not twice. A `resize` rebuild
   deliberately DOES re-publish (via `_hudShowSeq` being bumped there too):
   the rects genuinely moved, and stale hit-test geometry is worse than a
   second message.
   ======================================================================= */
/** Bumped once per HUD show (and once per resize rebuild). */
let _hudShowSeq = 0;
/** The last seq we published for. */
let _chipsPubSeq = -1;

export function publishHudChips(): void {
  if (!_isOverlay || !_hudEl) return;
  /* A PREVIEW PUBLISHES NOTHING, AND THAT IS THE WHOLE SUPPRESSION.
     Rust declines to publish the chip KEYS for the same show, so both of
     `pointer.rs`'s counts stay at zero and `sector_pick` is never reached —
     no armed chip, no beam, nothing a release or a click can fire. Doing it
     here rather than by threading a flag through the aiming code means there
     is no second mode to keep correct: a preview simply never tells Rust where
     anything is.
     `_chipsPubSeq` is deliberately left ALONE on this path, so it still holds
     the last REAL show's seq — a preview must not be able to consume the
     publish slot of the hold that follows it. That's necessary but not
     sufficient: the caller of this function (the rAF in buildHud's
     `overlay_fit_hud` handler) only fires it when the `seq` it captured at
     build time still matches `_hudShowSeq` — that guard is what stops a
     preview's own pending rAF from calling in here at all once a real hold
     has started and bumped the seq. */
  if (previewOverride(_lastPayload)) return;
  if (_chipsPubSeq === _hudShowSeq) return;      // already published this show
  _chipsPubSeq = _hudShowSeq;
  /* RESTING RECTS ONLY — never bloomed ones. `offsetLeft`/`offsetTop` are the
     untransformed layout position, so the bloom PUSH (a transform on the cell)
     cannot contaminate them. The SIZE is a different matter: bloom swaps the
     label text and raises the cap, so `offsetWidth` on a bloomed chip is the
     FULL-label width. That is why the size comes from `_apRest`, the snapshot
     the layout took before anything could bloom.
     Publish bloomed rects and Rust's next hit test runs against moved boxes,
     arms a different chip, moves a different set — a feedback oscillation with
     the cursor perfectly still. `.st-chip.ap` only, as always: specials are
     never in this snapshot and `pointer.rs` derives its dead zone from that. */
  /* THE STAGE ORIGIN, AND WHY IT IS ADDED HERE (2026-09-15).
     `offsetLeft`/`offsetTop` are measured from the chip's OFFSET PARENT,
     which is `#st-hud` (the nearest positioned ancestor). The block above
     reasons that `#st-hud` is `position: fixed; inset: 0`, so its offset
     origin IS the window client origin and these numbers are already
     window-relative. THAT STOPPED BEING TRUE IN PROBLEM 267 ROUND 3:
     `applyStage` now sets `#st-hud`'s left/top to the stage rectangle
     `overlay_fit_hud` returns, so every rect published from here was short
     by the stage origin — on the owner's panel (stage @ 255,138 css, dpr
     1.5) the whole chip cloud was handed to Rust 382 px left and 207 px
     above where it is drawn. Rust then measured each chip's BEARING from a
     centre that had not moved with it, which is the owner's 2026-09-15
     screenshot exactly: cursor near the top of the ring, the pill on the
     far RIGHT armed. Arithmetic: the chip drawn 500 px to the right of the
     centre was published at (500 − 382, 0 − 231) = 27° off north, while the
     chip drawn 450 px ABOVE it was published at (−382, −681) = 29° off
     north — so a cursor pointing due north picked the right-hand one, by
     2°. `sector_pick` was never wrong; its inputs were.
     The stage origin is added back here rather than subtracted in Rust
     because this is the only place that knows the offset parent moved. */
  const org = stageOrigin();
  const chips = Array.from(_hudEl.querySelectorAll<HTMLElement>(".st-chip.ap"))
    .map((c, i) => {
      const w = _apRest[i]?.w ?? c.offsetWidth;
      const h = _apRest[i]?.h ?? c.offsetHeight;
      return {
        x: org.x + c.offsetLeft - w / 2,
        y: org.y + c.offsetTop - h / 2,
        w, h,
      };
    });
  /* THE 41st CHIP IS DRAWN AND IS NOT HIT-TESTABLE. SAY SO.
     `pointer.rs`'s `MAX_CHIPS` is 40 and BOTH of its publish paths truncate
     with `.min(MAX_CHIPS)` — `publish_keys` for the letters and
     `publish_geometry` for the rects — silently, because a fixed-size static
     table is the right shape for something the hook thread reads. Its own
     comment says "26 letters is the real maximum (only alpha keys can carry
     bindings)", and through this app's UI that is true: `keyboard-matrix.ts`
     marks only `ALPHA_KEYS` bindable, so 26 is the ceiling a user can reach by
     clicking.
     IT IS NOT THE CEILING A CONFIG CAN REACH. `engine::hud_apps_for` iterates
     every `is_mapped()` binding in the profile with no key filter, and
     `config::profile_from_export` takes `bindings` from an imported file
     VERBATIM — no whitelist, no cap (PROBLEM 231 shipped that import). So a
     hand-edited or imported profile can carry 41+ mapped keys, this page will
     lay out and draw all of them, and chips 41.. would then aim at nothing
     with no error anywhere. Logged rather than clamped: clamping here would
     hide the same fact one layer up, and the ring is still CORRECT — it is the
     POINTER that stops at 40. */
  if (chips.length > 40 && _isOverlay) {
    invoke("overlay_log", {
      msg: `publishHudChips: ${chips.length} app chips published, but pointer.rs ` +
        `MAX_CHIPS is 40 — chips 41..${chips.length} are DRAWN and will not ` +
        `hit-test, so pointing at them arms nothing and releasing over them ` +
        `launches nothing. The keyboard can only bind 26; a profile this large ` +
        `came from an imported or hand-edited config.json.`,
    }).catch(() => {});
  }
  try {
    invoke("publish_hud_chips", {
      chips,
      dpr: window.devicePixelRatio || 1,
      // THE RING'S CENTRE, from the page that placed it. Rust used to
      // derive it from the overlay window's own centre; since round 3 the
      // window is the whole work area and the ring sits on a stage inside
      // it, so the two agree only when no appbar shortens the work area.
      centre: stageCentre(),
    })
      .catch(() => { /* Rust side not landed yet — see the block above */ });
  } catch { /* no IPC bridge at all (harness / dashboard) */ }
}

interface FlightGeo { x: number; y: number; w: number; h: number; s?: number }

/* =======================================================================
   THRUSTER CONVOY (toasts -> SPACE on hold) + SLINGSHOT DOWN (SPACE ->
   slots on release) - THRUSTER_SLING.md, 2026-08-18.

   ADAPTED, not transcribed: the source patch animates width/height/
   background/borderColor per frame. PROBLEM 115 bans exactly those in this
   file (layout+paint every frame; this overlay composites in software), so
   both flights use flightWarp's two-face cross-scale technique - same
   shapes, same timings, transform and opacity only.
   ======================================================================= */

/** Three-layer gradient exhaust plume, pointed opposite the travel angle.
 *  Gradients only - NEVER a blur filter here (PROBLEM 80). */
function mkPlume(travelAngleDeg: number): HTMLDivElement {
  const P = document.createElement("div");
  const ang = travelAngleDeg + 180;
  P.style.cssText =
    "position:absolute;left:0;top:0;pointer-events:none;" +
    "transform:rotate(" + ang + "deg);transform-origin:0 0";
  const layers: [number, number, string][] = [
    [96, 12, ".30"], [67, 8, ".55"], [40, 4.5, ".90"],
  ];
  for (const [len, th, a] of layers) {
    const bar = document.createElement("div");
    bar.style.cssText =
      "position:absolute;left:10px;top:0;width:" + len + "px;height:" + th + "px;" +
      "border-radius:999px;transform:translate(0,-50%);" +
      "background:linear-gradient(90deg,rgba(var(--st-glow-rgb)," + a + "),transparent)";
    P.appendChild(bar);
  }
  // Afterburner flicker: cheap scaleX loop, no filters.
  P.animate(
    [{ transform: "rotate(" + ang + "deg) scaleX(1)" },
     { transform: "rotate(" + ang + "deg) scaleX(.8)" },
     { transform: "rotate(" + ang + "deg) scaleX(1.08)" },
     { transform: "rotate(" + ang + "deg) scaleX(.88)" },
     { transform: "rotate(" + ang + "deg) scaleX(1)" }],
    { duration: 180, iterations: 24, easing: "linear" });
  return P;
}

/** Shed pressure rings + sparks from a mover's live position while it flies.
 *  Stops itself at `untilMs` or when the mover leaves the DOM. */
function shedExhaust(mover: HTMLElement, untilMs: number): void {
  const host = flightHost();
  const hr = host.getBoundingClientRect();
  const iv = window.setInterval(() => {
    if (!mover.isConnected) { window.clearInterval(iv); return; }
    const r = mover.getBoundingClientRect();
    const x = r.left - hr.left, y = r.top - hr.top;
    const ring = document.createElement("div");
    ring.style.cssText =
      "position:absolute;left:" + x + "px;top:" + y + "px;width:26px;height:26px;" +
      "border-radius:50%;border:1.5px solid rgba(var(--st-glow-rgb),.4);" +
      "pointer-events:none";
    host.appendChild(ring);
    ring.animate(
      [{ transform: "translate(-50%,-50%) scale(.3)", opacity: 0.8 },
       { transform: "translate(-50%,-50%) scale(1.9)", opacity: 0 }],
      { duration: 360, easing: "cubic-bezier(.22,1,.36,1)", fill: "forwards" });
    window.setTimeout(() => ring.remove(), 380);
    const dot = document.createElement("div");
    const jx = (Math.random() - 0.5) * 12, jy = (Math.random() - 0.5) * 12;
    dot.style.cssText =
      "position:absolute;left:" + (x + jx) + "px;top:" + (y + jy) + "px;" +
      "width:4px;height:4px;border-radius:50%;" +
      "background:rgba(var(--st-glow-rgb),.9);pointer-events:none";
    host.appendChild(dot);
    dot.animate(
      [{ transform: "translate(-50%,-50%) scale(1)", opacity: 0.9 },
       { transform: "translate(-50%,-50%) scale(.2)", opacity: 0 }],
      { duration: 300, easing: "linear", fill: "forwards" });
    window.setTimeout(() => dot.remove(), 320);
  }, EXHAUST_EVERY);
  window.setTimeout(() => window.clearInterval(iv), untilMs);
}

/**
 * Lift ONE pill off its slot and slam it into the SPACE key, rocket-style:
 * squat (THRUST_DIP px down at 11%), then a hard burn up the chord with plume
 * and shed exhaust. flightWarp's coordinate convention and two-face structure.
 * Returns ms from now until arrival (delay included).
 */
function flightThruster(o: {
  from: FlightGeo;
  to: FlightGeo;
  via?: { w: number; h: number; s: number } | null;
  html: string;
  delay?: number;
  onArrive?: () => void;
}): number {
  const { from, to, via, html, onArrive } = o;
  const delay = o.delay ?? 0;
  if (REDUCED()) { window.setTimeout(() => onArrive?.(), delay); return delay; }

  const host = flightHost();
  const dx = from.x - to.x;
  const dy = from.y - to.y;
  const ang = (Math.atan2(-dy, -dx) * 180) / Math.PI;   // slot -> SPACE
  const kA = from.s ?? 1;
  const kB = to.s ?? 1;
  const kV = via ? via.s : kA;
  const useVia = !!via && (Math.abs(via.w - from.w) > 1 || Math.abs(kV - kA) > 0.01);

  const mover = document.createElement("div");
  mover.style.cssText =
    "position:absolute;left:" + to.x + "px;top:" + to.y + "px;" +
    "will-change:transform;transform:translate(" + dx + "px," + dy + "px)";

  const trail = document.createElement("div");
  trail.className = "st-fly-trail";
  trail.style.width = Math.min(Math.hypot(dx, dy) * 0.8, 260) + "px";
  trail.style.transform = "translate(0,-50%) rotate(" + (ang + 180) + "deg) scaleX(0)";

  const pill = document.createElement("div");
  pill.className = "st-fly-wrap";

  const faceToast = document.createElement("div");
  faceToast.className = "st-fly face-toast";
  faceToast.style.width = from.w + "px";
  faceToast.style.height = from.h + "px";
  faceToast.style.background = "var(--st-pill-bg)";
  faceToast.style.border = "1px solid var(--st-pill-brd)";
  faceToast.innerHTML = html;

  const faceSpace = document.createElement("div");
  faceSpace.className = "st-fly face-space-pill";
  faceSpace.style.width = to.w + "px";
  faceSpace.style.height = to.h + "px";
  faceSpace.style.background = "var(--st-space-bg)";
  faceSpace.style.border = "1px solid var(--st-space-brd)";
  faceSpace.innerHTML = '<span class="face-space">SPACE</span>';
  faceSpace.style.opacity = "0";

  pill.append(faceToast, faceSpace);
  mover.append(trail, mkPlume(ang), pill);
  host.appendChild(mover);
  _flying++;

  const opt: KeyframeAnimationOptions = { duration: THRUST_MS, delay, fill: "both" };

  // Position: squat, then burn. ONE animation, one easing - the squat is a
  // keyframe INSIDE it, never a chained segment (the no-seam rule).
  mover.animate(
    [
      { transform: "translate(" + dx + "px," + dy + "px)" },
      { transform: "translate(" + dx + "px," + (dy + THRUST_DIP) + "px)", offset: 0.11 },
      { transform: "translate(0,0)" },
    ],
    { ...opt, easing: WARP_EASE });

  const toastToSpaceX = to.w / Math.max(1, from.w);
  const toastToSpaceY = to.h / Math.max(1, from.h);
  const spaceToToastX = from.w / Math.max(1, to.w);
  const spaceToToastY = from.h / Math.max(1, to.h);
  const T = (sx: number, sy: number) =>
    "translate(-50%,-50%) rotate(" + ang + "deg) scale(" + sx + "," + sy + ") rotate(" + (-ang) + "deg)";

  faceToast.animate(
    [
      { transform: T(kA, kA), opacity: 1 },
      { transform: T(kA * 0.94, kA * 1.06), opacity: 1, offset: 0.11 },   // the squat
      ...(useVia ? [{ transform: T(kV, kV), opacity: 0.6, offset: SETTLE_AT }] : []),
      { transform: T(THRUST_STRETCH, 0.62), opacity: 0, offset: SQUEEZE_AT },
      { transform: T(toastToSpaceX, toastToSpaceY), opacity: 0 },
    ],
    { ...opt, easing: WARP_EASE });

  faceSpace.animate(
    [
      { transform: T(spaceToToastX, spaceToToastY), opacity: 0 },
      { transform: T(THRUST_STRETCH, 0.62), opacity: 0, offset: SQUEEZE_AT },
      { transform: T(kB, kB), opacity: 1 },
    ],
    { ...opt, easing: WARP_EASE });

  trail.animate(
    [
      { transform: "translate(0,-50%) rotate(" + (ang + 180) + "deg) scaleX(0)", opacity: 0 },
      { transform: "translate(0,-50%) rotate(" + (ang + 180) + "deg) scaleX(1)", opacity: 0.9, offset: SQUEEZE_AT },
      { transform: "translate(0,-50%) rotate(" + (ang + 180) + "deg) scaleX(0)", opacity: 0 },
    ],
    { ...opt, easing: WARP_EASE });

  window.setTimeout(() => shedExhaust(mover, THRUST_MS * 0.8), delay + 10);
  window.setTimeout(() => shockAt(to), delay + THRUST_MS - 40);
  window.setTimeout(() => {
    mover.remove();
    _flying = Math.max(0, _flying - 1);
    onArrive?.();
  }, delay + THRUST_MS + 20);

  return delay + THRUST_MS;
}

/**
 * Fly ONE pill out of the SPACE key, around a curved arc, DIRECTLY into its
 * settled slot - one continuous position animation, one easing, no mid-air
 * stop of any kind (the whole point of this flight). SPACE face -> toast face
 * on the way. Returns ms until arrival.
 */
function flightSlingDown(o: {
  from: FlightGeo;
  to: FlightGeo;
  html: string;
  bow: number;
  delay?: number;
  onArrive?: () => void;
}): number {
  const { from, to, html, bow, onArrive } = o;
  const delay = o.delay ?? 0;
  if (REDUCED()) { window.setTimeout(() => onArrive?.(), delay); return delay; }

  const host = flightHost();
  const dx = from.x - to.x;
  const dy = from.y - to.y;
  const pts = arcPoints(dx, dy, bow, SLING_SAMPLES);
  const kB = to.s ?? 1;

  const mover = document.createElement("div");
  mover.style.cssText =
    "position:absolute;left:" + to.x + "px;top:" + to.y + "px;" +
    "will-change:transform;transform:translate(" + dx + "px," + dy + "px)";

  const trail = document.createElement("div");
  trail.className = "st-fly-trail";
  trail.style.width = Math.min(Math.hypot(dx, dy) * 0.72, 240) + "px";

  const pill = document.createElement("div");
  pill.className = "st-fly-wrap";

  const faceSpace = document.createElement("div");
  faceSpace.className = "st-fly face-space-pill";
  faceSpace.style.width = from.w + "px";
  faceSpace.style.height = from.h + "px";
  faceSpace.style.background = "var(--st-space-bg)";
  faceSpace.style.border = "1px solid var(--st-space-brd)";
  faceSpace.innerHTML = '<span class="face-space">SPACE</span>';

  const faceToast = document.createElement("div");
  faceToast.className = "st-fly face-toast";
  faceToast.style.width = to.w + "px";
  faceToast.style.height = to.h + "px";
  faceToast.style.background = "var(--st-pill-bg)";
  faceToast.style.border = "1px solid var(--st-pill-brd)";
  faceToast.innerHTML = html;
  faceToast.style.opacity = "0";

  pill.append(faceSpace, faceToast);
  mover.append(trail, pill);
  host.appendChild(mover);
  _flying++;

  const opt: KeyframeAnimationOptions = { duration: SLING_DOWN_MS, delay, fill: "both" };

  // 1 - position: the WHOLE arc in one animation with one easing. This is the
  //     no-pause guarantee - never split it.
  mover.animate(
    pts.map((p) => ({ transform: "translate(" + p.x + "px," + p.y + "px)", offset: p.t })),
    { ...opt, easing: CAPTURE_EASE });

  const spaceToSlotX = to.w / Math.max(1, from.w);
  const spaceToSlotY = to.h / Math.max(1, from.h);
  const slotToSpaceX = from.w / Math.max(1, to.w);
  const slotToSpaceY = from.h / Math.max(1, to.h);

  const shearD = (t: number) => {
    const up = Math.max(0, Math.min(1, (t - SLING_T0) / (SLING_MID - SLING_T0)));
    const k = t < SLING_MID
      ? 1 + (SLING_DOWN_STRETCH - 1) * up
      : 1 + (SLING_DOWN_STRETCH - 1) * Math.max(0, 1 - (t - SLING_MID) / (1 - SLING_MID));
    return { k: k, sy: 1 - 0.38 * (k - 1) / (SLING_DOWN_STRETCH - 1) };
  };
  const TD = (a: number, sx: number, sy: number) =>
    "translate(-50%,-50%) rotate(" + a + "deg) scale(" + sx + "," + sy + ") rotate(" + (-a) + "deg)";

  // 2 - SPACE face: 1:1 over the key at launch, gone by 17%.
  faceSpace.animate(
    pts.map((p) => {
      const a = p.a + 180;
      const sh = shearD(p.t);
      const bx = 1 + (spaceToSlotX - 1) * p.t;
      const by = 1 + (spaceToSlotY - 1) * p.t;
      return {
        transform: TD(a, bx * sh.k, by * sh.sy),
        opacity: p.t < 0.17 ? 1 - p.t / 0.17 : 0,
        offset: p.t,
      };
    }),
    { ...opt, easing: CAPTURE_EASE });

  // 3 - toast face: counter-scaled at launch, lands at exactly 1:1.
  faceToast.animate(
    pts.map((p) => {
      const a = p.a + 180;
      const sh = shearD(p.t);
      const bx = slotToSpaceX + (1 - slotToSpaceX) * p.t;
      const by = slotToSpaceY + (1 - slotToSpaceY) * p.t;
      const s = 1 + (kB - 1) * p.t;
      const IN0 = SLING_MID + 0.06;
      const fade = p.t < IN0 ? 0 : Math.min(1, (p.t - IN0) / 0.26);
      return { transform: TD(a, s * bx * sh.k, s * by * sh.sy), opacity: fade, offset: p.t };
    }),
    { ...opt, easing: CAPTURE_EASE });

  // 4 - trail rides the tangent; scaleX, never width; gone before landing.
  trail.animate(
    pts.map((p) => {
      const f = p.t < SLING_MID
        ? p.t / SLING_MID
        : Math.max(0, 1 - (p.t - SLING_MID) / (1 - SLING_MID));
      return {
        transform: "translate(0,-50%) rotate(" + (p.a + 180) + "deg) scaleX(" + f.toFixed(4) + ")",
        opacity: p.t < 0.06 || p.t > 0.94 ? 0 : 0.9,
        offset: p.t,
      };
    }),
    { ...opt, easing: CAPTURE_EASE });

  window.setTimeout(() => shockAt(to), delay + SLING_DOWN_MS - 60);
  window.setTimeout(() => {
    mover.remove();
    _flying = Math.max(0, _flying - 1);
    onArrive?.();
  }, delay + SLING_DOWN_MS + 20);

  return delay + SLING_DOWN_MS;
}

/**
 * PROBLEM 112 — fly ONE pill between its slot and the SPACE key, morphing
 * identity on the way.
 *
 * Coordinates are offsets from the flight origin — which is the window centre,
 * which is where the SPACE pill is — so SPACE is {0,0} and a toast slot is
 * wherever it happens to be. Sizes are always MEASURED off the live DOM, never
 * constants: a copy that starts at a hard-coded width does not sit on the pill
 * it replaces, and one that lands at a hard-coded width has to resize after
 * arrival.
 *
 * `via` is the size the original was still animating towards. A toast's
 * entrance runs 560ms, so a pill grabbed mid-entrance is only half grown; the
 * copy starts on that half-grown box (pixel-exact) and finishes the growth in
 * the first 18% of the flight, during the wind-up. Without it the growth the
 * eye was following just stops dead.
 *
 * Returns total ms from now until arrival (delay included).
 */
function flightWarp(o: {
  from: FlightGeo;
  to: FlightGeo;
  via?: { w: number; h: number; s: number } | null;
  html: string;
  toSpace: boolean;
  delay?: number;
  onArrive?: () => void;
}): number {
  const { from, to, via, html, toSpace, onArrive } = o;
  const delay = o.delay ?? 0;
  if (REDUCED()) {
    window.setTimeout(() => onArrive?.(), delay);
    return delay;
  }

  const host = flightHost();
  const dx = from.x - to.x;
  const dy = from.y - to.y;
  const ang = (Math.atan2(dy, dx) * 180) / Math.PI;
  const kA = from.s ?? 1;
  const kB = to.s ?? 1;
  const kV = via ? via.s : kA;
  const useVia = !!via && (Math.abs(via.w - from.w) > 1 || Math.abs(kV - kA) > 0.01);

  const mover = document.createElement("div");
  mover.style.cssText =
    `position:absolute;left:${to.x}px;top:${to.y}px;will-change:transform;` +
    `transform:translate(${dx}px,${dy}px)`;

  const trail = document.createElement("div");
  trail.className = "st-fly-trail";
  // Fixed length, scaled — never resized (PROBLEM 115).
  trail.style.width = `${Math.min(Math.hypot(dx, dy) * 0.8, 260)}px`;
  trail.style.transform = `translate(0,-50%) rotate(${ang}deg) scaleX(0)`;

  /* Each face is built at ITS OWN natural size and never resized. A single
     wrapper would have to lay one face out at the other's dimensions and then
     scale it, which stretches the text at that end of the flight. Two faces
     cross-fading means each is undistorted where it is actually being read,
     and the distorted one is the one fading out. */
  const pill = document.createElement("div");
  pill.className = "st-fly-wrap";

  const faceToast = document.createElement("div");
  faceToast.className = "st-fly face-toast";
  faceToast.style.width = `${from.w}px`;
  faceToast.style.height = `${from.h}px`;
  faceToast.style.background = "var(--st-pill-bg)";
  faceToast.style.border = "1px solid var(--st-pill-brd)";
  faceToast.innerHTML = html;
  faceToast.style.opacity = toSpace ? "1" : "0";

  const faceSpace = document.createElement("div");
  faceSpace.className = "st-fly face-space-pill";
  faceSpace.style.width = `${to.w}px`;
  faceSpace.style.height = `${to.h}px`;
  faceSpace.style.background = "var(--st-space-bg)";
  faceSpace.style.border = "1px solid var(--st-space-brd)";
  faceSpace.innerHTML = '<span class="face-space">SPACE</span>';
  faceSpace.style.opacity = toSpace ? "0" : "1";

  pill.append(faceToast, faceSpace);
  mover.append(trail, pill);
  host.appendChild(mover);
  _flying++;

  const opt: KeyframeAnimationOptions = { duration: WARP_MS, delay, fill: "both" };

  /* ---------------------------------------------------------------------
     PROBLEM 115 — TRANSFORM AND OPACITY ONLY.
     The first version animated `width`, `height`, `background` and
     `borderColor` on the pill and `width` on the trail. Every one of those
     forces LAYOUT or PAINT every frame, so the motion could never be smooth
     however the easing was tuned — and this overlay runs with --disable-gpu
     on machines that fail the compositing self-test (PROBLEM 80), where there
     is no headroom for per-frame layout at all.
     Nothing below touches layout or paint after the first frame.
     --------------------------------------------------------------------- */

  // How much each face must scale to match the OTHER's box.
  const toastToSpaceX = to.w / Math.max(1, from.w);
  const toastToSpaceY = to.h / Math.max(1, from.h);
  const spaceToToastX = from.w / Math.max(1, to.w);
  const spaceToToastY = from.h / Math.max(1, to.h);

  // Travel: pure translate.
  mover.animate(
    [{ transform: `translate(${dx}px,${dy}px)` }, { transform: "translate(0,0)" }],
    { ...opt, easing: WARP_EASE },
  );

  // Mid-flight the pill stretches ALONG THE TRAVEL AXIS — rotate into the
  // axis, scale, rotate back. That is what makes it read as a warp rather
  // than a resize. The transform function list must match across keyframes.
  const T = (sx: number, sy: number) =>
    `translate(-50%,-50%) rotate(${ang}deg) scale(${sx},${sy}) rotate(${-ang}deg)`;

  faceToast.animate(
    [
      { transform: T(kA, kA), opacity: toSpace ? 1 : 0 },
      ...(useVia
        ? [{ transform: T(kV, kV), opacity: toSpace ? 1 : 0, offset: SETTLE_AT }]
        : []),
      { transform: T(1.9, 0.5), opacity: 0, offset: SQUEEZE_AT },
      { transform: T(toastToSpaceX, toastToSpaceY), opacity: toSpace ? 0 : 1 },
    ],
    { ...opt, easing: WARP_EASE },
  );

  faceSpace.animate(
    [
      { transform: T(spaceToToastX, spaceToToastY), opacity: toSpace ? 0 : 1 },
      { transform: T(1.9, 0.5), opacity: 0, offset: SQUEEZE_AT },
      { transform: T(kB, kB), opacity: toSpace ? 1 : 0 },
    ],
    { ...opt, easing: WARP_EASE },
  );

  // Trail: scaleX, never width.
  trail.animate(
    [
      { transform: `translate(0,-50%) rotate(${ang}deg) scaleX(0)`, opacity: 0 },
      { transform: `translate(0,-50%) rotate(${ang}deg) scaleX(1)`, opacity: 0.85, offset: SQUEEZE_AT },
      { transform: `translate(0,-50%) rotate(${ang}deg) scaleX(0)`, opacity: 0 },
    ],
    { ...opt, easing: WARP_EASE },
  );

  window.setTimeout(() => shockAt(to), delay + WARP_MS - 40);
  window.setTimeout(() => {
    mover.remove();
    _flying = Math.max(0, _flying - 1);
    onArrive?.();
  }, delay + WARP_MS + 20);

  return delay + WARP_MS;
}

/* =======================================================================
   SLINGSHOT ARRIVAL - chip -> toast, the HUD-is-up direction only
   ======================================================================= */

/** The ring cell + chip for a launched app, or null (volume, clipboard, an app
 *  not on the ring). Case-insensitive on the leading word, so a toast reading
 *  "Spotify launched" finds the "Spotify" chip. */
function chipFor(name: string | undefined): { cell: HTMLElement; chip: HTMLElement } | null {
  if (!name || !_hudEl) return null;
  const key = name.trim().toLowerCase();
  for (const cell of Array.from(_hudEl.querySelectorAll<HTMLElement>("[data-st-app]"))) {
    const app = cell.dataset.stApp ?? "";
    if (!app) continue;
    if (key === app || key.startsWith(app + " ") || app.startsWith(key)) {
      const chip = cell.firstElementChild as HTMLElement | null;
      if (chip) return { cell, chip };
    }
  }
  return null;
}

/** Quadratic-bezier samples from (dx,dy) down to (0,0), bowed `bow` px
 *  perpendicular to the chord. Each sample carries the tangent angle so the
 *  pill can fly nose-first. */
function arcPoints(dx: number, dy: number, bow: number, n: number) {
  const len = Math.hypot(dx, dy) || 1;
  const px = -dy / len, py = dx / len;               // unit perpendicular
  const cx = dx / 2 + px * bow, cy = dy / 2 + py * bow;
  const out: { t: number; x: number; y: number; a: number }[] = [];
  for (let i = 0; i <= n; i++) {
    const t = i / n, u = 1 - t;
    out.push({
      t,
      x: u * u * dx + 2 * u * t * cx,
      y: u * u * dy + 2 * u * t * cy,
      a: (Math.atan2(2 * u * (cy - dy) + 2 * t * -cy,
                     2 * u * (cx - dx) + 2 * t * -cx) * 180) / Math.PI,
    });
  }
  return out;
}

/** Hide the chip and leave a dashed socket in its cell. */
function tearOut(c: { cell: HTMLElement; chip: HTMLElement }): void {
  const box = boxOf(c.chip);
  c.chip.style.visibility = "hidden";
  const ghost = document.createElement("div");
  ghost.className = "st-chip-socket";
  ghost.style.width = box.w + "px";
  ghost.style.height = box.h + "px";
  c.cell.appendChild(ghost);
  ghost.animate([{ opacity: 0 }, { opacity: 1, offset: 0.2 }, { opacity: 0.55 }],
    { duration: 300, fill: "both" });
}

/** Fill the socket back in: ghost fades, chip pops back with a spring. */
function refill(cell: HTMLElement): void {
  const ghost = cell.querySelector(".st-chip-socket");
  const chip = cell.firstElementChild as HTMLElement | null;
  if (ghost instanceof HTMLElement) {
    ghost.animate([{ opacity: 0.55 }, { opacity: 0 }], { duration: 220, fill: "forwards" });
    window.setTimeout(() => ghost.remove(), 240);
  }
  if (chip) {
    chip.style.visibility = "visible";
    chip.animate(
      [{ transform: "scale(.6)", opacity: 0 },
       { transform: "scale(1.06)", opacity: 1, offset: 0.7 },
       { transform: "scale(1)", opacity: 1 }],
      { duration: 360, easing: "cubic-bezier(.34,1.3,.4,1)" });
  }
}

/**
 * Fly ONE pill from its app's chip, around the OUTSIDE of the ring, into its
 * staged slot. Chip face cross-fades to toast face; the box morphs from the
 * chip's measured size to the slot's settled size.
 *
 * TRANSFORM AND OPACITY ONLY (PROBLEM 115). The patch this came from animated
 * `width`/`height` on the pill and `width` on the trail; both force layout on
 * every frame and cannot be smooth here, where the overlay composites in
 * software. The box morph is a scale() on each face, cross-scaled exactly as
 * flightWarp does it, and the trail is a fixed-width bar driven by scaleX().
 *
 * Coordinates are offsets from the flight origin (window centre), same
 * convention as flightWarp. Returns ms until arrival.
 */
function flightSling(o: {
  from: FlightGeo;
  to: FlightGeo;
  chipHtml: string;
  toastHtml: string;
  cell: HTMLElement;
  onArrive?: () => void;
}): number {
  const { from, to, chipHtml, toastHtml, cell, onArrive } = o;
  if (REDUCED()) { onArrive?.(); return 0; }

  const host = flightHost();
  const dx = from.x - to.x;
  const dy = from.y - to.y;
  // Bow AWAY from the window centre so the swing clears the ring and never
  // crosses the SPACE key. The chip's own x decides the side.
  const bow = SLING_BOW * (Math.sign(from.x) || 1);
  const pts = arcPoints(dx, dy, bow, SLING_SAMPLES);

  const mover = document.createElement("div");
  mover.style.cssText =
    "position:absolute;left:" + to.x + "px;top:" + to.y + "px;" +
    "will-change:transform;transform:translate(" + dx + "px," + dy + "px)";

  const trail = document.createElement("div");
  trail.className = "st-fly-trail";
  trail.style.width = Math.min(Math.hypot(dx, dy) * 0.72, 240) + "px";

  const pill = document.createElement("div");
  pill.className = "st-fly-wrap";

  // ONE BOX, always visible door to door (the source patch's design - my first
  // version put the background on the faces, and between the chip face fading
  // out at 17% and the toast face arriving at 60% the pill had NO visible box
  // for ~400ms, which on this machine read as "no animation at all").
  const box = document.createElement("div");
  box.className = "st-fly face-chip-pill";
  box.style.width = from.w + "px";
  box.style.height = from.h + "px";
  box.style.background = "var(--st-pill-bg)";
  box.style.border = "1px solid var(--st-pill-brd)";

  const faceToast = document.createElement("div");
  faceToast.className = "sl-face";
  faceToast.innerHTML = toastHtml;
  faceToast.style.opacity = "0";

  const faceChip = document.createElement("div");
  faceChip.className = "sl-face";
  faceChip.innerHTML = chipHtml;

  box.append(faceToast, faceChip);
  pill.appendChild(box);
  mover.append(trail, pill);
  host.appendChild(mover);

  // GEOMETRY LOG - so "nothing was visible" is a measurement, not a mystery.
  // One line per flight: where it starts, where it lands, and whether both are
  // actually inside the overlay window at flight time.
  {
    const iw = window.innerWidth, ih = window.innerHeight;
    const inWin = (p: FlightGeo) =>
      Math.abs(p.x) < iw / 2 + 40 && Math.abs(p.y) < ih / 2 + 40;
    invoke("overlay_log", {
      msg: `sling-geo: from=(${Math.round(from.x)},${Math.round(from.y)} ` +
           `${Math.round(from.w)}x${Math.round(from.h)})${inWin(from) ? "" : " OFF-WINDOW"} ` +
           `to=(${Math.round(to.x)},${Math.round(to.y)} ` +
           `${Math.round(to.w)}x${Math.round(to.h)})${inWin(to) ? "" : " OFF-WINDOW"} ` +
           `win=${iw}x${ih} bow=${Math.round(bow)}`,
    }).catch(() => {});
  }
  _flying++;
  _slingUntil = performance.now() + SLING_MS + 120;   // keep the ring alive

  const opt: KeyframeAnimationOptions = { duration: SLING_MS, fill: "both" };

  // 1 - travel: the sampled arc, one global capture easing.
  mover.animate(
    pts.map((p) => ({ transform: "translate(" + p.x + "px," + p.y + "px)", offset: p.t })),
    { ...opt, easing: CAPTURE_EASE });

  // Box morph factors: the one box grows from the chip's size to the slot's.
  const chipToSlotX = to.w / Math.max(1, from.w);
  const chipToSlotY = to.h / Math.max(1, from.h);

  /** Nose-first shear at t: peaks at SLING_MID, ramps from SLING_T0 - AFTER the
   *  chip face has faded, because shear on legible text reads as a broken
   *  glyph rather than as speed. */
  const shear = (t: number) => {
    const up = Math.max(0, Math.min(1, (t - SLING_T0) / (SLING_MID - SLING_T0)));
    const k = t < SLING_MID
      ? 1 + (SLING_STRETCH - 1) * up
      : 1 + (SLING_STRETCH - 1) * Math.max(0, 1 - (t - SLING_MID) / (1 - SLING_MID));
    return { k: k, sy: 1 - 0.38 * (k - 1) / (SLING_STRETCH - 1) };
  };
  // Rotate into the travel axis, scale, rotate back - the same T() shape
  // flightWarp uses, re-aimed at every sample so the ship follows its velocity.
  const T = (a: number, sx: number, sy: number) =>
    "translate(-50%,-50%) rotate(" + a + "deg) scale(" + sx + "," + sy + ") rotate(" + (-a) + "deg)";

  // 2 - the BOX: starts 1:1 over the real chip, morphs to the slot size while
  //     shearing nose-first. Visible for the whole flight.
  box.animate(
    pts.map((p) => {
      const a = p.a + 180;
      const sh = shear(p.t);
      const bx = 1 + (chipToSlotX - 1) * p.t;
      const by = 1 + (chipToSlotY - 1) * p.t;
      return { transform: T(a, bx * sh.k, by * sh.sy), offset: p.t };
    }),
    { ...opt, easing: CAPTURE_EASE });

  // 3 - the faces only cross-fade; the box carries all the motion.
  faceChip.animate(
    [{ opacity: 1 }, { opacity: 0, offset: 0.17 }, { opacity: 0 }],
    { ...opt, easing: "linear" });
  faceToast.animate(
    [{ opacity: 0 }, { opacity: 0, offset: SLING_MID + 0.1 },
     { opacity: 1, offset: SLING_MID + 0.34 }, { opacity: 1 }],
    { ...opt, easing: "linear" });

  // 4 - trail: scaleX, never width. Grows to mid, gone before landing.
  trail.animate(
    pts.map((p) => {
      const f = p.t < SLING_MID
        ? p.t / SLING_MID
        : Math.max(0, 1 - (p.t - SLING_MID) / (1 - SLING_MID));
      return {
        transform: "translate(0,-50%) rotate(" + (p.a + 180) + "deg) scaleX(" + f.toFixed(4) + ")",
        opacity: p.t < 0.06 || p.t > 0.94 ? 0 : 0.9,
        offset: p.t,
      };
    }),
    { ...opt, easing: CAPTURE_EASE });

  window.setTimeout(() => shockAt(to), SLING_MS - 70);
  window.setTimeout(() => {
    mover.remove();
    _flying = Math.max(0, _flying - 1);
    // The ring may already be gone if Space was released mid-flight.
    if (cell.isConnected) refill(cell);
    onArrive?.();
  }, SLING_MS + 20);

  return SLING_MS;
}

function shockAt(at: FlightGeo): void {
  if (REDUCED()) return;
  const r = document.createElement("div");
  r.className = "st-fly-shock";
  r.style.left = `${at.x}px`;
  r.style.top = `${at.y}px`;
  flightHost().appendChild(r);
  r.animate(
    [
      { transform: "translate(-50%,-50%) scale(.16)", opacity: 0.95 },
      { transform: "translate(-50%,-50%) scale(1)", opacity: 0 },
    ],
    { duration: 440, easing: "cubic-bezier(.22,1,.36,1)", fill: "forwards" },
  );
  window.setTimeout(() => r.remove(), 480);
}

/** Exported for the dev harness only (`preview.ts` `?spacering`), so the
 *  pill can be measured with and without a focused app. The overlay reaches
 *  it through the `guide-hud-show` listener, as it always has. */
export function showGuideHud(payload: GuideHudPayload): void {
  _slingHeld = false;
  /* PROBLEM 175 — a fresh hold is proof the previous one finished, so nothing
     from it may still be "busy". Without this, the latch described in
     hideGuideHud survives every subsequent hold: the user's own attempt to
     make the HUD appear again is exactly the event that used to be blocked. */
  window.clearTimeout(_hudBusyGuard);
  _hudBusy = false;
  applyStage(null);             // PROBLEM 137 - undo the handover pin / the
                                // last stage; buildHud's fit sets the new one
                                // SLINGSHOT - a fresh hold gets a fresh handover grace
  _lastPayload = payload;
  _hudActive = true;
  _hudShowSeq++;                // one chip-geometry publish per show
  _armed = null;                // a new hold arms nothing until Rust says so
  if (!_hudEl) {
    _hudEl = document.createElement("div");
    _hudEl.id = "st-hud";
    _hudEl.setAttribute("aria-label", "Space modifier guide");
    document.body.appendChild(_hudEl);
  }
  /* The entrance/exit durations live in ONE place — here — and are handed to
     the CSS as custom properties. The comment on HUD_OUT_MS used to say "MUST
     match #st-hud's transition in overlay-earthy.css", which is a drift
     waiting to happen: nothing enforced it, and the number is load-bearing
     for the teardown timers. Now the stylesheet reads these, so the two
     cannot disagree. The CSS keeps literal fallbacks for the first paint. */
  _hudEl.style.setProperty("--hud-in", `${HUD_IN_MS}ms`);
  _hudEl.style.setProperty("--hud-out", `${HUD_OUT_MS}ms`);
  // `collapsing` drives the per-ring exit; it must be off before the chips are
  // created, or they would be born in the collapsed state and then TRANSITION
  // into place, fighting st-bloom-in for the entrance.
  _hudEl.classList.remove("landed", "space-gone", "collapsing");
  anchorGlow("hud");
  const g = document.getElementById("st-toastglow");
  showToastGlow(g);

  if (absorbIntoSpace(payload)) return;

  /* ---- nothing on screen: build and MOVE first, reveal after ----
     Order matters. `.hidden` used to come off here, BEFORE buildHud, so the HUD
     painted inside the small bottom window the toast had left behind and the
     window then jumped out from under it. The move cannot be animated, so it
     has to happen while nothing is visible. */
  _hudEl.classList.remove("handoff");
  setToastLayerHidden(true);
  _hudEl.classList.add("hidden");
  buildHud(payload).then(() => {
    if (!_hudActive || !_hudEl) return;      // released during the move
    requestAnimationFrame(() => {
      if (!_hudActive || !_hudEl) return;
      _hudEl.classList.remove("hidden");
      sweep(300, 820, 190);
    });
  });
}

/**
 * PROBLEM 112 — the absorb: every live pill flies into the SPACE key, 95ms
 * apart, and the ring only pulses once the last has landed.
 *
 * The window grows ONCE, mid-flight, and never moves again this hold — a
 * returning toast lands in a staged slot inside the HUD window. That takes the
 * handover from two Win32 moves to one.
 *
 * Returns false if there is nothing to absorb, so the caller falls through to
 * the plain path.
 */
function absorbIntoSpace(payload: GuideHudPayload): boolean {
  // PROBLEM 113 — no rect, no absorb. `_rect` is the window position the pills'
  // start coordinates are computed against; if it is missing or stale the
  // flight begins from the wrong place (observed: pills launching off to one
  // side). Falling through to the plain path is always correct, just less
  // pretty, and is far better than a visibly wrong flight.
  if (!WARP || !_isOverlay || !_rect || REDUCED() || !_hudEl) return false;
  const live = _toasts.filter((t) => t.phase !== "leave");
  if (live.length === 0) return false;

  // Screen-space positions BEFORE the grow. The viewport itself is about to
  // move, so viewport coordinates alone are not enough — this is why the Rust
  // fit commands have to hand their rect back.
  const before = _rect;
  const src = live.map((t) => {
    const b = boxOf(t.el);
    return {
      t,
      sx: before.x + b.cx, sy: before.y + b.cy,
      w: b.w, h: b.h, s: b.s,
      html: t.el.innerHTML,
    };
  });

  live.forEach(freezeEntry);
  _absorbed = live.slice();
  setToastLayerHidden(false);
  live.forEach((t) => park(t, true));   // same tick as the copies being made

  const total = THRUST_MS + THRUST_STAGGER * (src.length - 1);
  _hudEl.classList.add("handoff");      // SPACE is delivered by the flight
  _hudEl.classList.remove("hidden");    // ring + chips wait on their delay
  buildHud(payload, Math.max(0, total - 120)).then((after) => {
    if (!_hudActive || !_hudEl) return;
    const now = after ?? _rect;
    if (!now) return;
    const host = flightHost().getBoundingClientRect();
    const to = spaceBox();
    src.forEach((sItem, i) => {
      const settled = settledBox(sItem.t.el);   // the size it was growing towards
      const last = i === src.length - 1;
      flightThruster({
        from: {
          x: sItem.sx - (now.x + host.left),
          y: sItem.sy - (now.y + host.top),
          w: sItem.w, h: sItem.h, s: sItem.s,
        },
        via: { w: settled.w, h: settled.h, s: settled.s },
        to,
        html: sItem.html,
        delay: THRUST_STAGGER * i,
        onArrive: () => {
          if (!last || !_hudEl) return;
          _hudEl.classList.add("landed");
          setStageAnchor(true);
          sweep(300, 820, 190);
        },
      });
    });
  });
  return true;
}

function hideGuideHud(actionPending = false): void {
  _hudActive = false;
  _hudBusy = true;
  // PROBLEM 114 — measure SPACE before `.hidden` scales the HUD to .93, so a
  // toast arriving in the grace window launches from its true position.
  if (_hudEl && _isOverlay) {
    _spaceExit = spaceBox();
    _spaceExitAt = performance.now();
  }
  const back = _absorbed.filter((t) => _toasts.includes(t));
  _absorbed = [];

  /* ---- THRUSTER_SLING: the pills come back out of SPACE and fly ONE
     continuous arc each, straight to their TRUE bottom-centre slots - "no
     pause, no hover, no mid-air stop" (owner, 2026-08-18). Reachable because
     PROBLEM 137's handover window extends to overlay_fit's own bottom edge:
     the grow is invisible (top edge fixed, ring pinned), the stack sits on
     its NORMAL bottom anchor, and the post-landing window shrink is invisible
     too because both windows share the same bottom edge. ---- */
  if (WARP && back.length > 0 && _isOverlay && !REDUCED() && _hudEl) {
    _stageMode = true;                  // no window fits while pills fly
    setToastLayerHidden(false);
    back.forEach((t) => {
      t.phase = "open";
      t.el.classList.remove("leave");
      t.el.classList.add("open");
      park(t, true);                    // hidden until its copy lands on it
    });

    const ringH = _stage ? _stage.h : window.innerHeight;
    invoke<Rect | null>("overlay_fit_handover", {
      width: _stage ? _stage.w : window.innerWidth, height: ringH, dpr: pageDpr(),
    }).then(() => {
      if (!_hudEl) return;
      pinStage(ringH);                  // pin: the ring must not move a pixel
      const from = spaceBox();          // AFTER the grow (same viewport as the
                                        // slots), BEFORE .hidden scales to .93
      _hudEl.classList.remove("landed");
      _hudEl.classList.add("hidden", "collapsing");  // ring folds away under the arcs
      sweep(760, 280, 150);
      setStageAnchor(false);            // normal anchor = the true final slots
      void document.body.offsetWidth;
      relayout();                       // depth attrs only - stage guard holds

      back.forEach((t, i) => {
        const to = settledBox(t.el);    // its exact slot - land HERE, directly
        const bow = SLING_DOWN_BOW * (i % 2 ? -1 : 1) * (1 + Math.floor(i / 2) * 0.35);
        flightSlingDown({
          from, to, html: t.el.innerHTML, bow, delay: STAGGER * i,
          onArrive: () => { park(t, false); thawEntry(t); },
        });
      });
    }).catch(() => {
      // The grow failed: reveal the pills in place rather than fly wrong.
      if (_hudEl) {
        _hudEl.classList.remove("landed");
        _hudEl.classList.add("hidden", "collapsing");
      }
      sweep(760, 280, 150);
      back.forEach((t) => { park(t, false); thawEntry(t); });
    });

    window.setTimeout(() => {
      _hudBusy = false;
      window.clearTimeout(_hudBusyGuard); // PROBLEM 175 — a real clear retires the deadline
      if (_hudActive) return;           // re-held mid-flight
      if (_hudEl) { _hudEl.innerHTML = ""; _hudEl.classList.remove("handoff", "collapsing"); }

      // PROBLEM 113 — LEAVE STAGE MODE. This was missing, and it broke toasts
      // for the rest of the stack's life.
      //
      // `_stageMode` blocks every window fit (fitToStack / requestFit), which
      // is correct DURING the handover — the pills are living inside the HUD's
      // window and a resize would yank them. But it was never cleared
      // afterwards, so:
      //   * `overlay_fit` never ran again, and overlay_fit is what SHOWS the
      //     window — so every later toast fired into a hidden window and the
      //     user saw nothing. Measured: 4 combos logged after a hold with ZERO
      //     overlay_fit lines between them.
      //   * `_rect` only updates on a fit, so it went stale, and the next
      //     absorb computed its start position from an out-of-date window rect
      //     — which is why a flight sometimes began off to one side.
      // It only recovered when the stack emptied and retire() reset the flag,
      // which is the "it self-heals eventually" the user described.
      if (_stageMode) {
        _stageMode = false;
        setStageAnchor(false);
      }
      anchorGlow("toast");
      // One fit now the handover is over: re-shows the window, restores toast
      // geometry, and refreshes _rect for the next absorb.
      if (_toasts.length > 0) relayout();
      else if (_isOverlay) invoke("overlay_toasts_done").catch(() => {});
    }, HUD_OUT_MS + SLING_DOWN_MS + STAGGER * back.length);
    return;
  }

  /* SLINGSHOT - the ring must OUTLIVE the handover, in two stages.
     The engine calls cancel_hud() BEFORE dispatching the action, so this runs
     FIRST and the toast arrives afterwards - anywhere from a few ms (focus) to
     several hundred (a cold launch, waiting on ShellExecute). Collapsing the
     ring on the usual 240ms schedule is exactly the owner's complaint: "as soon
     as I left the space key the guide disappeared... there was no time".

     Stage 1: hold the ring, once, for SLING_HANDOVER_MS, so a toast that is
              about to arrive still finds its chip to tear out of.
     Stage 2: if a flight did start, wait for it to land before collapsing.

     Both are bounded, and `_slingHeld` makes stage 1 strictly one-shot so this
     can never defer forever. Cost on a plain release with no shortcut: the ring
     lingers SLING_HANDOVER_MS. The owner asked for MORE time, not less. */
  /* PROBLEM 175 — THE LATCH. `_hudBusy` is set true at the top of this
     function and cleared ONLY inside the timeout at the bottom. Both SLING
     branches below `return` without reaching it; each schedules another
     `hideGuideHud(false)`, but guarded by `if (!_hudActive)` — so re-holding
     Space inside the grace window skips the rescheduled call and `_hudBusy`
     stays true for the rest of the session.

     `_hudBusy` gates `fitToStack()` and `requestFit()`, and `overlay_fit` is
     what SIZES AND SHOWS the overlay window. Latched, it means no toast and no
     HUD placement ever again until the app restarts.

     This is not a theory. The owner's 2026-08-24 log, from the overlay's own
     instrumentation:

       22:08:31.493  sling: "PiP: Top-Left"   hudActive=false hudBusy=true
       22:11:16.709  sling: "PiP: Top-Right"  hudActive=false hudBusy=true
       22:11:34.665  sling: "Frame Restored"  hudActive=false hudBusy=true

     `hudActive=false` with `hudBusy=true`, held across three minutes and many
     keypresses — and he confirmed restarting the app clears it until it happens
     again, which is exactly a session-scoped latch.

     The fix is a DEADLINE, not more `return` bookkeeping. Every deferral below
     is bounded (flightLeft, or SLING_HANDOVER_MS), so if `_hudBusy` is still
     set well past the longest of them, no path is coming to clear it and the
     flag is simply stuck. Clearing it then costs nothing: `_hudActive` is
     checked independently everywhere `_hudBusy` is, so an early clear can only
     permit a fit that the HUD's own state already allows.

     Generalise: a flag set on entry and cleared on ONE exit path is a latch
     waiting for a second exit path to be added. Either clear it on every
     return, or give it a deadline. This one has both now.  */
  const armBusyDeadline = (): void => {
    window.clearTimeout(_hudBusyGuard);
    _hudBusyGuard = window.setTimeout(() => {
      if (!_hudBusy || _hudActive) return;
      console.warn("toast: _hudBusy was still set with no HUD — clearing a stuck latch");
      _hudBusy = false;
      _stageMode = false;
      _slingStaged = false;
      _slingHeld = false;
      if (_isOverlay) {
        invoke("overlay_log", {
          msg: "toast: cleared a stuck _hudBusy latch (PROBLEM 175)",
        }).catch(() => {});
      }
      if (_toasts.length) requestFit();
      else if (_isOverlay) invoke("overlay_toasts_done").catch(() => {});
    }, SLING_HANDOVER_MS + SLING_MS + 600);
  };

  if (SLING && !REDUCED()) {
    armBusyDeadline();
    const flightLeft = Math.max(0, _slingUntil - performance.now());
    if (flightLeft > 0) {
      // A flight is in the air: fold the ring away UNDER it (pure visuals -
      // the flight lives in #st-flight, not #st-hud) and defer the real
      // teardown, whose refit would yank the window mid-flight, to landing.
      if (_hudEl) _hudEl.classList.add("hidden", "collapsing");
      sweep(760, 280, 150);
      window.setTimeout(() => { if (!_hudActive) hideGuideHud(false); }, flightLeft + 40);
      return;
    }
    // The engine says a toast is coming (combo fired). Hold the ring - and the
    // WINDOW, which Rust now leaves up for exactly this case - long enough for
    // the launch to produce it. One-shot per hold; bounded.
    if (actionPending && !_slingHeld) {
      _slingHeld = true;
      window.setTimeout(() => { if (!_hudActive) hideGuideHud(false); }, SLING_HANDOVER_MS);
      return;
    }
  }

  if (_hudEl) _hudEl.classList.add("hidden", "collapsing");
  sweep(760, 280, 150);
  window.setTimeout(() => {
    _hudBusy = false;
    window.clearTimeout(_hudBusyGuard); // PROBLEM 175 — a real clear retires the deadline
    if (_hudActive) return;             // re-held mid-fade; the HUD keeps the window
    if (_hudEl) { _hudEl.innerHTML = ""; _hudEl.classList.remove("handoff", "landed", "collapsing"); }
    anchorGlow(_stageMode ? "hud" : "toast");

    if (_toasts.length === 0) {
      hideToastGlow();
      setToastLayerHidden(false);
      if (_stageMode) { _stageMode = false; setStageAnchor(false); }
      _slingStaged = false;
      if (_isOverlay) invoke("overlay_toasts_done").catch(() => {});
      return;
    }
    // A toast fired mid-hold is already staged inside this window — leave it
    // be, no fit, no move. Anything else gets one clean fit now the HUD is gone.
    setToastLayerHidden(false);

    /* PROBLEM 136 - the pill landed ON this toast. Leave it exactly there.
       `to` is measured as settledBox(el) in the STAGED layout, so the flight
       touches down precisely on the real pill - and then this handover used to
       un-stage and re-fit, which does two things at once: setStageAnchor(false)
       moves the stack from top:50%+239px to bottom:74px, and overlay_fit
       resizes and moves the WINDOW under it. Net effect for the owner: "it
       pauses in the middle before jumping to toast".
       Staying staged is safe now in a way it was NOT in 1.0.46: the window is
       already up, because PROBLEM 135 stopped Rust hiding it while an action is
       pending, so no fit is needed to show anything. overlay_toasts_done still
       hides the window when the stack empties and retire() clears the flags, so
       nothing is stranded. */
    if (_stageMode && _slingStaged) {
      anchorGlow("toast");
      return;
    }

    /* SLINGSHOT / PROBLEM 113, RE-CREATED BY 1.0.46 AND FIXED HERE.
       A slingshot stages the toast INSIDE the HUD's window (_stageMode = true),
       and that flag blocks every overlay_fit. For the WARP handover skipping the
       fit is correct, because the pills go on living in that window. For the
       slingshot it is wrong: hideGuideHud has already WAITED for the flight to
       land, and the HUD window is collapsing right now - so leaving the flag set
       means the toast's own window is never fitted, and overlay_fit is what
       SHOWS it. The owner saw exactly that on 1.0.46: "the toast is not visible
       anymore... everything just went away as soon as I left the space."
       Measured: 2 combos after a hold with ZERO overlay_fit lines between them,
       which is the identical signature PROBLEM 113 recorded.
       Always un-stage and always fit. */
    if (_stageMode) {
      _stageMode = false;
      setStageAnchor(false);
      anchorGlow("toast");
    }
    relayout();
  }, HUD_OUT_MS + 20);
}

// Rebuild on resize/DPI change so the screen clamp still holds.
window.addEventListener("resize", () => {
  // PROBLEM 112 — buildHud now has side effects on _rect, and rebuilding
  // mid-flight would re-place the chips under a flight in progress.
  if (_hudActive && _lastPayload && _flying === 0) {
    // A resize genuinely MOVES every chip, so the published geometry has to be
    // refreshed. Bumping the seq is what re-arms `publishHudChips`; it is
    // still one publish per rebuild, never one per frame.
    _hudShowSeq++;
    buildHud(_lastPayload);
  }
});

/* ---------------- theme: ONE setting drives dashboard AND overlay ------- */
export function applyTheme(dark: boolean): void {
  document.body.classList.toggle("nocturne", dark);
}

/**
 * PROBLEM 185 — which of the three looks the overlay is wearing.
 *
 * `applyTheme` above takes a BOOLEAN, and that is the whole bug: `dark_mode` is
 * true for warcry AND for starry, because both sit on the same nocturne base
 * and each re-tints on top of it. So the overlay could not tell them apart and
 * wore starry's night-sky palette in warcry — the owner, 2026-08-25: *"for the
 * warcry theme the guide hud and toasts colour was not matched, it's still
 * using the ones from starry night."*
 *
 * The dashboard has always keyed its warcry rules off `data-theme` on <body>
 * (`themes.css`: `body.nocturne[data-theme="warcry"]`). The overlay is a
 * SEPARATE window that never had the attribute set, so every warcry rule
 * missed it. Setting the same attribute here means the overlay and the
 * dashboard select on one identical mechanism instead of two that can drift.
 *
 * **PROBLEM 255 — and `"auto"` is why this function no longer decides for
 * itself.** The theme pill can store the literal string `"auto"` ("match
 * Windows"), and this function's old rule — *anything that is not
 * warcry/starry is earthy* — rendered it in daylight while the dashboard
 * beside it, which already resolved `"auto"` through `main.ts::resolveTheme`,
 * was in Starry night. Two windows of one app in two palettes is exactly what
 * "ONE setting drives everything" exists to prevent. The rule now comes from
 * `src/theme-resolve.ts`, a leaf module the DASHBOARD imports too, so there
 * is one rule rather than two that agree by hand.
 *
 * The RAW value is remembered, never the resolved one:
 * [`reapplyResolvedTheme`] re-runs the resolution when Windows' own light/dark
 * setting changes, and it can only do that if it still knows the user asked
 * for "auto" rather than for whatever "auto" happened to mean at seed time.
 */
export function applyThemeName(theme: string): void {
  _rawThemeName = theme;
  const t = resolveTheme(theme);
  document.body.dataset.theme = t;
  // The nocturne base, set from the SAME resolved value rather than waiting
  // for a separate `theme-changed` bool. For a named theme this is identical
  // to what `applyTheme(dark_mode)` already does — Rust guarantees
  // `dark_mode == (theme != "earthy")` — so nothing changes for the three
  // fixed palettes. For `"auto"` it is the only thing that can be right: an
  // OS flip emits no Rust event at all, so `dark_mode` is stale the moment
  // the user changes their Windows setting, and a window wearing the starry
  // palette on a light nocturne base is unreadable.
  document.body.classList.toggle("nocturne", t !== "earthy");
}

/** The last raw value handed to [`applyThemeName`] — possibly `"auto"`. */
let _rawThemeName = "earthy";

/**
 * Re-run the resolution against the CURRENT OS setting. Wired by `overlay.ts`
 * to `theme-resolve.ts`'s `onSystemThemeChange`, which since REVIEW FIXES
 * 2026-09-05 (H4) fires from Rust's `os-theme-changed` event rather than from
 * this webview's `prefers-color-scheme` — a webview reports the colour scheme
 * it was TOLD to prefer, which is why the dashboard (pinned to
 * `"theme": "Light"`) and this window used to resolve `"auto"` differently.
 *
 * A no-op for a fixed theme by construction rather than by an early return:
 * `resolveTheme("starry")` is `"starry"` whatever the OS says, so re-applying
 * writes the same two attributes it already had.
 */
export function reapplyResolvedTheme(): void {
  applyThemeName(_rawThemeName);
}

/** Sound ticks on/off. Exported so overlay.ts can seed it at startup. */
export function applySound(on: boolean): void {
  _soundOn = on;
}

/**
 * PROBLEM 174 — "Guide-to-toast motion" on/off, from the user's setting.
 *
 * Turning it OFF mid-hold cannot be allowed to strand the machinery it was
 * driving. `_stageMode` blocks every window fit and `_absorbed` pills do not
 * age, so a flight that is in the air when the switch flips would leave the
 * overlay unfittable and a pill frozen wearing SPACE's identity — the exact
 * shape of the bug this setting exists to avoid. So: clear the flags, put any
 * absorbed pill back into the ordinary stack, and let the next fit run.
 *
 * `_flying` is deliberately NOT cleared. It counts flights that are physically
 * mid-animation and will decrement themselves; zeroing it would let a fit run
 * underneath one.
 */
export function applyFlight(on: boolean): void {
  const was = WARP || SLING;
  WARP = on;
  SLING = on;
  if (was && !on) {
    _absorbed.forEach((t) => park(t, false));
    _absorbed = [];
    if (_stageMode) { _stageMode = false; setStageAnchor(false); }
    _slingStaged = false;
    _slingHeld = false;
    _slingUntil = 0;
    _hudBusy = false;
    window.clearTimeout(_hudBusyGuard);
    if (_toasts.length) relayout();
  }
}

/* ---------------- listeners — registered ONLY by overlay.ts ------------- */
export async function initToastListener(): Promise<void> {
  await listen<string>("toast-notification", (e) => {
    if (typeof e.payload === "string") showToast(e.payload);
  });
  // LIVE TOAST (1.0.123) — keyed, updated in place; same global-emit rule.
  await listen<{ key?: string; text?: string } | null>("toast-live", (e) => {
    const p = e.payload;
    if (p && typeof p.key === "string" && typeof p.text === "string") showLiveToast(p.key, p.text);
  });
  await listen<{ key?: string } | null>("toast-live-end", (e) => {
    const p = e.payload;
    if (p && typeof p.key === "string") endLiveToast(p.key);
  });
  await listen<GuideHudPayload>("guide-hud-show", (e) => {
    if (e.payload) showGuideHud(e.payload);
  });
  await listen<boolean>("guide-hud-hide", (e) =>
    hideGuideHud(e.payload === true));
  /* ARMED CHIP — which binding fires if Space is released now.
     Same shape as every listener above it, and for the same documented
     reason: global `emit` + ONE listener in the overlay page is the only
     arrangement that has ever delivered here; `emit_to` never has.
     `index` is into the OUTER (apps) ring, in the order Rust sent them in
     `GuideHudPayload.apps`. `null` (or a missing/garbage payload) disarms —
     never leave a stale highlight on screen, because the whole point of it is
     to tell the truth about what will happen next. */
  await listen<{ index: number | null } | null>("hud-pointer", (e) => {
    const idx = e.payload && typeof e.payload.index === "number" ? e.payload.index : null;
    setArmedChip(idx);
  });
  await listen<boolean>("theme-changed", (e) => applyTheme(!!e.payload));
  // PROBLEM 185 — the theme NAME, which is what tells warcry from starry.
  await listen<string>("theme-name-changed", (e) => applyThemeName(String(e.payload ?? "earthy")));
  await listen<boolean>("sound-changed", (e) => { _soundOn = !!e.payload; });
  // PROBLEM 174 — `=== true`, not `!!`: absent must read as OFF.
  await listen<boolean>("flight-changed", (e) => applyFlight(e.payload === true));
}
