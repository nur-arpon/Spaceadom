/**
 * starry-sky.ts — the full Starry night scene (PROBLEMS 145 and 150).
 *
 * Sources, in order of authority:
 *  - design/night-scene4.md   — the 2026-08-20 delta spec: moon, 20
 *    constellations in exclusive bands, crest-field sea, rigged galleon,
 *    storm. Every number below that is not attributed elsewhere is from it.
 *  - design/moon.md           — the moon, in full detail.
 *  - design/Help Lab v2-4.dc.html — the lab; generators (seaField, seaWave,
 *    injectHeave, the band partition) are transcribed from it line for line,
 *    and night-markup.ts is its ocean subtree extracted verbatim.
 *  - src/constellations.js    — the 20 figures, byte-identical to the file
 *    the owner extracted from the lab.
 *
 * SCALE — the owner's 2026-08-20 verdict was "smaller than today", while the
 * lab GREW the ship. Resolution: the ocean band renders the lab's coordinate
 * space verbatim inside `.sky-ocean-world` (200px tall, lab numbers intact)
 * and the world is scaled 0.75 by CSS — band 150px, waterline 130.5px. The
 * ship's width attribute is additionally 379 -> 320 in night-markup.ts, so it
 * lands at 240px on screen. The moon is scaled by the same 0.75 numerically
 * (it lives in the viewport, not the world). Constellation SVGs render at
 * 0.85. The star tile got denser and finer in themes.css.
 *
 * GATING unchanged: theme=starry AND fun=on builds this; fun off keeps the
 * plain 1.0.57 drifting star tile from themes.css.
 *
 * STABILITY (PROBLEM 134's lesson): decoration only, dashboard webview only.
 * `body.is-blurred` and `body.sky-frozen` pause every CSS animation in the
 * scene, and every SCHEDULER below checks busy() before advancing, so a
 * minimised tray app spends nothing on any of this.
 */

import { sfx } from "../sfx";
// The same 8 entrances the special-key cards use, in the same order — spec §4
// and §5b are explicit that they share one list. (Fun OFF collapses cards to
// the iris, but this scene only exists with fun ON, so the variety stands.)
import { CARD_ANIMS } from "./special-cards";
import { CONSTELLATIONS, type Constellation } from "../constellations.js";
import { OCEAN_WORLD_HTML } from "./night-markup";

const SVG_NS = "http://www.w3.org/2000/svg";

// The ocean world's CSS scale is 0.75 (.sky-ocean-world, starry-sky.css); the
// MOON_PATH numbers below are pre-multiplied by the same 0.75 by hand, since
// the moon lives in viewport coordinates, not inside the scaled world.
/** Constellation render scale ("~15% smaller", the owner's 2026-08-20 verdict). */
const CON_SCALE = 0.85;

let _root: HTMLDivElement | null = null;
let _card: HTMLDivElement | null = null;
let _openIdx: number | null = null;
let _litIdx = -1;
let _outsideClose: ((e: PointerEvent) => void) | null = null;

// Schedulers. All setTimeout chains, all cleared in destroyStarrySky().
let _litT: number | undefined;
let _fadeT: number | undefined;
let _powT: number | undefined;
let _moonT: number | undefined;
let _heaveStyle: HTMLStyleElement | null = null;

/** Constellation indices currently faded out of the sky. */
const _hidden = new Set<number>();
let _moonLeg = 0;
let _pow = 0.34;              // moonPow: 0 = cloud wins, 1 = moon blazes

function reduced(): boolean {
  return document.documentElement.classList.contains("reduced-motion");
}

/** True while nobody is watching — or while a card has the sky frozen.
 *  Schedulers hold instead of advancing; CSS pauses the animations, and this
 *  keeps the moon's wander (a transition, unpausable) from picking a NEW leg
 *  under a frozen sky. */
function busy(): boolean {
  return document.hidden
    || document.body.classList.contains("is-blurred")
    || document.body.classList.contains("sky-frozen");
}

// ---------------------------------------------------------------------------
// Constellations — 20 figures, each in exactly ONE of three drift bands
// ---------------------------------------------------------------------------

/** One constellation SVG. Rendering semantics are the lab's, line for line:
 *  bbox +8, a 22px-overhung hit rect, r=16 halo circles, hairline links,
 *  stars r 1.4 (key star 1.9). Rendered at CON_SCALE via width/height only,
 *  so every local coordinate stays the lab's. */
