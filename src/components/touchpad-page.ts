/**
 * touchpad-page.ts — the Touchpad edge-gestures page (1.0.120, TOUCHPAD-BRIEF-T2).
 *
 * Transcribed from design/spaceadom-touchpad_1.html (six screens), with the
 * two-finger copy/demo changed to ONE finger per the T1b hardware result.
 * A LEAF module: it imports only types and the pure `vk-names` labels, so
 * `preview.ts` can render it. The host (main.ts) supplies config access, a
 * save, a close, the Windows touchpad-settings opener and the chord recorder
 * (the key editor's `chord_record_start/poll/stop`); live values arrive
 * through `setTouchpadLive`.
 *
 * 1.0.122 (owner, 2026-09-19 17:30): the bottom is a normal edge like the
 * other three (nothing is held back for a future version), and every
 * edge offers Brightness · Volume · Video scrub · Any shortcut · Nothing.
 * "Any shortcut" records two chords (slide up/right, slide down/left) with
 * the SAME recorder UI as the key editor's Key combo.
 *
 * Screens, all one component driven by state:
 *   1  default / first-run (the one-finger demo + the invitation card)
 *   2  a band selected (Does what · width/length · sensitivity · flip · drag)
 *   3  live test (the readout card + "The pointer is holding still")
 *   4  not a Precision Touchpad (the unavailable state)
 *   5a/5b  corner ownership (asked when a second overlapping band turns on)
 */

import type {
  AppConfig,
  Band,
  BandAction,
  BandActionKind,
  BandChords,
  TouchEdge,
  Touchpad,
  TouchpadLive,
  TouchpadPresence,
} from "../types";
import { chordLabel, vkLabel } from "./vk-names";

// --- host + module state ----------------------------------------------------

/** The key editor's chord recorder, behind `chord_record_start/poll/stop`. */
export interface ChordRecorderHost {
  start(): Promise<void>;
  /** The chord as of the last key-down, in press order (empty = nothing yet). */
  poll(): Promise<number[]>;
  stop(): Promise<void>;
}

export interface TouchpadPageHost {
  getConfig(): AppConfig | null;
  save(): void | Promise<void>;
  onClose(): void;
  openWindowsTouchpadSettings(): void;
  /** Absent (a host with no hook) = the record buttons do nothing. */
  recorder?: ChordRecorderHost;
}

type ChordDir = "forward" | "backward";
/** A recording in progress: which band, which direction, the poll timer. */
let recording: { edge: TouchEdge; dir: ChordDir; timer: number; polls: number } | null = null;
/** 100 ms polls; the hook disarms itself past RECORD_MAX_MS anyway (same as
 *  the key editor's RECORD_POLLS). */
const RECORD_POLLS = 150;

let root: HTMLElement | null = null;
let host: TouchpadPageHost | null = null;
let presence: TouchpadPresence = "none";
let delegated = false;
let selected: TouchEdge | null = null;
let live: TouchpadLive | null = null;
let showDemo = false;
/** A pending corner question: which two edges, and the just-enabled one. */
let cornerAsk: { vertical: TouchEdge; horizontal: TouchEdge; justEnabled: TouchEdge } | null = null;

const PAD_W = 780;

// --- defaults (mirror config/schema.rs, so a missing field never crashes) ---

export function defaultBand(edge: TouchEdge): Band {
  const action: BandAction =
    edge === "left" ? "brightness" : edge === "right" ? "volume" : edge === "top" ? "scrub" : "none";
  return {
    enabled: false,
    action,
    width: 0.07,
    length: edge === "top" || edge === "bottom" ? 0.8 : 0.7,
    sensitivity: 6,
    invert: false,
  };
}

export function defaultTouchpad(): Touchpad {
  return {
    left: defaultBand("left"),
    right: defaultBand("right"),
    top: defaultBand("top"),
    bottom: defaultBand("bottom"),
    corner_rule: "ask",
    corners: { tl: null, tr: null, bl: null, br: null },
    page_look: "chocolate",
    demo_seen: false,
    show_thumbnail: true,
  };
}

function tp(): Touchpad {
  const c = host?.getConfig();
  if (!c) return defaultTouchpad();
  if (!c.touchpad) c.touchpad = defaultTouchpad();
  return c.touchpad;
}

// --- labels -----------------------------------------------------------------

const EDGES: TouchEdge[] = ["left", "right", "top", "bottom"];
const BADGE: Record<TouchEdge, string> = { left: "L", right: "R", top: "T", bottom: "B" };
const EDGE_NAME: Record<TouchEdge, string> = {
  left: "Left edge",
  right: "Right edge",
  top: "Top edge",
  bottom: "Bottom edge",
};
const KIND_NAME: Record<BandActionKind, string> = {
  brightness: "Brightness",
  volume: "Volume",
  scrub: "Video scrub",
  none: "Nothing",
  chords: "Any shortcut",
};

/** The action's family. */
export function actionKind(a: BandAction | null | undefined): BandActionKind {
  if (!a) return "none";
  return typeof a === "string" ? a : "chords";
}

/** The two chords of an "Any shortcut" action, or null. */
export function chordsOf(a: BandAction | null | undefined): BandChords["chords"] | null {
  return a && typeof a === "object" && a.chords ? a.chords : null;
}

/** The label on the band's pill and the edge rows: the family name, or for
 *  "Any shortcut" the forward chord itself once one is recorded. */
