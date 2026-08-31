/**
 * browser-profile-picker.ts — "open this in a SPECIFIC browser profile".
 *
 * Chromium browsers take `--profile-directory="Profile 1"` and open straight
 * into one profile. This is the control that chooses it: a small chip in the key
 * editor that says "Opens normally" until the user presses it, and a SECOND PAGE
 * inside the editor listing every profile of the browser in question.
 *
 * WHY A PAGE AND NOT A POPOVER (owner's decision, 2026-08-26 handoff). The
 * popover this file used to draw (`.bp-pop`) hung off the chip and overflowed
 * the 460px panel by ~19px — measured: panel right edge 870, popover right edge
 * 888 — which gave the editor a horizontal scrollbar. More importantly the
 * picker was unreachable on the path people actually take: pressing a browser
 * tile in the app grid bound the key and destroyed the panel in the same tick,
 * so the chip never got a chance to exist. The page lives INSIDE the panel's
 * existing box, so it cannot overflow it and it cannot be missed.
 *
 * WHAT IT MUST NOT DO, and the reason it is shaped this way. The owner said it
 * twice: *"MAKE SURE THE DEFAULT BROWSER LAUNCHES FROM URL IF NOT EXPLICITLY
 * SET TO SPECIFIC."* So the chip starts at "Opens normally" and STAYS there
 * until somebody deliberately picks a profile — it never pre-selects, never
 * guesses from the URL, and never writes a value on open. The binding is
 * committed by the panel's existing instant-bind flow long before this control
 * is even drawn, so nothing here can slow that flow down or block it.
 *
 * A LEAF module (PROBLEM 148): it imports from app-grid.ts and the Tauri core
 * only — nothing from main.ts — so the preview harness can still render the key
 * editor that hosts it. `dismissable.ts` is no longer needed here: page 2 is not
 * a floating surface, it is a page of the editor, and the editor owns its own
 * Escape/backdrop handling.
 *
 * DELIBERATELY OUT OF SCOPE: extracting the browser's real profile AVATAR
 * images. They live in a versioned internal cache format that changes between
 * Chromium releases, and a wrong-looking avatar is worse than no avatar. Each
 * profile gets `paintLetterDisc` with its own initial, exactly like every other
 * missing-icon case in this app. The BROWSER's icon is real, because that comes
 * from the same shell extractor the app grid already uses.
 */
import { invoke } from "@tauri-apps/api/core";
import { paintAppDisc, paintLetterDisc } from "./app-grid";
import type { DetectedBrowser, BrowserProfile, DefaultBrowserInfo } from "../types.ts";

/**
 * A type-to-filter field appears above the grid once a browser holds MORE than
 * this many profiles. Six is not arbitrary: six tiles at 3 columns is two rows,
 * and the scroller is capped at three rows (246px), so six is the largest count
 * that needs no scrolling at all. Below it the field would be furniture.
 */
export const PROFILE_FILTER_THRESHOLD = 6;

/**
 * Last session's scan, persisted so the SECOND run paints in one frame.
 *
 * TODO (Rust, deferred): the handoff asks for this to be persisted "alongside
 * the config". There is no command for that today — `list_browser_profiles` is
 * the only browser IPC and it has no companion setter — so this uses the
 * WebView's own localStorage, which is per-origin and survives restarts. If a
 * Rust-side `last_known_browsers` field on AppConfig ever lands, move it there
 * and delete this: config.json is backed up and undoable, localStorage is not.
 */
const LS_KEY = "st-bp-browsers-v2";

/**
 * The v1 key, kept only to be deleted. 1.0.95 added `account_label` to every
 * profile, so a v1 entry is a valid-shaped list that is missing the one field
 * the tiles now lead with. Bumping the key is what makes a stale entry read as
 * "nothing cached" (`readLastKnown` returns null for an absent key) rather
 * than as a list of tiles labelled by the wrong field for one run.
 */
const LS_KEY_OLD = "st-bp-browsers-v1";

/**
 * The visible label for one profile: the signed-in account's local part, or
 * the browser's display name when it is not signed in.
 *
 * Rust computes it (`browser_profiles::account_label`) — this is the READ, and
 * the fallback exists solely for a list restored from an older localStorage
 * entry that predates the field. Every surface goes through here so no caller
 * can decide differently.
 */
