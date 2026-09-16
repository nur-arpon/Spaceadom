/**
 * middle-ring.ts — PROBLEM 267: the middle button's CURSOR-ANCHORED ICON RING,
 * the overlay page's half.
 *
 * A second HUD kind, next to the Space ring in `toast.ts`, and deliberately
 * NOT built out of it: this ring has its own DOM (`#st-mring`), its own
 * stylesheet (`styles/middle-ring.css`) and none of the Space HUD's bands,
 * bloom or beam. What it shares is the WINDOW and the event plumbing — global
 * `emit` from Rust, one listener per event registered here — and the one
 * handshake `initMiddleRing` is given so the toast stack knows the window is
 * spoken for (`beginExternalHud` / `endExternalHud` in toast.ts).
 *
 * THE PAGE NEEDS NO IPC WHILE THE RING IS UP. Rust lays the ring out
 * (`middle_ring::layout_ring_slots`), sizes and places the window
 * (`overlay_fit_ring`), publishes the hit table for the pointer poller and
 * sends ONE payload that already carries every icon as a `data:` URL. This
 * file draws that payload once, highlights whatever `hud-pointer` says is
 * armed, and plays the exit. The single command it ever sends is
 * `overlay_toasts_done`, through the handshake, when the exit has finished
 * and no toast wants the window — that is the terminal hide for a plain
 * release (Rust does NOT hide on that path; see `hide_guide_hud_pending`).
 *
 * The geometry is the design handoff's (`design_handoff_middle_ring/
 * README.md` + the artboards), transcribed, not re-derived: r=125, 70 px
 * tiles, 20 px badge, 130 px pill, 600/640 px scrim, 2r+14 guides. THE
 * MOTION IS RIPPLE ONLY (round 3, owner decision 2026-09-13; Bloom is gone):
 * the Space ring's own language plus the cursor-following fisheye wave. Fun
 * mode off = flat. `reduced` = final states only.
 *
 * THE WINDOW IS ONE BIG CANVAS (round 3): Rust sizes the overlay to the
 * whole work area of the cursor's monitor and the payload's `cx`/`cy` is the
 * press point inside it, so the ring, its scrim and the centre pill never
 * meet a window edge (the round-2 "boxy line" was the scrim reaching the
 * edge of a ring-sized window).
 *
 * LEAF MODULE: imports nothing from main.ts or toast.ts, so `preview.ts` can
 * render both scopes of the ring with a stub payload (`renderMiddleRing`).
 */
import { listen } from "@tauri-apps/api/event";

export interface RingItem {
  /** Badge text — the letter upper-cased, or the special's key ("Esc"). */
  key: string;
  /** The char Rust's `take_armed_key` returns for the tile; unused here. */
  code: string;
  name: string;
  /** A complete `data:` URL, or null → the letter disc. */
  icon: string | null;
  kind: "app" | "folder" | "link" | "special";
  ring: number;
  /** Compass degrees, clockwise from north. */
  angle_deg: number;
  radius: number;
  tile: number;
  /** Angular step to the neighbours on the same arc (Rust `RingSlot::pitch_deg`);
   *  a 1.0.110 payload lacks it and the wave falls back to 360 / count. */
  pitch_deg?: number;
  /** The pill's second line — the profile / account part of the binding's
   *  display name (Rust `split_display_name`), or null for one centred line. */
  account?: string | null;
}

export interface MiddleRingPayload {
  items: RingItem[];
  /** `my_eight` = Favourites (the wire name is unchanged) | `all`. */
  scope: "my_eight" | "all";
  /** Ring centre in the canvas window's CSS px — the press point. */
  cx: number;
  cy: number;
  /** ROUND 6: the monitor scale Rust divided every css number here by. The
   *  page rescales by `scale / devicePixelRatio` before drawing — WebView2's
   *  ratio can lag a monitor change (measured 1.12x on a 1.0 monitor beside a
   *  1.5 panel), and an uncorrected ring was drawn 12 % too large, past the
   *  room. Absent (an old payload, the preview stub) = no correction. */
  scale?: number;
  scrim: number;
  guides: number[];
  /** ROUND 6: the guides as ARCS — one per entry of `guides`, in the same
   *  order: a circle of diameter `d` of which only the compass span `lo..hi`
   *  is stroked (`0..360` = the whole circle). Every point of every arc is
   *  on-screen by construction (Rust `guide_arcs`). Absent (an old payload,
   *  the preview's stub) = full circles. */
  guide_arcs?: { d: number; lo: number; hi: number }[];
  /** ROUND 6: the room — the work area less any auto-hidden taskbar band —
   *  in this page's CSS px. The scrim is clipped to it so its fade ends at
   *  the screen edge instead of being cut by the window's. */
  room?: { x: number; y: number; w: number; h: number } | null;
  fun: boolean;
  reduced: boolean;
  /** How large the centre pill may grow for a long name (the free circle
   *  inside the innermost tiles, Rust `pill_max_diameter`); rests at 130. */
  pill_max?: number;
  /** `circle` | `half-N…` | `quarter-NE…` | `circle-clamped` — the shape Rust
   *  chose at the press point (owner decision 2026-09-13); `data-shape`. */
  shape?: string;
}

