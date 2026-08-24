/**
 * key-wake.ts — the keyboard reacts to a cursor SWEEP, not just to dwelling.
 *
 * WHAT THE OWNER ASKED FOR (2026-08-25):
 *
 *   *"Is it possible to have a bit more animation when the cursor is moved over
 *   the keys? Like if the cursor is moved from left to right, or up to down,
 *   the keys jump — I mean react to motion of the cursor a bit more… right now
 *   it does give animation but it needs a little bit of time to get focused.
 *   That animation is good. But when the cursor is moved in a very fast way
 *   over the keyboard it should show some motion and a reaction to it."*
 *
 * WHY IT LOOKS DEAD ON A FAST PASS, and why this is not a missing feature but
 * an arithmetic one. `styles.css` has:
 *
 *     .key        { transition: all 200ms var(--ease-out); }
 *     .key:hover  { transform: translateY(-4px); }
 *
 * Sweep the pointer across the board and each key is `:hover` for roughly
 * 20–40 ms. An ease-out over 200 ms covers well under a third of its travel in
 * that time, and then reverses. So the key lifts about a pixel and sinks back:
 * **the animation is correct and simply never gets to happen.** Nothing was
 * broken; dwelling was the only way to see it.
 *
 * Shortening the transition is the obvious fix and the wrong one — it would
 * make deliberate hover snappy and cheap, losing the thing he explicitly said
 * he likes ("that animation is good"). The two gestures want opposite
 * treatments: dwelling wants a soft settle, sweeping wants an instant, brief
 * kick. So they get separate channels.
 *
 * HOW THE TWO CHANNELS COMPOSE WITHOUT FIGHTING. `:hover` keeps `transform`
 * and its 200 ms transition, untouched. The wake uses the STANDALONE CSS
 * `translate` and `rotate` properties, which compose with `transform` rather
 * than replacing it — so neither channel can clobber the other, and no
 * variable plumbing or wrapper element is needed. `styles.css` has been
 * changed from `transition: all` to an explicit property list that OMITS
 * `translate` and `rotate`, so the wake lands on the frame it is written.
 * That omission is load-bearing: with `all`, the browser would ease every wake
 * write over 200 ms and reintroduce the exact lag being fixed.
 *
 * ONE COORDINATE SPACE, and this is the part the first version got wrong.
 * `wireKeyWake` is handed `#keyboard-scale`, which carries `transform: scale(s)`
 * — between 0.7 and 1.5 depending on the window. The first draft measured key
 * centres and pointer position in VIEWPORT pixels (post-scale) but wrote
 * `translate` in the element's LOCAL space (pre-scale, multiplied by every
 * ancestor scale on the way out). Radius and speed therefore lived in one space
 * and displacement in another, so the effect quietly changed character on every
 * display size — and the doc comment "in CSS px at scale 1" hid it, because it
 * is never at scale 1.
 *
 * Everything below is now in BOARD-LOCAL design pixels: the pointer is mapped
 * into that space once per frame, and every constant means the same thing on
 * every monitor. Displacement still scales with the board, which is correct and
 * matches `:hover`'s own `translateY(-4px)`.
 *
 * TIME, not frames. Attack, release, the velocity low-pass and the park timer
 * are all normalised against a 60 Hz reference using the rAF timestamp. Raw
 * per-frame lerp factors would make the whole effect stronger and shorter on a
 * 144 Hz panel and weaker and longer on a struggling one — i.e. worst exactly
 * where it can least afford to be.
 *
 * COST, because this runs on every frame the pointer moves and the app must
 * stay usable on a weak laptop:
 *   - Key rectangles are measured ONCE and re-measured only on resize or a
 *     board rescale. Per-frame `getBoundingClientRect()` over ~70 keys would be
 *     a layout thrash and is exactly what makes effects like this feel heavy.
 *   - The per-key test is a squared-distance compare; the square root is taken
 *     only for the handful of keys actually inside the radius.
 *   - The loop parks itself when everything has settled and the pointer has
 *     stopped, so an idle dashboard costs nothing.
 *   - It does not run under reduced motion, `lite-scene`, sky mode, or while
 *     the key editor has the board blurred out behind it.
 */

