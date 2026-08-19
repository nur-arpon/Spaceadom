/**
 * keyboard-matrix.ts — the dashboard's hero: an interactive keyboard.
 *
 * V14: re-skinned to the Earthy design (Dashboard Earthy v2.dc.html) —
 * 16-unit board, U=56/G=10 geometry, flat cream keys, terracotta tint when
 * bound, staggered cascade on first paint. The BEHAVIOUR is V13's and is
 * deliberately unchanged: drag/drop of .exe/.lnk/URL, right-click context
 * menu, icon extraction, binding read/write, spring pop when a binding lands.
 *
 * Layout note: the board is fixed-geometry. It is scaled to fit by main.ts
 * (fitKeyboard) — never let it size itself off the viewport, that is the bug
 * the user saw in the previous attempt.
 */
import { invoke } from "@tauri-apps/api/core";
import { boardSpecial, boardCardIndex, toggleSpecialCard } from "./special-cards";
import { showToast } from "./toast";
import type { AppConfig, KeyBinding, Profile } from "../types.ts";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

export function cleanLabel(raw: string): string {
  // 1. Remove common extensions
  let cleaned = raw.replace(/\.(exe|lnk|bat|cmd|url|app|com|org|net|io|co)$/i, "");

  // 2. Remove common web prefixes
  cleaned = cleaned.replace(/^(https?:\/\/)?(www\.)?/i, "");

  // 3. Convert separators (dashes, underscores) to spaces
  cleaned = cleaned.replace(/[-_]/g, " ");

  // 4. Insert space between camelCase (e.g., DiscordApp -> Discord App)
  cleaned = cleaned.replace(/([a-z])([A-Z])/g, "$1 $2");

  // 5. Remove trailing "App" (e.g. Discord App -> Discord)
  cleaned = cleaned.replace(/\bApp$/i, "");

  // 6. Title Case the result
  cleaned = cleaned.replace(/\b\w/g, (char) => char.toUpperCase());

  return cleaned.trim() || raw;
}

// ---------------------------------------------------------------------------
// Layout — 16 units wide, exactly as the mockup's ROWS table.
// Each key: [displayLabel, dataKey, widthUnits]
// ---------------------------------------------------------------------------

const ROWS: [string, string, number][][] = [
  [
    ["`", "grave", 1], ["1", "1", 1], ["2", "2", 1], ["3", "3", 1],
    ["4", "4", 1], ["5", "5", 1], ["6", "6", 1], ["7", "7", 1],
    ["8", "8", 1], ["9", "9", 1], ["0", "0", 1], ["-", "minus", 1],
    ["=", "equal", 1], ["⌫", "backspace", 2], ["Del", "delete", 1],
  ],
  [
    ["Tab", "tab", 1.5], ["Q", "q", 1], ["W", "w", 1], ["E", "e", 1],
    ["R", "r", 1], ["T", "t", 1], ["Y", "y", 1], ["U", "u", 1],
    ["I", "i", 1], ["O", "o", 1], ["P", "p", 1], ["[", "lbracket", 1],
    ["]", "rbracket", 1], ["\\", "backslash", 1.5], ["PgUp", "pgup", 1],
  ],
  [
    ["Caps", "caps", 1.75], ["A", "a", 1], ["S", "s", 1], ["D", "d", 1],
    ["F", "f", 1], ["G", "g", 1], ["H", "h", 1], ["J", "j", 1],
    ["K", "k", 1], ["L", "l", 1], [";", "semicolon", 1], ["'", "quote", 1],
    ["↵", "enter", 2.25], ["PgDn", "pgdn", 1],
  ],
  [
    ["Shift", "lshift", 2.25], ["Z", "z", 1], ["X", "x", 1], ["C", "c", 1],
    ["V", "v", 1], ["B", "b", 1], ["N", "n", 1], ["M", "m", 1],
    [",", "comma", 1], [".", "period", 1], ["/", "slash", 1],
    ["Shift", "rshift", 1.75], ["↑", "up", 1], ["Fn", "rfn", 1],
  ],
  [
    ["Ctrl", "lctrl", 1.25], ["Fn", "lfn", 1], ["Win", "win", 1.25],
    ["Alt", "lalt", 1.25], ["SPACE", "space", 6.25], ["Alt", "ralt", 1],
    ["Ctrl", "rctrl", 1], ["←", "left", 1], ["↓", "down", 1], ["→", "right", 1],
  ],
];

