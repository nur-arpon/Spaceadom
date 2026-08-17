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
const WARP = false;

/* ---------------------------------------------------------------------------
   SLINGSHOT — the HUD -> toast arrival, and ONLY that direction.
   Owner's request 2026-08-17: he kept the ring's entrance (he likes the ripple)
   and asked for the handover INTO the toast to become a real, visible move. He
   also said explicitly: do NOT bring back 1.0.33's warp, implement this fresh.
   So this is gated on its own flag and does not turn WARP on: the toast->SPACE
   absorb and the return-on-release stay off, exactly as he left them.
   He is writing the opposite direction (toast -> HUD) himself.
   --------------------------------------------------------------------------- */
const SLING = true;
const SLING_MS = 940;      // door to door - slow ON PURPOSE, this is the set piece
const SLING_BOW = 150;     // px the arc swings out past the chord
const SLING_SAMPLES = 22;  // bezier keyframes; 22 is smooth, more buys nothing
const SLING_STRETCH = 1.9; // nose-first stretch at mid-flight (warp uses 2.5)
const SLING_MID = 0.5;     // fraction at which the box is smallest / trail longest
const SLING_T0 = 0.16;     // stretch ramps from here - AFTER the face fade (0.17)
/* Long tail - orbital capture. Fast off the chip, then a long deceleration into
   the slot. Do NOT reuse WARP_EASE: its late snap fights the arc. */
const CAPTURE_EASE = "cubic-bezier(.3,0,.08,1)";
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

const WARP_MS = 560;      // one flight, door to door
const STAGGER = 95;       // between pills when several fly at once
const HUD_OUT_MS = 220;   // MUST match #st-hud's transition in overlay-earthy.css
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

/* ---- PROBLEM 112 state ---- */
type Rect = { x: number; y: number; w: number; h: number };
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