/** Radius of influence, in BOARD-LOCAL design px. */
const RADIUS = 132;
/** Pointer speed at which the impulse reaches full strength, board-local px per 16.67 ms. */
const SPEED_REF = 26;
/** Peak displacement along the direction of travel, board-local px. */
const PUSH = 5.5;
/** Peak lift. Negative is up. */
const LIFT = -7;
/** Peak bank, degrees. */
const TILT = 3.2;
/** Fraction of the remaining gap closed per 16.67 ms while rising. */
const ATTACK = 0.55;
/** Same, while returning. Lower = longer settle. */
const RELEASE = 0.14;
/** Velocity low-pass, per 16.67 ms. */
const SMOOTH = 0.35;
/** Below this the key is considered settled and its inline styles are cleared. */
const EPSILON = 0.06;
/** How far off the line of travel a key must be to bank fully, board-local px. */
const TILT_SPREAD = 22;
/** Park the loop after this long with no pointer movement. */
const PARK_AFTER_MS = 500;
/** One frame at the reference rate. */
const REF_FRAME_MS = 1000 / 60;

interface WakeKey {
  el: HTMLElement;
  /** Centre in BOARD-LOCAL design px. */
  cx: number;
  cy: number;
  /** Current animated amount, 0..1. */
  w: number;
  /** Latched direction of travel at the moment this key was caught. */
  dx: number;
  dy: number;
  /** Latched bank, -1..1, continuous through zero. */
  side: number;
  /** True while the element carries inline translate/rotate. */
  dirty: boolean;
}

let _keys: WakeKey[] = [];
let _board: HTMLElement | null = null;
let _running = false;
let _rafId = 0;
let _lastT = 0;

/** Board origin in viewport px, and its cumulative scale. */
let _originX = 0;
let _originY = 0;
let _scale = 1;

/** Pointer in viewport px; velocity in board-local px per reference frame. */
let _px = 0, _py = 0;
let _lastPx = 0, _lastPy = 0;
let _vx = 0, _vy = 0;
let _havePointer = false;
let _stillMs = 0;

function enabled(): boolean {
  const root = document.documentElement;
  if (root.classList.contains("reduced-motion")) return false;
  if (document.body.classList.contains("lite-scene")) return false;
  // Sky mode hides the board entirely; the key editor blurs it to 5px at 50%
  // opacity behind its own panel. In both the board is on screen and not being
  // looked at or pointed at, and animating a blurred layer is the most
  // expensive thing this module could possibly do for the least visible return.
  if (document.body.classList.contains("sky-mode")) return false;
  if (document.getElementById("stage")?.classList.contains("editing")) return false;
  return true;
}

/**
 * Re-measure every key. Call after anything that moves the board: a window
 * resize, the board's scale changing, a profile re-render that rebuilds cells.
 */
export function measureKeyWake(): void {
  if (!_board) return;

  // STRIP FIRST. `getBoundingClientRect()` returns the DISPLACED rect of a key
  // that currently carries an inline translate, so measuring mid-wake would
  // bake the live displacement into the cached centre — and the wake would then
  // be applied again on top of a centre that already contained it, making keys
  // drift further with every re-measure. Nothing flickers: these are rewritten
  // on the next frame.
  for (const k of _keys) if (k.dirty) { k.el.style.translate = ""; k.el.style.rotate = ""; }

  const br = _board.getBoundingClientRect();
  _originX = br.left;
  _originY = br.top;
  // offsetWidth is the UNSCALED layout width; the rect is the scaled one. Their
  // ratio is the cumulative scale of every ancestor, which is what maps a
  // viewport delta into board-local design px.
  _scale = _board.offsetWidth > 0 ? br.width / _board.offsetWidth : 1;
  if (!(_scale > 0.01)) _scale = 1;

  const cells = _board.querySelectorAll<HTMLElement>(".key");
  const next: WakeKey[] = [];
  cells.forEach((el) => {
    const r = el.getBoundingClientRect();
    if (r.width === 0 && r.height === 0) return;      // hidden board
    const prev = _keys.find((k) => k.el === el);
    next.push({
      el,
      cx: (r.left + r.width / 2 - _originX) / _scale,
      cy: (r.top + r.height / 2 - _originY) / _scale,
      // Carry the in-flight animation across a re-measure so a resize mid-sweep
      // does not visibly reset the board.
      w: prev?.w ?? 0,
      dx: prev?.dx ?? 0,
      dy: prev?.dy ?? 0,
      side: prev?.side ?? 0,
      dirty: false,
    });
  });
  // Any key that vanished between measures must not keep an inline transform.
  for (const old of _keys) {
    if (old.dirty && !next.some((k) => k.el === old.el)) clear(old);
  }
  _keys = next;
}