function buildCon(c: Constellation, i: number): SVGSVGElement {
  const w = Math.max(...c.pts.map((p) => p[0])) + 8;
  const h = Math.max(...c.pts.map((p) => p[1])) + 8;
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("width", (w * CON_SCALE).toFixed(1));
  svg.setAttribute("height", (h * CON_SCALE).toFixed(1));
  svg.setAttribute("viewBox", `0 0 ${w} ${h}`);
  svg.dataset.con = String(i);
  svg.classList.add("sky-con");

  const title = document.createElementNS(SVG_NS, "title");
  title.textContent = c.name;
  svg.appendChild(title);

  // The generous hit area: stars are ~3px, so pressing ANYWHERE near the
  // figure must count. A rect hanging 22px outside the bbox plus a 16px halo
  // behind every star (spec: "you never have to hit a star exactly").
  const hit = document.createElementNS(SVG_NS, "rect");
  hit.setAttribute("x", "-22");
  hit.setAttribute("y", "-22");
  hit.setAttribute("width", String(w + 44));
  hit.setAttribute("height", String(h + 44));
  hit.setAttribute("fill", "transparent");
  hit.style.pointerEvents = "auto";
  svg.appendChild(hit);

  for (const [a, b] of c.lines) {
    const ln = document.createElementNS(SVG_NS, "line");
    ln.setAttribute("x1", String(c.pts[a][0]));
    ln.setAttribute("y1", String(c.pts[a][1]));
    ln.setAttribute("x2", String(c.pts[b][0]));
    ln.setAttribute("y2", String(c.pts[b][1]));
    ln.setAttribute("stroke", "#d6e2ff");
    ln.setAttribute("stroke-width", "0.55");
    ln.classList.add("sky-con-line");
    svg.appendChild(ln);
  }

  c.pts.forEach((p) => {
    const halo = document.createElementNS(SVG_NS, "circle");
    halo.setAttribute("cx", String(p[0]));
    halo.setAttribute("cy", String(p[1]));
    halo.setAttribute("r", "16");
    halo.setAttribute("fill", "transparent");
    halo.style.pointerEvents = "auto";
    svg.appendChild(halo);
  });

  c.pts.forEach((p, k) => {
    const star = document.createElementNS(SVG_NS, "circle");
    star.setAttribute("cx", String(p[0]));
    star.setAttribute("cy", String(p[1]));
    // r is a CSS property here, not an attribute — the lit/open +0.5 bump has
    // to transition, and an attribute cannot. The class carries which star is
    // the key one; starry-sky.css owns both radii and the bump.
    star.setAttribute("fill", "#fff");
    star.classList.add("sky-con-star");
    if (k === 0) star.classList.add("is-key");
    // Per-star twinkle stagger, only meaningful while its figure is lit.
    star.style.setProperty("--tw-dur", `${(1.1 + (k % 4) * 0.25).toFixed(2)}s`);
    star.style.setProperty("--tw-del", `${((k % 5) * 0.18).toFixed(2)}s`);
    svg.appendChild(star);
  });

  svg.addEventListener("click", (e) => {
    e.stopPropagation();
    if (_openIdx === i) closeCard();
    else openCard(i, svg);
  });
  return svg;
}

/**
 * Partition the 20 figures across the three 1400px drift bands — lab-verbatim.
 *
 * THE BUG THIS FIXES: the three bands used to render the SAME list, so
 * panning the sky showed every constellation three times. Now each belongs to
 * exactly one band (shuffled 7/7/6), each in its own slot with jitter, so the
 * full 4200px loop (~9 minutes) never repeats a figure and the layout is
 * different every launch.
 */
function partitionBands(): { ci: number; x: number; y: number }[][] {
  const idx = CONSTELLATIONS.map((_, i) => i);
  for (let i = idx.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    const t = idx[i]; idx[i] = idx[j]; idx[j] = t;
  }
  const bands: number[][] = [[], [], []];
  idx.forEach((v, k) => bands[k % 3].push(v));
  return bands.map((list) => {
    const slot = 1400 / list.length;
    return list.map((ci, k) => ({
      ci,
      x: Math.round(slot * k + 16 + Math.random() * Math.max(48, slot - 132)),
      y: Math.round(Math.min(70, Math.max(3, CONSTELLATIONS[ci].pos[1] + (Math.random() * 11 - 5.5))) * 10) / 10,
    }));
  });
}

/** The info card, FIXED and clamped to the viewport, centred under the
 *  figure's live (mid-drift) position. Entrance cycles the 8 animations. */
function openCard(i: number, svg: SVGSVGElement): void {
  closeCard(true);           // silent: the entrance below is the sound here
  const c = CONSTELLATIONS[i];
  const r = svg.getBoundingClientRect();
  const vw = window.innerWidth || 1240;
  const vh = window.innerHeight || 1000;
  const x = Math.round(Math.min(Math.max(r.left + r.width / 2, 118), vw - 118));
  const y = Math.round(Math.min(r.bottom + 12, vh - 160));

  const card = document.createElement("div");
  card.dataset.conCard = "1";
  card.className = "sky-card";
  // Same reason as the special-key card: it is a <body> child, and a click on
  // it must not reach the document listener that closes every popover.
  card.addEventListener("click", (e) => e.stopPropagation());
  card.style.left = `${x}px`;
  card.style.top = `${y}px`;
  const [anim, ease] = CARD_ANIMS[i % CARD_ANIMS.length];
  // Index-matched to the animation on purpose: sounds.js cycles the same 8
  // characters in the same order, so the genie card gets the genie sound.
  sfx.cardOpen(i);
  if (!reduced()) card.style.animation = `${anim} 520ms ${ease} both`;
  else card.style.transform = "translateX(-50%)";

  const name = document.createElement("div");
  name.className = "sky-card-name";
  name.textContent = c.name;
  const fact = document.createElement("div");
  fact.className = "sky-card-fact";
  fact.textContent = c.fact;
  const season = document.createElement("div");
  season.className = "sky-card-season";
  season.textContent = `Best seen: ${c.season}`;
  card.append(name, fact, season);
  document.body.appendChild(card);

  _card = card;
  _openIdx = i;
  _root?.querySelectorAll<SVGSVGElement>(`[data-con="${i}"]`).forEach((s) => s.classList.add("is-open"));
  // The WHOLE sky freezes while a card is up — stars, figures, waves, ship —
  // and resumes the moment it closes (spec §5b).
  document.body.classList.add("sky-frozen");
}

function closeCard(silent = false): void {
  // Only when a card was really up: closeCard() also runs on teardown and at
  // the top of every open, and a close sound with nothing closing is noise.
  if (!silent && _card) sfx.cardClose();
  _card?.remove();
  _card = null;
  if (_openIdx !== null) {
    _root?.querySelectorAll(`[data-con="${_openIdx}"]`).forEach((s) => s.classList.remove("is-open"));
  }
  _openIdx = null;
  document.body.classList.remove("sky-frozen");
}

/**
 * One figure lights at a time. The beat is RANDOM (2.6–6s, lab-verbatim), not
 * a fixed interval, and only ever picks from figures currently visible.
 */