/** How long the pill's old name takes to yield to the new one. */
const NAME_XFADE_MS = 120;

/* ---- THE CENTRE PILL (round 3): two lines, readable. Line 1 = the app's
        short name, LARGER; line 2 = the profile / account, smaller beneath.
        The pill rests at 130 px and may grow to the payload's `pill_max`
        (the free inner circle); the text shrinks to fit but never below the
        floors; past that, ellipsis. All CSS px, so the same at every scale. */
const PILL_REST = 130;
/** Horizontal padding inside the pill (both sides together). */
const PILL_PAD = 24;
const APP_PX = 19;
const APP_MIN_PX = 15;
const ACCT_PX = 13;
const ACCT_MIN_PX = 12;

/* ---- RIPPLE: the SPACE RING's motion language, transcribed from
        overlay-earthy.css / toast.ts — NOT invented here. Each number names
        the rule it came from. ------------------------------------------ */
/** `#st-hud` entrance: HUD_IN_MS, cubic-bezier(.2,.8,.2,1). */
const R_SHELL_IN_MS = 220;
/** `#st-hud.hidden` exit: HUD_OUT_MS = 65 % of the entrance, ease-in. */
const R_SHELL_OUT_MS = 143;
/** `.st-chip > i { animation: st-bloom-in 620ms cubic-bezier(.34,1.3,.4,1) }`. */
const R_TILE_IN_MS = 620;
/** toast.ts `put(spChips, …, 120)`: the earliest chips start 120 ms in. */
const R_TILE_DELAY0_MS = 120;
/** toast.ts `STAG_SPAN` / `stagger(n)`: `min(26, 340 / (n − 1))` per chip. */
const R_STAG_SPAN_MS = 340;
const R_STAG_MAX_MS = 26;
/** `.st-space-pop 560ms cubic-bezier(.34,1.3,.4,1)` — the centre pill's arrival. */
const R_PILL_IN_MS = 560;
/** `#st-hud.collapsing .st-chip`: 110 ms to scale .72 + fade. */
const R_TILE_OUT_MS = 110;
/** toast.ts BLOOM_PUSH_ARMED / BLOOM_PUSH_NEIGHBOUR — the "aim boost". */
const R_PUSH_ARMED = 40;
const R_PUSH_NEIGHBOUR = 24;
/** The page-side lerp between ≤ 60 Hz aim samples (the brief's ≈ 120 ms). */
const R_LERP_TAU_MS = 120;
/** The fisheye wave's key scales by angular distance in SLOTS: the tile
 *  under the cursor's bearing, its two neighbours, the next pair, the rest.
 *  Between keys the value follows a raised cosine, so the swell is
 *  continuous in the bearing and flows round the ring with no steps. */
const WAVE_KEYS = [1.35, 1.15, 1.0, 0.85] as const;

/* ---- THE ICON RING'S OWN EASING (owner, 2026-09-15) ----------------------
   The Space ring's shell uses `cubic-bezier(.2,.8,.2,1)` in and
   `(.4,0,1,1)` out — hand-tuned numbers that are NOT touched here. This ring
   gets curves built from the same φ its radii and capacities are: 1/φ³ =
   .236, 1/φ² = .382, 1/φ = .618. Both are monotone in x and y, so neither
   overshoots; `--mr-ease-out`/`--mr-ease-in` are written on `.st-mring`
   itself and the stylesheet falls back to the old values if they are absent
   (the preview harness renders without them). */
const PHI_EASE_OUT = "cubic-bezier(.236, .618, .618, 1)";
const PHI_EASE_IN = "cubic-bezier(.618, 0, .764, .382)";

let _el: HTMLDivElement | null = null;
let _payload: MiddleRingPayload | null = null;
let _armed: number | null = null;
let _active = false;
let _exitTimer: number | undefined;
let _seq = 0;

/** Per-chip entrance stagger, toast.ts's `stagger(n)` verbatim. Still the
 *  UNIT the Fibonacci delay below is measured in — only the multiplier
 *  changed from the tile's index to its Fibonacci step. */
export function rippleStagger(n: number): number {
  return n > 1 ? Math.min(R_STAG_MAX_MS, R_STAG_SPAN_MS / (n - 1)) : R_STAG_MAX_MS;
}

/* ---- THE ENTRANCE ORDER (owner, 2026-09-15): Fibonacci, not linear. -------
   `delay(i) = base + stagger × fib(i)` reads as accelerating rather than
   metronomic, because the gaps between successive tiles are themselves the
   sequence. It cannot run away: the multiplier CYCLES through the first
   `FIB_STEPS` terms, so tile 8 starts again from the beginning rather than
   waiting 21 steps, and the whole entrance still finishes inside the 340 ms
   `R_STAG_SPAN_MS` budget the Space ring set (worst case: 13 × 26 = 338 ms
   past the 120 ms head start). Icon ring only — the Space ring's own
   stagger is tuned and is not touched. */