function clear(k: WakeKey): void {
  k.el.style.translate = "";
  k.el.style.rotate = "";
  k.w = 0;
  k.dirty = false;
}

function frame(now: number): void {
  _rafId = 0;
  if (!enabled()) { stop(); return; }

  // Normalise to a 60 Hz reference so the effect is identical on a 60, 120 or
  // 144 Hz panel, and on a laptop dropping frames. Clamped: a tab that was
  // backgrounded, or a park, can hand us a multi-second gap.
  const dt = _lastT ? Math.min(50, now - _lastT) : REF_FRAME_MS;
  _lastT = now;
  const f = dt / REF_FRAME_MS;
  const attack = 1 - Math.pow(1 - ATTACK, f);
  const release = 1 - Math.pow(1 - RELEASE, f);
  const smooth = 1 - Math.pow(1 - SMOOTH, f);

  // Velocity in BOARD-LOCAL px per reference frame, so SPEED_REF means the same
  // thing at every board scale.
  const rawVx = (_px - _lastPx) / _scale / f;
  const rawVy = (_py - _lastPy) / _scale / f;
  _lastPx = _px;
  _lastPy = _py;
  _vx += (rawVx - _vx) * smooth;
  _vy += (rawVy - _vy) * smooth;

  const speed = Math.hypot(_vx, _vy);
  _stillMs = speed < 0.4 ? _stillMs + dt : 0;

  const inv = speed > 0.001 ? 1 / speed : 0;
  const ux = _vx * inv;
  const uy = _vy * inv;
  const strength = Math.min(speed / SPEED_REF, 1);

  // Pointer in board-local design px — the same space as every key centre.
  const lx = (_px - _originX) / _scale;
  const ly = (_py - _originY) / _scale;

  const R2 = RADIUS * RADIUS;
  let busy = false;

  for (const k of _keys) {
    let target = 0;
    if (_havePointer && strength > 0.01) {
      const dx = k.cx - lx;
      const dy = k.cy - ly;
      const d2 = dx * dx + dy * dy;
      // Squared compare first; the square root is paid only for the handful of
      // keys actually inside the radius.
      if (d2 < R2) {
        const t = 1 - Math.sqrt(d2) / RADIUS;
        // Smoothstep: a linear falloff makes the edge of the radius a visible
        // ring as keys pop in and out of range.
        target = t * t * (3 - 2 * t) * strength;
      }
    }

    // Instant attack, slow release. This asymmetry IS the effect: the kick has
    // to land on the frame the pointer arrives, and the settle has to outlast
    // it or the board looks like it is twitching rather than reacting.
    const rate = target > k.w ? attack : release;
    k.w += (target - k.w) * rate;

    if (target > k.w * 0.5) {
      k.dx = ux;
      k.dy = uy;
      // Bank away from the line of travel — 2D cross product of the direction
      // and the offset to this key, so keys either side of the path tip
      // opposite ways and the sweep reads as a wake rather than a bulge.
      //
      // CLAMPED AND LATCHED, not `Math.sign()`. That quantity is exactly zero
      // on the line the pointer is travelling along and discontinuous through
      // it, so sub-pixel jitter flipped the sign — and `rotate` has no
      // transition, so the keys nearest the cursor snapped 2*TILT degrees in a
      // single frame, strobing at maximum amplitude precisely where the eye was
      // looking. Latching it beside dx/dy also stops a key re-deciding its bank
      // 300 ms into its release, long after the wake has gone.
      const cross = ux * (k.cy - ly) - uy * (k.cx - lx);
      k.side = Math.max(-1, Math.min(1, cross / TILT_SPREAD));
    }

    if (k.w < EPSILON) {
      if (k.dirty) clear(k);
      continue;
    }
    busy = true;
    k.dirty = true;

    const w = k.w;
    const tx = k.dx * PUSH * w;
    const ty = k.dy * PUSH * w + LIFT * w;
    k.el.style.translate = `${tx.toFixed(2)}px ${ty.toFixed(2)}px`;
    k.el.style.rotate = `${(k.side * TILT * w).toFixed(2)}deg`;
  }

  // Park when nothing is moving and nothing is still settling. An idle
  // dashboard must not hold a rAF loop open — this app sits in the tray all day
  // on a laptop.
  if (busy || _stillMs < PARK_AFTER_MS) {
    _rafId = requestAnimationFrame(frame);
  } else {
    _running = false;
    _lastT = 0;      // the next start must not inherit a stale timestamp
  }
}