export function actionName(a: BandAction | null | undefined): string {
  const c = chordsOf(a);
  if (c) return c.forward.length ? chordLabel(c.forward) : KIND_NAME.chords;
  return KIND_NAME[actionKind(a)];
}

const isVertical = (e: TouchEdge) => e === "left" || e === "right";
/** "up"/"down" on a vertical edge, "right"/"left" on a horizontal one. */
function dirWord(edge: TouchEdge, dir: ChordDir): string {
  return isVertical(edge) ? (dir === "forward" ? "up" : "down") : dir === "forward" ? "right" : "left";
}

function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]!));
}

// --- public API -------------------------------------------------------------

export function initTouchpadPage(el: HTMLElement, h: TouchpadPageHost): void {
  root = el;
  host = h;
  // The two buttons every state shares (Esc/Keyboard, Back to the keyboard,
  // Open touchpad settings) are wired ONCE, by delegation on the root, so no
  // later innerHTML render can orphan them — in the real app they did
  // nothing (owner, 2026-09-19 16:55) while the preview stub showed them fine.
  if (!delegated) {
    delegated = true;
    el.addEventListener("click", (e) => {
      const t = (e.target as HTMLElement | null)?.closest<HTMLElement>("[data-tp]");
      if (!t || !el.contains(t)) return;
      if (t.dataset.tp === "close") { e.preventDefault(); host?.onClose(); }
      else if (t.dataset.tp === "open-settings") { e.preventDefault(); host?.openWindowsTouchpadSettings(); }
    });
  }
  selected = null;
  live = null;
  cornerAsk = null;
  showDemo = tp().demo_seen === false;
  render();
}

export function setTouchpadPresence(p: TouchpadPresence): void {
  if (presence === p) return;
  presence = p;
  if (root && !root.hidden) render();
}

export function touchpadPresence(): TouchpadPresence {
  return presence;
}

export function setTouchpadLive(payload: TouchpadLive): void {
  const wasLive = !!live?.edge;
  live = payload;
  if (!root || root.hidden) return;
  if (!payload.edge) {
    if (wasLive) render();
    return;
  }
  // A cheap in-place update while live, so the readout follows the finger
  // without rebuilding the whole page 30 times a second.
  if (!wasLive) {
    render();
  } else {
    paintLive();
  }
}

/** Re-render (e.g. after the host opens the page or the config changed). */
export function renderTouchpadPage(): void {
  if (root) render();
}

// --- rendering --------------------------------------------------------------

function render(): void {
  if (!root) return;
  if (recording) stopChordRecording(false);
  const t = tp();
  root.dataset.look = t.page_look === "app" ? "app" : "chocolate";

  if (presence !== "precision") {
    root.innerHTML = unavailableHtml();
    wireCommon();
    return;
  }
  if (cornerAsk) {
    root.innerHTML = cornerHtml();
    wireCorner();
    return;
  }
  root.innerHTML = pageHtml();
  wireCommon();
  wirePage();
  paintLive();
}

function topbar(status: string, cls = ""): string {
  return `
    <header class="sp-topbar">
      <button type="button" class="sp-pill" data-tp="close"><span class="sp-badge">Esc</span>Keyboard</button>
      <h1>Touchpad</h1>
      <span class="sp-spacer"></span>
      <span class="sp-status ${cls}"><span class="dot"></span>${esc(status)}</span>
    </header>`;
}

/** One drawn band. `role` selects the visual state. */
function bandHtml(edge: TouchEdge): string {
  const t = tp();
  const b = t[edge];
  const wpx = Math.round(b.width * PAD_W);
  const len = `${Math.round(b.length * 100)}%`;
  const liveOn = live?.edge === edge;
  const cls = [
    "sp-band",
    `sp-band--${edge}`,
    b.enabled && !liveOn ? "is-on" : "",
    liveOn ? "is-live" : "",
    selected === edge ? "is-selected" : "",
  ]
    .filter(Boolean)
    .join(" ");
  const pill = b.enabled
    ? `<span class="sp-pill"><span class="sp-badge">${BADGE[edge]}</span>${esc(actionName(b.action))}</span>`
    : `<span class="sp-pill sp-pill--off"><span class="sp-badge sp-badge--muted">${BADGE[edge]}</span>${esc(actionName(b.action))} · Off</span>`;
  const handles =
    selected === edge && b.enabled
      ? `<span class="sp-handle sp-handle--width" data-drag="width"></span>
         <span class="sp-handle sp-handle--start" data-drag="start"></span>
         <span class="sp-handle sp-handle--end" data-drag="end"></span>`
      : "";
  return `<div class="${cls}" data-band="${edge}" style="--band-w:${wpx}px; --band-len:${len};">${pill}${handles}</div>`;
}

function demoHtml(): string {
  // One finger lands at the edge and slides; the band lights, a value ticks.
  return `
    <div class="sp-band sp-band--left" style="--band-w:54px; --band-len:70%;" data-demo="1">
      <span class="sp-sim">
        <span class="sim-glow"></span>
        <span class="sim-one"><i></i></span>
      </span>
    </div>
    <div class="sp-cap" style="left:240px; top:100px; width:300px; height:30px; --tail:178px;">
      <span class="line"></span>
      <span class="dot"></span>
      <span class="box">
        <span class="nub"></span>
        <span class="t t1"><span class="d"></span>A finger that starts in the middle just moves the pointer</span>
        <span class="t t2"><span class="d"></span>Start at the edge — brightness 72%</span>
      </span>
    </div>`;
}

