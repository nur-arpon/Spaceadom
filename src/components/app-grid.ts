/**
 * app-grid.ts — the grid of apps detected on this device.
 *
 * Extracted from key-detail-panel.ts on 2026-08-25 because the App-exceptions
 * setting needs the SAME picker the key editor uses (the owner's explicit
 * requirement: "the similar option of choosing apps when pressing letters
 * comes up"). It is SHARED, not forked — two copies of a list that has already
 * needed PROBLEM 97's truncation notice and the icon `onerror` fallback is
 * exactly how the two drift and only one gets the next fix.
 *
 * A LEAF module (PROBLEM 148): it imports nothing from main.ts, so both the
 * key editor and the settings panel can use it and the preview harness can
 * still render either one.
 */
import { invoke } from "@tauri-apps/api/core";
import type { AppInfo } from "../types.ts";

/** Detected apps, fetched once per session and reused by every caller. */
let _apps: AppInfo[] | null = null;
let _appsPromise: Promise<AppInfo[]> | null = null;

/** Fallback disc colours, matching the mockup's earthy set. */
const DISC_COLORS = [
  "#c67139", "#b08a3e", "#a8552f", "#8a6c4a",
  "#c2884e", "#6e3a15", "#7a8a5e", "#5f7052",
];

/**
 * Paint `disc` as a coloured letter fallback for `name` — the SAME rule
 * everywhere an app's icon might be missing (the grid tile, an exception
 * tile, a conflict row). `seed` (e.g. a list index) just spreads the colour
 * band so a run of icon-less entries doesn't render as one flat colour.
 */
export function paintLetterDisc(disc: HTMLElement, name: string, seed: number): void {
  disc.innerHTML = "";
  disc.style.background = DISC_COLORS[seed % DISC_COLORS.length];
  disc.textContent = (name[0] || "?").toUpperCase();
}

/**
 * Paint `disc` with `icon` (a base64 PNG, as returned by every app-info
 * command) or fall back to the letter disc — the one place this decision is
 * made, so a malformed payload can never show the browser's broken-image
 * glyph anywhere it is used.
 */
export function paintAppDisc(
  disc: HTMLElement,
  icon: string | null | undefined,
  name: string,
  seed: number,
): void {
  if (!icon) { paintLetterDisc(disc, name, seed); return; }
  const img = document.createElement("img");
  img.onerror = () => paintLetterDisc(disc, name, seed);
  img.src = `data:image/png;base64,${icon}`;
  img.alt = "";
  disc.innerHTML = "";
  disc.appendChild(img);
}

/**
 * PROBLEM 97 — this was `slice(0, 60)` with the comment "the grid scrolls; 60
 * is plenty". It is not plenty, and the truncation was SILENT: this machine
 * has 210 Start Menu shortcuts plus Store apps, so scrolling the unfiltered
 * grid showed only the first 60 alphabetically while searching appeared to
 * reveal apps that "weren't there" — because a query narrows the set below the
 * cap. The user reported exactly that.
 *
 * The cap now exists only as a rendering-cost backstop for an implausibly
 * large machine, and when it bites it SAYS SO. A list that quietly stops is
 * indistinguishable from a scanner that missed something.
 */
const RENDER_CAP = 500;

/** Kick off (or reuse) the Start-Menu scan. Never rejects. */
export function loadApps(): Promise<AppInfo[]> {
  if (_apps) return Promise.resolve(_apps);
  if (!_appsPromise) {
    _appsPromise = invoke<AppInfo[]>("list_start_menu_apps")
      .then((list) => { _apps = list; return list; })
      .catch(() => { _apps = []; return []; });
  }
  return _appsPromise;
}

/** The cached list, or null if the scan has not finished yet. */
export function cachedApps(): AppInfo[] | null {
  return _apps;
}

export interface AppGridOptions {
  /** Search text. Empty shows everything (up to RENDER_CAP). */
  query: string;
  /** Called with the app the user pressed. */
  onPick: (app: AppInfo) => void;
  /** Marks a tile as the current choice. */
  isCurrent?: (app: AppInfo) => boolean;
  /** Copy for "this device has no apps at all". */
  emptyText?: string;
  /** True while the caller is still waiting; shows the scanning note. */
  stillLoading?: boolean;
}

/**
 * Draw `apps` into `grid`, with `empty` carrying the "nothing here" copy.
 *
 * Both elements are the caller's; this only ever writes their contents, so the
 * key editor keeps its `#ed-grid` / `#ed-empty` ids and the settings panel can
 * use plain classes.
 */
export function renderAppGrid(
  grid: HTMLElement,
  empty: HTMLElement,
  apps: AppInfo[],
  opts: AppGridOptions,
): void {
  const q = opts.query.trim().toLowerCase();
  const filtered = q ? apps.filter((a) => a.name.toLowerCase().includes(q)) : apps;
  const shown = filtered.slice(0, RENDER_CAP);
  const truncated = filtered.length - shown.length;

  grid.innerHTML = "";
  if (shown.length === 0) {
    empty.hidden = false;
    empty.textContent = apps.length === 0
      ? (opts.emptyText ?? "No apps detected on this device")
      : `No apps match “${opts.query.trim()}”`;
    return;
  }
  empty.hidden = true;

  shown.forEach((app, i) => {
    const tile = document.createElement("div");
    tile.className = "ed-tile" + (opts.isCurrent?.(app) ? " current" : "");
    tile.style.animationDelay = `${100 + Math.min(i, 20) * 22}ms`;
    tile.title = app.path;

    const disc = document.createElement("span");
    disc.className = "ed-tile-disc";
    paintAppDisc(disc, app.icon_base64, app.name, i);

    const name = document.createElement("span");
    name.className = "ed-tile-name";
    name.textContent = app.name;         // textContent — user data

    tile.append(disc, name);
    tile.addEventListener("click", () => opts.onPick(app));
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
}

/**
 * The whole cycle: draw immediately from cache, or show "Scanning…" and draw
 * when the scan lands. `stillValid` lets the caller abandon a late result
 * (panel closed, different key opened).
 */
export function drawAppGrid(
  grid: HTMLElement,
  empty: HTMLElement,
  opts: AppGridOptions,
  stillValid: () => boolean = () => true,
): void {
  if (_apps) {
    renderAppGrid(grid, empty, _apps, opts);
    return;
  }
  empty.hidden = false;
  empty.textContent = "Scanning this device…";
  grid.innerHTML = "";
  void loadApps().then((list) => {
    if (stillValid()) renderAppGrid(grid, empty, list, opts);
  });
}

/**
 * The lowercase exe STEM for an app path — the exact form
 * `hook/exclusions.rs::normalize_stem` produces and stores, so the two sides
 * of the exception list agree without a translation step.
 */
export function exeStem(path: string): string {
  const name = path.trim().replace(/^"|"$/g, "").split(/[\\/]/).pop() ?? "";
  const lower = name.toLowerCase();
  return lower.endsWith(".exe") ? lower.slice(0, -4) : lower;
}

/**
 * The first cached app whose exe stem matches, or null if the scan hasn't
 * landed yet (or nothing matched). Bare filenames work too — `exeStem` only
 * splits on a path separator when one is present — so a running process's
 * plain "product.exe" name can be looked up directly, no path required.
 */
export function findAppByStem(stem: string): AppInfo | null {
  if (!_apps) return null;
  for (const a of _apps) if (exeStem(a.path) === stem) return a;
  return null;
}