function scheduleRelight(): void {
  _litT = window.setTimeout(scheduleRelight, 2600 + Math.random() * 3400);
  if (!_root || busy()) return;
  const vis = CONSTELLATIONS.map((_, i) => i).filter((i) => !_hidden.has(i) && i !== _litIdx);
  if (!vis.length) return;
  const i = vis[Math.floor(Math.random() * vis.length)];
  _root.querySelectorAll(".sky-con.is-lit").forEach((s) => s.classList.remove("is-lit"));
  _root.querySelectorAll(`[data-con="${i}"]`).forEach((s) => s.classList.add("is-lit"));
  _litIdx = i;
}

function setHidden(i: number, hide: boolean): void {
  if (hide) _hidden.add(i); else _hidden.delete(i);
  _root?.querySelector<HTMLElement>(`.sky-con-wrap[data-wrap="${i}"]`)?.classList.toggle("is-hidden", hide);
}

/**
 * Appear and vanish (spec §2): ~45% start hidden; every 5.2–12s one figure
 * crosses over on a 3.4s opacity fade, keeping the hidden count floating
 * between 30% and 55% — roughly 3–4 visible per band. Nothing vanishes while
 * it is lit or while its card is open; hidden figures take no presses.
 */
function scheduleFade(): void {
  _fadeT = window.setTimeout(scheduleFade, 5200 + Math.random() * 6800);
  if (!_root || busy()) return;
  const n = CONSTELLATIONS.length;
  const hide = _hidden.size < n * 0.3 || (_hidden.size < n * 0.55 && Math.random() < 0.5);
  if (hide) {
    const vis = CONSTELLATIONS.map((_, i) => i)
      .filter((i) => !_hidden.has(i) && i !== _openIdx && i !== _litIdx);
    if (vis.length) setHidden(vis[Math.floor(Math.random() * vis.length)], true);
  } else if (_hidden.size) {
    const pool = Array.from(_hidden);
    setHidden(pool[Math.floor(Math.random() * pool.length)], false);
  }
}

// ---------------------------------------------------------------------------
// The sea — three crest fields and a silhouette horizon, generated at load
// (lab-verbatim: seaField / seaWave / injectHeave, values from night-scene §3)
// ---------------------------------------------------------------------------

interface FieldOpts {
  h: number; n: number; bias: number; minW: number; maxW: number; maxH: number;
  aTop: number; aBot: number; col: string; foam: number;
}

/** A 1560px transparent tile of individual crest marks — size, brightness and
 *  density read as distance. Three mark kinds (swell/crest/chop), a dark
 *  understroke for volume past t>.45, tapered foam lips, seamless wrap. */
function seaField(o: FieldOpts): string {
  const W = 1560, R = Math.random;
  let s = "";
  for (let i = 0; i < o.n; i++) {
    const x = R() * W;
    const t = Math.pow(R(), o.bias);
    const y = 3 + t * (o.h - 8);
    let w = (o.minW + t * (o.maxW - o.minW)) * (0.5 + R() * 1);
    let h = (1.5 + t * o.maxH) * (0.55 + R() * 0.9);
    const kind = R();
    if (kind < 0.4) { h *= 0.34; w *= 1.55; }        // long shallow swell
    else if (kind > 0.88) { h *= 1.6; w *= 0.42; }   // short steep chop
    const a0 = (o.aTop + t * (o.aBot - o.aTop)) * (0.5 + R() * 0.8);
    const k = 0.38 + R() * 0.4;
    const px = x + w * k;
    const lw = (0.5 + t * 1.2).toFixed(2);
    const d = "M" + x.toFixed(1) + " " + y.toFixed(1) + " Q " + (x + w * k * 0.5).toFixed(1) + " " + (y - h).toFixed(1) +
      " " + px.toFixed(1) + " " + (y - h * 0.76).toFixed(1) + " T " + (x + w).toFixed(1) + " " + (y + h * 0.2).toFixed(1);
    let m = "";
    if (t > 0.45 && R() < 0.4) m += "<path d='" + d + "' fill='none' stroke='rgba(2,5,12," + (a0 * 1.5).toFixed(3) +
      ")' stroke-width='" + (+lw + 1.2).toFixed(2) + "' stroke-linecap='round' transform='translate(0," + (1.2 + t * 1.6).toFixed(1) + ")'/>";
    m += "<path d='" + d + "' fill='none' stroke='rgba(" + o.col + "," + a0.toFixed(3) + ")' stroke-width='" + lw + "' stroke-linecap='round'/>";
    if (t > 0.38 && R() < o.foam) {
      m += "<path d='M" + (px - w * 0.14).toFixed(1) + " " + (y - h * 0.58).toFixed(1) + " Q " + px.toFixed(1) + " " + (y - h * 0.95).toFixed(1) +
        " " + (px + w * 0.16).toFixed(1) + " " + (y - h * 0.5).toFixed(1) + "' fill='none' stroke='rgba(232,243,255," +
        Math.min(0.62, a0 * 1.7).toFixed(3) + ")' stroke-width='" + (0.8 + t * 1.7).toFixed(2) + "' stroke-linecap='round'/>";
      const dn = 1 + Math.round(R() * 3 * t);
      for (let q = 0; q < dn; q++) m += "<circle cx='" + (px + (R() - 0.5) * w * 0.6).toFixed(1) + "' cy='" + (y - h - R() * 7 * t).toFixed(1) +
        "' r='" + (0.35 + R() * 0.65).toFixed(1) + "' fill='rgba(226,239,255," + Math.min(0.5, a0 * 1.1).toFixed(3) + ")'/>";
    }
    s += m;
    if (x + w > W) s += "<g transform='translate(-" + W + ",0)'>" + m + "</g>";
  }
  const svg = "<svg xmlns='http://www.w3.org/2000/svg' width='" + W + "' height='" + o.h + "'>" + s + "</svg>";
  return 'url("data:image/svg+xml,' + encodeURIComponent(svg) + '")';
}