function pageHtml(): string {
  const t = tp();
  const onCount = EDGES.filter((e) => t[e].enabled).length;
  const statusText = live?.edge ? "You are sliding now" : onCount === 0 ? "Nothing on yet" : `${onCount} edge${onCount === 1 ? "" : "s"} on`;
  const statusCls = live?.edge ? "sp-status--live" : onCount > 0 ? "sp-status--on" : "";

  const bands = EDGES.map(bandHtml).join("");
  const armed = live?.edge ? " is-armed" : "";
  const firstRun = onCount === 0 && !live?.edge;

  // The stage: the pad plus (first run) the demo + invitation card, or
  // (live) the readout card + "pointer holding still" pill.
  const demo = showDemo ? demoHtml() : "";
  const invite = firstRun
    ? `
      <div class="sp-card sp-card--empty">
        <div style="display:flex;align-items:center;justify-content:center;gap:5px;height:16px;">
          <span style="width:5px;height:5px;border-radius:999px;background:var(--sp-text-4);"></span>
          <span style="width:7px;height:7px;border-radius:999px;background:#7A5F4C;"></span>
          <span style="width:11px;height:11px;border-radius:999px;background:var(--sp-accent);margin-left:3px;"></span>
        </div>
        <h2>Start at the edge</h2>
        <p>Watch the left edge: a finger that starts in the middle just moves the pointer; a finger that starts inside the band slides the brightness. Turn on the left edge and try it — then give the top edge to a video.</p>
        <button type="button" class="sp-btn sp-btn--primary" style="margin-top:16px;" data-tp="invite-left">Turn on the left edge</button>
        <div><button type="button" class="sp-btn sp-btn--ghost" style="margin-top:10px;" data-tp="show-demo">Show me again</button></div>
      </div>`
    : "";
  const liveCard = live?.edge ? liveCardHtml() : "";
  const hint = live?.edge
    ? `<p class="hint">This is your real touchpad. Everything here follows your finger.</p>`
    : `<p class="hint">One finger, inside the band. The pointer holds still while you slide.</p>`;

  const panel = selected ? bandPanelHtml(selected) : edgesPanelHtml(onCount);

  return `
    <div class="sp-app">
      ${topbar(statusText, statusCls)}
      <div class="sp-body">
        <main class="sp-stage">
          <div class="sp-pad${armed}" data-pad="1">
            ${bands}
            ${demo}
            ${invite}
            ${liveCard}
          </div>
          ${hint}
        </main>
        ${panel}
      </div>
    </div>`;
}

/** "3 steps" / "1 step" / "−2 steps" for the chords readout. */
function stepsText(n: number | null | undefined): string {
  const v = n ?? 0;
  const shown = v < 0 ? `−${-v}` : String(v);
  return `${shown} step${Math.abs(v) === 1 ? "" : "s"}`;
}

function liveCardHtml(): string {
  const pct = live?.value_pct ?? 0;
  const edge = live?.edge as TouchEdge;
  const kind: BandActionKind = live?.action ?? "volume";
  const chords = kind === "chords";
  // Chords: the chord name where the percent goes, a step counter under it,
  // no meter (a step count has no full scale).
  const big = chords ? esc(live?.chord || KIND_NAME.chords) : `${pct}%`;
  const label = chords ? stepsText(live?.steps) : KIND_NAME[kind];
  return `
    <div class="sp-card sp-card--read" data-live-card="1">
      <div style="display:flex;align-items:center;gap:8px;">
        <span class="sp-badge">${BADGE[edge]}</span><span style="font-size:12px;color:var(--sp-text-2);">${EDGE_NAME[edge]}</span>
      </div>
      <div class="sp-readout"><b data-live-pct${chords ? ' style="font-size:22px;"' : ""}>${big}</b><span data-live-label>${label}</span></div>
      ${chords ? "" : `<div class="sp-meter" style="--value:${pct}%;" data-live-meter><i></i></div>`}
      <p style="color:var(--sp-text-3);font-size:12.5px;">Keep sliding to change it. Lift your finger when it is right.</p>
    </div>
    <span class="sp-pill sp-pill--off" style="position:absolute;left:50%;transform:translateX(-50%);bottom:76px;">
      <span style="width:12px;height:2px;border-radius:999px;background:var(--sp-text-4);"></span>The pointer is holding still
    </span>`;
}