const FIB_STEPS = 7;
/** 1, 1, 2, 3, 5, 8, 13 — the same sequence `RING_CAPS` is drawn from. */
const FIB: readonly number[] = (() => {
  const f = [1, 1];
  while (f.length < FIB_STEPS) f.push(f[f.length - 1] + f[f.length - 2]);
  return f;
})();

/** The Fibonacci multiplier for tile `i`, cycling through the first seven
 *  terms. Exported for the preview's DOM checks and for tests. */
export function fibStep(i: number): number {
  return FIB[((i % FIB_STEPS) + FIB_STEPS) % FIB_STEPS];
}

/** The entrance delay for tile `i`, ms. */
export function tileDelay(i: number, stagger: number): number {
  return Math.round(R_TILE_DELAY0_MS + stagger * fibStep(i));
}

/** Compass-degree distance folded into [0, 180] (`middle_ring::compass_dist`). */
export function compassDist(a: number, b: number): number {
  const d = ((a - b) % 360 + 360) % 360;
  return d > 180 ? 360 - d : d;
}

/**
 * The fisheye wave: the scale for a tile `slots` slots away from the
 * cursor's bearing (fractional — `compassDist / pitch`). Piecewise raised-
 * cosine interpolation through WAVE_KEYS: 0 → 1.35, 1 → 1.15, 2 → 1.0,
 * ≥ 3 → .85, continuous and monotone in between, so as the bearing sweeps
 * the magnification travels round the ring like a wave.
 */
export function waveScale(slots: number): number {
  const d = Math.max(0, slots);
  const i = Math.floor(d);
  if (i >= WAVE_KEYS.length - 1) return WAVE_KEYS[WAVE_KEYS.length - 1];
  const t = d - i;
  const w = (1 - Math.cos(Math.PI * t)) / 2;
  return WAVE_KEYS[i] + (WAVE_KEYS[i + 1] - WAVE_KEYS[i]) * w;
}

/** The ratio the page must apply to every css number Rust sent: the scale
 *  Rust assumed over the ratio this page actually has. 1 when either is
 *  unknown or they agree (the single-panel case, always). */
export function pageRescale(assumed: number | undefined, actual: number): number {
  if (!assumed || !actual || !isFinite(assumed) || !isFinite(actual)) return 1;
  const k = assumed / actual;
  return Math.abs(k - 1) < 0.005 ? 1 : k;
}

/** The payload with every length and position multiplied by `pageRescale`
 *  — a NEW object; the caller's payload is never mutated. Angles, counts and
 *  names are untouched. */
export function rescaleForThisPage(p: MiddleRingPayload): MiddleRingPayload {
  const k = pageRescale(p.scale, typeof window !== "undefined" ? window.devicePixelRatio : 1);
  if (k === 1) return p;
  return {
    ...p,
    cx: p.cx * k,
    cy: p.cy * k,
    scrim: p.scrim * k,
    guides: p.guides.map((d) => d * k),
    guide_arcs: p.guide_arcs?.map((g) => ({ d: g.d * k, lo: g.lo, hi: g.hi })),
    room: p.room ? { x: p.room.x * k, y: p.room.y * k, w: p.room.w * k, h: p.room.h * k } : p.room,
    pill_max: p.pill_max === undefined ? undefined : p.pill_max * k,
    items: p.items.map((it) => ({ ...it, radius: it.radius * k, tile: it.tile * k })),
  };
}

/** A point on a circle of radius `r` at compass degrees `a` (0 = up,
 *  clockwise), in the SVG's own frame whose origin is the circle's centre. */
function polar(r: number, a: number): { x: number; y: number } {
  const t = (a * Math.PI) / 180;
  return { x: r * Math.sin(t), y: -r * Math.cos(t) };
}

/** The `d` attribute of a dashed guide: the whole circle for `0..360`,
 *  else one arc from `lo` to `hi` compass degrees (`hi` may exceed 360). */
export function guidePath(d: number, lo: number, hi: number): string {
  const r = d / 2;
  const span = hi - lo;
  if (span >= 360 - 1e-6) {
    return `M ${-r} 0 A ${r} ${r} 0 1 1 ${r} 0 A ${r} ${r} 0 1 1 ${-r} 0`;
  }
  const a = polar(r, lo);
  const b = polar(r, hi);
  const large = span > 180 ? 1 : 0;
  return `M ${a.x.toFixed(2)} ${a.y.toFixed(2)} A ${r} ${r} 0 ${large} 1 ${b.x.toFixed(2)} ${b.y.toFixed(2)}`;
}

/** One guide as an element: an SVG box of the guide's diameter centred on
 *  the anchor like every other child of `.st-mring`, a single dashed path
 *  inside it. `data-lo`/`data-hi` carry the span for the preview's DOM
 *  checks. */
