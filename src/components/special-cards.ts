/**
 * special-cards.ts — the eight special shortcuts, and the card that explains
 * one when you press it (spec §4).
 *
 * WHAT THIS FIXES: the bottom tray has been a row of inert labels since V14.
 * "␣ ⌫ — Force Close" tells you the keys and nothing else; there was no way
 * to find out what a special actually does without reading the manual. Now
 * pressing a tray chip — or the special key on the board itself — opens a card
 * above it with the real explanation and how to press it.
 *
 * A LEAF MODULE. It imports the sound kit and nothing else from the app, so
 * `preview.ts` can render the cards without dragging main.ts's bootstrap in
 * behind it (which is what blanked the dev harness the first time).
 *
 * The copy is the design's, VERBATIM (design/design-system-overhaul-3.md and
 * the v3 lab's SPECIALS array) — CLAUDE.md: the design is a specification.
 */
import { sfx } from "../sfx";

/**
 * The eight card entrances, in the order spec §4 tables them. Index-matched
 * to `sfx.cardOpen(i)`, so the genie card gets the genie sound.
 *
 * The @keyframes themselves live in styles/starry-sky.css and are SHARED with
 * the constellation cards — one definition, two callers. That is also why
 * they still carry the `sky-` prefix: renaming them would touch motion the
 * owner has already signed off on, to buy nothing.
 */
export const CARD_ANIMS: [string, string][] = [
  ["sky-genieIn",    "cubic-bezier(.3,1.05,.4,1)"],
  ["sky-warpDropIn", "cubic-bezier(.22,1.2,.36,1)"],
  ["sky-irisIn",     "cubic-bezier(.25,.9,.3,1)"],
  ["sky-hingeIn",    "cubic-bezier(.3,1.4,.45,1)"],
  ["sky-slingIn",    "cubic-bezier(.25,1.25,.4,1)"],
  ["sky-unfurlIn",   "cubic-bezier(.3,1.3,.45,1)"],
  ["sky-boingIn",    "cubic-bezier(.3,1,.4,1)"],
  ["sky-tvIn",       "cubic-bezier(.2,.9,.3,1)"],
];

export interface SpecialSpec {
  /** Matches the board's key id (keyboard-matrix), where one exists. */
  id: string;
  combo: string;
  name: string;
  desc: string;
  how: string;
}

/** Verbatim from the v3 lab's SPECIALS array. Order is the tray's order. */
export const SPECIALS: SpecialSpec[] = [
  { id: "esc", combo: "␣ Esc", name: "Boss Key",
    desc: "Hides every window and mutes your PC in one hit. Press it again and everything comes back exactly as it was.",
    how: "Hold Space, tap Esc" },
  { id: "grave", combo: "␣ `", name: "PiP Cycle",
    desc: "Shrinks the window you're using into a small corner view. Keep tapping to hop corners, go fullscreen, then back to normal.",
    how: "Hold Space, tap ` (above Tab)" },
  { id: "backspace", combo: "␣ ⌫", name: "Force Close",
    desc: "Force-quits the app in front — even when it's frozen and the close button won't listen.",
    how: "Hold Space, tap Backspace" },
  { id: "up", combo: "␣ ↑↑", name: "Scroll Top",
    desc: "Jumps straight to the top of whatever you're reading.",
    how: "Hold Space, tap ↑ twice" },
  { id: "comma", combo: "␣ ,", name: "Smart Search",
    desc: "Searches the web for the text you've highlighted, in one move.",
    how: "Hold Space, tap comma" },
  { id: "period", combo: "␣ .", name: "Pause",
    desc: "Puts Spaceadom to sleep so Space acts normal for a while. The same keys wake it up.",
    how: "Hold Space, tap period" },
  { id: "ralt", combo: "␣ RAlt", name: "Cycle Profile",
    desc: "Switches to your next profile — a different set of apps on the same keys.",
    how: "Hold Space, tap Right Alt" },
  { id: "scroll", combo: "␣ Scroll", name: "Opacity",
    desc: "Fades the window under your cursor so you can see what's behind it.",
    how: "Hold Space, roll the mouse wheel" },
];

/**
 * The lab's `id` for Backspace is "back"; this app's keyboard-matrix calls the
 * key "backspace". The board is the older name and it is used in geometry,
 * bindings and logs, so the CARD adopts the board's id rather than the reverse.
 * `down` (Scroll Btm) is on the board but has no tray chip — it shares Scroll
 * Top's card, which describes the pair.
 */
const BOARD_TO_SPECIAL: Record<string, string> = {
  grave: "grave", backspace: "backspace", comma: "comma",
  period: "period", up: "up", down: "up", ralt: "ralt",
};

let _card: HTMLElement | null = null;
let _openFor: HTMLElement | null = null;
let _wired = false;