function edgesPanelHtml(onCount: number): string {
  const t = tp();
  const cornerRuleLabel =
    t.corner_rule === "ask" ? "Ask me each time" : t.corner_rule === "always_horizontal" ? "Top / bottom wins" : "Left / right wins";
  const row = (edge: TouchEdge, suggested = false): string => {
    const b = t[edge];
    return `
      <div class="sp-row${suggested ? "" : ""}"${suggested ? ' style="border-color:var(--sp-band-edge);"' : ""}>
        <button type="button" style="display:flex;align-items:center;gap:10px;flex:1 1 auto;text-align:left;" data-open-edge="${edge}">
          <span class="sp-badge ${b.enabled ? "" : "sp-badge--muted"}">${BADGE[edge]}</span>
          <span style="flex:1 1 auto;"><span class="label">${EDGE_NAME[edge]}</span><span class="sub">${esc(actionName(b.action))}${suggested ? " — suggested" : ""}</span></span>
        </button>
        <button type="button" class="sp-switch" role="switch" aria-checked="${b.enabled}" data-toggle-edge="${edge}" aria-label="Turn ${b.enabled ? "off" : "on"} ${EDGE_NAME[edge].toLowerCase()}"></button>
      </div>`;
  };
  return `
    <aside class="sp-panel">
      <div style="display:flex;align-items:baseline;justify-content:space-between;">
        <h2>Edges</h2><span style="font-size:12px;color:var(--sp-text-3);">${onCount} of 4 on</span>
      </div>
      ${row("left")}
      ${row("right")}
      ${row("top", !t.top.enabled)}
      ${row("bottom")}
      <span class="sp-spacer"></span>
      <div class="sp-well">
        <p style="font-weight:600;color:var(--sp-text-2);">How it works</p>
        <p style="margin-top:5px;">Put one finger inside a band and slide. A finger that starts in the middle still moves the pointer, as it always did.</p>
      </div>
      <button type="button" class="sp-row sp-row--dim" style="padding:10px 11px;" data-cycle-corner="1">
        <span style="flex:1 1 auto;">
          <span style="display:block;font-size:12.5px;color:var(--sp-text-2);">When bands overlap</span>
          <span style="display:block;margin-top:2px;font-size:11px;color:var(--sp-text-4);">${cornerRuleLabel}</span>
        </span>
        <svg width="7" height="12" viewBox="0 0 7 12" fill="none" aria-hidden="true"><path d="M1 1 L6 6 L1 11" style="stroke:var(--sp-text-4);" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>
      </button>
    </aside>`;
}

function bandPanelHtml(edge: TouchEdge): string {
  const b = tp()[edge];
  const wpx = Math.round(b.width * PAD_W);
  const lenPct = Math.round(b.length * 100);
  const sensLabel = b.sensitivity <= 3 ? "Slow" : b.sensitivity >= 8 ? "Fast" : "Medium";
  const sensPct = Math.round(((b.sensitivity - 1) / 9) * 100);
  const kind = actionKind(b.action);
  const actionRow = (a: BandActionKind, label: string): string => {
    const on = kind === a;
    return `<button type="button" class="sp-row${on ? " sp-row--picked" : ""}" style="padding:9px 11px;border-radius:var(--sp-r-ctl);font-size:13px;${on ? "font-weight:600;" : ""}" aria-pressed="${on}" data-set-action="${a}">
      <span style="width:16px;height:16px;flex:0 0 auto;border-radius:999px;border:${on ? "5px solid var(--sp-accent)" : "1.5px solid var(--sp-line-strong)"};"></span>${label}
    </button>`;
  };
  const flipSub =
    kind === "volume"
      ? "Sliding up would lower the volume"
      : kind === "brightness"
        ? "Sliding up would dim the screen"
        : kind === "chords"
          ? `Sliding ${dirWord(edge, "forward")} would send the ${dirWord(edge, "backward")} shortcut`
          : "Sliding right would rewind";
  const chordFields = kind === "chords" ? chordFieldsHtml(edge, b) : "";
  return `
    <aside class="sp-panel">
      <button type="button" class="sp-btn sp-btn--ghost" style="align-self:flex-start;padding:3px 10px 3px 6px;font-size:11.5px;color:var(--sp-text-3);" data-tp="all-edges">&lsaquo;&nbsp; All edges</button>
      <div style="display:flex;align-items:center;gap:10px;">
        <span class="sp-pill" style="font-size:13px;"><span class="sp-badge">${BADGE[edge]}</span>${EDGE_NAME[edge]}</span>
        <span class="sp-spacer"></span>
        <button type="button" class="sp-switch" role="switch" aria-checked="${b.enabled}" data-toggle-edge="${edge}" aria-label="Turn ${b.enabled ? "off" : "on"} ${EDGE_NAME[edge].toLowerCase()}"></button>
      </div>
      <div>
        <h2>Does what</h2>
        <div style="margin-top:8px;display:flex;flex-direction:column;gap:6px;">
          ${actionRow("brightness", "Brightness")}
          ${actionRow("volume", "Volume")}
          ${actionRow("scrub", "Video scrub")}
          ${actionRow("chords", "Any shortcut")}
          ${actionRow("none", "Nothing")}
        </div>
      </div>
      ${chordFields}
      <div>
        <h2>The band</h2>
        <div class="sp-row" style="margin-top:8px;padding:9px 11px;border-radius:var(--sp-r-ctl);">
          <span style="flex:1 1 auto;"><span class="label" style="font-weight:400;">Width</span><span class="sub" style="font-size:11px;color:var(--sp-text-4);">Drag the inner edge</span></span>
          <span class="sp-stepper">
            <button type="button" aria-label="Narrower" data-step="width-">&minus;</button><span class="value" data-val="width">${wpx} px</span><button type="button" aria-label="Wider" data-step="width+">+</button>
          </span>
        </div>
        <div class="sp-row" style="margin-top:6px;padding:9px 11px;border-radius:var(--sp-r-ctl);">
          <span style="flex:1 1 auto;"><span class="label" style="font-weight:400;">Length</span><span class="sub" style="font-size:11px;color:var(--sp-text-4);">Drag either end</span></span>
          <span class="sp-stepper">
            <button type="button" aria-label="Shorter" data-step="length-">&minus;</button><span class="value" data-val="length">${lenPct}%</span><button type="button" aria-label="Longer" data-step="length+">+</button>
          </span>
        </div>
      </div>
      <div>
        <div style="display:flex;align-items:baseline;justify-content:space-between;">
          <label for="tp-sens" style="font-size:13px;">How fast it moves</label>
          <span style="font-size:12px;color:var(--sp-text-3);" data-sens-label>${sensLabel}</span>
        </div>
        <div class="sp-slider" style="--value:${sensPct}%;" data-sens-shell>
          <span class="track"></span><span class="fill"></span>
          <input id="tp-sens" type="range" min="1" max="10" value="${b.sensitivity}" data-sens>
        </div>
      </div>
      <div class="sp-row" style="padding:10px 11px;border-radius:var(--sp-r-ctl);">
        <span style="flex:1 1 auto;"><span class="label" style="font-weight:400;">Flip the direction</span><span class="sub" style="font-size:11px;color:var(--sp-text-4);">${flipSub}</span></span>
        <button type="button" class="sp-switch" role="switch" aria-checked="${b.invert}" data-flip aria-label="Flip the direction"></button>
      </div>
      <span class="sp-spacer"></span>
      <p class="foot">Try it now — slide one finger along the ${EDGE_NAME[edge].toLowerCase()} of your touchpad.</p>
    </aside>`;
}