function guideElement(d: number, lo: number, hi: number, i: number): SVGSVGElement {
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("class", "mr-guide");
  svg.dataset.ring = String(i);
  svg.dataset.lo = lo.toFixed(1);
  svg.dataset.hi = hi.toFixed(1);
  // +2 px so a 1 px stroke on the rim is never clipped by its own box.
  const box = d + 2;
  svg.setAttribute("width", `${box}`);
  svg.setAttribute("height", `${box}`);
  svg.setAttribute("viewBox", `${-box / 2} ${-box / 2} ${box} ${box}`);
  svg.style.width = `${box}px`;
  svg.style.height = `${box}px`;
  const path = document.createElementNS(NS, "path");
  path.setAttribute("d", guidePath(d, lo, hi));
  svg.appendChild(path);
  return svg;
}

/** The scrim's `clip-path: inset(...)` so it ends at the ROOM's edges (the
 *  screen, less any auto-hidden taskbar band) — the scrim is a square of
 *  edge `scrim` centred on `(cx, cy)`; each inset is how far that square
 *  reaches past the room on that side, never negative. `null` when the
 *  room is unknown or the square is wholly inside it. */
export function scrimClip(
  cx: number,
  cy: number,
  scrim: number,
  room: { x: number; y: number; w: number; h: number } | null,
): string | null {
  if (!room) return null;
  const h = scrim / 2;
  const top = Math.max(0, room.y - (cy - h));
  const left = Math.max(0, room.x - (cx - h));
  const bottom = Math.max(0, cy + h - (room.y + room.h));
  const right = Math.max(0, cx + h - (room.x + room.w));
  if (top === 0 && left === 0 && bottom === 0 && right === 0) return null;
  return `inset(${top.toFixed(1)}px ${right.toFixed(1)}px ${bottom.toFixed(1)}px ${left.toFixed(1)}px)`;
}

/** The tile's centre offset from the ring centre, screen axes (y down). */
export function tileOffset(angleDeg: number, radius: number): { dx: number; dy: number } {
  const a = (angleDeg * Math.PI) / 180;
  return { dx: radius * Math.sin(a), dy: -radius * Math.cos(a) };
}

/**
 * Draw the ring into `host` from a payload. PURE with respect to the app:
 * touches only the DOM it creates, so the preview harness can call it with a
 * stub. Returns the ring element. `armed` pre-lights one tile (the preview
 * shows the hovered state the artboards show).
 */