export function labelOf(p: BrowserProfile): string {
  return p.account_label?.trim() || p.display_name;
}

/**
 * exe (lowercased) -> the friendly browser name it had when last seen.
 *
 * SEPARATE from the browser list and ADDITIVE, because it answers a question
 * the list cannot: what was the name of a browser that is no longer installed?
 * The list is replaced by every scan; this is merged by every scan and never
 * pruned. That is the whole point — "Brave (missing)" needs a name from a scan
 * where Brave was still there. A handful of browsers, so it cannot grow large.
 */
const LS_NAMES = "st-bp-names-v1";

/**
 * Detected browsers, fetched once per session and reused by every caller —
 * the same `_apps`/`_appsPromise` shape `app-grid.ts` uses, for the same
 * reason. The Rust side caches too; this stops the IPC round trip as well.
 *
 * The scan walks AppData and takes ~1.5s on the owner's machine, so it is
 * warmed in the background by `warmBrowsers()` rather than being paid for on
 * the first press.
 */
let _browsers: DetectedBrowser[] | null = null;
let _browsersPromise: Promise<DetectedBrowser[]> | null = null;

/** Last session's list, read from localStorage exactly once. */
let _lastKnown: DetectedBrowser[] | null = readLastKnown();

const _names: Record<string, string> = readNames();
if (_lastKnown) rememberNames(_lastKnown);

function readNames(): Record<string, string> {
  try {
    const parsed = JSON.parse(localStorage.getItem(LS_NAMES) || "{}");
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed : {};
  } catch (_) {
    return {};
  }
}

function rememberNames(list: DetectedBrowser[]): void {
  let changed = false;
  for (const b of list) {
    const k = b.browser_exe.trim().toLowerCase();
    if (_names[k] !== b.browser_name) { _names[k] = b.browser_name; changed = true; }
  }
  if (!changed) return;
  try { localStorage.setItem(LS_NAMES, JSON.stringify(_names)); } catch (_) { /* quota */ }
}

