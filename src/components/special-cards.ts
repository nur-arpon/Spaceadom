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
import type { AppConfig, KeyBinding } from "../types";

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
  /**
   * PHASE A (2026-09-18): the SPECIAL's id (`boss_key`, `pip`, …) — Rust's
   * `SPECIAL_IDS` — not a board key any more. Which KEY a special sits on is
   * the active profile's business (`resolveSpecials`). The one non-special
   * row, the opacity gesture, keeps the id `scroll`.
   */
  id: string;
  combo: string;
  name: string;
  desc: string;
  how: string;
}

/**
 * From the v3 lab's SPECIALS array, with two owner-ordered changes
 * (2026-08-20):
 *  - Scroll Bottom added — the lab never listed it, but the engine has had
 *    ␣↓↓ since V13, and its board key was opening SCROLL TOP's card.
 *  - Smart Search's copy rewritten. The lab said "searches the web for the
 *    text you've highlighted", which is not what the feature has ever done —
 *    it moves your CURSOR to the right box (v11's FocusInputEngine). The
 *    card now describes the real behaviour, which was also retargeted the
 *    same day (see focus_engine.rs).
 */
export const SPECIALS: SpecialSpec[] = [
  { id: "boss_key", combo: "␣ Esc", name: "Boss Key",
    desc: "Hides every window and mutes your PC in one hit. Press it again and everything comes back exactly as it was.",
    how: "Hold Space, tap Esc" },
  { id: "pip", combo: "␣ `", name: "PiP Cycle",
    desc: "Shrinks the window you're using into a small corner view. Keep tapping to hop corners, go fullscreen, then back to normal.",
    how: "Hold Space, tap ` (above Tab)" },
  { id: "pip_fullscreen", combo: "␣ Tab", name: "Fullscreen PiP",
    desc: "Like PiP Cycle, but for a window that is playing fullscreen: it shrinks to a corner without leaving fullscreen, and comes back the same way.",
    how: "Hold Space, tap Tab" },
  { id: "force_close", combo: "␣ ⌫", name: "Force Close",
    desc: "Force-quits the app in front — even when it's frozen and the close button won't listen.",
    how: "Hold Space, tap Backspace" },
  { id: "scroll_top", combo: "␣ ↑↑", name: "Scroll Top",
    desc: "Jumps straight to the top of whatever you're reading.",
    how: "Hold Space, tap ↑ twice" },
  { id: "scroll_bottom", combo: "␣ ↓↓", name: "Scroll Bottom",
    desc: "Jumps straight to the bottom of whatever you're reading.",
    how: "Hold Space, tap ↓ twice" },
  { id: "search", combo: "␣ ,", name: "Smart Search",
    desc: "Puts your cursor where you'd type, in one press: YouTube and Spotify search, the address bar on other pages and new tabs, the message box in WhatsApp and Discord.",
    how: "Hold Space, tap comma" },
  { id: "pause", combo: "␣ .", name: "Pause",
    desc: "Puts Spaceadom to sleep so Space acts normal for a while. The same keys wake it up.",
    how: "Hold Space, tap period" },
  { id: "voice_typing", combo: "␣ ;", name: "Voice Typing",
    desc: "Opens Windows' own dictation: speak, and the words are typed wherever your cursor is. The same tile lives on the middle-button ring. If the panel listens but nothing appears, check Windows' default microphone — a virtual device (SteelSeries Sonar, for one) can feed it silence.",
    how: "Hold Space, tap semicolon" },
  { id: "screenshot", combo: "␣ /", name: "Screenshot",
    desc: "Opens Windows' own region snip: drag over what you want and it lands on the clipboard (and in your Screenshots folder, if Snipping Tool is set to save). The same tile lives on the middle-button ring.",
    how: "Hold Space, tap /" },
  { id: "osk", combo: "␣ '", name: "On-screen Keyboard",
    desc: "Shows Windows' own on-screen keyboard; press again to hide it. Handy with just a mouse in hand. The same tile lives on the middle-button ring.",
    how: "Hold Space, tap '" },
  { id: "cycle_profile", combo: "␣ RAlt", name: "Cycle Profile",
    desc: "Switches to your next profile — a different set of apps on the same keys.",
    how: "Hold Space, tap Right Alt" },
  // PHASE A step 3 (2026-09-19) — Windows' own Win+Shift+←/→, sent as one
  // chord. Reversible by the other arrow; nothing to recover from.
  { id: "move_window_left", combo: "␣ ←", name: "Window to Left Screen",
    desc: "Throws the window you're in onto the screen to the left (Windows' own Win+Shift+←). Tap → to bring it back.",
    how: "Hold Space, tap ←" },
  { id: "move_window_right", combo: "␣ →", name: "Window to Right Screen",
    desc: "Throws the window you're in onto the screen to the right (Windows' own Win+Shift+→). Tap ← to bring it back.",
    how: "Hold Space, tap →" },
  { id: "scroll", combo: "␣ Scroll", name: "Opacity",
    desc: "Fades the window under your cursor so you can see what's behind it.",
    how: "Hold Space, roll the mouse wheel" },
];

/**
 * PHASE A — the board's label for a key id, for the cards' "␣ X" combo and
 * the "Hold Space, tap X" line. Mirrors Rust's `engine::specials::key_label`
 * (the HUD's key column) so the tray and the ring name a key the same way.
 * Letters are upper-cased; an unknown id comes back upper-cased too.
 */