function reduced(): boolean {
  return document.documentElement.classList.contains("reduced-motion")
    || window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function fun(): boolean {
  return document.body.dataset.fun !== "off";
}

/** Index of a special by id, or -1. */
export function specialIndex(id: string): number {
  return SPECIALS.findIndex((s) => s.id === id);
}

/** The card index for a BOARD key. Spec §4: board keys use i+3 "so neighbors
 *  differ" — the same key never performs the same entrance in both places. */
export function boardCardIndex(boardKey: string): number {
  const id = BOARD_TO_SPECIAL[boardKey];
  if (!id) return -1;
  const i = specialIndex(id);
  return i < 0 ? -1 : i + 3;
}

/** The special a board key describes, or null if it is not a special. */
export function boardSpecial(boardKey: string): SpecialSpec | null {
  const id = BOARD_TO_SPECIAL[boardKey];
  return id ? SPECIALS.find((s) => s.id === id) ?? null : null;
}

/**
 * @param silent  the card is being REPLACED by another one. It leaves at once
 *                and without a sound: a card fading out underneath the card
 *                that replaced it is two cards on screen, and the outgoing one
 *                still hit-tests. That was real — pressing eight tray chips in
 *                a row left eight cards stacked.
 */
export function closeSpecialCard(silent = false): void {
  if (!_card) { _openFor = null; return; }
  const card = _card;
  _card = null;
  _openFor?.setAttribute("aria-expanded", "false");
  _openFor = null;
  if (silent || reduced()) { card.remove(); return; }

  sfx.cardClose();
  // "Close = fast fade" (§4). From this instant the card is scenery: it must
  // not take a press, and nothing may find it by its marker any more.
  card.removeAttribute("data-spec-card");
  card.style.pointerEvents = "none";
  card.style.animation = "none";
  card.style.transition = "opacity 140ms var(--ease-in), transform 140ms var(--ease-in)";
  card.style.opacity = "0";
  card.style.transform = "translateX(-50%) scale(.96)";
  card.addEventListener("transitionend", () => card.remove(), { once: true });
  // transitionend does not fire on a hidden or discarded document — and this
  // window spends most of its life in the tray.
  window.setTimeout(() => card.remove(), 400);
}

/**
 * Open the card for `spec`, anchored ABOVE `trigger` and centred on it.
 *
 * `position: fixed` and screen coordinates on purpose: the tray chip sits in a
 * normal dock, but the board key lives inside `#keyboard-scale`, which is
 * TRANSFORMED to fit the window. A card positioned relative to that would be
 * scaled with it — text and all. getBoundingClientRect() already reports
 * post-transform screen pixels, so a fixed card lands correctly from either.
 */
export function openSpecialCard(trigger: HTMLElement, spec: SpecialSpec, i: number): void {
  closeSpecialCard(true);           // one card at a time — tray and board share it
  wireOnce();

  const r = trigger.getBoundingClientRect();
  const vw = window.innerWidth || 1240;
  const card = document.createElement("div");
  card.className = "spec-card";
  card.dataset.specCard = "1";
  card.setAttribute("role", "dialog");
  card.setAttribute("aria-label", spec.name);
  // Half the card (120px) plus a 10px margin, so it never runs off an edge.
  card.style.left = `${Math.round(Math.min(Math.max(r.left + r.width / 2, 130), vw - 130))}px`;
  card.style.bottom = `${Math.round(Math.max(window.innerHeight - r.top + 10, 10))}px`;

  const anims = CARD_ANIMS[i % CARD_ANIMS.length];
  if (reduced()) {
    card.style.transform = "translateX(-50%)";
  } else if (fun()) {
    card.style.animation = `${anims[0]} 520ms ${anims[1]} both`;
  } else {
    // Fun OFF: "plainIn 180ms + tick" (§4). Same end state, no personality.
    card.style.animation = "spec-plainIn 180ms var(--ease-out) both";
  }

  const combo = document.createElement("div");
  combo.className = "spec-card-combo";
  combo.textContent = spec.combo;
  const name = document.createElement("div");
  name.className = "spec-card-name";
  name.textContent = spec.name;
  const desc = document.createElement("div");
  desc.className = "spec-card-desc";
  desc.textContent = spec.desc;
  const how = document.createElement("div");
  how.className = "spec-card-how";
  how.textContent = spec.how;
  card.append(combo, name, desc, how);

  document.body.appendChild(card);
  _card = card;
  _openFor = trigger;
  trigger.setAttribute("aria-expanded", "true");
  sfx.cardOpen(i);
}

/** Press behaviour: the same trigger closes its own card (§5b's rule, and
 *  what anyone expects from a thing that opened on a press). */
export function toggleSpecialCard(trigger: HTMLElement, spec: SpecialSpec, i: number): void {
  if (_openFor === trigger) { closeSpecialCard(); return; }
  openSpecialCard(trigger, spec, i);
}

/**
 * Outside press and Escape, wired once for the whole app.
 *
 * Capture phase: #stage closes every popover on click (PROBLEM 98) and the
 * board cells stop propagation of their own presses, so a bubbling listener
 * would miss most of the ways a user leaves this card.
 */
function wireOnce(): void {
  if (_wired) return;
  _wired = true;
  document.addEventListener("pointerdown", (e) => {
    if (!_card) return;
    const t = e.target as HTMLElement | null;
    if (t?.closest?.("[data-spec-card]") || t?.closest?.("[data-spec]")) return;
    closeSpecialCard();
  }, true);
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && _card) { e.stopPropagation(); closeSpecialCard(); }
  }, true);
}
