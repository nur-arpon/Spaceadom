/**
 * key-detail-panel.ts — the key editor.
 *
 * V14: no longer a right-side slide-in. It blooms OUT of the key you pressed
 * and collapses back into it (Dashboard Earthy v2.dc.html): search, a grid of
 * apps actually detected on this device, Browse files…, and a paste-a-path /
 * URL row. Picking an app assigns it immediately — the mockup has no
 * Save/Cancel pair, and neither does this.
 *
 * Preserved from V13 because they are real, working behaviour:
 *   · list_start_menu_apps + extract_icon_cmd (real icons, never letter discs)
 *   · pick_file for manual browsing
 *   · show_conflict_check before committing a binding
 *   · label auto-detection from exe name / URL hostname
 */
import { invoke } from "@tauri-apps/api/core";
import { showToast } from "./toast";
import { getKeyCell, animateKeyPop, cleanLabel } from "./keyboard-matrix";
import type { AppConfig, AppInfo, KeyBinding, ConflictResult } from "../types.ts";

let _panel: HTMLElement | null = null;
let _backdrop: HTMLElement | null = null;
let _config: AppConfig | null = null;
let _currentKey: string | null = null;
let _onSave: ((key: string, binding: KeyBinding) => void) | null = null;
let _onClosed: (() => void) | null = null;

/** Detected apps, fetched once and reused for every key. */
let _apps: AppInfo[] | null = null;
let _appsPromise: Promise<AppInfo[]> | null = null;
let _query = "";

/** Fallback disc colours, matching the mockup's earthy set. */
const DISC_COLORS = [
  "#c67139", "#b08a3e", "#a8552f", "#8a6c4a",
  "#c2884e", "#6e3a15", "#7a8a5e", "#5f7052",
];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export function initKeyDetailPanel(
  panel: HTMLElement,
  config: AppConfig,
  onSave: (key: string, binding: KeyBinding) => void,
  onClosed?: () => void,
): void {
  _panel = panel;
  _backdrop = document.getElementById("editor-backdrop");
  _config = config;
  _onSave = onSave;
  _onClosed = onClosed ?? null;

  _backdrop?.addEventListener("click", () => closePanel());

  // Warm the app list in the background so the first open is not a blank grid.
  void loadApps();
}

export function openPanel(key: string, config: AppConfig, origin?: HTMLElement): void {
  _config = config;
  _currentKey = key;
  _query = "";

  if (!_panel) return;

  // --fx/--fy = vector from the stage centre to the key that was pressed.
  // The bloom animation starts there and lands centred; closing reverses it.
  const stage = document.getElementById("stage");
  const cell = origin ?? getKeyCell(key);
  if (stage && cell) {
    const sr = stage.getBoundingClientRect();
    const kr = cell.getBoundingClientRect();
    const dx = kr.left + kr.width / 2 - (sr.left + sr.width / 2);
    const dy = kr.top + kr.height / 2 - (sr.top + sr.height / 2);
    _panel.style.setProperty("--fx", `${Math.round(dx)}px`);
    _panel.style.setProperty("--fy", `${Math.round(dy)}px`);
  } else {
    _panel.style.setProperty("--fx", "0px");
    _panel.style.setProperty("--fy", "0px");
  }

  renderPanel(key);

  _panel.hidden = false;
  _panel.classList.remove("closing");
  _panel.classList.add("open");
  _panel.setAttribute("aria-hidden", "false");

  if (_backdrop) {
    _backdrop.hidden = false;
    // Next frame, so the opacity transition actually runs.
    requestAnimationFrame(() => _backdrop!.classList.add("shown"));
  }
  document.getElementById("stage")?.classList.add("editing");

  _panel.querySelector<HTMLInputElement>("#ed-search")?.focus();
}

export function closePanel(): void {
  if (!_panel || _panel.hidden) return;

  _panel.classList.remove("open");
  _panel.classList.add("closing");        // collapses back into the key
  _panel.setAttribute("aria-hidden", "true");
  _backdrop?.classList.remove("shown");
  document.getElementById("stage")?.classList.remove("editing");

  const key = _currentKey;
  _currentKey = null;

  window.setTimeout(() => {
    if (!_panel) return;
    if (_panel.classList.contains("open")) return;  // reopened mid-exit
    _panel.hidden = true;
    _panel.classList.remove("closing");
    if (_backdrop) _backdrop.hidden = true;
  }, 280);

  if (key && _onClosed) _onClosed();
}