interface WaveOpts {
  h: number; base: number; amp: number; harm: [number, number][];
  c1: string; c2: string; line: string; lw: number;
  foamR: number; foamA: number; spray: number;
  chop?: [number, number][]; chopAmp?: number; deep?: number;
}

/** The horizon — the one place a filled silhouette is still correct: water
 *  meeting sky. Sum of harmonics warped so crests peak and troughs stay
 *  broad, foam only on the strongest third of crests. Lab-verbatim. */
function seaWave(o: WaveOpts): string {
  const W = 1560, R = Math.random, step = 6, N = Math.round(W / step);
  const ph = o.harm.map(() => R() * 6.2832);
  const tot = o.harm.reduce((s, h) => s + h[1], 0);
  const ns: number[] = [], ys: number[] = [];
  for (let k = 0; k <= N; k++) {
    const x = k * step;
    let s = 0;
    o.harm.forEach((h, i) => { s += h[1] * Math.sin(6.2832 * h[0] * x / W + ph[i]); });
    let n = s / tot;
    n = n >= 0 ? Math.pow(n, 1.42) : -Math.pow(-n, 0.74);
    ns.push(n);
    ys.push(+(o.base - n * o.amp).toFixed(1));
  }
  let d = "M0 " + ys[0];
  for (let k = 1; k <= N; k++) d += "L" + (k * step) + " " + ys[k];
  let foam = "";
  for (let k = 2; k < N - 1; k++) {
    if (!(ns[k] > ns[k - 1] && ns[k] >= ns[k + 1] && ns[k] > 0.46)) continue;
    const x = k * step, y = ys[k], s = Math.min(1, (ns[k] - 0.46) / 0.54);
    const rx = +(o.foamR * (0.45 + s)).toFixed(1), ry = +(0.9 + s * 1.2).toFixed(1);
    foam += "<ellipse cx='" + x + "' cy='" + (y + 1).toFixed(1) + "' rx='" + rx + "' ry='" + ry + "' fill='rgba(214,232,255," + (o.foamA * (0.4 + 0.6 * s)).toFixed(2) + ")'/>";
    if (s > 0.4) foam += "<path d='M" + (x - rx * 0.95).toFixed(1) + " " + (y + 1.6).toFixed(1) + " Q " + x + " " + (y - 2.2 - s * 2.6).toFixed(1) + " " + (x + rx * 0.7).toFixed(1) + " " + (y + 0.4).toFixed(1) + "' fill='none' stroke='rgba(228,241,255," + (o.foamA * 0.85).toFixed(2) + ")' stroke-width='" + (0.65 + s * 0.65).toFixed(1) + "' stroke-linecap='round'/>";
    const dn = Math.round(s * o.spray);
    for (let q = 0; q < dn; q++) foam += "<circle cx='" + (x + (R() - 0.5) * rx * 2.6).toFixed(1) + "' cy='" + (y - 1 - R() * (3 + s * 8)).toFixed(1) + "' r='" + (0.5 + R() * 0.7).toFixed(1) + "' fill='rgba(222,238,255," + (0.12 + R() * 0.3).toFixed(2) + ")'/>";
  }
  let chop = "";
  if (o.chop) {
    const ph2 = o.chop.map(() => R() * 6.2832), t2 = o.chop.reduce((s, h) => s + h[1], 0);
    let c = "";
    for (let k = 0; k <= N; k++) {
      const x = k * step;
      let s = 0;
      o.chop.forEach((h, i) => { s += h[1] * Math.sin(6.2832 * h[0] * x / W + ph2[i]); });
      c += (k ? "L" : "M") + x + " " + (ys[k] - (s / t2) * (o.chopAmp ?? 0) + 1.8).toFixed(1);
    }
    chop = "<path d='" + c + "' fill='none' stroke='rgba(168,196,244,.15)' stroke-width='.7'/>";
  }
  // The lab supports a deep-tone band that FOLLOWS A WAVE PATH (never the old
  // rectangle — spec §3 bans that outright). The line config passes no `deep`,
  // so this stays dormant; it is ported so a future depth band cannot be
  // "improved" back into a rect.
  let deepPath = "";
  if (o.deep && o.base + o.deep < o.h - 6) {
    const phD = R() * 6.2832;
    let dp = "";
    for (let k = 0; k <= N; k++) {
      const x = k * step;
      const y = ys[k] * 0.5 + o.base * 0.5 + o.deep + Math.sin(6.2832 * 3 * x / W + phD) * 2.6 + Math.sin(6.2832 * 7 * x / W + phD * 1.9) * 1.3;
      dp += (k ? "L" : "M") + x + " " + y.toFixed(1);
    }
    deepPath = "<path d='" + dp + "L" + W + " " + o.h + "L0 " + o.h + "Z' fill='" + o.c2 + "'/>";
  }
  const svg = "<svg xmlns='http://www.w3.org/2000/svg' width='" + W + "' height='" + o.h + "'>" +
    "<path d='" + d + "L" + W + " " + o.h + "L0 " + o.h + "Z' fill='" + o.c1 + "'/>" + deepPath +
    "<path d='" + d + "' fill='none' stroke='" + o.line + "' stroke-width='" + o.lw + "' stroke-linejoin='round'/>" + chop + foam + "</svg>";
  return 'url("data:image/svg+xml,' + encodeURIComponent(svg) + '")';
}

/**
 * The heave keyframes — the sea moves UP AND DOWN, not sideways. Generated at
 * load from a sum of sines, 48 stops, one signal for the vertical and a
 * DIFFERENT one for the horizontal so the axes are uncorrelated. Durations
 * 71/59/47/37 are co-prime ON PURPOSE (~42h before the layers' relative phase
 * repeats) — do not round them to nicer numbers. Lab-verbatim.
 */