export function renderMiddleRing(
  host: HTMLElement,
  payloadIn: MiddleRingPayload,
  armed: number | null = null,
): HTMLDivElement {
  const payload = rescaleForThisPage(payloadIn);
  const ring = document.createElement("div");
  ring.className = "st-mring";
  ring.classList.toggle("flat", !payload.fun);
  ring.classList.toggle("fun", payload.fun);
  ring.classList.toggle("reduced", payload.reduced);
  ring.classList.toggle("all", payload.scope === "all");
  // `st-mring` carries every motion rule itself (ripple is the only motion
  // since round 3). NOT a bare `.ripple` class: styles.css (loaded by
  // overlay.html too) owns `.ripple` — the dashboard's press ripple — and it
  // matched this element once (a 130 px accent circle and a 520 ms animation
  // on the shell instead of the 220 ms transition; measured in the preview).
  ring.dataset.shape = payload.shape ?? "circle";
  ring.style.setProperty("--mr-cx", `${payload.cx}px`);
  ring.style.setProperty("--mr-cy", `${payload.cy}px`);
  ring.style.setProperty("--mr-enter", `${R_SHELL_IN_MS}ms`);
  ring.style.setProperty("--mr-exit", `${R_SHELL_OUT_MS}ms`);
  ring.style.setProperty("--mr-tile-in", `${R_TILE_IN_MS}ms`);
  ring.style.setProperty("--mr-tile-out", `${R_TILE_OUT_MS}ms`);
  ring.style.setProperty("--mr-pill-in", `${R_PILL_IN_MS}ms`);
  ring.style.setProperty("--mr-hover", `${NAME_XFADE_MS}ms`);
  // THE RING'S OWN EASING, derived from φ (owner, 2026-09-15). The Space
  // ring's curves are hand-tuned and stay where they are; this one's control
  // points are 1/φ³, 1/φ and 1/φ² — .236 / .618 / .382 — so the shell's
  // arrival and departure are built from the same constant as its radii.
  ring.style.setProperty("--mr-ease-out", PHI_EASE_OUT);
  ring.style.setProperty("--mr-ease-in", PHI_EASE_IN);
  const stagger = rippleStagger(payload.items.length);

  // The scrim — theme bg → transparent, 600/640 px. Only with fun on: the
  // flat artboard (7) has none.
  if (payload.fun) {
    const scrim = document.createElement("div");
    scrim.className = "mr-scrim";
    scrim.style.width = `${payload.scrim}px`;
    scrim.style.height = `${payload.scrim}px`;
    const clip = scrimClip(payload.cx, payload.cy, payload.scrim, payload.room ?? null);
    if (clip) scrim.style.clipPath = clip;
    ring.appendChild(scrim);
  }
  // The dashed guide(s): ARCS since round 6 — each stroked only over the
  // span its ring's tiles occupy, so a guide never crosses the screen edge.
  // An SVG per guide, not a bordered div: a div's border is a whole circle
  // or nothing, and `conic-gradient` masks have no dash.
  payload.guides.forEach((d, i) => {
    const arc = payload.guide_arcs?.[i] ?? { d, lo: 0, hi: 360 };
    ring.appendChild(guideElement(arc.d, arc.lo, arc.hi, i));
  });

  payload.items.forEach((it, i) => {
    const { dx, dy } = tileOffset(it.angle_deg, it.radius);
    // THREE LAYERS, the Space ring's cell/inner split one step further, and
    // for the same reason it exists there ("a filled animation beats a plain
    // transform declaration"): three things want this tile's transform and
    // they must never share one property.
    //   .mr-tile  the POSITIONER — the ring offset, plus the wave's scale
    //             (`--ws`, written per frame by the lerp below).
    //   .mr-lift  the AIM PUSH — `--px/--py`, moved by the CSS spring
    //             (170 ms cubic-bezier(.34,1.3,.4,1), `.st-chip`'s).
    //   .mr-face  the SURFACE — colours, glow, badge, and the keyframed
    //             entrance / exit / pop, which fill without stepping on the
    //             two above.
    const tile = document.createElement("div");
    tile.className = "mr-tile";
    tile.dataset.i = String(i);
    tile.dataset.ring = String(it.ring);
    tile.dataset.kind = it.kind;
    tile.dataset.angle = String(it.angle_deg);
    tile.classList.toggle("outer", it.ring > 0);
    tile.style.setProperty("--dx", `${dx}px`);
    tile.style.setProperty("--dy", `${dy}px`);
    tile.style.setProperty("--tile", `${it.tile}px`);
    tile.title = it.name;
    const lift = document.createElement("div");
    lift.className = "mr-lift";
    const face = document.createElement("div");
    face.className = "mr-face";
    // `st-bloom-in`'s --fx/--fy: the vector from the tile's place BACK to
    // the centre, so the keyframe blooms it outward from the cursor; the
    // stagger is toast.ts's, from the same 120 ms first-chip offset.
    face.style.setProperty("--fx", `${Math.round(-dx)}px`);
    face.style.setProperty("--fy", `${Math.round(-dy)}px`);
    face.style.setProperty("--mr-delay", `${tileDelay(i, stagger)}ms`);
    if (it.icon) {
      const img = document.createElement("img");
      img.className = "mr-icon";
      img.alt = "";
      img.draggable = false;
      img.src = it.icon;
      // A broken data URL (a hand-edited config) must fall to the disc, not
      // to a broken-image glyph — the owner's priority is real icons, and a
      // wrong icon is worse than a letter.
      img.addEventListener("error", () => {
        img.replaceWith(discFor(it));
      });
      face.appendChild(img);
    } else {
      face.appendChild(discFor(it));
    }
    const badge = document.createElement("span");
    badge.className = "mr-badge";
    badge.textContent = it.key;
    face.appendChild(badge);
    lift.appendChild(face);
    tile.appendChild(lift);
    if (armed === i) tile.classList.add("armed");
    ring.appendChild(tile);
  });

  // The centre pill: two name layers so a change can crossfade, each with
  // the two lines (app / account).
  const pill = document.createElement("div");
  pill.className = "mr-pill";
  pill.style.setProperty("--mr-pill", `${PILL_REST}px`);
  pill.append(nameLayer(true), nameLayer(false));
  ring.appendChild(pill);
  host.appendChild(ring);
  if (armed !== null && payload.items[armed]) {
    const it = payload.items[armed];
    fitPill(pill, pill.firstElementChild as HTMLElement, it.name, it.account ?? null, payload.pill_max ?? PILL_REST);
  }
  return ring;
}

function nameLayer(on: boolean): HTMLSpanElement {
  const layer = document.createElement("span");
  layer.className = on ? "mr-name is-on" : "mr-name";
  const app = document.createElement("span");
  app.className = "mr-app";
  const acct = document.createElement("span");
  acct.className = "mr-acct";
  layer.append(app, acct);
  return layer;
}

/**
 * Put `app` / `account` into a name layer and size the pill for it: grow
 * the pill up to `pillMax` first, then shrink the two font sizes (never
 * below the floors), then let the CSS ellipsis take what is left. Measured
 * with `scrollWidth`, which is the text's natural width under
 * `overflow: hidden`. Exported for the preview's DOM checks.
 */