export function getCurrentKey(): string | null {
  if (!_panel || _panel.hidden) return null;
  return _currentKey;
}

export function updatePanelConfig(config: AppConfig): void {
  _config = config;
  if (_currentKey && _panel && !_panel.hidden) renderPanel(_currentKey);
}

// ---------------------------------------------------------------------------
// Detected apps
// ---------------------------------------------------------------------------

function loadApps(): Promise<AppInfo[]> {
  if (_apps) return Promise.resolve(_apps);
  if (!_appsPromise) {
    _appsPromise = invoke<AppInfo[]>("list_start_menu_apps")
      .then((list) => { _apps = list; return list; })
      .catch(() => { _apps = []; return []; });
  }
  return _appsPromise;
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

function renderPanel(key: string): void {
  if (!_panel || !_config) return;

  const binding = getBinding(key);
  const bound = !!(binding && (binding.app || binding.web_url));
  const boundLabel = bound
    ? binding!.label ||
      (binding!.app
        ? cleanLabel(binding!.app.split(/[\\/]/).pop() || "")
        : cleanLabel(binding!.web_url || ""))
    : "";

  _panel.innerHTML = `
    <div class="ed-head">
      <span class="ed-cap" id="ed-cap"></span>
      <span class="ed-title-wrap">
        <span class="ed-title">Space + ${escapeHtml(key.toUpperCase())}</span>
        <span class="ed-sub" id="ed-sub"></span>
      </span>
      <button class="ed-close" id="ed-close" aria-label="Close">✕</button>
    </div>

    <input class="input" id="ed-search" placeholder="Search apps…" autocomplete="off" spellcheck="false" />

    <div class="ed-section">Apps on this device</div>
    <div id="ed-grid-scroll">
      <div id="ed-grid"></div>
      <div class="ed-empty" id="ed-empty" hidden></div>
    </div>

    <div id="ed-conflict" hidden></div>

    <!-- NOTE: this block is inside a template literal — no backticks in here,
         they terminate the string (TS1127 "Invalid character").
         The mockup has a "drag an .exe / URL here" affordance next to Browse.
         Removed at the user's request (2026-08-11): on Windows the .exe files
         people can actually find in Explorer are usually INSTALLERS
         (something-setup.exe), so inviting a drag points them at exactly the
         wrong file. Searching this list or pasting a path is clearer and
         correct. The drop handler on the keys still works if anyone tries it;
         it is simply no longer advertised. -->
    <div class="ed-row">
      <button class="btn ed-browse" id="ed-browse">Browse files…</button>
    </div>

    <div class="ed-row-tight">
      <input class="input" id="ed-path" placeholder="…or paste a file path / URL" autocomplete="off" spellcheck="false" />
      <button class="btn btn-primary" id="ed-assign" disabled>Assign</button>
    </div>

    <div class="ed-foot">
      ${bound ? `<button class="btn btn-danger" id="ed-remove">Remove binding</button>` : ""}
      <button class="btn btn-primary" id="ed-done">Done</button>
    </div>
  `;

  // --- header: real icon when we have one, otherwise the key letter ---
  const cap = _panel.querySelector<HTMLElement>("#ed-cap")!;
  if (binding?.icon_override) {
    const img = document.createElement("img");
    img.src = `data:image/png;base64,${binding.icon_override}`;
    img.alt = "";
    cap.appendChild(img);
  } else {
    cap.textContent = key.toUpperCase();
  }

  const sub = _panel.querySelector<HTMLElement>("#ed-sub")!;
  sub.textContent = bound ? `Bound to ${boundLabel}` : "Not bound yet";

  // --- wiring ---
  _panel.querySelector("#ed-close")!.addEventListener("click", () => closePanel());
  _panel.querySelector("#ed-done")!.addEventListener("click", () => closePanel());
  _panel.querySelector("#ed-browse")!.addEventListener("click", handleBrowse);
  _panel.querySelector("#ed-remove")?.addEventListener("click", handleRemove);

  const search = _panel.querySelector<HTMLInputElement>("#ed-search")!;
  search.value = _query;
  search.addEventListener("input", () => {
    _query = search.value;
    renderGrid();
  });

  const path = _panel.querySelector<HTMLInputElement>("#ed-path")!;
  const assign = _panel.querySelector<HTMLButtonElement>("#ed-assign")!;
  const syncAssign = () => { assign.disabled = path.value.trim().length === 0; };
  path.addEventListener("input", syncAssign);
  path.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && path.value.trim()) assignFromPath(path.value.trim());
  });
  assign.addEventListener("click", () => {
    if (path.value.trim()) assignFromPath(path.value.trim());
  });
  syncAssign();

  renderGrid();
}