function kick(): void {
  if (_running || !enabled()) return;
  _running = true;
  _lastT = 0;
  _rafId = requestAnimationFrame(frame);
}

function stop(): void {
  if (_rafId) cancelAnimationFrame(_rafId);
  _rafId = 0;
  _running = false;
  _lastT = 0;
  for (const k of _keys) if (k.dirty) clear(k);
}

/**
 * Start the wake for a keyboard container.
 *
 * Pointer tracking is on `window`, not on the board: a sweep that STARTS
 * outside the keyboard and crosses it is the exact gesture being animated, and
 * a listener on the board only learns about it once the pointer is inside.
 */
export function wireKeyWake(board: HTMLElement): void {
  _board = board;
  measureKeyWake();

  window.addEventListener(
    "pointermove",
    (e) => {
      _px = e.clientX;
      _py = e.clientY;
      if (!_havePointer) { _lastPx = _px; _lastPy = _py; _havePointer = true; }
      _stillMs = 0;
      kick();
    },
    { passive: true },
  );

  // A pointer that leaves the window stops producing moves, so without this the
  // last velocity would keep a key held up until the loop parks.
  //
  // `pointerout` with a null relatedTarget, not `pointerleave`: pointerleave
  // does not bubble, and on `window` it is unreliable for a pointer exiting the
  // client area entirely.
  window.addEventListener("pointerout", (e) => {
    if (e.relatedTarget === null) { _havePointer = false; kick(); }
  });
  window.addEventListener("blur", () => { _havePointer = false; kick(); });

  // Rects move whenever the board is re-laid-out or rescaled.
  const remeasure = () => measureKeyWake();
  window.addEventListener("resize", remeasure);
  // The board carries `transform: scale()`, and a transform does NOT trigger
  // ResizeObserver — it observes border-box layout size, which the scale leaves
  // unchanged. Observing the OUTER container is what actually catches a rescale,
  // since that is what `fit()` measures and reacts to.
  const ro = new ResizeObserver(remeasure);
  ro.observe(board);
  const outer = document.getElementById("keyboard-outer");
  if (outer) ro.observe(outer);
  // Cells are rebuilt on profile switches and binding edits. Coalesced to one
  // measure per frame: a profile re-render replaces every row, and a
  // synchronous re-measure per mutation record would be ~70 forced layouts.
  let pending = 0;
  const mo = new MutationObserver(() => {
    if (pending) return;
    pending = requestAnimationFrame(() => { pending = 0; measureKeyWake(); });
  });
  mo.observe(board, { childList: true, subtree: true });
}

/**
 * Turn the wake off and clean up, or turn it back on.
 *
 * Called when "Visual effects" changes: `enabled()` is only consulted inside
 * the loop, so a board left mid-wake when effects are switched off would keep
 * its inline transforms until something else happened to start the loop again.
 */
export function applyKeyWakeMotion(): void {
  if (enabled()) { measureKeyWake(); kick(); } else { stop(); }
}