/** Keys that carry alpha bindings (a–z) — the only clickable ones. */
const ALPHA_KEYS = new Set("abcdefghijklmnopqrstuvwxyz".split(""));

/**
 * Preset special functions, labelled on the keys they actually live on.
 * The user asked for these to be discoverable on the board, not only in a
 * tray (V13_TO_V14_METHOD §3.5). Esc is not on this board, so it stays in
 * the bottom tray with the rest of the reference list.
 */
const SPECIAL_ON_KEY: Record<string, string> = {
  grave:     "PiP Cycle",
  backspace: "Force Close",
  comma:     "Search",
  period:    "Pause",
  up:        "Scroll Top",
  down:      "Scroll Btm",
  ralt:      "Profile",
};

// Board geometry (mockup: U=56, G=10).
// DESIGN_W is 1048, not the 1046 that 16 clean units would give: the
// fractional-unit keys (1.5/1.75/2.25) are rounded to whole pixels
// individually, and those roundings add 2px per row. Measured, not assumed —
// every row renders at exactly 1048. The mockup uses 1048 for the same reason.
const U = 56;
const GAP = 10;
export const DESIGN_W = 1048;
export const DESIGN_H = 5 * U + 4 * GAP;     // 320

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------
let _config: AppConfig | null = null;
let _onKeySelect: ((key: string, cell: HTMLDivElement) => void) | null = null;
let _onConfigChange: (() => void) | null = null;
/** The cascade runs once, on first paint only — not on every re-render. */
let _cascadeDone = false;

// ---------------------------------------------------------------------------
// Press feedback — EVERY key, not just the bindable letters (PROBLEM 39)
// ---------------------------------------------------------------------------
// The mockup's pressKey() fires a ripple + 520Hz tick for ANY key label and
// only additionally opens the editor for letters. Tapping Tab or Shift and
// getting a ripple is a large part of why the board feels good, so the
// feedback lives HERE, on every cell — not in main.ts's select callback,
// where it would (and previously did) reach letters only.

/** Sound state mirrors the "Sound ticks" setting; main.ts feeds it. */
let _soundOn = false;
let _ac: AudioContext | null = null;

export function setKeyboardSound(on: boolean): void {
  _soundOn = on;
}

/** 520Hz sine tick on press — same voice as the mockup's beep(520). */
function keyBeep(): void {
  if (!_soundOn) return;
  try {
    _ac = _ac || new AudioContext();
    const o = _ac.createOscillator(), g = _ac.createGain();
    o.type = "sine";
    o.frequency.value = 520;
    g.gain.setValueAtTime(0.05, _ac.currentTime);
    g.gain.exponentialRampToValueAtTime(0.0001, _ac.currentTime + 0.09);
    o.connect(g); g.connect(_ac.destination);
    o.start(); o.stop(_ac.currentTime + 0.1);
  } catch { /* never break the board for a tick */ }
}

/** Expanding 130px ring from the key's centre, on the stage layer.
 *  (Moved here from main.ts so non-letter keys get it too, and so the
 *  preview harness shows it without wiring anything.) */