function renderGrid(): void {
  const grid = _panel?.querySelector<HTMLElement>("#ed-grid");
  const empty = _panel?.querySelector<HTMLElement>("#ed-empty");
  if (!grid || !empty) return;

  const draw = (apps: AppInfo[]) => {
    const q = _query.trim().toLowerCase();
    const filtered = q ? apps.filter((a) => a.name.toLowerCase().includes(q)) : apps;

    // PROBLEM 97 — this was `slice(0, 60)` with the comment "the grid scrolls;
    // 60 is plenty". It is not plenty, and the truncation was SILENT: this
    // machine has 210 Start Menu shortcuts plus Store apps, so scrolling the
    // unfiltered grid showed only the first 60 alphabetically while searching
    // appeared to reveal apps that "weren't there" — because a query narrows
    // the set below the cap. The user reported exactly that.
    //
    // The cap now exists only as a rendering-cost backstop for an implausibly
    // large machine, and when it bites it SAYS SO. A list that quietly stops
    // is indistinguishable from a scanner that missed something.
    const RENDER_CAP = 500;
    const shown = filtered.slice(0, RENDER_CAP);
    const truncated = filtered.length - shown.length;

    grid.innerHTML = "";
    if (shown.length === 0) {
      empty.hidden = false;
      empty.textContent = apps.length === 0
        ? "No apps detected on this device"
        : `No apps match “${_query}”`;
      return;
    }
    empty.hidden = true;

    const currentApp = _currentKey ? getBinding(_currentKey)?.app ?? null : null;

    shown.forEach((app, i) => {
      const tile = document.createElement("div");
      tile.className = "ed-tile" + (currentApp === app.path ? " current" : "");
      tile.style.animationDelay = `${100 + Math.min(i, 20) * 22}ms`;
      tile.title = app.path;

      const disc = document.createElement("span");
      disc.className = "ed-tile-disc";
      const letterFallback = () => {
        disc.innerHTML = "";
        disc.style.background = DISC_COLORS[i % DISC_COLORS.length];
        disc.textContent = (app.name[0] || "?").toUpperCase();
      };
      if (app.icon_base64) {
        const img = document.createElement("img");
        // If the payload is ever malformed, show the letter disc rather than
        // the browser's broken-image glyph — a torn-paper icon on every tile
        // is what a CSP block looked like before `img-src data:` was added.
        img.onerror = letterFallback;
        img.src = `data:image/png;base64,${app.icon_base64}`;
        img.alt = "";
        disc.appendChild(img);
      } else {
        letterFallback();
      }

      const name = document.createElement("span");
      name.className = "ed-tile-name";
      name.textContent = app.name;         // textContent — user data

      tile.append(disc, name);
      tile.addEventListener("click", () =>
        commit({
          app: app.path,
          web_url: null,
          label: app.name,
          icon_override: app.icon_base64 ?? null,
        }),
      );
      grid.appendChild(tile);
    });

    // PROBLEM 97 — never let the list stop without saying why.
    if (truncated > 0) {
      const note = document.createElement("div");
      note.className = "ed-grid-note";
      note.style.cssText =
        "grid-column:1/-1; padding:8px 4px 2px; font-size:11px; opacity:.6; text-align:center;";
      note.textContent =
        `+${truncated} more app${truncated === 1 ? "" : "s"} — type in the search box to narrow the list`;
      grid.appendChild(note);
    }
  };

  if (_apps) draw(_apps);
  else {
    empty.hidden = false;
    empty.textContent = "Scanning this device…";
    void loadApps().then((list) => { if (_currentKey) draw(list); });
  }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

async function handleBrowse(): Promise<void> {
  let path: string | null = null;
  try {
    path = await invoke<string | null>("pick_file", {
      filterName: "Executable",
      filterExt: ["exe", "lnk"],
    });
  } catch (_) { /* dialog cancelled or unavailable */ }
  if (!path) return;
  await assignFromPath(path);
}

/** Shared by the paste row, Enter in that row, and Browse files…. */
async function assignFromPath(raw: string): Promise<void> {
  const value = raw.trim();
  if (!value || !_currentKey) return;

  const isUrl = /^https?:\/\//i.test(value);
  if (isUrl) {
    let host = value;
    try { host = new URL(value).hostname.replace(/^www\./, ""); } catch (_) { /* keep raw */ }
    commit({ app: null, web_url: value, label: cleanLabel(host), icon_override: null });
    return;
  }

  // PROBLEM 96 — refuse installers and background helpers. The picker filters
  // to .exe/.lnk, which is right, but `setup.exe` IS an .exe and looks just as
  // bindable as the real program; binding it re-runs the installer on every
  // key press. Checked HERE rather than in the Browse handler so the paste row
  // is covered by the same rule.
  try {
    const problem = await invoke<string | null>("check_app_path", { path: value });
    if (problem) {
      showToast(`⚠️ ${problem}`);
      return;
    }
  } catch (_) { /* if the check itself fails, do not block the user */ }

  let icon: string | null = null;
  try {
    icon = await invoke<string | null>("extract_icon_cmd", { exePath: value });
  } catch (_) { /* icon is a nicety */ }

  const name = value.split(/[\\/]/).pop() || value;
  commit({
    app: value,
    web_url: null,
    label: cleanLabel(name),
    icon_override: icon,
  });
}

function handleRemove(): void {
  commit({ app: null, web_url: null, label: null, icon_override: null }, true);
}

/**
 * Commit a binding: conflict-check first, then save, pop the key, close.
 * `skipConflict` is used by Remove — clearing a key can never conflict.
 */
async function commit(binding: KeyBinding, skipConflict = false): Promise<void> {
  const key = _currentKey;
  if (!key || !_onSave) return;

  if (!skipConflict) {
    try {
      const conflict = await invoke<ConflictResult>("show_conflict_check", {
        keyCombo: `Space+${key.toUpperCase()}`,
      });
      if (conflict.has_conflict) {
        showConflict(conflict, binding);
        return;
      }
    } catch (_) { /* the check is advisory; never block a binding on it */ }
  }

  _onSave(key, binding);

  const label = binding.label ?? key.toUpperCase();
  showToast(
    binding.app || binding.web_url
      ? `✅ Space+${key.toUpperCase()} → ${label}`
      : `🗑️ Cleared: Space+${key.toUpperCase()}`,
  );

  closePanel();
  // Pop the key after the editor has collapsed back into it.
  window.setTimeout(() => {
    const cell = getKeyCell(key);
    if (cell) animateKeyPop(cell);
  }, 220);
}

function showConflict(conflict: ConflictResult, binding: KeyBinding): void {
  const box = _panel?.querySelector<HTMLElement>("#ed-conflict");
  if (!box) return;
  box.hidden = false;
  box.innerHTML = `
    <div class="warning-banner" style="margin-top:12px; flex-direction:column; align-items:stretch;">
      <div><strong>${escapeHtml(conflict.conflicting_combo ?? "This shortcut")}</strong> conflicts with ${escapeHtml(conflict.description ?? "an existing shortcut")}.</div>
      <div style="display:flex; gap:8px; margin-top:8px;">
        <button class="btn btn-sm btn-primary" id="ed-conflict-go">Bind anyway</button>
        <button class="btn btn-sm" id="ed-conflict-no">Cancel</button>
      </div>
    </div>
  `;
  box.querySelector("#ed-conflict-go")!.addEventListener("click", () => {
    box.hidden = true;
    void commit(binding, true);
  });
  box.querySelector("#ed-conflict-no")!.addEventListener("click", () => {
    box.hidden = true;
  });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function getBinding(key: string): KeyBinding | undefined {
  const profile = _config?.profiles.find(
    (p: any) => p.name === _config?.active_profile,
  );
  return profile?.bindings[key];
}

/** Only used for strings we control (key letters, backend conflict text). */
function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!,
  );
}