function hideToastGlow(): void {
  const g = document.getElementById("st-toastglow");
  if (g) g.style.opacity = "0";
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
  // where the ring clamp shrinks the window too.
  const y = Math.min(250, Math.max(120, window.innerHeight / 2 - 44));
  for (const el of [c, g]) {
    if (!(el instanceof HTMLElement)) continue;
    const isGlow = el.id === "st-toastglow";
    if (on) {
      el.style.bottom = "auto";
      el.style.top = "50%";
      el.style.transform = `translate(-50%, ${isGlow ? y + 46 : y}px)`;
    } else {
      el.style.top = "";
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
      animation: st-toast-glow 4s ease-in-out infinite;
      opacity: 0; transition: opacity .3s;`;
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
    const depth = _toasts.slice(i + 1).filter((x) => x.phase === "open").length;
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
    if (_isOverlay && !_hudActive) invoke("overlay_toasts_done").catch(() => {});
  }
}

/** Timestamp of the last window fit, for the coalescing below. */
let _lastFitAt = 0;
let _fitTimer: number | undefined;

function fitToStack(): void {
  // Dashboard toasts must not touch the overlay window (see _isOverlay).
  if (!_isOverlay) return;
  // Never resize while the HUD is up OR still fading: a toast arriving then
  // used to shrink the window mid-fade — a violent visual jump.
  if (_hudActive || _hudBusy || _stageMode || _flying > 0) return;
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
  if (_hudActive || _hudBusy || _stageMode || _flying > 0) return;
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
  if (_hudActive) setToastLayerHidden(true);

  const glow = document.getElementById("st-toastglow");
  if (glow) glow.style.opacity = "1";

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
      tearOut(c);

      const D = flightSling({
        from, to, chipHtml, toastHtml: el.innerHTML, cell: c.cell,
        onArrive: () => { park(entry, false); beep(640); },
      });
      armEntry(entry, D + duration, D + duration + LEAVE_MS);
      return;
    }
    // No chip on the ring (volume, clipboard, an unlisted app): fall through to
    // the plain path below. Deliberately NOT the SPACE ejection the source
    // patch specifies - that is a WARP flight, and WARP stays off.
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
    anchorGlow("toast");
  }

  const open = () => { entry.phase = "open"; el.classList.add("open"); beep(640); relayout(); };
  if (REDUCED()) open(); else window.setTimeout(open, OPEN_AT);
  armEntry(entry, LEAVE_AT, DIE_AT);
  relayout();
}

/* =======================================================================
   GUIDE HUD — radial bloom, centred on screen, viewport-aware
   ======================================================================= */
interface GuideHudPayload {
  profile: string;
  apps: [string, string][];      // [key, label] — ONLY assigned letters
  specials: [string, string][];
}
let _hudEl: HTMLDivElement | null = null;
let _lastPayload: GuideHudPayload | null = null;

/** Estimated chip width — used ONLY to pick the ring radii before the real
 *  chips exist. Placement itself uses MEASURED widths (PROBLEM 77). */
function estW(label: string, special: boolean): number {
  return Math.min(label.length * 6.8, 118) + (special ? 64 : 56);
}

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
  const N = 720;
  // cumulative arc length of the ellipse, sampled at N points
  const cum = new Array<number>(N + 1);
  cum[0] = 0;
  for (let i = 1; i <= N; i++) {
    const t = (i / N) * Math.PI * 2;
    const dx = rx * (Math.cos(t) - Math.cos(((i - 1) / N) * Math.PI * 2));
    const dy = ry * (Math.sin(t) - Math.sin(((i - 1) / N) * Math.PI * 2));
    cum[i] = cum[i - 1] + Math.hypot(dx, dy);
  }
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

function buildHud(payload: GuideHudPayload, entranceDelay = 0): Promise<Rect | null> {
  if (!_hudEl) return Promise.resolve(null);
  const specials = payload.specials ?? [];
  const apps = payload.apps ?? [];   // already only assigned letters

  // NO glow element here — a large blurred surface makes this transparent
  // window compose zero pixels. See the removal note at the top of
  // overlay-earthy.css before adding one back.
  _hudEl.innerHTML = '<div class="pulse"></div><div class="space">SPACE</div>';
  if (entranceDelay > 0 && !REDUCED()) {
    const pulseEl = _hudEl.querySelector(".pulse") as HTMLElement | null;
    if (pulseEl) pulseEl.style.animationDelay = `${entranceDelay}ms`;
  }

  // PROBLEM 77 — build ALL chips first (unpositioned), measure their REAL
  // rendered widths, and only then compute the ring geometry and angles.
  // Estimates remain solely as a fallback for a zero measurement.
  const make = (a: [string, string], special: boolean): HTMLDivElement => {
    const c = document.createElement("div");
    c.className = "st-chip " + (special ? "sp" : "ap");
    const inner = document.createElement("i");
    const kbd = document.createElement("kbd");
    kbd.textContent = a[0];
    const span = document.createElement("span");
    span.textContent = a[1];
    inner.append(kbd, span);
    c.appendChild(inner);
    // SLINGSHOT - the flight has to find the launched app's chip. The label is
    // the only thing a toast ("Brave launched") and a chip ("Brave") share.
    c.dataset.stApp = a[1].toLowerCase();
    _hudEl!.appendChild(c);
    return c;
  };
  const spChips = specials.map((a) => make(a, true));
  const apChips = apps.map((a) => make(a, false));
  const width = (c: HTMLDivElement, a: [string, string], special: boolean) => {
    const w = (c.firstElementChild as HTMLElement | null)?.offsetWidth ?? 0;
    return w > 0 ? w : estW(a[1], special);
  };
  const spW = spChips.map((c, i) => width(c, specials[i], true));
  const apW = apChips.map((c, i) => width(c, apps[i], false));

  const innerHalf = spW.length ? Math.max(...spW) / 2 : 0;
  const outerHalf = apW.length ? Math.max(...apW) / 2 : 0;

  let rin = 115 + innerHalf + 26;                 // 115 clears the SPACE pill
  let rout = rin + innerHalf + outerHalf + 26;
  let ryi = 118, ryo = 196;                       // ellipses, not circles

  // Clamp to the SCREEN (not the window — the window is sized FROM these).
  // This is what fits all 26 chips on any display/aspect.
  const vw = window.screen.availWidth || window.innerWidth;
  const vh = window.screen.availHeight || window.innerHeight;
  const scale = Math.min(1, (vw / 2 - outerHalf - 24) / rout, (vh / 2 - 40) / ryo);
  rin *= scale; rout *= scale; ryi *= scale; ryo *= scale;

  const put = (chips: HTMLDivElement[], rx: number, ry: number,
               delay0: number, as: number[]) =>
    chips.forEach((c, i) => {
      const x = Math.cos(as[i]) * rx, y = Math.sin(as[i]) * ry;
      c.style.left = `calc(50% + ${Math.round(x)}px)`;
      c.style.top = `calc(50% + ${Math.round(y)}px)`;
      const inner = c.firstElementChild as HTMLElement;
      // --fx/--fy = vector from final position BACK to centre → blooms outward
      inner.style.cssText +=
        `;--fx:${Math.round(-x)}px;--fy:${Math.round(-y)}px;` +
        (REDUCED() ? "animation:none;" : `animation-delay:${entranceDelay + delay0 + i * 26}ms;`);
    });

  // 14px clearance between neighbours on the rim; outer ring half-step so its
  // chips sit in the inner ring's gaps.
  put(spChips, rin, ryi, 120, arcAngles(spW, rin, ryi, 14, 0));
  put(apChips, rout, ryo, 300,
      arcAngles(apW, rout, ryo, 14, apps.length ? Math.PI / apps.length : 0));

  // Size the window to the bloom box + PAD. PAD clears the 340px ring-pulse
  // and its glow — without it the circle looks "cut off at the back".
  // NOTE: deliberately NOT the full work area; a fullscreen transparent
  // window composes ZERO pixels on this machine.
  const PAD = 180;
  const w = Math.round(Math.max(2 * (rout + outerHalf) + PAD, 360 + PAD));
  const h = Math.round(Math.max(2 * ryo + PAD, 360 + PAD));
  // PROBLEM 112 — hand the promise back. The window MOVE is the one thing here
  // that cannot be animated, so callers must be able to wait for it and keep
  // content invisible until it has landed.
  return invoke<Rect | null>("overlay_fit_hud", { width: w, height: h })
    .then((r) => { if (r) _rect = r; return r ?? null; })
    .catch(() => null);
}

interface FlightGeo { x: number; y: number; w: number; h: number; s?: number }

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

function showGuideHud(payload: GuideHudPayload): void {
  _slingHeld = false;   // SLINGSHOT - a fresh hold gets a fresh handover grace
  _lastPayload = payload;
  _hudActive = true;
  if (!_hudEl) {
    _hudEl = document.createElement("div");
    _hudEl.id = "st-hud";
    _hudEl.setAttribute("aria-label", "Space modifier guide");
    document.body.appendChild(_hudEl);
  }
  _hudEl.classList.remove("landed", "space-gone");
  anchorGlow("hud");
  const g = document.getElementById("st-toastglow");
  if (g) g.style.opacity = "1";

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

  const total = WARP_MS + STAGGER * (src.length - 1);
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
      flightWarp({
        from: {
          x: sItem.sx - (now.x + host.left),
          y: sItem.sy - (now.y + host.top),
          w: sItem.w, h: sItem.h, s: sItem.s,
        },
        via: { w: settled.w, h: settled.h, s: settled.s },
        to,
        html: sItem.html,
        toSpace: true,
        delay: STAGGER * i,
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

  /* ---- PROBLEM 112: the pills come back out of SPACE ---- */
  if (WARP && back.length > 0 && _isOverlay && !REDUCED() && _hudEl) {
    const from = spaceBox();            // BEFORE .hidden scales the HUD to .93
    _hudEl.classList.remove("landed");  // SPACE leaves as the pills
    _hudEl.classList.add("hidden");     // ring + chips collapse
    sweep(760, 280, 150);

    // No window move: the stack lands inside the HUD's own window.
    _stageMode = true;
    setStageAnchor(true);
    setToastLayerHidden(false);
    back.forEach((t) => {
      t.phase = "open";
      t.el.classList.remove("leave");
      t.el.classList.add("open");
      park(t, true);
    });
    relayout();

    back.forEach((t, i) => {
      const to = settledBox(t.el);      // its exact slot and width
      flightWarp({
        from, to, html: t.el.innerHTML, toSpace: false, delay: STAGGER * i,
        onArrive: () => { park(t, false); thawEntry(t); },
      });
    });

    window.setTimeout(() => {
      _hudBusy = false;
      if (_hudActive) return;           // re-held mid-flight
      if (_hudEl) { _hudEl.innerHTML = ""; _hudEl.classList.remove("handoff"); }

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
    }, HUD_OUT_MS + WARP_MS + STAGGER * back.length);
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
  if (SLING && !REDUCED()) {
    const flightLeft = Math.max(0, _slingUntil - performance.now());
    if (flightLeft > 0) {
      // A flight is in the air: fold the ring away UNDER it (pure visuals -
      // the flight lives in #st-flight, not #st-hud) and defer the real
      // teardown, whose refit would yank the window mid-flight, to landing.
      if (_hudEl) _hudEl.classList.add("hidden");
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

  if (_hudEl) _hudEl.classList.add("hidden");
  sweep(760, 280, 150);
  window.setTimeout(() => {
    _hudBusy = false;
    if (_hudActive) return;             // re-held mid-fade; the HUD keeps the window
    if (_hudEl) { _hudEl.innerHTML = ""; _hudEl.classList.remove("handoff", "landed"); }
    anchorGlow(_stageMode ? "hud" : "toast");

    if (_toasts.length === 0) {
      hideToastGlow();
      setToastLayerHidden(false);
      if (_stageMode) { _stageMode = false; setStageAnchor(false); }
      if (_isOverlay) invoke("overlay_toasts_done").catch(() => {});
      return;
    }
    // A toast fired mid-hold is already staged inside this window — leave it
    // be, no fit, no move. Anything else gets one clean fit now the HUD is gone.
    setToastLayerHidden(false);
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
  if (_hudActive && _lastPayload && _flying === 0) buildHud(_lastPayload);
});

/* ---------------- theme: ONE setting drives dashboard AND overlay ------- */
export function applyTheme(dark: boolean): void {
  document.body.classList.toggle("nocturne", dark);
}

/** Sound ticks on/off. Exported so overlay.ts can seed it at startup. */
export function applySound(on: boolean): void {
  _soundOn = on;
}

/* ---------------- listeners — registered ONLY by overlay.ts ------------- */
export async function initToastListener(): Promise<void> {
  await listen<string>("toast-notification", (e) => {
    if (typeof e.payload === "string") showToast(e.payload);
  });
  await listen<GuideHudPayload>("guide-hud-show", (e) => {
    if (e.payload) showGuideHud(e.payload);
  });
  await listen<boolean>("guide-hud-hide", (e) =>
    hideGuideHud(e.payload === true));
  await listen<boolean>("theme-changed", (e) => applyTheme(!!e.payload));
  await listen<boolean>("sound-changed", (e) => { _soundOn = !!e.payload; });
}