/** The two recorder fields of "Any shortcut": what a slide each way sends. */
function chordFieldsHtml(edge: TouchEdge, b: Band): string {
  const c = chordsOf(b.action) ?? { forward: [], backward: [] };
  const field = (dir: ChordDir): string => {
    const keys = c[dir];
    const rec = recording?.edge === edge && recording.dir === dir;
    const caps = keys.length
      ? keys.map((vk, i) => `${i ? '<span class="plus">+</span>' : ""}<kbd>${esc(vkLabel(vk))}</kbd>`).join("")
      : `<span class="none">${rec ? "Listening…" : "No keys yet"}</span>`;
    return `
      <div class="sp-chord" data-chord-field="${dir}">
        <span class="label">Slide ${dirWord(edge, dir)} sends…</span>
        <div class="sp-chord-caps" data-chord-caps="${dir}">${caps}</div>
        <div class="sp-chord-btns">
          <button type="button" class="sp-btn${rec ? " is-recording" : ""}" data-chord-rec="${dir}">${rec ? "Recording… press now" : "Press the keys…"}</button>
          <button type="button" class="sp-btn sp-btn--ghost" data-chord-clear="${dir}"${keys.length ? "" : " disabled"}>Clear</button>
        </div>
      </div>`;
  };
  return `
      <div>
        <h2>The shortcuts</h2>
        <p style="margin-top:6px;font-size:11.5px;line-height:1.5;color:var(--sp-text-4);">Press the keys together, then let go. Each step of the slide sends it once; sliding back sends the other one.</p>
        <div style="margin-top:8px;display:flex;flex-direction:column;gap:6px;">
          ${field("forward")}
          ${field("backward")}
        </div>
      </div>`;
}

function unavailableHtml(): string {
  return `
    <div class="sp-app">
      ${topbar("Not available here")}
      <div class="sp-body">
        <main class="sp-stage">
          <div class="sp-pad is-dead">
            <div class="sp-band sp-band--top is-dead" style="--band-w:54px; --band-len:80%;"></div>
            <div class="sp-band sp-band--left is-dead" style="--band-w:54px; --band-len:67%;"></div>
            <div class="sp-band sp-band--right is-dead" style="--band-w:54px; --band-len:67%;"></div>
            <div class="sp-band sp-band--bottom is-dead" style="--band-w:54px; --band-len:80%;"></div>
            <div class="sp-card sp-card--empty" style="top:148px;width:400px;background:var(--sp-surface-2);">
              <div style="display:flex;justify-content:center;">
                <svg width="44" height="32" viewBox="0 0 44 32" fill="none" aria-hidden="true">
                  <rect x="1.1" y="1.1" width="41.8" height="29.8" rx="5" style="stroke:var(--sp-text-4);" stroke-width="1.6"/>
                  <path d="M13 16 H31" style="stroke:var(--sp-text-3);" stroke-width="1.8" stroke-linecap="round"/>
                </svg>
              </div>
              <h2 style="font-size:18px;">Edge gestures need a Precision Touchpad</h2>
              <p style="font-size:13.5px;">This PC&rsquo;s touchpad uses its own driver, so Windows never tells Spaceadom where your finger is.</p>
              <div style="margin-top:18px;display:flex;align-items:center;justify-content:center;gap:10px;">
                <button type="button" class="sp-btn" data-tp="open-settings">Open touchpad settings</button>
                <button type="button" class="sp-btn sp-btn--ghost" data-tp="close">Back to the keyboard</button>
              </div>
            </div>
          </div>
          <p class="hint" style="color:var(--sp-text-4);">Everything else in Spaceadom works as usual.</p>
        </main>
        <aside class="sp-panel" style="background:var(--sp-surface);border-color:var(--sp-line-soft);box-shadow:none;">
          <div class="sp-well" style="background:var(--sp-well);border-color:var(--sp-line);">
            <p style="font-size:12px;">These settings are here for when you move to a laptop with a Precision Touchpad.</p>
          </div>
          <h2 style="color:var(--sp-text-4);">Edges</h2>
          ${EDGES.map(
            (e) => `<div class="sp-row sp-row--dim" style="background:var(--sp-surface);">
              <span class="sp-badge sp-badge--dim">${BADGE[e]}</span>
              <span style="flex:1 1 auto;"><span class="label">${EDGE_NAME[e]}</span><span class="sub">${esc(actionName(defaultBand(e).action))}</span></span>
              <span class="sp-switch" aria-disabled="true"></span>
            </div>`,
          ).join("")}
          <span class="sp-spacer"></span>
          <p class="foot">Most laptops made since about 2016 have one.</p>
        </aside>
      </div>
    </div>`;
}