function injectHeave(): HTMLStyleElement {
  const cfg = [
    { name: "heave0", dx: -1560, amp: 5, sc: 0.04, surge: 26, harm: [[1, 1], [2, 0.6], [3, 0.42], [5, 0.28]] as [number, number][] },
    { name: "heave1", dx: 1560, amp: 8, sc: 0.06, surge: 40, harm: [[1, 1], [2, 0.66], [3, 0.46], [5, 0.3], [7, 0.18]] as [number, number][] },
    { name: "heave2", dx: -1560, amp: 13, sc: 0.09, surge: 58, harm: [[1, 1], [2, 0.74], [3, 0.52], [4, 0.32], [7, 0.2]] as [number, number][] },
    { name: "heave3", dx: 1560, amp: 19, sc: 0.13, surge: 80, harm: [[1, 1], [2, 0.82], [3, 0.58], [5, 0.36], [8, 0.22]] as [number, number][] },
  ];
  const N = 48;
  const css = cfg.map((c) => {
    const ph = c.harm.map(() => Math.random() * 6.2832);
    const tot = c.harm.reduce((s, h) => s + h[1], 0);
    let out = "@keyframes " + c.name + "{";
    for (let k = 0; k <= N; k++) {
      const t = k / N;
      let s = 0, g = 0;
      c.harm.forEach((h, i) => { s += h[1] * Math.sin(6.2832 * h[0] * t + ph[i]); });
      c.harm.forEach((h, i) => { g += h[1] * Math.sin(6.2832 * (h[0] + 1) * t + ph[i] * 1.7 + 1.1); });
      const n = s / tot, m = g / tot;
      out += (t * 100).toFixed(2) + "%{transform:translateY(" + (n * c.amp).toFixed(2) + "px) scaleY(" +
        (1 + Math.abs(n) * c.sc).toFixed(3) + ");background-position-x:" + (c.dx * t + m * c.surge).toFixed(1) + "px}";
    }
    return out + "}";
  }).join("\n");
  const el = document.createElement("style");
  el.id = "st-heave";
  el.textContent = css;
  document.head.appendChild(el);
  return el;
}

/**
 * The storm — `design/storm-clouds.md`, transcribed.
 *
 * The owner handed over that standalone spec after three wrong attempts here,
 * with one instruction: *"use this, you got wrong enough times."* So every
 * number below is its §5 drop-in, verbatim, and the deviations I invented in
 * 1.0.68 (a 500px bank, repositioned masses, a greyed ramp) are GONE.
 *
 * Two things the spec is emphatic about, both of which a "tidy-up" would undo:
 *
 *  - **Storm cloud on a night sky must be LIGHTER than the sky in its
 *    mid-tones, not darker.** §2: "The first attempt used near-black masses
 *    and they were completely invisible against a #131a2e sky." The
 *    `48,62,96` and `60,76,112` steps are what make the massing read. Only the
 *    innermost core is darker than the ground. My greyed ramp pulled those
 *    steps toward slate and cost exactly that.
 *  - **Lopsided radii are load-bearing.** A cloud on `border-radius: 50%` is a
 *    smudged circle; all four corners must differ.
 *
 * The falloff reaching 0 alpha at 88% — before the element edge — is also
 * deliberate: stop it short and the blur reveals a circular seam.
 *
 * SCALE: these are REAL SCREEN PIXELS. The container counter-scales out of the
 * ocean world's 0.75 (see #st-clouds in starry-sky.css), so `left: -110px;
 * bottom: 150px; 700x260` means what the spec says it means, and the blur
 * radii are not resampled by an ancestor transform. It stays a CHILD of the
 * world, after the water layers and before the ship, so the masts and rigging
 * read in front of the bank and the water behind it (§1).
 */
const STORM_MASSES: [number, number, number, number, string, number, number][] = [
  // left, bottom, w, h, border-radius, blur px, drift s   — storm-clouds.md §2
  [   0,  44, 320, 158, "58% 42% 46% 54%", 12, 23],
  [ 160, 100, 360, 142, "46% 54% 58% 42%", 15, 31],
  [  70,   4, 268, 110, "52% 48% 44% 56%", 10, 19],
  [ 320,  24, 300, 128, "44% 56% 52% 48%", 13, 27],
  [ 460,  88, 230, 104, "56% 44% 48% 52%", 12, 35],
  [ -50, 108, 214,  98, "48% 52% 56% 44%", 14, 29],
];

/** Per-mass alphas A/B/C/D from storm-clouds.md §2, same row order. */
const STORM_ALPHA: [number, number, number, number][] = [
  [0.90, 0.73, 0.29, 0.39],
  [0.90, 0.67, 0.27, 0.36],
  [0.90, 0.62, 0.25, 0.34],
  [0.90, 0.66, 0.27, 0.36],
  [0.78, 0.55, 0.21, 0.29],
  [0.76, 0.52, 0.21, 0.28],
];

function stormHtml(): string {
  const masses = STORM_MASSES.map(([x, y, w, h, radius, blur, drift], i) => {
    const [a, b, c, d] = STORM_ALPHA[i];
    return `<span data-storm="${i}" style="position:absolute;left:${x}px;bottom:${y}px;width:${w}px;height:${h}px;` +
      `border-radius:${radius};background:` +
      `radial-gradient(ellipse 58% 62% at 42% 62%,rgba(10,15,30,.92),rgba(24,34,60,${a}) 38%,` +
      `rgba(48,62,96,${b}) 58%,rgba(60,76,112,${c}) 74%,rgba(60,76,112,0) 88%),` +
      `radial-gradient(ellipse 44% 32% at 66% 22%,rgba(158,182,228,${d}),rgba(158,182,228,0) 68%);` +
      `filter:blur(${blur}px);animation:cloudDrift ${drift}s ease-in-out infinite alternate"></span>`;
  }).join("");

  // §3 — two strikes per 17s cycle, the first a double-flicker. Gone in under
  // half a second; a lingering flash reads as a lamp, not lightning.
  const bolt = `<span data-storm="bolt" style="position:absolute;left:100px;bottom:56px;width:360px;height:158px;` +
    `border-radius:50%;background:radial-gradient(ellipse 50% 44% at 44% 42%,rgba(214,231,255,.55),` +
    `rgba(186,208,252,.18) 42%,transparent 72%);filter:blur(12px);animation:lightning 17s linear infinite"></span>`;

  return masses + bolt;
}