export function fitPill(
  pill: HTMLElement,
  layer: HTMLElement,
  app: string,
  account: string | null,
  pillMax: number,
): { pill: number; appPx: number; acctPx: number } {
  const appEl = layer.querySelector<HTMLElement>(".mr-app");
  const acctEl = layer.querySelector<HTMLElement>(".mr-acct");
  if (!appEl || !acctEl) return { pill: PILL_REST, appPx: APP_PX, acctPx: ACCT_PX };
  appEl.textContent = app;
  acctEl.textContent = account ?? "";
  layer.classList.toggle("one-line", !account);
  appEl.style.fontSize = `${APP_PX}px`;
  acctEl.style.fontSize = `${ACCT_PX}px`;
  // Natural widths at the base sizes, measured unconstrained.
  appEl.style.maxWidth = "none";
  acctEl.style.maxWidth = "none";
  const need = Math.max(appEl.scrollWidth, account ? acctEl.scrollWidth : 0);
  const max = Math.max(PILL_REST, pillMax);
  let d = Math.min(max, Math.max(PILL_REST, Math.ceil(need + PILL_PAD)));
  let appPx = APP_PX, acctPx = ACCT_PX;
  const inner = d - PILL_PAD;
  if (need > inner && need > 0) {
    // Shrink both lines by the same factor, floored.
    const k = inner / need;
    appPx = Math.max(APP_MIN_PX, Math.floor(APP_PX * k * 10) / 10);
    acctPx = Math.max(ACCT_MIN_PX, Math.floor(ACCT_PX * k * 10) / 10);
    appEl.style.fontSize = `${appPx}px`;
    acctEl.style.fontSize = `${acctPx}px`;
    d = max;
  }
  appEl.style.maxWidth = `${d - PILL_PAD}px`;
  acctEl.style.maxWidth = `${d - PILL_PAD}px`;
  pill.style.setProperty("--mr-pill", `${d}px`);
  return { pill: d, appPx, acctPx };
}

/** The letter disc: the special's key, or the name's initial. */
function discFor(it: RingItem): HTMLSpanElement {
  const disc = document.createElement("span");
  disc.className = "mr-disc";
  disc.dataset.kind = it.kind;
  disc.textContent = it.kind === "special" ? it.key : (Array.from(it.name)[0] ?? it.key).toUpperCase();
  return disc;
}

/** Light one tile (or none) and crossfade its name into the pill. The armed
 *  tile and its two angular neighbours (same ring) also get the Space
 *  ring's aim push — 40 / 24 px outward along their own radius, moved by
 *  `.mr-lift`'s 170 ms spring; the JS gate is the same one `paintBloom`
 *  uses (an inline custom property beats any stylesheet rule, so
 *  reduced/flat must simply never write one). `pillMax` is the payload's. */
export function setRingArmed(
  ring: HTMLElement,
  index: number | null,
  items: RingItem[],
  pillMax = PILL_REST,
): void {
  const tiles = ring.querySelectorAll<HTMLElement>(".mr-tile");
  tiles.forEach((t, i) => t.classList.toggle("armed", i === index));
  ring.classList.toggle("aiming", index !== null);
  const pushable = !ring.classList.contains("flat")
    && !ring.classList.contains("reduced") && !reducedMotion();
  const push = new Map<number, number>();
  if (pushable && index !== null && items[index]) {
    push.set(index, R_PUSH_ARMED);
    // Nearest two by on-screen angle within the SAME ring (toast.ts sorts
    // by `geo`, the rendered angle, never by payload order).
    const ringNo = items[index].ring;
    const order = items
      .map((it, i) => ({ i, a: it.angle_deg, r: it.ring }))
      .filter((o) => o.r === ringNo)
      .sort((p, q) => p.a - q.a);
    const k = order.findIndex((o) => o.i === index);
    if (k >= 0 && order.length > 2) {
      push.set(order[(k - 1 + order.length) % order.length].i, R_PUSH_NEIGHBOUR);
      push.set(order[(k + 1) % order.length].i, R_PUSH_NEIGHBOUR);
    } else if (k >= 0 && order.length === 2) {
      push.set(order[(k + 1) % 2].i, R_PUSH_NEIGHBOUR);
    }
  }
  tiles.forEach((t, i) => {
    const lift = t.firstElementChild as HTMLElement | null;
    if (!lift) return;
    const p = push.get(i);
    t.classList.toggle("boost", p !== undefined);
    if (p !== undefined && items[i]) {
      const a = (items[i].angle_deg * Math.PI) / 180;
      lift.style.setProperty("--px", `${Math.round(p * Math.sin(a))}px`);
      lift.style.setProperty("--py", `${Math.round(-p * Math.cos(a))}px`);
    } else {
      lift.style.removeProperty("--px");
      lift.style.removeProperty("--py");
    }
  });
  const pill = ring.querySelector<HTMLElement>(".mr-pill");
  const names = ring.querySelectorAll<HTMLElement>(".mr-name");
  if (!pill || names.length < 2) return;
  const on = ring.querySelector<HTMLElement>(".mr-name.is-on") ?? names[0];
  const off = on === names[0] ? names[1] : names[0];
  const it = index !== null ? items[index] : undefined;
  const app = it ? it.name : "";
  const acct = it ? it.account ?? null : null;
  const cur = on.querySelector<HTMLElement>(".mr-app")?.textContent ?? "";
  const curAcct = on.querySelector<HTMLElement>(".mr-acct")?.textContent ?? "";
  if (cur === app && curAcct === (acct ?? "")) return;
  fitPill(pill, off, app, acct, pillMax);
  off.classList.add("is-on");
  on.classList.remove("is-on");
}

/** overlay.ts puts `.reduced-motion` on <html> from the app's own "Visual
 *  effects" setting; the payload's `reduced` flag is the same fact from Rust.
 *  Read both, exactly as toast.ts's `REDUCED()` reads the class. */