function readLastKnown(): DetectedBrowser[] | null {
  try {
    // One-shot cleanup of the superseded key, so the bump does not leave a
    // full browser list (icons included) parked in quota forever.
    localStorage.removeItem(LS_KEY_OLD);
    const raw = localStorage.getItem(LS_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    // Shape-check rather than trust: a stale key from an older schema must read
    // as "nothing cached", not crash the editor on its first paint.
    if (!Array.isArray(parsed)) return null;
    const ok = parsed.filter(
      (b: any) =>
        b && typeof b.browser_exe === "string" && Array.isArray(b.profiles),
    ) as DetectedBrowser[];
    return ok.length ? ok : null;
  } catch (_) {
    return null;
  }
}

function writeLastKnown(list: DetectedBrowser[]): void {
  try {
    localStorage.setItem(LS_KEY, JSON.stringify(list));
  } catch (_) {
    // Quota. The icons are the only large part (base64 PNGs), and the icon is
    // the one thing that can be re-extracted for free next run — so drop them
    // rather than lose the whole cache and paint an empty page 2 forever.
    try {
      localStorage.setItem(
        LS_KEY,
        JSON.stringify(list.map((b) => ({ ...b, icon_base64: null }))),
      );
    } catch (_) { /* nothing cached this run; the page still works, just later */ }
  }
}

/** Kick off (or reuse) the browser scan. Never rejects. */
export function loadBrowsers(): Promise<DetectedBrowser[]> {
  if (_browsers) return Promise.resolve(_browsers);
  if (!_browsersPromise) {
    _browsersPromise = invoke<DetectedBrowser[]>("list_browser_profiles")
      .then((list) => {
        _browsers = list;
        _lastKnown = list;
        writeLastKnown(list);
        rememberNames(list);
        return list;
      })
      .catch(() => { _browsers = []; return []; });
  }
  return _browsersPromise;
}

/** The FRESH list, or null if this session's scan has not landed yet. */
export function cachedBrowsers(): DetectedBrowser[] | null {
  return _browsers;
}

/**
 * The best list available RIGHT NOW: this session's scan if it has landed,
 * otherwise last session's. Null only on a genuine first run.
 *
 * This is the "second run onward" decision made concrete — everything that has
 * to answer instantly (does this exe have profiles? what does the chip say?)
 * reads this, and everything that needs the truth awaits `loadBrowsers()`.
 */
export function knownBrowsers(): DetectedBrowser[] | null {
  return _browsers ?? _lastKnown;
}

/** True while a fresh scan is still in flight — drives the `checking` dot. */
export function isRefreshingBrowsers(): boolean {
  return _browsers === null;
}

/** Start the scan without waiting for it. Called when the editor initialises. */
export function warmBrowsers(): void {
  void loadBrowsers();
}

/**
 * Is `path` one of the detected browsers? Compared by EXACT path, not by name:
 * "Chrome" is a name several things answer to, while
 * `C:\Program Files\Google\Chrome\Application\chrome.exe` is one program.
 * Case-insensitive because Windows paths are.
 *
 * Returns null when NOTHING is known yet, which the caller must treat as "not
 * yet known" rather than "no". On the second run onward that case does not
 * arise, because last session's list is already in hand.
 */
export function findBrowserByExe(path: string): DetectedBrowser | null {
  const list = knownBrowsers();
  if (!list) return null;
  const want = path.trim().toLowerCase();
  return list.find((b) => b.browser_exe.toLowerCase() === want) ?? null;
}

export interface ProfileSelection {
  /**
   * Which browser to NAME. For a URL binding this is the stored `browser_exe`;
   * for an APP binding it is `binding.app`, because the exe already is the app
   * and storing it twice is how the two get to disagree. Either way the chip
   * needs it to look the browser up and read the profile's display name.
   */
  browserExe: string | null;
  /** The pinned profile folder, or null. */
  profileDir: string | null;
  /**
   * The profile's human name AS STORED IN THE BINDING. It is the only label
   * available when the profile folder has since been deleted — the browser's
   * Local State no longer lists it, so "ARPON'S STUDIES missing" can only come
   * from here. Without it the chip falls back to the folder name and reads
   * "Default missing", which names the wrong thing.
   */
  profileName?: string | null;
  /**
   * True only when the binding actually STORES a browser choice — i.e. a URL
   * pinned to a specific browser. It is what separates "this key opens Brave
   * because it is bound to Brave" from "somebody pinned this to Brave", and
   * therefore whether the chip is filled and carries a ✕ that has something to
   * clear. A ✕ that writes three nulls over three nulls is a control that can
   * do nothing.
   */
  exePinned?: boolean;
}

export interface ProfileChipHandlers {
  /** Chip body pressed — open page 2. `browser` is null when it is not known. */
  onOpen: (browser: DetectedBrowser | null) => void;
  /** The ✕ — clear the pin (three nulls). Absent while nothing is pinned. */
  onClear: () => void;
}

/**
 * Mount the chip into `container`.
 *
 * `current` is what the binding holds right now (both null on a fresh bind).
 * The chip no longer picks anything itself — the body opens page 2 and the ✕
 * clears — so it has no `onChange`. That is the fix: a popover hanging off a
 * 460px panel had nowhere to go, and the page it now opens is the same width as
 * the panel by construction.
 */
export function renderProfileChip(
  container: HTMLElement,
  current: ProfileSelection,
  handlers: ProfileChipHandlers,
): void {
  container.innerHTML = "";
  container.classList.add("bp-host");

  const chip = document.createElement("button");
  chip.type = "button";
  chip.className = "bp-chip";
  chip.setAttribute("aria-haspopup", "true");
  container.appendChild(chip);

  // The ✕ lives OUTSIDE the chip button — a button inside a button is invalid
  // HTML and the inner one's clicks are unreliable across engines. It is
  // positioned into the chip's trailing edge by CSS instead.
  let clearBtn: HTMLButtonElement | null = null;

  const paintChip = (sel: ProfileSelection): void => {
    chip.innerHTML = "";
    clearBtn?.remove();
    clearBtn = null;

    const disc = document.createElement("span");
    disc.className = "ed-tile-disc bp-chip-disc";

    const browser = sel.browserExe ? findBrowserByExe(sel.browserExe) : null;
    // Only claim something is MISSING once a list actually exists. Before that
    // the honest answer is "not yet known", and a warning-coloured chip on a
    // machine where the scan simply has not landed is a lie.
    const scanned = knownBrowsers() !== null;

    chip.classList.remove("bp-chip-set", "bp-chip-warn");

    if (!sel.profileDir && !sel.exePinned) {
      // The default state, and the one every existing binding is in.
      disc.classList.add("bp-chip-disc-none");
      disc.textContent = "◍";
      const text = document.createElement("span");
      text.className = "bp-chip-text bp-chip-profile";
      text.textContent = "Opens normally";
      chip.title = "Opens in your default browser, as usual";
      const caret = document.createElement("span");
      caret.className = "bp-chip-caret";
      caret.textContent = "▾";
      chip.append(disc, text, caret);
      return;
    }

    chip.classList.add("bp-chip-set");

    // A pinned browser that is no longer installed. The binding is NOT
    // rewritten — reinstalling it must simply work again — so the chip carries
    // the stored name, which is the only thing available offline.
    if (sel.browserExe && sel.exePinned && scanned && !browser) {
      chip.classList.add("bp-chip-warn");
      disc.classList.add("bp-chip-disc-warn");
      disc.textContent = "!";
      const name = document.createElement("span");
      name.className = "bp-chip-browser";
      // The binding stores no browser NAME — only the exe, the profile folder
      // and the profile's display name (types.ts). So the friendly name comes
      // from LAST SESSION'S list, which still has it, and only falls back to
      // the exe's leaf when even that is gone. Measured without this: an
      // uninstalled Brave read "brave.exe (missing)", which is true but is not
      // what the user called it.
      name.textContent = `${rememberedName(sel.browserExe) ?? exeLeaf(sel.browserExe)} (missing)`;
      const sep = document.createElement("span");
      sep.className = "bp-chip-sep";
      sep.textContent = "→";
      const prof = document.createElement("span");
      prof.className = "bp-chip-profile";
      prof.textContent = "default";
      chip.title = `${sel.browserExe} is not installed — links open in your default browser`;
      chip.append(disc, name, sep, prof);
      addClear();
      return;
    }

    const browserLabel = browser?.browser_name ?? exeLeaf(sel.browserExe ?? "");
    paintAppDisc(disc, browser?.icon_base64, browserLabel, 0);

    const name = document.createElement("span");
    name.className = "bp-chip-browser";        // flex-shrink: 0 — never truncates
    name.textContent = browserLabel;           // textContent — user data
    chip.append(disc, name);

    if (sel.profileDir) {
      const profile = browser?.profiles.find((p) => p.directory === sel.profileDir);
      const gone = !!browser && scanned && !profile;
      const sep = document.createElement("span");
      sep.className = "bp-chip-sep";
      sep.textContent = "·";
      const prof = document.createElement("span");
      // min-width: 0 + ellipsis — the profile name is the only thing that gives.
      prof.className = "bp-chip-profile" + (gone ? " bp-chip-gone" : "");
      // The ACCOUNT LABEL, same as the picker headline and the HUD chip. A
      // pin made before 1.0.95 stored the display name instead; it still
      // reads correctly here because the live profile is preferred and only
      // an uninstalled browser falls back to the stored string.
      prof.textContent = gone
        ? `${sel.profileName ?? sel.profileDir} missing`
        : profile ? labelOf(profile) : sel.profileName ?? sel.profileDir;
      if (gone) chip.classList.add("bp-chip-warn");
      // A gone profile has no live info_cache entry to read an email from —
      // only the live case can ever know one, which is also why this must
      // read `profile?.email` and never `sel` (the stored binding carries no
      // email at all — see ProfileSelection). The TOOLTIP is the only place
      // the full address is ever shown.
      chip.title = gone
        ? `${browserLabel} still opens, but the profile folder "${sel.profileDir}" is gone`
        : profile?.email
          ? `Opens in ${browserLabel}, profile "${profile.display_name}" (${profile.email})`
          : `Opens in ${browserLabel}, profile "${prof.textContent}"`;
      chip.append(sep, prof);
    } else {
      chip.title = `Opens in ${browserLabel}`;
    }

    addClear();
  };

  function addClear(): void {
    clearBtn = document.createElement("button");
    clearBtn.type = "button";
    clearBtn.className = "bp-chip-x";
    clearBtn.textContent = "✕";
    clearBtn.title = "Clear this pin — the link goes back to your default browser";
    clearBtn.setAttribute("aria-label", "Clear browser profile pin");
    clearBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      console.info("bp: pin cleared from chip ✕");
      handlers.onClear();
    });
    container.appendChild(clearBtn);
  }

  let selection: ProfileSelection = { ...current };
  paintChip(selection);
  // The pin path is INSTRUMENTED end to end (2026-08-26). It shipped broken and
  // silent, and "I clicked it and nothing happened" is not a diagnosis — these
  // lines (chip / open / pick, plus commit+onSave in key-detail-panel.ts) say
  // exactly how far a press got. Kept to one line per step deliberately.
  console.info(
    `bp: chip rendered — exe=${current.browserExe ?? "(default)"} dir=${current.profileDir ?? "(none)"} ` +
    `scanned=${knownBrowsers()?.length ?? "pending"}${isRefreshingBrowsers() ? " (refreshing)" : ""}`,
  );

  // If the chip is drawn before ANY list exists, its labels and the browser icon
  // are the placeholder ones. Repaint once real data arrives.
  if (!knownBrowsers()) {
    void loadBrowsers().then(() => {
      if (container.isConnected) paintChip(selection);
    });
  }

  chip.addEventListener("click", () => {
    const browser = selection.browserExe ? findBrowserByExe(selection.browserExe) : null;
    console.info(
      `bp: chip opened page — ${browser ? browser.browser_name : "no pinned browser"}; ` +
      `${knownBrowsers()?.length ?? 0} browser(s) known`,
    );
    handlers.onOpen(browser);
  });
}