function spawnRipple(cell: HTMLElement): void {
  const stage = document.getElementById("stage");
  if (!stage) return;
  // Reads the .reduced-motion class main.ts set, so the in-app "Visual
  // effects" override is honoured (PROBLEM 47). Querying the OS media query
  // here directly is what silently removed the ripple on a tester's machine.
  if (document.documentElement.classList.contains("reduced-motion")) return;

  const sr = stage.getBoundingClientRect();
  const kr = cell.getBoundingClientRect();
  const r = document.createElement("div");
  r.className = "ripple";
  r.style.left = `${kr.left + kr.width / 2 - sr.left}px`;
  r.style.top = `${kr.top + kr.height / 2 - sr.top}px`;
  stage.appendChild(r);
  window.setTimeout(() => r.remove(), 560);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export function initKeyboardMatrix(
  container: HTMLElement,
  config: AppConfig,
  onKeySelect: (key: string, cell: HTMLDivElement) => void,
  onConfigChange: () => void,
): void {
  _config = config;
  _onKeySelect = onKeySelect;
  _onConfigChange = onConfigChange;
  renderMatrix(container);
  window.setTimeout(() => {
    _cascadeDone = true;
    // Strip the intro animation from every cell once the cascade is over.
    // This is the mockup's `introDone` re-render, which clears the animation
    // from the style attribute — the detail whose omission caused PROBLEM 40.
    // With `backwards` fill this is belt-and-braces (the fill already
    // releases the transforms), but an attached-forever animation is exactly
    // the kind of latent hazard that bit once already; remove it outright.
    container.querySelectorAll<HTMLDivElement>(".key").forEach((c) => {
      c.style.animation = "";
      c.style.animationDelay = "";
    });
  }, 1700);
}

export function updateMatrix(container: HTMLElement, config: AppConfig): void {
  _config = config;
  // Re-skin the alpha cells in place — a full re-render would replay the
  // cascade and flash the whole board on every binding change.
  container.querySelectorAll<HTMLDivElement>(".key[data-key]").forEach((cell) => {
    const key = cell.dataset.key!;
    if (!ALPHA_KEYS.has(key)) return;
    applyKeyState(cell, key);
  });
}

/** The cell for a key, so callers can animate from its position. */
export function getKeyCell(key: string): HTMLDivElement | null {
  return document.querySelector<HTMLDivElement>(`.key[data-key="${key}"]`);
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

function renderMatrix(container: HTMLElement): void {
  container.innerHTML = "";

  ROWS.forEach((row, ri) => {
    const rowEl = document.createElement("div");
    rowEl.className = "kb-row";
    row.forEach(([label, key, units], ci) => {
      rowEl.appendChild(createKeyCell(label, key, units, ri, ci));
    });
    container.appendChild(rowEl);
  });
}

function createKeyCell(
  label: string,
  key: string,
  units: number,
  ri: number,
  ci: number,
): HTMLDivElement {
  const cell = document.createElement("div");
  cell.className = "key";
  cell.dataset.key = key;
  cell.style.width = `${Math.round(units * U + (units - 1) * GAP)}px`;

  // Cascade: rows sweep in at 55ms each, keys at 16ms within a row.
  // `backwards`, NOT `both` (PROBLEM 40): a completed animation with a
  // forwards fill keeps its final keyframe's transform applied at
  // animation-level precedence FOREVER, which overrides the :hover lift and
  // the :active press-down — the entire board goes motion-dead after the
  // intro. `backwards` fills only during the stagger delay (keys must be
  // invisible before their turn) and releases the transform channel when the
  // animation ends. The final keyframe equals the natural state, so there is
  // no visual difference in the cascade itself.
  if (!_cascadeDone) {
    cell.style.animation = `st-key-in 560ms var(--ease-spring) backwards`;
    cell.style.animationDelay = `${ri * 55 + ci * 16}ms`;
  }

  const labelEl = document.createElement("span");
  labelEl.className = "key-label" + (label.length > 1 ? " multi" : "");
  labelEl.textContent = label;
  cell.appendChild(labelEl);

  // Press feedback on EVERY key — ripple + tick, exactly like the mockup's
  // pressKey(). Letters get their own click listener as well (below) for the
  // editor; registration order means the ripple always fires first, and
  // main.ts opens the editor on a 90ms delay so the ripple is seen.
  cell.addEventListener("click", () => {
    spawnRipple(cell);
    keyBeep();
  });

  if (key === "space") {
    cell.classList.add("space");
  } else if (ALPHA_KEYS.has(key)) {
    cell.classList.add("bindable");
    applyKeyState(cell, key);
    attachKeyListeners(cell, key);
  } else if (SPECIAL_ON_KEY[key]) {
    // Preset special function — labelled, but not user-rebindable yet
    // (rebinding the presets is explicitly deferred work).
    cell.classList.add("special");
    const sub = document.createElement("span");
    sub.className = "key-app";
    const txt = document.createElement("span");
    txt.textContent = SPECIAL_ON_KEY[key];
    sub.appendChild(txt);
    cell.appendChild(sub);
    cell.title = `Space + ${label} — ${SPECIAL_ON_KEY[key]}`;

    // PROBLEM 148 — pressing it explains it (spec §4). These keys are not
    // bindable, so the press was doing nothing but a ripple; the card is the
    // only thing on the board that ever tells you what a special DOES.
    const spec = boardSpecial(key);
    const idx = boardCardIndex(key);
    if (spec && idx >= 0) {
      cell.dataset.spec = spec.id;
      cell.setAttribute("aria-expanded", "false");
      cell.addEventListener("click", (e) => {
        e.stopPropagation();
        toggleSpecialCard(cell, spec, idx);
      });
    }
  }

  return cell;
}

function applyKeyState(cell: HTMLDivElement, key: string): void {
  const binding = getBinding(key);
  const isMapped = !!(binding && (binding.app || binding.web_url));

  cell.classList.toggle("bound", isMapped);
  cell.classList.remove("drop-target");

  // Drop any existing sub-label, then re-add it if the key is bound.
  cell.querySelector(".key-app")?.remove();
  if (!isMapped) {
    cell.title = `Space + ${key.toUpperCase()} — not bound`;
    return;
  }

  const label =
    binding!.label ||
    (binding!.app
      ? cleanLabel(binding!.app.split(/[\\/]/).pop() || "")
      : binding!.web_url
        ? cleanLabel(binding!.web_url)
        : "URL");

  const sub = document.createElement("span");
  sub.className = "key-app";

  // Real extracted icon when we have one (V13 feature, verified) — never
  // downgrade this to a coloured letter disc.
  if (binding!.icon_override) {
    const img = document.createElement("img");
    img.src = `data:image/png;base64,${binding!.icon_override}`;
    img.alt = "";
    sub.appendChild(img);
  }
  const txt = document.createElement("span");
  txt.textContent = label;          // textContent — app names are user data
  sub.appendChild(txt);
  cell.appendChild(sub);

  cell.title = `Space + ${key.toUpperCase()} — ${label}`;
}

// ---------------------------------------------------------------------------
// Event listeners
// ---------------------------------------------------------------------------

function attachKeyListeners(cell: HTMLDivElement, key: string): void {
  cell.addEventListener("click", () => {
    if (_onKeySelect) _onKeySelect(key, cell);
  });

  cell.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    showContextMenu(e.clientX, e.clientY, key, cell);
  });

  cell.addEventListener("dragover", (e) => {
    e.preventDefault();
    cell.classList.add("drop-target");
  });
  cell.addEventListener("dragleave", () => cell.classList.remove("drop-target"));
  cell.addEventListener("drop", (e) => {
    e.preventDefault();
    cell.classList.remove("drop-target");
    handleDrop(e, key, cell);
  });
}