function cornerHtml(): string {
  const ask = cornerAsk!;
  const v = ask.vertical;
  const h = ask.horizontal;
  const opt = (edge: TouchEdge, sub: string): string =>
    `<button type="button" class="sp-row" style="padding:11px;" aria-pressed="false" data-corner-owner="${edge}">
      <span style="width:16px;height:16px;flex:0 0 auto;border-radius:999px;border:1.5px solid var(--sp-line-strong);"></span>
      <span class="sp-badge">${BADGE[edge]}</span>
      <span style="flex:1 1 auto;"><span class="label">${esc(actionName(tp()[edge].action))}</span><span class="sub">${sub}</span></span>
    </button>`;
  return `
    <div class="sp-app">
      ${topbar("A corner to decide")}
      <div class="sp-body">
        <main class="sp-stage">
          <div style="display:flex;align-items:center;gap:14px;padding:14px 16px;border-radius:var(--sp-r-card);background:var(--sp-surface);border:1px solid var(--sp-line-strong);box-shadow:var(--sp-sh-card);max-width:520px;">
            <span style="display:inline-flex;align-items:center;justify-content:center;width:28px;height:28px;flex:0 0 auto;border-radius:999px;background:var(--sp-surface-3);border:1px solid var(--sp-line-strong);color:var(--sp-text-2);font-size:15px;font-weight:700;">?</span>
            <span style="flex:1 1 auto;">
              <span style="display:block;font-size:13.5px;font-weight:600;">Your ${EDGE_NAME[h].toLowerCase()} and ${EDGE_NAME[v].toLowerCase()} bands now share a corner</span>
              <span style="display:block;margin-top:3px;font-size:12.5px;color:var(--sp-text-2);">A finger landing in the shaded square could mean either one. Pick which.</span>
            </span>
          </div>
        </main>
        <aside class="sp-panel" style="width:300px;">
          <h2>Who gets this corner?</h2>
          ${opt(h, EDGE_NAME[h])}
          ${opt(v, EDGE_NAME[v])}
          <button type="button" class="sp-row sp-row--dim" style="padding:10px 11px;border-radius:var(--sp-r-ctl);" role="checkbox" aria-checked="false" data-corner-all>
            <span class="sp-check" aria-hidden="true"></span>
            <span style="flex:1 1 auto;font-size:12px;line-height:1.4;color:var(--sp-text-2);">Do the same in every corner from now on</span>
          </button>
          <p style="font-size:12.5px;line-height:1.55;color:var(--sp-text-2);">Only the finger that <em>starts</em> in the square is affected. Slide in from one band and that band keeps it.</p>
          <span class="sp-spacer"></span>
          <button type="button" class="sp-btn" style="font-size:12.5px;" data-corner-shorten>Shorten the ${EDGE_NAME[ask.justEnabled].toLowerCase()} band instead</button>
          <p class="foot">Then they never touch, and there is nothing to decide.</p>
        </aside>
      </div>
    </div>`;
}

// --- live in-place paint (no full rebuild while sliding) --------------------

function paintLive(): void {
  if (!root) return;
  const b = root.querySelector<HTMLElement>("[data-live-pct]");
  if (live?.action === "chords") {
    if (b) b.textContent = live.chord || KIND_NAME.chords;
    const l = root.querySelector<HTMLElement>("[data-live-label]");
    if (l) l.textContent = stepsText(live.steps);
    return;
  }
  const pct = live?.value_pct ?? 0;
  if (b) b.textContent = `${pct}%`;
  const m = root.querySelector<HTMLElement>("[data-live-meter]");
  if (m) m.style.setProperty("--value", `${pct}%`);
}

// --- wiring -----------------------------------------------------------------

function save(): void {
  void host?.save();
}

function wireCommon(): void {
  // Delegated on the root in initTouchpadPage; nothing per render.
}

