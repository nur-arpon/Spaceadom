/**
 * starry-sky.ts — the full Starry night scene (PROBLEM 145).
 *
 * Spec: design/design-system-overhaul-3.md §5b, geometry and ocean markup
 * transcribed VERBATIM from the lab (`design system overhaul-v3.dc.html`) —
 * the CONS array, the wave tiles, the galleon and its crew are the lab's own,
 * not re-drawn (CLAUDE.md: the design is a specification, not a suggestion).
 *
 * GATING — the owner's final rule (2026-08-19): the 1.0.57 drifting star sky
 * is the DEFAULT for Starry night with Fun mode OFF. Fun ON adds everything
 * here: the twelve constellations sailing with the stars, the ocean, and the
 * pirate ship. So this module builds only under theme=starry AND fun=on, and
 * the palette + star tile live in themes.css unconditionally for the theme.
 *
 * STABILITY (PROBLEM 134's lesson, applied before it can bite):
 *  - This is DECORATION. It exists only in the dashboard webview; the overlay
 *    never loads this module.
 *  - Every animation is transform/background-position/opacity. The one
 *    `filter: drop-shadow` lights ONE constellation at a time, on a handful of
 *    3px circles — the dashboard already runs `blur(5px)` over the whole board
 *    while editing, so this is well inside precedent.
 *  - `body.is-blurred` (window unfocused/minimised) pauses every animation in
 *    the scene via CSS, and the lighting interval checks it too. For a tray
 *    app that is nearly always.
 *  - While a card is open the WHOLE sky freezes (`body.sky-frozen`), star
 *    tile included, per spec — and resumes the moment it closes.
 */

import { sfx } from "../sfx";
// The same 8 entrances the special-key cards use, in the same order — spec §4
// and §5b are explicit that they share one list.
import { CARD_ANIMS } from "./special-cards";

/** One constellation: scatter position in the 1400px sky cell (x px, y %),
 *  local star points, and which points join. Verbatim from the lab. */
interface Con {
  name: string;
  season: string;
  fact: string;
  pos: [number, number];
  pts: [number, number][];
  lines: [number, number][];
}

const CONS: Con[] = [
  { name: "Ursa Major", season: "Spring", fact: "The Great Bear. Its seven brightest stars are the Big Dipper — the two front bowl stars point straight to Polaris.", pos: [193, 50.5], pts: [[2,38],[20,30],[38,32],[54,40],[76,34],[86,54],[62,60]], lines: [[0,1],[1,2],[2,3],[3,4],[4,5],[5,6],[6,3]] },
  { name: "Cassiopeia", season: "Autumn", fact: "The vain queen on her throne — an unmistakable W of five stars, circling the pole opposite the Big Dipper.", pos: [778, 39.6], pts: [[2,30],[20,8],[40,26],[60,4],[78,32]], lines: [[0,1],[1,2],[2,3],[3,4]] },
  { name: "Cygnus", season: "Summer", fact: "The Swan, flying down the Milky Way. Its shape earns it the name Northern Cross; bright Deneb marks the tail.", pos: [370, 27.9], pts: [[44,2],[44,28],[44,54],[44,80],[12,40],[76,36]], lines: [[0,1],[1,2],[2,3],[4,1],[1,5]] },
  { name: "Scorpius", season: "Summer", fact: "The scorpion that felled Orion — a long J-shaped hook low on the horizon, with red Antares as its heart.", pos: [307, 11], pts: [[70,4],[60,14],[52,26],[50,42],[54,58],[64,68],[78,72],[88,64]], lines: [[0,1],[1,2],[2,3],[3,4],[4,5],[5,6],[6,7]] },
  { name: "Lyra", season: "Summer", fact: "The lyre of Orpheus. Tiny, but it holds Vega — one of the brightest stars in the whole sky.", pos: [138, 72.6], pts: [[30,4],[22,20],[40,24],[16,44],[34,48]], lines: [[0,1],[0,2],[1,2],[1,3],[2,4],[3,4]] },
  { name: "Ursa Minor", season: "All year", fact: "The Little Bear. Polaris, the North Star, sits at the tip of its tail and barely moves all night.", pos: [576, 49], pts: [[50,6],[42,18],[36,30],[28,40],[14,36],[10,48],[24,52]], lines: [[0,1],[1,2],[2,3],[3,4],[4,5],[5,6],[6,3]] },
  { name: "Orion", season: "Winter", fact: "The Hunter. Three belt stars in a perfect row; red Betelgeuse marks a shoulder, blue-white Rigel a foot.", pos: [1013, 55.6], pts: [[10,4],[54,10],[22,38],[32,44],[42,50],[6,84],[62,78]], lines: [[0,2],[1,4],[2,3],[3,4],[2,5],[4,6]] },
  { name: "Gemini", season: "Winter", fact: "The Twins. Castor and Pollux head two parallel chains of stars, side by side like stick figures.", pos: [130, 7.7], pts: [[14,6],[10,26],[8,48],[6,68],[40,4],[44,24],[48,46],[52,66]], lines: [[0,1],[1,2],[2,3],[4,5],[5,6],[6,7],[0,4]] },
  { name: "Taurus", season: "Winter", fact: "The Bull. A V of stars — the Hyades cluster — draws its face; orange Aldebaran is the bull's fiery eye.", pos: [1173, 60.1], pts: [[10,8],[26,30],[36,44],[48,34],[68,10]], lines: [[0,1],[1,2],[2,3],[3,4]] },
  { name: "Canis Major", season: "Winter", fact: "The Great Dog at Orion's heel — home of Sirius, the brightest star in Earth's night sky.", pos: [1003, 10.8], pts: [[46,6],[38,20],[46,34],[30,44],[52,48],[40,62],[28,74],[50,74]], lines: [[0,1],[1,2],[2,3],[2,4],[4,5],[5,6],[5,7]] },
  { name: "Leo", season: "Spring", fact: "The Lion. A backwards question mark — the Sickle — forms its mane, with Regulus at the heart.", pos: [1219, 16.8], pts: [[70,10],[58,4],[46,10],[44,24],[54,34],[74,36],[24,58],[6,64],[30,72]], lines: [[0,1],[1,2],[2,3],[3,4],[4,5],[5,8],[8,7],[7,6],[6,5]] },
  { name: "Pegasus", season: "Autumn", fact: "The winged horse. Its Great Square of four stars is autumn's landmark — star-hop outward from its corners.", pos: [1185, 34.9], pts: [[20,10],[60,8],[64,46],[24,50],[86,62]], lines: [[0,1],[1,2],[2,3],[3,0],[2,4]] },
];