// ---------------------------------------------------------------------------
// Drag-and-drop
// ---------------------------------------------------------------------------

async function handleDrop(
  e: DragEvent,
  key: string,
  cell: HTMLDivElement,
): Promise<void> {
  const dt = e.dataTransfer;
  if (!dt) return;

  // File drop (from Windows Explorer)
  if (dt.files.length > 0) {
    const file = dt.files[0];
    const path: string = (file as any).path || "";
    if (!path) {
      // Tauri's webview does not expose File.path on every drop route. Say so
      // rather than failing silently — "drag & drop does nothing" was a real
      // user report with no visible cause.
      showToast("⚠️ Could not read that file's path — use Browse instead");
      return;
    }

    const lower = path.toLowerCase();
    if (lower.endsWith(".exe") || lower.endsWith(".lnk")) {
      await assignAppBinding(key, cell, path);
    } else {
      showToast("⚠️ Drop an .exe or .lnk");
    }
    return;
  }

  // URL drop (from browser address bar or link)
  const url = dt.getData("text/uri-list") || dt.getData("text/plain");
  if (url && (url.startsWith("http://") || url.startsWith("https://"))) {
    await assignUrlBinding(key, cell, url.trim());
  }
}

async function assignAppBinding(
  key: string,
  cell: HTMLDivElement,
  exePath: string,
): Promise<void> {
  if (!_config) return;

  let iconB64: string | null = null;
  try {
    iconB64 = await invoke<string | null>("extract_icon_cmd", { exePath });
  } catch (_) { /* icon is a nicety; never block the binding on it */ }

  const exeName = exePath.split(/[\\/]/).pop() ?? exePath;
  const label = cleanLabel(exeName);

  updateBinding(key, {
    app: exePath,
    web_url: null,
    label,
    icon_override: iconB64 ?? undefined,
  });

  applyKeyState(cell, key);
  animateKeyPop(cell);
  showToast(`⚡ Assigned: ${label} → Space+${key.toUpperCase()}`);
  if (_onConfigChange) _onConfigChange();
}