// ---------------------------------------------------------------------------
// Page 2 — the profile page
// ---------------------------------------------------------------------------

export interface ProfilePageOptions {
  /** "Which Brave profile?" */
  title: string;
  /** "Space + W is already bound to Brave" */
  subtitle: string;
  /** "Skip this and Space + W opens Brave the way it always has." */
  hint: string;
  /**
   * Which browsers to list. ONE entry = the app-grid flow ("Which Brave
   * profile?"). MANY = a URL that has not chosen a browser yet.
   */
  browsers: DetectedBrowser[];
  /** What the binding holds now, so the right tile reads `.current`. */
  selection: ProfileSelection;
  /**
   * The leading "no specific choice" row. null hides it — which is right when
   * there is nothing to undo. It is NEVER pre-selected on a fresh binding: the
   * absence of a pin is already the default-browser behaviour.
   */
  resetLabel: string | null;
  /**
   * A profile tile was pressed. The page reports WHICH BROWSER'S tile it was;
   * the caller decides whether to store `browser_exe` (URL binding) or to leave
   * it null because the exe already IS `binding.app` (app binding). Two sources
   * of truth for one path is the bug this split avoids.
   *
   * `label` is the ACCOUNT LABEL (`labelOf`) — the email's local part, or the
   * display name when unsigned. It is what gets stored as
   * `browser_profile_name`, which is why the FULL address is never passed:
   * that field is written to `config.json` and read back into the HUD.
   */
  onPick: (browserExe: string, dir: string, label: string) => void;
  /** The reset row was pressed. */
  onReset: () => void;
  /** ← — back to page 1 with the binding intact. */
  onBack: () => void;
  /** ✕ — close the whole editor. */
  onClose: () => void;
  /** Done — close the editor, keeping whatever is bound. */
  onDone: () => void;
}