// ---------------------------------------------------------------------------
// The moon — moon.md, every number scaled by WORLD_SCALE (0.75) because the
// moonset geometry is measured against the waterline, and our waterline is
// 174px * 0.75 = 130.5px above the viewport bottom.
// ---------------------------------------------------------------------------

interface MoonLeg {
  /** Viewport-coordinate CSS values for the moon element (already scaled). */
  x: string; y: string;
  /** The SPEC'S ORIGINAL x, split for the moonbeam: the beam lives inside
   *  .sky-ocean-world, which is a 1:1 replica of the lab's coordinate space
   *  (the 0.75 happens in CSS), so the beam uses the UNSCALED numbers. */
  wPct: number | null; wPx: number;
  s: number; w: number; d: number; hold: number;
}

/** moon.md §5, ×0.75: disc 88px, path px offsets scaled, dwells unchanged. */
const MOON_PATH: MoonLeg[] = [
  { x: "calc(94% - 88px)", y: "150px",              wPct: 94, wPx: -118, s: 1.00, w: 0,    d: 60000, hold: 64000 },
  { x: "66%",              y: "calc(100% - 425px)", wPct: 66, wPx: 0,    s: 1.03, w: 0.16, d: 58000, hold: 4000 },
  { x: "38%",              y: "calc(100% - 299px)", wPct: 38, wPx: 0,    s: 1.07, w: 0.44, d: 56000, hold: 4000 },
  { x: "63px",             y: "calc(100% - 222px)", wPct: null, wPx: 84, s: 1.10, w: 0.72, d: 50000, hold: 10000 },
  { x: "14px",             y: "calc(100% - 155px)", wPct: null, wPx: 18, s: 1.14, w: 1.00, d: 34000, hold: 62000 },
  { x: "20px",             y: "calc(100% - 242px)", wPct: null, wPx: 26, s: 1.10, w: 0.70, d: 40000, hold: 3000 },
  { x: "44%",              y: "calc(100% - 390px)", wPct: 44, wPx: 0,    s: 1.04, w: 0.28, d: 52000, hold: 3000 },
];
const MOON_EASE = "cubic-bezier(.37,.02,.55,1)";

/** moon.md §8 drop-in, glow insets/blurs and the disc ×0.75. The four glow
 *  layers each run to 0 alpha at 100% with 6–7 stops — that is what keeps the
 *  halo edgeless; do not "simplify" the gradients. */
function moonHtml(): string {
  return `
  <div class="sky-moon" id="st-moon">
    <div class="sky-moon-breathe" id="st-moon-inner">
      <span class="sky-moon-glow" style="inset:-142px;filter:blur(26px);animation:moonHaze 17s ease-in-out infinite;background:radial-gradient(circle, rgba(196,214,255,.085) 0%, rgba(195,213,255,.055) 20%, rgba(193,211,253,.032) 37%, rgba(191,209,251,.016) 54%, rgba(189,207,250,.006) 72%, rgba(189,207,250,.001) 87%, rgba(189,207,250,0) 100%)"></span>
      <span class="sky-moon-glow" style="inset:-72px;filter:blur(17px);background:radial-gradient(circle, rgba(224,237,255,.14) 0%, rgba(218,232,255,.098) 22%, rgba(211,227,254,.062) 40%, rgba(205,222,252,.032) 57%, rgba(201,219,251,.012) 74%, rgba(201,219,251,.003) 88%, rgba(201,219,251,0) 100%)"></span>
      <span class="sky-moon-glow" style="inset:-30px;filter:blur(10px);background:radial-gradient(circle, rgba(247,251,255,.26) 0%, rgba(240,247,255,.19) 24%, rgba(230,241,255,.115) 43%, rgba(222,235,255,.055) 61%, rgba(216,231,255,.019) 79%, rgba(216,231,255,0) 100%)"></span>
      <span class="sky-moon-glow" style="inset:-8px;filter:blur(5px);background:radial-gradient(circle, rgba(252,253,255,.40) 0%, rgba(246,250,255,.30) 46%, rgba(238,246,255,.14) 74%, rgba(232,242,255,0) 100%)"></span>
      <span class="sky-moon-disc"></span>
      <span class="sky-moon-wash">
        <i style="left:2%;top:6%;width:52%;height:56%;border-radius:58% 42% 48% 52%;background:#b5aa96"></i>
        <i style="left:42%;top:14%;width:44%;height:40%;border-radius:46% 54% 52% 48%;background:#b9af9b"></i>
      </span>
      <span class="sky-moon-maria">
        <i style="left:4%;top:21%;width:27%;height:41%;border-radius:62% 38% 44% 56%;background:#b1a68f"></i>
        <i style="left:19%;top:10%;width:27%;height:25%;border-radius:54% 46% 51% 49%;background:#aca18e"></i>
        <i style="left:35%;top:19%;width:15%;height:13%;border-radius:48% 52% 46% 54%;background:#b6ab97"></i>
        <i style="left:45%;top:21%;width:19%;height:20%;border-radius:51% 49% 44% 56%;background:#b0a591"></i>
        <i style="left:54%;top:33%;width:22%;height:23%;border-radius:44% 56% 57% 43%;background:#ada291"></i>
        <i style="left:73%;top:29%;width:10%;height:11%;border-radius:50%;background:#b7ac98"></i>
        <i style="left:23%;top:51%;width:15%;height:13%;border-radius:53% 47% 49% 51%;background:#b9af96"></i>
        <i style="left:62%;top:52%;width:12%;height:15%;border-radius:47% 53% 52% 48%;background:#bcb29b"></i>
        <i style="left:12%;top:44%;width:9%;height:8%;border-radius:50%;background:#bdb39d"></i>
      </span>
    </div>
  </div>`;
}