/** The 8 card entrances, cycled by constellation index (spec §4/§5b). */
const SVG_NS = "http://www.w3.org/2000/svg";
const LIGHT_EVERY_MS = 4200;

let _root: HTMLDivElement | null = null;
let _card: HTMLDivElement | null = null;
let _openIdx: number | null = null;
let _litIdx = -1;
let _litTimer: number | undefined;
let _outsideClose: ((e: PointerEvent) => void) | null = null;

function reduced(): boolean {
  return document.documentElement.classList.contains("reduced-motion");
}

/** One constellation SVG. Rendering semantics are the lab's, line for line:
 *  bbox +8, a 22px-overhung hit rect, r=16 halo circles, hairline links,
 *  stars r 1.4 (key star 1.9). */
function buildCon(c: Con, i: number): SVGSVGElement {
  const w = Math.max(...c.pts.map((p) => p[0])) + 8;
  const h = Math.max(...c.pts.map((p) => p[1])) + 8;
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("width", String(w));
  svg.setAttribute("height", String(h));
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
    star.setAttribute("r", k === 0 ? "1.9" : "1.4");
    star.setAttribute("fill", "#fff");
    star.classList.add("sky-con-star");
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

/** The info card, FIXED and clamped to the viewport, centred under the
 *  figure's live (mid-drift) position. Entrance cycles the 8 animations. */
function openCard(i: number, svg: SVGSVGElement): void {
  closeCard(true);           // silent: the entrance below is the sound here
  const c = CONS[i];
  const r = svg.getBoundingClientRect();
  const vw = window.innerWidth || 1240;
  const vh = window.innerHeight || 1000;
  const x = Math.round(Math.min(Math.max(r.left + r.width / 2, 118), vw - 118));
  const y = Math.round(Math.min(r.bottom + 12, vh - 160));

  const card = document.createElement("div");
  card.dataset.conCard = "1";
  card.className = "sky-card";
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

/** Every LIGHT_EVERY_MS one random figure lights up. Never the same one twice
 *  in a row, never while hidden/unfocused (the interval keeps ticking but the
 *  work is skipped — cheaper than tearing the timer up and down on focus). */
function tickLighting(): void {
  if (!_root || document.hidden || document.body.classList.contains("is-blurred")) return;
  let i = Math.floor(Math.random() * CONS.length);
  if (i === _litIdx) i = (i + 1) % CONS.length;
  _root.querySelectorAll(".sky-con.is-lit").forEach((s) => s.classList.remove("is-lit"));
  _root.querySelectorAll(`[data-con="${i}"]`).forEach((s) => s.classList.add("is-lit"));
  _litIdx = i;
}

/** Build the whole scene. Idempotent: a second call while built is a no-op. */
export function buildStarrySky(): void {
  if (_root) return;

  const root = document.createElement("div");
  root.id = "st-sky";
  root.setAttribute("aria-hidden", "true");

  // Constellation layer: THREE side-by-side copies of the 1400px cell, each on
  // its own conDrift keyframe, so every figure sails across with the star tile
  // and comes around again seamlessly (spec: identical 180s period).
  for (let copy = 0; copy < 3; copy++) {
    const layer = document.createElement("div");
    layer.className = "sky-con-layer";
    layer.style.animationName = `sky-conDrift${copy}`;
    CONS.forEach((c, i) => {
      const wrap = document.createElement("div");
      wrap.className = "sky-con-wrap";
      wrap.style.left = `${c.pos[0]}px`;
      wrap.style.top = `${c.pos[1]}%`;
      wrap.appendChild(buildCon(c, i));
      layer.appendChild(wrap);
    });
    root.appendChild(layer);
  }

  // The ocean: water, horizon glow, three parallax wave strips, star
  // reflections, the black galleon with her nine crew, and two gulls.
  // Transcribed verbatim from the lab; OCEAN_HTML is injected at build time
  // from the extracted file so it can never drift from the design by retyping.
  const ocean = document.createElement("div");
  ocean.className = "sky-ocean";
  ocean.innerHTML = OCEAN_HTML;
  root.appendChild(ocean);

  document.body.appendChild(root);
  _root = root;

  _litTimer = window.setInterval(tickLighting, LIGHT_EVERY_MS);
  tickLighting();

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

  window.clearInterval(_litTimer);
  _litTimer = undefined;
  if (_outsideClose) {
    document.removeEventListener("pointerdown", _outsideClose, true);
    _outsideClose = null;
  }
  _root?.remove();
  _root = null;
  _litIdx = -1;
}

/** Called from applyLook(): the scene exists exactly when Starry night AND
 *  Fun mode are both on. Fun off keeps the 1.0.57 star tile from themes.css —
 *  the owner's chosen default — with none of this. */
export function syncStarrySky(theme: string, fun: boolean): void {
  if (theme === "starry" && fun) buildStarrySky();
  else destroyStarrySky();
}

// Verbatim lab markup (design system overhaul-v3.dc.html), band root only
// re-anchored from fixed to absolute inside .sky-ocean.
const OCEAN_HTML = `<div style="position:absolute;inset:0;pointer-events:none">
      <div style="position:absolute;left:0;right:0;top:26px;bottom:0;background:linear-gradient(180deg,#0b1226 0%,#070d1d 55%,#04070f 100%)"></div>
      <div style="position:absolute;left:0;right:0;top:0;height:27px;background:radial-gradient(340px 34px at 50% 100%,rgba(190,210,255,.16),transparent 70%),linear-gradient(180deg,rgba(140,165,220,0),rgba(140,165,220,.12))"></div>
      <span style="position:absolute;left:22%;top:70px;width:2px;height:13px;background:linear-gradient(180deg,rgba(214,226,255,.55),transparent);animation:twinkle 2.3s ease-in-out infinite alternate"></span>
      <span style="position:absolute;left:57%;top:96px;width:2px;height:9px;background:linear-gradient(180deg,rgba(214,226,255,.4),transparent);animation:twinkle 1.7s ease-in-out .5s infinite alternate"></span>
      <span style="position:absolute;left:81%;top:64px;width:2px;height:15px;background:linear-gradient(180deg,rgba(214,226,255,.5),transparent);animation:twinkle 2.8s ease-in-out .9s infinite alternate"></span>
      <div style="position:absolute;left:0;right:0;top:14px;height:46px;background:url(data:image/svg+xml,%3Csvg%20xmlns%3D%27http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%27%20width%3D%27520%27%20height%3D%2746%27%3E%3Cdefs%3E%3ClinearGradient%20id%3D%27a%27%20x1%3D%270%27%20y1%3D%270%27%20x2%3D%270%27%20y2%3D%271%27%3E%3Cstop%20offset%3D%270%27%20stop-color%3D%27%23101c3a%27%2F%3E%3Cstop%20offset%3D%271%27%20stop-color%3D%27%230a1226%27%2F%3E%3C%2FlinearGradient%3E%3C%2Fdefs%3E%3Cpath%20d%3D%27M0%2030%20C%2010.8%2028.5%2021.1%2021.8%2027.1%2021.8%20C%2039.4%2022.2%2054.2%2031.4%2068.1%2031.4%20C%2084.8%2029.1%20100.6%2025.2%20109.8%2025.2%20C%20120.8%2025.4%20134.1%2031.4%20146.6%2031.4%20C%20158.5%2029.2%20169.8%2025.4%20176.3%2025.4%20C%20192.9%2025.7%20212.8%2030.2%20231.5%2030.2%20C%20244.9%2028.9%20257.6%2023.7%20265.0%2023.7%20C%20275.0%2024.0%20287.1%2030.9%20298.5%2030.9%20C%20309.1%2029.2%20319.2%2025.6%20325.1%2025.6%20C%20339.1%2025.9%20355.9%2030.6%20371.7%2030.6%20C%20390.8%2028.6%20408.9%2022.3%20419.4%2022.3%20C%20431.6%2022.7%20446.4%2031.8%20460.3%2031.8%20C%20473.8%2028.6%20486.7%2022.4%20494.2%2022.4%20C%20501.9%2022.7%20511.2%2032.2%20520.0%2030.0%20L520%2046%20L0%2046%20Z%27%20fill%3D%27url%28%2523a%29%27%2F%3E%3Cpath%20d%3D%27M0%2030%20C%2010.8%2028.5%2021.1%2021.8%2027.1%2021.8%20C%2039.4%2022.2%2054.2%2031.4%2068.1%2031.4%20C%2084.8%2029.1%20100.6%2025.2%20109.8%2025.2%20C%20120.8%2025.4%20134.1%2031.4%20146.6%2031.4%20C%20158.5%2029.2%20169.8%2025.4%20176.3%2025.4%20C%20192.9%2025.7%20212.8%2030.2%20231.5%2030.2%20C%20244.9%2028.9%20257.6%2023.7%20265.0%2023.7%20C%20275.0%2024.0%20287.1%2030.9%20298.5%2030.9%20C%20309.1%2029.2%20319.2%2025.6%20325.1%2025.6%20C%20339.1%2025.9%20355.9%2030.6%20371.7%2030.6%20C%20390.8%2028.6%20408.9%2022.3%20419.4%2022.3%20C%20431.6%2022.7%20446.4%2031.8%20460.3%2031.8%20C%20473.8%2028.6%20486.7%2022.4%20494.2%2022.4%20C%20501.9%2022.7%20511.2%2032.2%20520.0%2030.0%27%20fill%3D%27none%27%20stroke%3D%27rgba%28150%2C180%2C235%2C.16%29%27%20stroke-width%3D%270.7%27%20stroke-linecap%3D%27round%27%2F%3E%3C%2Fsvg%3E) repeat-x;background-size:520px 46px;animation:waveA 11s linear infinite,waveBob 4.6s ease-in-out infinite alternate"></div>
      <div style="position:absolute;left:0;right:0;top:30px;height:60px;background:url(data:image/svg+xml,%3Csvg%20xmlns%3D%27http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%27%20width%3D%27520%27%20height%3D%2760%27%3E%3Cdefs%3E%3ClinearGradient%20id%3D%27b%27%20x1%3D%270%27%20y1%3D%270%27%20x2%3D%270%27%20y2%3D%271%27%3E%3Cstop%20offset%3D%270%27%20stop-color%3D%27%23132244%27%2F%3E%3Cstop%20offset%3D%271%27%20stop-color%3D%27%230a1226%27%2F%3E%3C%2FlinearGradient%3E%3C%2Fdefs%3E%3Cpath%20d%3D%27M0%2034%20C%2018.2%2031.3%2035.6%2019.3%2045.6%2019.3%20C%2055.0%2020.0%2066.2%2034.3%2076.8%2034.3%20C%2098.1%2032.6%20118.3%2026.1%20130.0%2026.1%20C%20152.3%2026.5%20179.0%2034.1%20204.2%2034.1%20C%20222.9%2031.2%20240.7%2018.2%20251.0%2018.2%20C%20261.0%2019.0%20273.1%2035.7%20284.4%2035.7%20C%20308.1%2032.4%20330.5%2025.1%20343.5%2025.1%20C%20354.9%2025.5%20368.6%2035.6%20381.5%2035.6%20C%20400.1%2031.8%20417.9%2022.0%20428.1%2022.0%20C%20436.0%2022.6%20445.5%2037.3%20454.4%2037.3%20C%20470.1%2031.8%20485.0%2021.9%20493.7%2021.9%20C%20501.6%2022.5%20511.0%2035.0%20520.0%2034.0%20L520%2060%20L0%2060%20Z%27%20fill%3D%27url%28%2523b%29%27%2F%3E%3Cpath%20d%3D%27M0%2034%20C%2018.2%2031.3%2035.6%2019.3%2045.6%2019.3%20C%2055.0%2020.0%2066.2%2034.3%2076.8%2034.3%20C%2098.1%2032.6%20118.3%2026.1%20130.0%2026.1%20C%20152.3%2026.5%20179.0%2034.1%20204.2%2034.1%20C%20222.9%2031.2%20240.7%2018.2%20251.0%2018.2%20C%20261.0%2019.0%20273.1%2035.7%20284.4%2035.7%20C%20308.1%2032.4%20330.5%2025.1%20343.5%2025.1%20C%20354.9%2025.5%20368.6%2035.6%20381.5%2035.6%20C%20400.1%2031.8%20417.9%2022.0%20428.1%2022.0%20C%20436.0%2022.6%20445.5%2037.3%20454.4%2037.3%20C%20470.1%2031.8%20485.0%2021.9%20493.7%2021.9%20C%20501.6%2022.5%20511.0%2035.0%20520.0%2034.0%27%20fill%3D%27none%27%20stroke%3D%27rgba%28160%2C190%2C240%2C.22%29%27%20stroke-width%3D%270.9%27%20stroke-linecap%3D%27round%27%2F%3E%3Cellipse%20cx%3D%2745.6%27%20cy%3D%2720.5%27%20rx%3D%277.4%27%20ry%3D%271.1%27%20fill%3D%27rgba%28205%2C225%2C255%2C0.19%29%27%2F%3E%3Ccircle%20cx%3D%2754.3%27%20cy%3D%2714.6%27%20r%3D%270.6%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.23%29%27%2F%3E%3Ccircle%20cx%3D%2740.5%27%20cy%3D%2717.5%27%20r%3D%270.6%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.54%29%27%2F%3E%3Ccircle%20cx%3D%2740.5%27%20cy%3D%2715.6%27%20r%3D%270.8%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.23%29%27%2F%3E%3Cpath%20d%3D%27M35.3%2022.7%20q%2010.3%201.6%2020.6%200%27%20fill%3D%27none%27%20stroke%3D%27rgba%28190%2C214%2C255%2C.13%29%27%20stroke-width%3D%27.7%27%2F%3E%3Cellipse%20cx%3D%27251.0%27%20cy%3D%2719.4%27%20rx%3D%277.9%27%20ry%3D%271.1%27%20fill%3D%27rgba%28205%2C225%2C255%2C0.27%29%27%2F%3E%3Ccircle%20cx%3D%27251.4%27%20cy%3D%2714.9%27%20r%3D%270.6%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.40%29%27%2F%3E%3Ccircle%20cx%3D%27244.0%27%20cy%3D%2716.9%27%20r%3D%270.8%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.35%29%27%2F%3E%3Ccircle%20cx%3D%27260.8%27%20cy%3D%2712.8%27%20r%3D%270.8%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.40%29%27%2F%3E%3Cpath%20d%3D%27M240.0%2021.6%20q%2011.0%201.6%2022.1%200%27%20fill%3D%27none%27%20stroke%3D%27rgba%28190%2C214%2C255%2C.13%29%27%20stroke-width%3D%27.7%27%2F%3E%3Cellipse%20cx%3D%27428.1%27%20cy%3D%2723.2%27%20rx%3D%276.0%27%20ry%3D%271.1%27%20fill%3D%27rgba%28205%2C225%2C255%2C0.24%29%27%2F%3E%3Ccircle%20cx%3D%27436.9%27%20cy%3D%2721.5%27%20r%3D%270.8%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.44%29%27%2F%3E%3Ccircle%20cx%3D%27422.5%27%20cy%3D%2720.6%27%20r%3D%270.9%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.44%29%27%2F%3E%3Ccircle%20cx%3D%27430.9%27%20cy%3D%2721.8%27%20r%3D%270.8%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.31%29%27%2F%3E%3Cpath%20d%3D%27M419.7%2025.4%20q%208.4%201.6%2016.8%200%27%20fill%3D%27none%27%20stroke%3D%27rgba%28190%2C214%2C255%2C.13%29%27%20stroke-width%3D%27.7%27%2F%3E%3Cellipse%20cx%3D%27493.7%27%20cy%3D%2723.1%27%20rx%3D%276.1%27%20ry%3D%271.1%27%20fill%3D%27rgba%28205%2C225%2C255%2C0.34%29%27%2F%3E%3Ccircle%20cx%3D%27488.1%27%20cy%3D%2719.5%27%20r%3D%270.7%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.44%29%27%2F%3E%3Ccircle%20cx%3D%27498.0%27%20cy%3D%2720.2%27%20r%3D%270.5%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.35%29%27%2F%3E%3Ccircle%20cx%3D%27491.7%27%20cy%3D%2720.9%27%20r%3D%270.7%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.37%29%27%2F%3E%3Cpath%20d%3D%27M485.2%2025.3%20q%208.5%201.6%2017.0%200%27%20fill%3D%27none%27%20stroke%3D%27rgba%28190%2C214%2C255%2C.13%29%27%20stroke-width%3D%27.7%27%2F%3E%3C%2Fsvg%3E) repeat-x;background-size:520px 60px;animation:waveB 8s linear infinite,waveBob 3.8s ease-in-out .6s infinite alternate"></div>
      <div style="position:absolute;left:-24px;bottom:40px;animation:shipBob 3.8s ease-in-out infinite alternate">
        <svg width="272" height="196" viewBox="0 0 250 180" style="transform:perspective(650px) rotateY(13deg) rotate(-15deg);transform-origin:40% 78%">
          <path d="M16 96 L18 74 L46 70 L48 100 Z" fill="#04060f" stroke="rgba(155,180,232,.26)" stroke-width=".8"></path>
          <path d="M14 120 C 16 146 46 156 110 156 C 170 156 210 145 226 114 L 244 100 L 208 118 C 186 136 68 140 36 124 Z" fill="#04060f" stroke="rgba(155,180,232,.26)" stroke-width="1"></path>
          <path d="M34 122 C 70 134 182 130 206 116" fill="none" stroke="rgba(155,180,232,.14)" stroke-width=".7"></path>
          <path d="M52 124 L 82 48 M100 130 L 124 24 M144 128 L 166 52 M82 48 L 20 92 M166 52 L 238 100 M124 24 L 84 46 M124 24 L 164 50" stroke="rgba(150,175,225,.13)" stroke-width=".6" fill="none"></path>
          <path d="M80 136 L 83 44" stroke="#04060f" stroke-width="2.4"></path>
          <path d="M122 140 L 126 22" stroke="#04060f" stroke-width="2.6"></path>
          <path d="M164 134 L 167 50" stroke="#04060f" stroke-width="2.2"></path>
          <path d="M64 56 Q 83 66 102 54 L 100 84 L 94 78 L 88 87 L 81 80 L 74 89 L 67 82 Z" fill="#05070f" stroke="rgba(155,180,232,.26)" stroke-width=".8"></path>
          <path d="M62 94 Q 83 105 104 92 L 102 124 L 95 117 L 88 126 L 81 118 L 74 127 L 66 119 Z" fill="#05070f" stroke="rgba(155,180,232,.26)" stroke-width=".8"></path>
          <path d="M106 32 Q 126 45 146 30 L 144 64 L 137 57 L 130 66 L 123 58 L 116 67 L 108 59 Z" fill="#05070f" stroke="rgba(155,180,232,.26)" stroke-width=".8"></path>
          <path d="M104 72 Q 126 85 148 70 L 146 106 L 139 98 L 132 107 L 125 99 L 118 108 L 110 100 Z" fill="#05070f" stroke="rgba(155,180,232,.26)" stroke-width=".8"></path>
          <path d="M150 56 Q 166 67 182 54 L 180 88 L 173 81 L 166 90 L 159 82 Z" fill="#05070f" stroke="rgba(155,180,232,.26)" stroke-width=".8"></path>
          <path d="M226 114 L 248 94" stroke="#04060f" stroke-width="2.6"></path>
          <path d="M224 108 L 244 96 L 228 116 Z" fill="#05070f"></path>
          <path d="M126 10 L 166 17 L 159 24 L 166 31 L 126 38 Z" fill="#030510" stroke="rgba(175,200,248,.3)" stroke-width=".7"></path>
          <circle cx="141" cy="22" r="3.6" fill="#e8ecf7"></circle>
          <circle cx="139.5" cy="21.1" r="1" fill="#030510"></circle>
          <circle cx="142.5" cy="21.1" r="1" fill="#030510"></circle>
          <path d="M137.6 25.4 L 144.4 26.6 M137.6 26.6 L 144.4 25.4" stroke="#030510" stroke-width=".8"></path>
          <path d="M134 30.5 L 148 33 M148 30.5 L 134 33" stroke="#e8ecf7" stroke-width="1.3"></path>
          <rect x="70" y="127" width="7" height="5" fill="#0b1226" stroke="rgba(155,180,232,.26)" stroke-width=".5"></rect>
          <path d="M64 131 L 76 130 M96 135 L 108 134 M126 137 L 138 136 M152 133 L 164 132" stroke="#1a2138" stroke-width="3.2" stroke-linecap="round"></path>
          <circle cx="64" cy="131" r="3.4" fill="rgba(255,120,55,.22)"></circle>
          <circle cx="64" cy="131" r="1.2" fill="#ff7a3c" opacity=".9"></circle>
          <circle cx="58" cy="129" r="5.6" fill="rgba(200,210,230,.07)"></circle>
          <circle cx="50" cy="126" r="4.2" fill="rgba(200,210,230,.05)"></circle>
          <circle cx="28" cy="110" r="4.6" fill="rgba(255,130,60,.2)"></circle>
          <circle cx="28" cy="110" r="1.7" fill="#ffab6a" opacity=".9"></circle>
          <circle cx='30' cy='82' r='2.415' fill='#04060f'/><path d='M25.17 80.005 Q 30 75.49 34.83 80.005 L 33.57 78.955 Q 30 77.17 26.43 78.955 Z' fill='#04060f'/><path d='M28.005 84.52 q 1.9949999999999999 -1.05 3.9899999999999998 0 L 34.620000000000005 91.03 q -4.620000000000001 1.5750000000000002 -9.240000000000002 0 Z' fill='#04060f'/><path d='M28.53 91.03 L 27.06 97.96 M31.47 91.03 L 32.94 97.96' stroke='#04060f' stroke-width='1.9949999999999999' fill='none'/>
          <path d="M31 85 L 45 76" stroke="#04060f" stroke-width="1.9"></path>
          <path d="M45 76 L 58 68" stroke="#101c3a" stroke-width="3.3"></path>
          <circle cx="59" cy="67.4" r="1.2" fill="#d6e2ff" opacity=".9"></circle>
          <circle cx="72" cy="118" r="5" fill="none" stroke="#04060f" stroke-width="1.7"></circle>
          <path d="M72 113 L 72 123 M67 118 L 77 118 M68.5 114.5 L 75.5 121.5 M75.5 114.5 L 68.5 121.5" stroke="#04060f" stroke-width=".9"></path>
          <circle cx='62' cy='106' r='2.1849999999999996' fill='#04060f'/><path d='M59.53 104.67 q 2.4699999999999998 -1.52 4.9399999999999995 0 l 1.805 1.045 l -2.09 0.38 Z' fill='#04060f'/><path d='M60.195 108.28 q 1.805 -0.95 3.61 0 L 66.18 114.17 q -4.18 1.4249999999999998 -8.36 0 Z' fill='#04060f'/><path d='M60.67 114.17 L 59.34 120.44 M63.33 114.17 L 64.66 120.44' stroke='#04060f' stroke-width='1.805' fill='none'/>
          <path d="M63.5 110 L 70 115" stroke="#04060f" stroke-width="1.7"></path>
          <circle cx='98' cy='118' r='2.1849999999999996' fill='#04060f'/><path d='M95.53 116.67 q 2.4699999999999998 -1.52 4.9399999999999995 0 l 1.805 1.045 l -2.09 0.38 Z' fill='#04060f'/><path d='M96.195 120.28 q 1.805 -0.95 3.61 0 L 102.18 126.17 q -4.18 1.4249999999999998 -8.36 0 Z' fill='#04060f'/><path d='M96.67 126.17 L 95.34 132.44 M99.33 126.17 L 100.66 132.44' stroke='#04060f' stroke-width='1.805' fill='none'/>
          <path d="M99 121 L 108 112" stroke="#04060f" stroke-width="1.7"></path>
          <path d="M108 112 L 116 100" stroke="#101c3a" stroke-width="1.9"></path>
          <circle cx="116.4" cy="99.6" r="1" fill="#d6e2ff" opacity=".85"></circle>
          <circle cx='130' cy='124' r='2.07' fill='#04060f'/><path d='M127.66 122.74 q 2.3400000000000003 -1.4400000000000002 4.680000000000001 0 l 1.71 0.9900000000000001 l -1.9800000000000002 0.36000000000000004 Z' fill='#04060f'/><path d='M128.29 126.16 q 1.71 -0.9 3.42 0 L 133.96 131.74 q -3.9600000000000004 1.35 -7.920000000000001 0 Z' fill='#04060f'/><path d='M128.74 131.74 L 127.48 137.68 M131.26 131.74 L 132.52 137.68' stroke='#04060f' stroke-width='1.71' fill='none'/>
          <path d="M131 127 L 141 130" stroke="#04060f" stroke-width="1.6"></path>
          <path d="M141 130 L 152 133" stroke="#1c2438" stroke-width="1.4"></path>
          <circle cx='158' cy='120' r='2.07' fill='#04060f'/><path d='M153.86 118.29 Q 158 114.42 162.14 118.29 L 161.06 117.39 Q 158 115.86 154.94 117.39 Z' fill='#04060f'/><path d='M156.29 122.16 q 1.71 -0.9 3.42 0 L 161.96 127.74 q -3.9600000000000004 1.35 -7.920000000000001 0 Z' fill='#04060f'/><path d='M156.74 127.74 L 155.48 133.68 M159.26 127.74 L 160.52 133.68' stroke='#04060f' stroke-width='1.71' fill='none'/>
          <path d="M159 123 L 168 116 L 172 104" stroke="#04060f" stroke-width="1.5" fill="none"></path>
          <circle cx='190' cy='112' r='2.07' fill='#04060f'/><path d='M185.86 110.29 Q 190 106.42 194.14 110.29 L 193.06 109.39 Q 190 107.86 186.94 109.39 Z' fill='#04060f'/><path d='M188.29 114.16 q 1.71 -0.9 3.42 0 L 193.96 119.74 q -3.9600000000000004 1.35 -7.920000000000001 0 Z' fill='#04060f'/><path d='M188.74 119.74 L 187.66 125.5' stroke='#04060f' stroke-width='1.71' fill='none'/><path d='M191.26 119.74 L 192.16 124.6 l -0.18000000000000002 0.9' stroke='#04060f' stroke-width='1.08' fill='none'/>
          <path d="M191 115 L 200 106" stroke="#04060f" stroke-width="1.6"></path>
          <circle cx="201" cy="105" r="1" fill="#d6e2ff" opacity=".7"></circle>
          <path d="M75 46 L 91 46" stroke="#04060f" stroke-width="2.6"></path>
          <circle cx='83' cy='38' r='1.8399999999999999' fill='#04060f'/><path d='M80.92 36.88 q 2.08 -1.2800000000000002 4.16 0 l 1.52 0.8800000000000001 l -1.7600000000000002 0.32000000000000006 Z' fill='#04060f'/><path d='M81.48 39.92 q 1.52 -0.8 3.04 0 L 86.52 44.88 q -3.5200000000000005 1.2000000000000002 -7.040000000000001 0 Z' fill='#04060f'/><path d='M81.88 44.88 L 80.76 50.16 M84.12 44.88 L 85.24 50.16' stroke='#04060f' stroke-width='1.52' fill='none'/>
          <path d="M84 41 L 91 36" stroke="#101c3a" stroke-width="2.1"></path>
          <circle cx="91.8" cy="35.6" r=".9" fill="#d6e2ff" opacity=".85"></circle>
          <circle cx="226" cy="96" r="2.2" fill="#04060f"></circle>
          <path d="M226 98 L 225 105 M225 105 L 221 111 M225 105 L 228 112" stroke="#04060f" stroke-width="1.7" fill="none"></path>
          <g style="animation:birdFly 5.5s ease-in-out infinite alternate">
            <path d="M200 44 q 7 -6 13 -1 q 6 -5 13 1" fill="none" stroke="#04060f" stroke-width="1.7" stroke-linecap="round"></path>
            <path d="M209 48 q 4 -3 8 -1" fill="none" stroke="#04060f" stroke-width="1.2" stroke-linecap="round"></path>
          </g>
          <g style="animation:birdFly 7s ease-in-out .8s infinite alternate">
            <path d="M172 26 q 5 -4 9 -.6 q 4 -3.6 9 .6" fill="none" stroke="#04060f" stroke-width="1.3" stroke-linecap="round"></path>
          </g>
        </svg>
      </div>
      <div style="position:absolute;left:0;right:0;bottom:0;height:132px;background:url(data:image/svg+xml,%3Csvg%20xmlns%3D%27http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%27%20width%3D%27520%27%20height%3D%27132%27%3E%3Cdefs%3E%3ClinearGradient%20id%3D%27c%27%20x1%3D%270%27%20y1%3D%270%27%20x2%3D%270%27%20y2%3D%271%27%3E%3Cstop%20offset%3D%270%27%20stop-color%3D%27%230c1730%27%2F%3E%3Cstop%20offset%3D%271%27%20stop-color%3D%27%2303060e%27%2F%3E%3C%2FlinearGradient%3E%3C%2Fdefs%3E%3Cpath%20d%3D%27M0%2040%20C%2027.4%2036.3%2053.5%2019.3%2068.6%2019.3%20C%2083.7%2020.4%20101.9%2044.1%20119.0%2044.1%20C%20156.0%2035.7%20191.1%2016.4%20211.4%2016.4%20C%20228.3%2017.6%20248.4%2044.8%20267.5%2044.8%20C%20295.4%2037.8%20321.9%2027.6%20337.3%2027.6%20C%20357.7%2028.2%20382.2%2042.5%20405.3%2042.5%20C%20429.8%2036.3%20453.0%2019.2%20466.5%2019.2%20C%20482.6%2020.2%20501.8%2041.2%20520.0%2040.0%20L520%20132%20L0%20132%20Z%27%20fill%3D%27url%28%2523c%29%27%2F%3E%3Cpath%20d%3D%27M0%2040%20C%2027.4%2036.3%2053.5%2019.3%2068.6%2019.3%20C%2083.7%2020.4%20101.9%2044.1%20119.0%2044.1%20C%20156.0%2035.7%20191.1%2016.4%20211.4%2016.4%20C%20228.3%2017.6%20248.4%2044.8%20267.5%2044.8%20C%20295.4%2037.8%20321.9%2027.6%20337.3%2027.6%20C%20357.7%2028.2%20382.2%2042.5%20405.3%2042.5%20C%20429.8%2036.3%20453.0%2019.2%20466.5%2019.2%20C%20482.6%2020.2%20501.8%2041.2%20520.0%2040.0%27%20fill%3D%27none%27%20stroke%3D%27rgba%28185%2C210%2C252%2C.4%29%27%20stroke-width%3D%271.1%27%20stroke-linecap%3D%27round%27%2F%3E%3Cellipse%20cx%3D%2768.6%27%20cy%3D%2720.5%27%20rx%3D%2710.3%27%20ry%3D%271.1%27%20fill%3D%27rgba%28205%2C225%2C255%2C0.26%29%27%2F%3E%3Ccircle%20cx%3D%2777.9%27%20cy%3D%2712.4%27%20r%3D%270.8%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.37%29%27%2F%3E%3Ccircle%20cx%3D%2754.7%27%20cy%3D%2718.2%27%20r%3D%270.9%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.21%29%27%2F%3E%3Ccircle%20cx%3D%2773.5%27%20cy%3D%2718.0%27%20r%3D%271.0%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.40%29%27%2F%3E%3Cpath%20d%3D%27M54.1%2022.7%20q%2014.5%201.6%2028.9%200%27%20fill%3D%27none%27%20stroke%3D%27rgba%28190%2C214%2C255%2C.13%29%27%20stroke-width%3D%27.7%27%2F%3E%3Cellipse%20cx%3D%27211.4%27%20cy%3D%2717.6%27%20rx%3D%2711.8%27%20ry%3D%271.1%27%20fill%3D%27rgba%28205%2C225%2C255%2C0.32%29%27%2F%3E%3Ccircle%20cx%3D%27208.0%27%20cy%3D%277.4%27%20r%3D%270.8%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.44%29%27%2F%3E%3Ccircle%20cx%3D%27195.2%27%20cy%3D%2716.0%27%20r%3D%270.9%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.48%29%27%2F%3E%3Ccircle%20cx%3D%27211.6%27%20cy%3D%275.9%27%20r%3D%270.9%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.38%29%27%2F%3E%3Cpath%20d%3D%27M194.9%2019.8%20q%2016.5%201.6%2033.1%200%27%20fill%3D%27none%27%20stroke%3D%27rgba%28190%2C214%2C255%2C.13%29%27%20stroke-width%3D%27.7%27%2F%3E%3Cellipse%20cx%3D%27466.5%27%20cy%3D%2720.4%27%20rx%3D%2710.4%27%20ry%3D%271.1%27%20fill%3D%27rgba%28205%2C225%2C255%2C0.29%29%27%2F%3E%3Ccircle%20cx%3D%27466.0%27%20cy%3D%2715.0%27%20r%3D%271.0%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.30%29%27%2F%3E%3Ccircle%20cx%3D%27461.5%27%20cy%3D%2719.0%27%20r%3D%270.7%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.29%29%27%2F%3E%3Ccircle%20cx%3D%27461.8%27%20cy%3D%2714.4%27%20r%3D%270.6%27%20fill%3D%27rgba%28215%2C232%2C255%2C0.42%29%27%2F%3E%3Cpath%20d%3D%27M451.9%2022.6%20q%2014.6%201.6%2029.2%200%27%20fill%3D%27none%27%20stroke%3D%27rgba%28190%2C214%2C255%2C.13%29%27%20stroke-width%3D%27.7%27%2F%3E%3C%2Fsvg%3E) repeat-x;background-size:520px 132px;animation:waveC 6s linear infinite,waveBob 3.2s ease-in-out .3s infinite alternate"></div>
    </div>`;