function reducedMotion(): boolean {
  return document.documentElement.classList.contains("reduced-motion");
}

/* ---------------------------------------------------------------------------
   THE FISHEYE WAVE (ripple only). Rust streams the cursor's compass bearing
   at ≤ 60 Hz (`middle-ring-aim`, null inside the dead zone); this lerps each
   tile's scale toward `waveScale(angular distance in slots)` with a ~120 ms
   time constant, writing `--ws` on the positioner. One rAF loop, running
   only while a target is still being approached; ≤ 60 inline writes per
   frame, transform only, so the software compositor sees the same load the
   Space ring's 35 blooming chips already give it.
   --------------------------------------------------------------------------- */
interface WaveState {
  ring: HTMLElement;
  tiles: HTMLElement[];
  angles: number[];
  /** Slot pitch per tile, degrees: 360 / (tiles in that ring). */
  pitch: number[];
  cur: number[];
  aim: number | null;
  raf: number;
  last: number;
}
let _wave: WaveState | null = null;

/** Targets for a bearing: `waveScale(compassDist / pitch)`, or 1 everywhere
 *  when there is no bearing (dead zone — the ring rests uniform). */
export function waveTargets(angles: number[], pitch: number[], aim: number | null): number[] {
  return angles.map((a, i) => (aim === null ? 1 : waveScale(compassDist(aim, a) / pitch[i])));
}

function waveStart(ring: HTMLElement, items: RingItem[]): void {
  waveStop();
  const tiles = Array.from(ring.querySelectorAll<HTMLElement>(".mr-tile"));
  const perRing = new Map<number, number>();
  items.forEach((it) => perRing.set(it.ring, (perRing.get(it.ring) ?? 0) + 1));
  _wave = {
    ring,
    tiles,
    angles: items.map((it) => it.angle_deg),
    pitch: items.map((it) => it.pitch_deg && it.pitch_deg > 0 ? it.pitch_deg : 360 / Math.max(1, perRing.get(it.ring) ?? 1)),
    cur: items.map(() => 1),
    aim: null,
    raf: 0,
    last: 0,
  };
}

function waveStop(): void {
  if (_wave?.raf) cancelAnimationFrame(_wave.raf);
  _wave = null;
}

/** A new bearing sample (or null): retarget and make sure the loop runs. */
function waveAim(angle: number | null): void {
  if (!_wave) return;
  _wave.aim = angle;
  if (!_wave.raf) {
    _wave.last = performance.now();
    _wave.raf = requestAnimationFrame(waveFrame);
  }
}

function waveFrame(now: number): void {
  const w = _wave;
  if (!w) return;
  w.raf = 0;
  const dt = Math.min(64, Math.max(0, now - w.last));
  w.last = now;
  const k = 1 - Math.exp(-dt / R_LERP_TAU_MS);
  const tgt = waveTargets(w.angles, w.pitch, w.aim);
  let settled = true;
  for (let i = 0; i < w.tiles.length; i++) {
    const next = w.cur[i] + (tgt[i] - w.cur[i]) * k;
    if (Math.abs(next - w.cur[i]) > 0.0005) {
      w.cur[i] = next;
      w.tiles[i].style.setProperty("--ws", next.toFixed(4));
      settled = false;
    } else if (Math.abs(tgt[i] - w.cur[i]) > 0.0005) {
      settled = false;
    }
  }
  if (!settled) w.raf = requestAnimationFrame(waveFrame);
}

/** For the preview and tests: the current per-tile wave scales, or null. */
export function waveSnapshot(): number[] | null {
  return _wave ? _wave.cur.slice() : null;
}

/* ---------------------------------------------------------------------------
   The live overlay: listeners + the window handshake
   --------------------------------------------------------------------------- */

export interface RingWindowHandshake {
  /** The ring is about to own the window — park the toast stack. */
  own: () => void;
  /** The ring has left the window — one fit or the terminal hide. */
  release: () => void;
}

let _handshake: RingWindowHandshake | null = null;

function host(): HTMLElement {
  return document.body;
}

function showRing(payload: MiddleRingPayload): void {
  window.clearTimeout(_exitTimer);
  _seq++;
  if (_el) {
    _el.remove();
    _el = null;
  }
  _payload = payload;
  _armed = null;
  _active = true;
  _handshake?.own();
  waveStop();
  _el = renderMiddleRing(host(), payload, null);
  // THE ENTRANCE — and why 1.0.110 had none (the owner: "no animation at
  // all"). This used to add `.in` inside a requestAnimationFrame. A rAF
  // callback runs BEFORE the frame's style recalc, so an element inserted
  // this turn had no computed style yet when the class landed: no
  // before-change style, no CSS transition — the ring simply appeared in
  // its final state. Measured on 2026-09-13 with `getAnimations()`: append +
  // class = [] ; append + forced style read + class = [opacity, transform].
  // So: force the style read (the start state is now computed), THEN add
  // the class, synchronously — no frame to wait for, nothing to race.
  // Reduced motion takes the final state directly, as before.
  if (payload.reduced || reducedMotion()) {
    _el.classList.add("in");
  } else {
    void _el.getBoundingClientRect();
    _el.classList.add("in");
  }
  if (payload.fun && !payload.reduced && !reducedMotion()) {
    waveStart(_el, payload.items);
  }
}