function wirePage(): void {
  if (!root) return;
  const t = tp();

  root.querySelectorAll<HTMLElement>('[data-tp="show-demo"]').forEach((el) =>
    el.addEventListener("click", () => {
      showDemo = true;
      render();
    }),
  );
  root.querySelectorAll<HTMLElement>('[data-tp="invite-left"]').forEach((el) =>
    el.addEventListener("click", () => {
      enableEdge("left", true);
    }),
  );
  root.querySelectorAll<HTMLElement>('[data-tp="all-edges"]').forEach((el) =>
    el.addEventListener("click", () => {
      selected = null;
      render();
    }),
  );
  root.querySelectorAll<HTMLElement>("[data-open-edge]").forEach((el) =>
    el.addEventListener("click", () => {
      selected = el.dataset.openEdge as TouchEdge;
      render();
    }),
  );
  root.querySelectorAll<HTMLElement>("[data-toggle-edge]").forEach((el) =>
    el.addEventListener("click", () => {
      const edge = el.dataset.toggleEdge as TouchEdge;
      enableEdge(edge, !t[edge].enabled);
    }),
  );
  root.querySelectorAll<HTMLElement>("[data-band]").forEach((el) =>
    el.addEventListener("click", () => {
      selected = el.dataset.band as TouchEdge;
      render();
    }),
  );
  root.querySelectorAll<HTMLElement>("[data-set-action]").forEach((el) =>
    el.addEventListener("click", () => {
      if (!selected) return;
      const kind = el.dataset.setAction as BandActionKind;
      const b = t[selected];
      if (kind === "chords") {
        // Keep chords already recorded on this band; start empty otherwise.
        if (!chordsOf(b.action)) b.action = { chords: { forward: [], backward: [] } };
      } else {
        b.action = kind;
      }
      save();
      render();
    }),
  );
  root.querySelectorAll<HTMLElement>("[data-chord-rec]").forEach((el) =>
    el.addEventListener("click", () => {
      if (!selected) return;
      const dir = el.dataset.chordRec as ChordDir;
      if (recording && recording.edge === selected && recording.dir === dir) {
        stopChordRecording(true);
      } else {
        startChordRecording(selected, dir);
      }
    }),
  );
  root.querySelectorAll<HTMLElement>("[data-chord-clear]").forEach((el) =>
    el.addEventListener("click", () => {
      if (!selected) return;
      const dir = el.dataset.chordClear as ChordDir;
      const c = chordsOf(t[selected].action);
      if (!c) return;
      c[dir] = [];
      save();
      render();
    }),
  );
  root.querySelectorAll<HTMLElement>("[data-step]").forEach((el) =>
    el.addEventListener("click", () => {
      if (!selected) return;
      stepGeometry(selected, el.dataset.step!);
    }),
  );
  const sens = root.querySelector<HTMLInputElement>("[data-sens]");
  if (sens && selected) {
    sens.addEventListener("input", () => {
      const b = t[selected!];
      b.sensitivity = Math.max(1, Math.min(10, parseInt(sens.value, 10) || 6));
      const shell = root!.querySelector<HTMLElement>("[data-sens-shell]");
      shell?.style.setProperty("--value", `${Math.round(((b.sensitivity - 1) / 9) * 100)}%`);
      const lbl = root!.querySelector<HTMLElement>("[data-sens-label]");
      if (lbl) lbl.textContent = b.sensitivity <= 3 ? "Slow" : b.sensitivity >= 8 ? "Fast" : "Medium";
    });
    sens.addEventListener("change", () => save());
  }
  const flip = root.querySelector<HTMLElement>("[data-flip]");
  if (flip && selected) {
    flip.addEventListener("click", () => {
      const b = t[selected!];
      b.invert = !b.invert;
      flip.setAttribute("aria-checked", String(b.invert));
      save();
    });
  }
  root.querySelectorAll<HTMLElement>("[data-cycle-corner]").forEach((el) =>
    el.addEventListener("click", () => {
      t.corner_rule = t.corner_rule === "ask" ? "always_horizontal" : t.corner_rule === "always_horizontal" ? "always_vertical" : "ask";
      save();
      render();
    }),
  );
  wireDrag();
}