/**
 * Build the page-2 body into `host`.
 *
 * This is `drawPicker`'s old popover body, re-homed: same tiles, same letter
 * discs, same "one header per browser even when there is only one" rule. What
 * is new is the header, the filter, the count, and the cache-first paint.
 */
export function renderProfilePage(host: HTMLElement, opts: ProfilePageOptions): void {
  host.innerHTML = "";
  host.className = "bp-page";

  let filter = "";

  // ---- header -------------------------------------------------------------
  const head = document.createElement("div");
  head.className = "bp-page-head";

  const back = document.createElement("button");
  back.type = "button";
  back.className = "bp-page-btn bp-back";
  back.textContent = "←";
  back.title = "Back to the app list — the key stays bound";
  back.setAttribute("aria-label", "Back");
  back.addEventListener("click", opts.onBack);

  const titles = document.createElement("span");
  titles.className = "bp-page-titles";
  const title = document.createElement("span");
  title.className = "bp-page-title";
  title.textContent = opts.title;              // textContent — carries user data
  const sub = document.createElement("span");
  sub.className = "bp-page-sub";
  sub.textContent = opts.subtitle;
  titles.append(title, sub);

  const close = document.createElement("button");
  close.type = "button";
  close.className = "bp-page-btn bp-page-close";
  close.textContent = "✕";
  close.title = "Close";
  close.setAttribute("aria-label", "Close");
  close.addEventListener("click", opts.onClose);

  head.append(back, titles, close);

  const rule = document.createElement("div");
  rule.className = "bp-page-rule";

  // ---- body ---------------------------------------------------------------
  const body = document.createElement("div");
  body.className = "bp-page-body";

  const totalProfiles = opts.browsers.reduce((n, b) => n + b.profiles.length, 0);

  // The filter, present only above the threshold. In the multi-browser case the
  // count that matters is the TOTAL on screen, for the same reason: it is how
  // many names the user would otherwise have to read.
  let filterInput: HTMLInputElement | null = null;
  if (totalProfiles > PROFILE_FILTER_THRESHOLD) {
    const wrap = document.createElement("div");
    wrap.className = "bp-filter-wrap";

    const glyph = document.createElement("span");
    glyph.className = "bp-filter-glyph";
    glyph.textContent = "⌕";

    filterInput = document.createElement("input");
    filterInput.type = "text";
    filterInput.className = "bp-filter";
    filterInput.placeholder = `Filter ${totalProfiles} profiles…`;
    filterInput.autocomplete = "off";
    filterInput.spellcheck = false;

    const clear = document.createElement("button");
    clear.type = "button";
    clear.className = "bp-filter-x";
    clear.textContent = "✕";
    clear.hidden = true;
    clear.title = "Clear the filter";
    clear.setAttribute("aria-label", "Clear the filter");
    clear.addEventListener("click", () => {
      filter = "";
      filterInput!.value = "";
      clear.hidden = true;
      filterInput!.focus();
      paint(knownBrowsers());
    });

    filterInput.addEventListener("input", () => {
      filter = filterInput!.value;
      clear.hidden = filter.length === 0;
      console.info(`bp: page filter — "${filter}"`);
      paint(knownBrowsers());
    });

    wrap.append(glyph, filterInput, clear);
    body.appendChild(wrap);
  }

  // The leading "back to normal" row, when there is something to undo.
  if (opts.resetLabel) {
    const reset = document.createElement("button");
    reset.type = "button";
    reset.className = "bp-reset";
    reset.textContent = opts.resetLabel;
    reset.addEventListener("click", opts.onReset);
    body.appendChild(reset);
  }

  const scroll = document.createElement("div");
  scroll.className = "bp-scroll";
  body.appendChild(scroll);

  // ---- footer -------------------------------------------------------------
  const foot = document.createElement("div");
  foot.className = "bp-page-foot";
  const hint = document.createElement("span");
  hint.className = "bp-page-hint";
  hint.textContent = opts.hint;
  const done = document.createElement("button");
  done.type = "button";
  done.className = "btn btn-primary bp-done";
  done.textContent = "Done";
  done.addEventListener("click", opts.onDone);
  foot.append(hint, done);

  host.append(head, rule, body, foot);

  // ---- the grid, repainted on every filter keystroke and on every refresh ---
  const paint = (source: DetectedBrowser[] | null, xfade = false): void => {
    scroll.innerHTML = "";

    // Re-read the browsers from `source` by exe, so a refresh that renamed or
    // removed a profile lands here without the caller re-opening the page.
    const live = opts.browsers.map((want) => {
      const fresh = source?.find(
        (b) => b.browser_exe.toLowerCase() === want.browser_exe.toLowerCase(),
      );
      return fresh ?? want;
    });

    const q = filter.trim().toLowerCase();
    let shown = 0;
    let total = 0;

    for (const b of live) {
      total += b.profiles.length;
      // Matches the account label, the display name, the folder name AND the
      // full address — so typing "Profile 9" finds the profile whose owner
      // never named it, and typing the domain finds every account on it even
      // though the domain is not rendered anywhere on the tile.
      const profiles = q
        ? b.profiles.filter(
            (p) =>
              labelOf(p).toLowerCase().includes(q) ||
              p.display_name.toLowerCase().includes(q) ||
              (p.email ?? "").toLowerCase().includes(q) ||
              p.directory.toLowerCase().includes(q),
          )
        : b.profiles;
      shown += profiles.length;
      if (q && profiles.length === 0) continue;

      // The header is drawn even when only ONE browser was found (owner's
      // decision 2026-08-26): the page must have the same structure on every
      // machine, so nobody has to learn two layouts.
      const head2 = document.createElement("div");
      head2.className = "bp-browser";
      const hdisc = document.createElement("span");
      hdisc.className = "ed-tile-disc bp-browser-disc";
      paintAppDisc(hdisc, b.icon_base64, b.browser_name, 0);
      const hname = document.createElement("span");
      hname.className = "bp-browser-name";
      hname.textContent = b.browser_name;      // textContent — user data
      const count = document.createElement("span");
      count.className = "bp-count";
      // While filtering the count says "3 of 15" — the filter must never hide
      // how much it hid.
      count.textContent = q
        ? `${profiles.length} of ${b.profiles.length}`
        : `${b.profiles.length} profile${b.profiles.length === 1 ? "" : "s"}`;
      head2.append(hdisc, hname, count);

      // Cache-first paint: a 5px sage dot and the word "checking" say a refresh
      // is running behind last session's list. No spinner — the list is already
      // usable, and a spinner over usable content reads as "wait".
      if (isRefreshingBrowsers()) {
        const chk = document.createElement("span");
        chk.className = "bp-checking";
        chk.innerHTML = "";
        const dot = document.createElement("i");
        dot.className = "bp-checking-dot";
        const word = document.createElement("span");
        word.textContent = "checking";
        chk.append(dot, word);
        head2.appendChild(chk);
      }

      head2.title = b.browser_exe;
      scroll.appendChild(head2);

      const grid = document.createElement("div");
      grid.className = "ed-grid bp-grid" + (xfade ? " bp-xfade" : "");
      profiles.forEach((p, i) => {
        const tile = document.createElement("div");
        // The SAME tile vocabulary as the app grid — .ed-tile carries the
        // radius, hover lift, press shrink, `.current` accent and the
        // content-visibility budget. `.bp-tile` adds only what genuinely
        // differs: these nest under a browser header, so they are smaller, and
        // they use min-height so a long name grows the tile instead of clipping.
        const isCurrent =
          opts.selection.browserExe?.toLowerCase() === b.browser_exe.toLowerCase() &&
          opts.selection.profileDir === p.directory;
        tile.className = "ed-tile bp-tile" + (isCurrent ? " current" : "");
        tile.style.animationDelay = `${60 + Math.min(i, 12) * 18}ms`;
        // THE ONLY PLACE THE FULL ADDRESS APPEARS (owner, 2026-08-31). Two
        // profiles can share a display name — the whole reason this feature
        // exists — and two accounts can share a local part across domains, so
        // the tooltip is where the whole address settles it. `null`/absent
        // (not signed in) adds nothing; never an empty segment.
        const label = labelOf(p);
        tile.title = p.email
          ? `${b.browser_name} — ${p.display_name} — ${p.email}  (${p.directory})`
          : `${b.browser_name} — ${p.display_name}  (${p.directory})`;

        const disc = document.createElement("span");
        disc.className = "ed-tile-disc";
        // Profile AVATARS are deliberately not extracted (see the file header).
        // The initial is the same fallback every other missing icon gets, and
        // it is seeded from the LABEL so the letter matches the headline.
        // `i + 1` seeds off the browser's own disc colour so a browser and its
        // first profile do not come out the same shade.
        paintLetterDisc(disc, label, i + 1);

        const name = document.createElement("span");
        name.className = "ed-tile-name";
        name.textContent = label;              // textContent — user data

        tile.append(disc, name);
        // The browser's own name for the profile, as a SECOND, dimmer line —
        // present only when the headline is an account label, i.e. when the
        // profile is signed in AND the two actually differ. A profile that is
        // not signed in already has its name as the headline; repeating it
        // would be the "Brave — Brave" mistake `hud_label` guards against, and
        // a tile with no second line must not grow at all (the grid's
        // min-height is the floor; this element simply does not exist there).
        if (p.email && label.toLowerCase() !== p.display_name.toLowerCase()) {
          const sub = document.createElement("span");
          sub.className = "bp-tile-sub";
          sub.textContent = p.display_name;     // textContent — user data
          tile.appendChild(sub);
        }
        tile.addEventListener("click", () => {
          // The label is derived from an email address, so it does NOT go in
          // the log — `dir` identifies the tile just as well and identifies
          // nobody. Same rule in key-detail-panel's commit line.
          console.info(`bp: profile picked — exe=${b.browser_exe} dir=${p.directory}`);
          opts.onPick(b.browser_exe, p.directory, label);
        });
        grid.appendChild(tile);
      });
      scroll.appendChild(grid);
    }

    if (live.length === 0) {
      // Same tone as app-grid's own empty state: say WHAT WAS LOOKED FOR, so a
      // genuinely empty machine is distinguishable from a scan that failed.
      const note = document.createElement("div");
      note.className = "bp-nomatch";
      const t = document.createElement("strong");
      t.textContent = "No Chromium browsers found";
      const d = document.createElement("span");
      d.textContent =
        "Spaceadom looked in AppData and Program Files for anything with a " +
        "Chromium profile store. Links will keep opening in your default browser.";
      note.append(t, d);
      scroll.appendChild(note);
    } else if (q && shown === 0) {
      const note = document.createElement("div");
      note.className = "bp-nomatch";
      const t = document.createElement("strong");
      t.textContent = `No profile matches “${filter.trim()}”`;
      const d = document.createElement("span");
      d.textContent = `Clear the filter to see all ${total} again.`;
      note.append(t, d);
      scroll.appendChild(note);
    }

    // The bottom fade is a gradient stop on the page's own colour, painted ONLY
    // when there is something below the fold. A permanent fade over a short
    // list dims the last row for no reason.
    scroll.classList.toggle("is-scrollable", scroll.scrollHeight > scroll.clientHeight);
  };

  paint(knownBrowsers());

  // Cache-first: last session's list is already on screen. When the fresh scan
  // lands, repaint in place with a 300ms cross-fade — no spinner, no jump, and
  // no re-layout, because the page's box is fixed by the panel.
  if (isRefreshingBrowsers()) {
    void loadBrowsers().then((list) => {
      if (!host.isConnected) return;
      console.info(`bp: page refreshed — ${list.length} browser(s) from the live scan`);
      paint(list, true);
    });
  }

  // The filter is the only thing on this page worth typing into, so it takes
  // focus. Nothing else here is keyboard-hostile: ← ✕ and Done are all buttons.
  filterInput?.focus();
}