async function assignUrlBinding(
  key: string,
  cell: HTMLDivElement,
  url: string,
): Promise<void> {
  let hostname = url;
  try { hostname = new URL(url).hostname; } catch (_) { /* keep raw */ }

  const label = cleanLabel(hostname);
  updateBinding(key, { app: null, web_url: url, label, icon_override: undefined });
  applyKeyState(cell, key);
  animateKeyPop(cell);
  showToast(`🌐 URL mapped: ${label} → Space+${key.toUpperCase()}`);
  if (_onConfigChange) _onConfigChange();
}

// ---------------------------------------------------------------------------
// Context menu
// ---------------------------------------------------------------------------

function showContextMenu(
  x: number,
  y: number,
  key: string,
  cell: HTMLDivElement,
): void {
  const menu = document.getElementById("context-menu")!;
  menu.innerHTML = `
    <div class="context-menu-item" id="ctx-edit" role="menuitem">Edit binding</div>
    <div class="context-menu-item danger" id="ctx-clear" role="menuitem">Clear binding</div>
  `;

  menu.style.display = "block";
  menu.style.left = `${Math.min(x, window.innerWidth - 176)}px`;
  menu.style.top = `${Math.min(y, window.innerHeight - 110)}px`;

  const close = () => {
    menu.style.display = "none";
    document.removeEventListener("click", close);
  };

  menu.querySelector("#ctx-edit")!.addEventListener("click", () => {
    close();
    if (_onKeySelect) _onKeySelect(key, cell);
  });

  menu.querySelector("#ctx-clear")!.addEventListener("click", () => {
    close();
    clearBinding(key, cell);
  });

  setTimeout(() => document.addEventListener("click", close), 10);
}

// ---------------------------------------------------------------------------
// Binding helpers
// ---------------------------------------------------------------------------

function getBinding(key: string): KeyBinding | undefined {
  if (!_config) return undefined;
  const profile = _config.profiles.find(
    (p: Profile) => p.name === _config!.active_profile,
  );
  return profile?.bindings[key];
}

function updateBinding(key: string, binding: Partial<KeyBinding>): void {
  if (!_config) return;
  const profile = _config.profiles.find(
    (p: Profile) => p.name === _config!.active_profile,
  );
  if (profile) {
    profile.bindings[key] = {
      ...(profile.bindings[key] ?? {}),
      ...binding,
    } as KeyBinding;
  }
}

function clearBinding(key: string, cell: HTMLDivElement): void {
  updateBinding(key, { app: null, web_url: null, label: null, icon_override: null });
  applyKeyState(cell, key);
  animateKeyPop(cell);
  showToast(`🗑️ Cleared: Space+${key.toUpperCase()}`);
  if (_onConfigChange) _onConfigChange();
}

// ---------------------------------------------------------------------------
// Animation
// ---------------------------------------------------------------------------

/** One spring pop as a binding lands on the key. */
export function animateKeyPop(cell: HTMLDivElement): void {
  cell.classList.remove("popping");
  void cell.offsetWidth; // force reflow so the animation restarts
  cell.classList.add("popping");
  setTimeout(() => cell.classList.remove("popping"), 560);
}