/** Apply one wander leg to the moon and its water reflection. First paint
 *  passes instant=true so nothing animates into place. */
function applyMoonLeg(leg: number, instant: boolean): void {
  const p = MOON_PATH[leg];
  const moon = document.getElementById("st-moon");
  const beam = document.getElementById("st-moonbeam");
  if (!moon || !beam) return;

  moon.style.transition = instant ? "none"
    : `left ${p.d}ms ${MOON_EASE}, top ${p.d}ms ${MOON_EASE}, transform ${p.d}ms ease-in-out, filter ${p.d}ms linear`;
  moon.style.left = p.x;
  moon.style.top = p.y;
  moon.style.transform = `scale(${p.s})`;
  // Warmth: atmospheric reddening near the horizon, on the whole group so the
  // halo warms with the disc (moon.md §5).
  moon.style.filter = `sepia(${(p.w * 0.4).toFixed(2)}) saturate(${(1 + p.w * 0.5).toFixed(2)}) brightness(${(1 - p.w * 0.05).toFixed(2)})`;

  beam.style.transition = instant ? "none"
    : `left ${p.d}ms ${MOON_EASE}, opacity ${p.d}ms linear, filter ${p.d}ms linear`;
  beam.style.left = p.wPct !== null
    ? `calc(${p.wPct}% + ${p.wPx - 16}px)`
    : `${p.wPx - 16}px`;
  beam.style.filter = `blur(7px) sepia(${(p.w * 0.4).toFixed(2)}) saturate(${(1 + p.w * 0.6).toFixed(2)})`;
  // Beam opacity depends on BOTH the leg's warmth and moonPow, and moves on
  // its own 13s clock — so applyPow writes the opacity transition last and
  // the leg's left/filter transitions are restored right after it.
  applyPow();
  beam.style.transition = instant ? "none"
    : `left ${p.d}ms ${MOON_EASE}, opacity 13s linear, filter ${p.d}ms linear`;
}

function moonNext(): void {
  if (busy()) {
    // Nobody is watching — hold this leg and look again soon rather than
    // flying the moon around an invisible window.
    _moonT = window.setTimeout(moonNext, 5000);
    return;
  }
  _moonLeg = (_moonLeg + 1) % MOON_PATH.length;
  const p = MOON_PATH[_moonLeg];
  applyMoonLeg(_moonLeg, false);
  _moonT = window.setTimeout(moonNext, p.d + p.hold);
}

/**
 * Moonlight vs storm (night-scene §1): a cycle independent of position,
 * weighted to the extremes — 36% cloud wins, 36% the moon blazes through,
 * 28% ordinary night. Everything it drives transitions over 13s LINEAR so it
 * reads as weather moving, not a switch.
 */
function schedulePow(): void {
  _powT = window.setTimeout(schedulePow, 13000 + Math.random() * 17000);
  if (busy()) return;
  const r = Math.random();
  _pow = r < 0.36 ? 0.05 + Math.random() * 0.15
       : r < 0.72 ? 0.84 + Math.random() * 0.16
       :            0.34 + Math.random() * 0.3;
  applyPow();
}

function applyPow(): void {
  const inner = document.getElementById("st-moon-inner");
  const clouds = document.getElementById("st-clouds");
  const beam = document.getElementById("st-moonbeam");
  const pw = _pow;
  if (inner) {
    inner.style.transition = "filter 13s linear";
    // Brightness on the GROUP, not the disc: the glow layers brighten with
    // it, so the halo blazes rather than the disc just going white.
    inner.style.filter = `brightness(${(1 + pw * 0.66).toFixed(3)}) contrast(${(1 + pw * 0.08).toFixed(3)}) saturate(${(1 - pw * 0.07).toFixed(3)})`;
  }
  if (clouds) {
    clouds.style.transition = "opacity 13s linear";
    clouds.style.opacity = (1 - pw * 0.64).toFixed(3);
  }
  if (beam) {
    const w = MOON_PATH[_moonLeg].w;
    // 13s linear like the other two — a pow change must not ride the current
    // wander leg's 34-60s opacity transition (or snap instantly during the
    // 64s home dwell, when no leg transition is set at all). applyMoonLeg
    // re-arms the leg transition immediately after calling this.
    beam.style.transition = "opacity 13s linear";
    beam.style.opacity = Math.min(1, (0.45 + 0.55 * w) * (0.72 + 0.62 * pw)).toFixed(3);
  }
}

// ---------------------------------------------------------------------------
// Build / teardown
// ---------------------------------------------------------------------------