/** ±1 px width / ±5 % length via the steppers. */
function stepGeometry(edge: TouchEdge, step: string): void {
  const b = tp()[edge];
  if (step === "width-") b.width = clamp(b.width - 1 / PAD_W, 0.04, 0.25);
  else if (step === "width+") b.width = clamp(b.width + 1 / PAD_W, 0.04, 0.25);
  else if (step === "length-") b.length = clamp(b.length - 0.05, 0.3, 1.0);
  else if (step === "length+") b.length = clamp(b.length + 0.05, 0.3, 1.0);
  save();
  render();
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

/** Pointer-drag the width / start / end handles of the selected band. */
function wireDrag(): void {
  if (!root || !selected) return;
  const pad = root.querySelector<HTMLElement>("[data-pad]");
  if (!pad) return;
  const edge = selected;
  const b = tp()[edge];
  root.querySelectorAll<HTMLElement>("[data-drag]").forEach((handle) => {
    handle.addEventListener("pointerdown", (ev) => {
      ev.preventDefault();
      handle.setPointerCapture(ev.pointerId);
      const kind = handle.dataset.drag!;
      const rect = pad.getBoundingClientRect();
      const startX = ev.clientX;
      const startY = ev.clientY;
      const startW = b.width;
      const startLen = b.length;
      const move = (e: PointerEvent) => {
        if (kind === "width") {
          const dpx = isVertical(edge)
            ? (edge === "right" ? startX - e.clientX : e.clientX - startX)
            : (edge === "bottom" ? startY - e.clientY : e.clientY - startY);
          b.width = clamp(startW + dpx / rect.width, 0.04, 0.25);
        } else {
          // Either end: moving it out by d lengthens the centred band by 2d.
          const d = isVertical(edge) ? Math.abs(e.clientY - startY) : Math.abs(e.clientX - startX);
          const span = isVertical(edge) ? rect.height : rect.width;
          const dir = (kind === "start") === (e[isVertical(edge) ? "clientY" : "clientX"] < (isVertical(edge) ? startY : startX)) ? 1 : -1;
          b.length = clamp(startLen + (dir * 2 * d) / span, 0.3, 1.0);
        }
        applyBandStyle(edge);
      };
      const up = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        save();
        render();
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
    });
  });
}

function applyBandStyle(edge: TouchEdge): void {
  const el = root?.querySelector<HTMLElement>(`[data-band="${edge}"]`);
  if (!el) return;
  const b = tp()[edge];
  el.style.setProperty("--band-w", `${Math.round(b.width * PAD_W)}px`);
  el.style.setProperty("--band-len", `${Math.round(b.length * 100)}%`);
}

// --- the chord recorder ("Any shortcut") -------------------------------------
// The same arrangement as key-detail-panel.ts's startRecording/stopRecording:
// arm the hook, poll every 100 ms, take the chord as of the last key-down.
// The recorded keys land in the band's action as they arrive (so a full
// re-render never loses them); stopping saves.

function startChordRecording(edge: TouchEdge, dir: ChordDir): void {
  const rec = host?.recorder;
  if (!rec) return;
  if (recording) stopChordRecording(false);
  void rec.start().catch(() => {});
  const timer = window.setInterval(() => {
    if (!recording) return;
    recording.polls += 1;
    if (recording.polls > RECORD_POLLS) {
      stopChordRecording(true);
      return;
    }
    rec
      .poll()
      .then((keys) => {
        if (!recording || recording.edge !== edge || recording.dir !== dir) return;
        if (keys.length) {
          const c = chordsOf(tp()[edge].action);
          if (c) {
            c[dir] = keys.slice(0, 8);
            paintChordCaps(edge, dir);
          }
        }
      })
      .catch(() => {});
  }, 100);
  recording = { edge, dir, timer, polls: 0 };
  paintChordCaps(edge, dir);
  const btn = root?.querySelector<HTMLButtonElement>(`[data-chord-rec="${dir}"]`);
  if (btn) {
    btn.textContent = "Recording… press now";
    btn.classList.add("is-recording");
  }
}

/** End the recording; `commit` saves and re-renders (a plain re-render is
 *  what called us otherwise, so it must not recurse). */
function stopChordRecording(commit: boolean): void {
  if (!recording) return;
  window.clearInterval(recording.timer);
  recording = null;
  void host?.recorder?.stop().catch(() => {});
  if (commit) {
    save();
    render();
  }
}

/** Repaint one field's key caps in place while recording. */
function paintChordCaps(edge: TouchEdge, dir: ChordDir): void {
  const caps = root?.querySelector<HTMLElement>(`[data-chord-caps="${dir}"]`);
  if (!caps) return;
  const c = chordsOf(tp()[edge].action);
  const keys = c ? c[dir] : [];
  caps.innerHTML = "";
  if (!keys.length) {
    const none = document.createElement("span");
    none.className = "none";
    none.textContent = recording ? "Listening…" : "No keys yet";
    caps.appendChild(none);
  } else {
    keys.forEach((vk, i) => {
      if (i) {
        const plus = document.createElement("span");
        plus.className = "plus";
        plus.textContent = "+";
        caps.appendChild(plus);
      }
      const k = document.createElement("kbd");
      k.textContent = vkLabel(vk);
      caps.appendChild(k);
    });
  }
  const clear = root?.querySelector<HTMLButtonElement>(`[data-chord-clear="${dir}"]`);
  if (clear) clear.disabled = !keys.length;
}

// --- enabling a band (with the corner question) -----------------------------

function enableEdge(edge: TouchEdge, on: boolean): void {
  const t = tp();
  t[edge].enabled = on;
  selected = edge;
  // Turning a band ON may create a new vertical+horizontal overlap whose
  // corner is unresolved and whose rule is "ask" → ask now (screen 5a).
  if (on && t.corner_rule === "ask") {
    const perp: TouchEdge[] = isVertical(edge) ? ["top", "bottom"] : ["left", "right"];
    for (const other of perp) {
      if (t[other].enabled) {
        const vertical = isVertical(edge) ? edge : other;
        const horizontal = isVertical(edge) ? other : edge;
        if (cornerOwner(vertical, horizontal) === null) {
          cornerAsk = { vertical, horizontal, justEnabled: edge };
          save();
          render();
          return;
        }
      }
    }
  }
  save();
  render();
}

function cornerOwner(v: TouchEdge, h: TouchEdge): TouchEdge | null {
  const c = tp().corners;
  if (v === "left" && h === "top") return c.tl;
  if (v === "right" && h === "top") return c.tr;
  if (v === "left" && h === "bottom") return c.bl;
  if (v === "right" && h === "bottom") return c.br;
  return null;
}

function setCornerOwner(v: TouchEdge, h: TouchEdge, owner: TouchEdge): void {
  const c = tp().corners;
  if (v === "left" && h === "top") c.tl = owner;
  else if (v === "right" && h === "top") c.tr = owner;
  else if (v === "left" && h === "bottom") c.bl = owner;
  else if (v === "right" && h === "bottom") c.br = owner;
}

function wireCorner(): void {
  if (!root || !cornerAsk) return;
  const ask = cornerAsk;
  let doAll = false;
  root.querySelectorAll<HTMLElement>("[data-corner-all]").forEach((el) =>
    el.addEventListener("click", () => {
      doAll = !doAll;
      el.setAttribute("aria-checked", String(doAll));
      const chk = el.querySelector(".sp-check");
      chk?.setAttribute("aria-checked", String(doAll));
    }),
  );
  root.querySelectorAll<HTMLElement>("[data-corner-owner]").forEach((el) =>
    el.addEventListener("click", () => {
      const owner = el.dataset.cornerOwner as TouchEdge;
      const t = tp();
      if (doAll) {
        t.corner_rule = owner === ask.vertical || owner === "left" || owner === "right" ? "always_vertical" : "always_horizontal";
      } else {
        setCornerOwner(ask.vertical, ask.horizontal, owner);
      }
      cornerAsk = null;
      save();
      render();
    }),
  );
  root.querySelectorAll<HTMLElement>("[data-corner-shorten]").forEach((el) =>
    el.addEventListener("click", () => {
      // Escape: shorten the just-enabled band so the bands no longer meet.
      const b = tp()[ask.justEnabled];
      b.length = clamp(b.length - 0.15, 0.3, 1.0);
      cornerAsk = null;
      save();
      render();
    }),
  );
  wireCommon();
}