/**
 * Hide. `actionPending` = a tile was released or clicked: the Space ring's
 * launch — no pop, the armed tile holds while the rest collapse
 * (`#st-hud.collapsing`), inside the shell's own exit. Otherwise the exit
 * alone. BOUNDED setTimeouts, never animation events — an `animationend`
 * that fails to fire would leave the window up forever, and Rust is relying
 * on this file for the terminal hide of a plain release.
 */
function hideRing(actionPending: boolean): void {
  if (!_el || !_active) {
    // Nothing drawn (a show that never rendered, or a hide that arrived
    // twice): still hand the window back. `release` is idempotent, and a
    // window left up with nothing in it is the exact PROBLEM 135 shape
    // this file is trusted to prevent.
    _active = false;
    _handshake?.release();
    return;
  }
  _active = false;
  waveStop();
  const el = _el;
  const seq = _seq;
  const reduced = !!_payload?.reduced || reducedMotion();
  const exitMs = Math.max(R_SHELL_OUT_MS, R_TILE_OUT_MS);
  const finish = () => {
    if (seq !== _seq) return; // a newer show owns the DOM now
    el.remove();
    if (_el === el) _el = null;
    _payload = null;
    _armed = null;
    _handshake?.release();
  };
  const exit = () => {
    if (seq !== _seq) return;
    el.classList.remove("in");
    el.classList.add("out");
    if (actionPending) el.classList.add("launched");
    _exitTimer = window.setTimeout(finish, reduced ? 0 : exitMs + 20);
  };
  if (reduced) {
    finish();
  } else {
    exit();
  }
}

/**
 * Register the ring's listeners. Called ONCE from overlay.ts, after
 * `initToastListener` has resolved, with the toast stack's handshake. Global
 * `emit` + one listener here is the only event arrangement that has ever
 * delivered in this app; `emit_to` never has.
 *
 * `hud-pointer` is SHARED with the Space ring: Rust's poller emits one event
 * whichever ring is up, and each listener applies it only to its own live
 * DOM — `toast.ts`'s `setArmedChip` paints into `#st-hud` (empty while this
 * ring is up) and this one paints into `#st-mring` (absent while that one is).
 */
export async function initMiddleRing(handshake: RingWindowHandshake): Promise<void> {
  _handshake = handshake;
  await listen<MiddleRingPayload>("middle-ring-show", (e) => {
    if (e.payload && Array.isArray(e.payload.items)) showRing(e.payload);
  });
  await listen<boolean>("middle-ring-hide", (e) => hideRing(e.payload === true));
  await listen<{ index: number | null } | null>("hud-pointer", (e) => {
    if (!_el || !_active || !_payload) return;
    const idx = e.payload && typeof e.payload.index === "number" ? e.payload.index : null;
    _armed = idx !== null && idx >= 0 && idx < _payload.items.length ? idx : null;
    setRingArmed(_el, _armed, _payload.items, _payload.pill_max ?? PILL_REST);
  });
  // PROBLEM 267 follow-up — the cursor's bearing, ≤ 60 Hz while the ring is
  // up. `angle` is compass degrees or null inside the dead zone. Only the
  // wave reads it; arming stays with `hud-pointer` above, so the two cannot
  // disagree about the pick.
  await listen<{ angle: number | null } | null>("middle-ring-aim", (e) => {
    if (!_el || !_active || !_wave) return;
    const a = e.payload && typeof e.payload.angle === "number" && isFinite(e.payload.angle)
      ? e.payload.angle : null;
    waveAim(a);
  });
}

/** The preview harness's hooks into the wave: start it on a rendered ring,
 *  then feed bearings as Rust would. Not used by the overlay. */
export function previewWave(ring: HTMLElement, items: RingItem[]): void {
  waveStart(ring, items);
}
export function previewAim(angle: number | null): void {
  waveAim(angle);
}
/** Drive one lerp frame by hand (the browser pane delivers no frames). */
export function previewWaveStep(now: number): void {
  if (_wave) {
    if (_wave.raf) cancelAnimationFrame(_wave.raf);
    _wave.raf = 0;
    waveFrame(now);
  }
}

/** For tests and the preview: the durations the CSS is handed, and the pill's sizes. */
export const MIDDLE_RING_MOTION = {
  NAME_XFADE_MS,
  R_SHELL_IN_MS, R_SHELL_OUT_MS, R_TILE_IN_MS, R_TILE_DELAY0_MS, R_PILL_IN_MS, R_TILE_OUT_MS,
  R_PUSH_ARMED, R_PUSH_NEIGHBOUR, R_LERP_TAU_MS, WAVE_KEYS,
  PILL_REST, PILL_PAD, APP_PX, APP_MIN_PX, ACCT_PX, ACCT_MIN_PX,
} as const;