/** Build the whole scene. Idempotent: a second call while built is a no-op. */
export function buildStarrySky(): void {
  if (_root) return;

  const root = document.createElement("div");
  root.id = "st-sky";
  root.setAttribute("aria-hidden", "true");

  // The moon FIRST, so everything else paints over it — that, plus the ocean
  // band's higher z-index, is the whole occlusion model: when the moon sets
  // it simply travels behind the water and the galleon, no masking anywhere.
  root.insertAdjacentHTML("beforeend", moonHtml());

  // Constellations: three 1400px drift bands, each owning its EXCLUSIVE
  // subset of the 20 figures (partitionBands), sailing with the star tile.
  const bands = partitionBands();
  bands.forEach((band, copy) => {
    const layer = document.createElement("div");
    layer.className = "sky-con-layer";
    layer.style.animationName = `sky-conDrift${copy}`;
    band.forEach(({ ci, x, y }) => {
      const wrap = document.createElement("div");
      wrap.className = "sky-con-wrap";
      wrap.dataset.wrap = String(ci);
      wrap.style.left = `${x}px`;
      wrap.style.top = `${y}%`;
      wrap.appendChild(buildCon(CONSTELLATIONS[ci], ci));
      layer.appendChild(wrap);
    });
    root.appendChild(layer);
  });

  // The ocean: the lab's 200px world, verbatim (night-markup.ts), scaled 0.75
  // by .sky-ocean-world. The four wave layers get their generated tiles here;
  // the heave keyframes must exist BEFORE the markup lands or the first
  // frame plays the animations' absence.
  if (!reduced()) _heaveStyle = injectHeave();
  const ocean = document.createElement("div");
  ocean.className = "sky-ocean";
  const world = document.createElement("div");
  world.className = "sky-ocean-world";
  world.innerHTML = OCEAN_WORLD_HTML;

  // Re-author the storm over the lab's (see stormHtml). night-markup.ts keeps
  // the lab's six masses verbatim so the original stays diff-able against the
  // design; this is the ONE deliberate deviation, and it lives here rather
  // than there so it can never be mistaken for transcription drift.
  const cloudBox = world.querySelector("#st-clouds");
  if (cloudBox) cloudBox.innerHTML = stormHtml();
  ocean.appendChild(world);
  root.appendChild(ocean);

  // night-scene §3 — the three fields and the horizon line, exact values.
  const sea = {
    line: seaWave({ h: 46, base: 30, amp: 6.5, harm: [[6, 1], [9, 0.72], [13, 0.5], [19, 0.3], [27, 0.18], [37, 0.11]], c1: "#0d1830", c2: "#0a1226", line: "rgba(158,188,244,.3)", lw: 0.8, foamR: 4, foamA: 0.22, spray: 2, chop: [[15, 1], [24, 0.6], [38, 0.32]], chopAmp: 1.4 }),
    far: seaField({ h: 34, n: 150, bias: 1.5, minW: 16, maxW: 44, maxH: 3, aTop: 0.1, aBot: 0.26, col: "172,198,244", foam: 0.22 }),
    mid: seaField({ h: 58, n: 120, bias: 1.2, minW: 28, maxW: 82, maxH: 5.4, aTop: 0.12, aBot: 0.34, col: "178,204,248", foam: 0.4 }),
    near: seaField({ h: 104, n: 92, bias: 1, minW: 44, maxW: 148, maxH: 9.5, aTop: 0.14, aBot: 0.44, col: "186,210,250", foam: 0.58 }),
  };
  const bg = (id: string, url: string) => {
    const el = world.querySelector<HTMLElement>(`#${id}`);
    if (el) el.style.background = `${url} repeat-x`;
  };
  // THE STORM STAYS INSIDE THE SCALED WORLD, WITH THE SHIP.
  //
  // It was briefly moved out to render at the lab's full size, on my reading
  // that a storm is sky. The owner corrected that immediately and he is right:
  // *"the storm was supposed to be behind the ship to give it scary
  // atmosphere, never for sky."* It is the galleon's weather, so it scales
  // with the galleon — a full-size storm behind a 0.75 ship is a different
  // scene, not a bigger one. Do not move it again.

  bg("st-wave-line", sea.line);
  bg("st-wave-far", sea.far);
  bg("st-wave-mid", sea.mid);
  bg("st-wave-near", sea.near);

  document.body.appendChild(root);
  _root = root;

  // Start hidden: ~45% of figures, shuffled (spec §2) — the sky never opens
  // with all 20 on screen.
  _hidden.clear();
  const startHidden = CONSTELLATIONS.map((_, i) => i)
    .sort(() => Math.random() - 0.5)
    .slice(0, Math.round(CONSTELLATIONS.length * 0.45));
  startHidden.forEach((i) => setHidden(i, true));

  // Schedulers. Reduced motion keeps the moon home, the clouds at an ordinary
  // night and the sky static — final states, no motion (CLAUDE.md rule).
  applyMoonLeg(0, true);
  applyPow();
  if (!reduced()) {
    _litT = window.setTimeout(scheduleRelight, 2600 + Math.random() * 3400);
    _fadeT = window.setTimeout(scheduleFade, 4000 + Math.random() * 4000);
    _powT = window.setTimeout(schedulePow, 5500);
    _moonT = window.setTimeout(moonNext, MOON_PATH[0].hold);
  }

  // Press anywhere OUTSIDE the card (panels, sky, another figure handles its
  // own toggle) closes it. Capture phase, so panels cannot swallow it first.
  _outsideClose = (e: PointerEvent) => {
    if (_openIdx === null) return;
    const t = e.target as HTMLElement | null;
    if (t?.closest?.("[data-con-card]") || t?.closest?.("[data-con]")) return;
    closeCard();
  };
  document.addEventListener("pointerdown", _outsideClose, true);
}

export function destroyStarrySky(): void {
  closeCard(true);   // silent: the theme/fun switch that tore the sky down
                     // has already made its own sound
  window.clearTimeout(_litT);
  window.clearTimeout(_fadeT);
  window.clearTimeout(_powT);
  window.clearTimeout(_moonT);
  _litT = _fadeT = _powT = _moonT = undefined;
  _heaveStyle?.remove();
  _heaveStyle = null;
  if (_outsideClose) {
    document.removeEventListener("pointerdown", _outsideClose, true);
    _outsideClose = null;
  }
  _root?.remove();
  _root = null;
  _litIdx = -1;
  _moonLeg = 0;
  _hidden.clear();
}

/** Called from applyLook(): the scene exists exactly when Starry night AND
 *  Fun mode are both on. Fun off keeps the 1.0.57 star tile from themes.css —
 *  the owner's chosen default — with none of this. */
export function syncStarrySky(theme: string, fun: boolean): void {
  if (theme === "starry" && fun) buildStarrySky();
  else destroyStarrySky();
}