// ---------------------------------------------------------------------------
// The OS default browser — for 4b's leading disc in the paste row
// ---------------------------------------------------------------------------

/**
 * Resolved from `get_default_browser`, which reads the http/https UserChoice
 * handler — the SAME resolver `run_browser` uses to actually launch a URL.
 * That is the point of the command existing: the disc physically cannot name a
 * different browser than the key opens, because there is one source of truth
 * rather than two guesses. (The old candidate-list guess, `find_browser_cmd`,
 * would have shown Brave's icon on a machine whose default is Firefox.)
 *
 * `null` from Rust means no handler is registered, or its command line will not
 * parse — the same condition that makes launching a URL fail. The disc falls
 * back to the unset treatment then, never to a guess.
 */
let _default: DefaultBrowserInfo | null = null;
let _defaultPromise: Promise<DefaultBrowserInfo | null> | null = null;

/** Kick off (or reuse) the default-browser lookup. Never rejects. */
export function loadDefaultBrowser(): Promise<DefaultBrowserInfo | null> {
  if (_default) return Promise.resolve(_default);
  if (!_defaultPromise) {
    _defaultPromise = invoke<DefaultBrowserInfo | null>("get_default_browser")
      .then((info) => { _default = info ?? null; return _default; })
      .catch(() => null);
  }
  return _defaultPromise;
}

/** The resolved default browser, or null if the lookup has not landed / failed. */
export function cachedDefaultBrowser(): DefaultBrowserInfo | null {
  return _default;
}

/** Start the lookup without waiting for it. Cheap and cached on the Rust side. */
export function warmDefaultBrowser(): void {
  void loadDefaultBrowser();
}

/**
 * The friendly name this exe had LAST time it was seen, or null.
 *
 * Deliberately reads `_lastKnown` and not `knownBrowsers()`: the whole point is
 * to answer for a browser that the CURRENT scan no longer finds.
 */
function rememberedName(exe: string): string | null {
  return _names[exe.trim().toLowerCase()] ?? null;
}

/** "…\Application\brave.exe" -> "brave.exe". Used only when neither the scan
 *  nor last session's list has an entry for a stored path. */
function exeLeaf(path: string): string {
  return path.trim().replace(/^"|"$/g, "").split(/[\\/]/).pop() || path;
}