export const KEY_LABELS: Record<string, string> = {
  esc: "Esc", backtick: "`", tab: "Tab", backspace: "⌫", ralt: "RAlt",
  comma: ",", period: ".", semicolon: ";", slash: "/", quote: "'",
  up: "↑", down: "↓", left: "←", right: "→", enter: "↵", delete: "Del",
  pgup: "PgUp", pgdn: "PgDn", minus: "-", equal: "=", lbracket: "[",
  rbracket: "]", backslash: "\\", caps: "Caps", lshift: "Shift", rshift: "Shift",
  lctrl: "Ctrl", rctrl: "Ctrl", lalt: "Alt", win: "Win", home: "Home",
  end: "End", insert: "Ins",
};

export function keyLabel(id: string): string {
  if (id in KEY_LABELS) return KEY_LABELS[id]!;
  const f = /^f(\d{1,2})$/.exec(id);
  if (f) return `F${f[1]}`;
  return id.toUpperCase();
}

/** The spoken name of a key for "Hold Space, tap …". */
function keySpoken(id: string): string {
  switch (id) {
    case "backtick": return "` (above Tab)";
    case "backspace": return "Backspace";
    case "ralt": return "Right Alt";
    case "comma": return "comma";
    case "period": return "period";
    case "semicolon": return "semicolon";
    case "lshift": case "rshift": return "Shift";
    case "lctrl": case "rctrl": return "Ctrl";
    case "lalt": return "Alt";
    default: return keyLabel(id);
  }
}

/**
 * PHASE A — the short word a special shows under its key on the board
 * ("Boss", "PiP", "Snip", "Dictate", "Keys"…): today's `SPECIAL_ON_KEY`
 * words, keyed by special id now that the key is the user's choice.
 */
export const SPECIAL_SHORT: Record<string, string> = {
  boss_key: "Boss", pip: "PiP Cycle", pip_fullscreen: "Full PiP",
  force_close: "Force Close", cycle_profile: "Profile", search: "Search",
  pause: "Pause", voice_typing: "Dictate", screenshot: "Snip", osk: "Keys",
  scroll_top: "Scroll Top", scroll_bottom: "Scroll Btm",
  move_window_left: "Move ←", move_window_right: "Move →",
};

/** The card for a special id (`SPECIALS` entry), or null. */
export function specialSpec(id: string): SpecialSpec | null {
  return SPECIALS.find((s) => s.id === id) ?? null;
}

/**
 * PHASE A — where each special sits in `config`'s active profile: the key
 * id whose binding is `{ kind: "special", id }`, or null when it is on no
 * key. First match in sorted key order; the editor refuses a second copy.
 */
export function keyForSpecial(config: AppConfig | null, id: string): string | null {
  if (!config) return null;
  const p = config.profiles.find((x) => x.name === config.active_profile);
  if (!p) return null;
  for (const k of Object.keys(p.bindings).sort()) {
    const b: KeyBinding | undefined = p.bindings[k];
    if (b?.action?.kind === "special" && b.action.id === id) return k;
  }
  return null;
}

/**
 * PHASE A — `SPECIALS` with `combo` and `how` DERIVED from the active
 * profile: "␣ F1" / "Hold Space, tap F1" for a Boss Key moved to F1, the
 * double-tap wording for the two scroll specials wherever they are, and
 * "Not on any key — assign it from any key's editor" for one that is bound
 * nowhere. The opacity gesture row is static (it is not a key). The copy of
 * `desc` is untouched — it is the design's.
 */
export function resolveSpecials(config: AppConfig | null): SpecialSpec[] {
  return SPECIALS.map((spec) => {
    if (spec.id === "scroll") return spec;
    const key = keyForSpecial(config, spec.id);
    if (!key) {
      return { ...spec, combo: "␣ —", how: "Not on any key — assign it from any key's editor" };
    }
    const twice = spec.id === "scroll_top" || spec.id === "scroll_bottom";
    const label = keyLabel(key);
    return {
      ...spec,
      combo: twice ? `␣ ${label}${label}` : `␣ ${label}`,
      how: twice ? `Hold Space, tap ${keySpoken(key)} twice` : `Hold Space, tap ${keySpoken(key)}`,
    };
  });
}

let _card: HTMLElement | null = null;
let _openFor: HTMLElement | null = null;
let _wired = false;

function reduced(): boolean {
  return document.documentElement.classList.contains("reduced-motion")
    || window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function fun(): boolean {
  // === "on": fun is off-by-default since 2026-08-20, so an unset attribute
  // (a window that never ran applyLook) must read as OFF, not on.
  return document.body.dataset.fun === "on";
}

/** Index of a special by id, or -1. */
export function specialIndex(id: string): number {
  return SPECIALS.findIndex((s) => s.id === id);
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
  // The card lives on <body>, so its clicks reach the document listener that
  // closes every popover (main.ts). Without this, pressing a card opened from
  // the specials tray closed the tray underneath it (PROBLEM 98's rule, one
  // more surface).
  card.addEventListener("click", (e) => e.stopPropagation());
  // Half the card (120px) plus a 10px margin, so it never runs off an edge.
  card.style.left = `${Math.round(Math.min(Math.max(r.left + r.width / 2, 130), vw - 130))}px`;
  card.style.bottom = `${Math.round(Math.max(window.innerHeight - r.top + 10, 10))}px`;

  const anims = CARD_ANIMS[i % CARD_ANIMS.length];
  if (reduced()) {
    card.style.transform = "translateX(-50%)";
  } else if (fun()) {
    card.style.animation = `${anims[0]} 520ms ${anims[1]} both`;
  } else {
    // Fun OFF: every card opens with the IRIS wipe — the owner's rule of
    // 2026-08-20 ("all cards use iris when fun toggle off"), which replaced
    // the spec's plainIn. Fun ON keeps the full 8-animation variety above.
    card.style.animation = `sky-irisIn 520ms ${CARD_ANIMS[2][1]} both`;
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
