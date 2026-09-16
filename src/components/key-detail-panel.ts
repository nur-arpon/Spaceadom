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
// The app grid is SHARED with the App-exceptions setting (2026-08-25). Do not
// re-inline it here: two copies drift and only one gets the next fix.
import { loadApps, cachedApps, drawAppGrid, paintAppDisc, initPickerRefreshListener } from "./app-grid";
// PROBLEM 242 — the first-run tour watches the editor from OUTSIDE. These five
// calls are the whole integration: they are placed here, not in main.ts, because
// this file is the one door both the real dashboard and preview.html go through,
// and a hook wired at the caller would exist in only one of them. tour.ts is a
// leaf and imports nothing back, so there is no cycle.
//
// The last two are the 2026-09-06 follow-up: the tour has a step 2b for the
// browser-profile picker, and `openProfilePage`/`closeProfilePage` below are the
// only two functions in the app that know page 2 exists.
import {
  tourEditorOpened,
  tourBindingSaved,
  tourEditorClosed,
  tourProfilePickerOpened,
  tourProfilePickerClosed,
} from "./tour";
// "Open this in a specific browser profile" (2026-08-26). Also a leaf module.
import {
  warmBrowsers,
  warmDefaultBrowser,
  findBrowserByExe,
  knownBrowsers,
  loadBrowsers,
  renderProfileChip,
  renderProfilePage,
  loadDefaultBrowser,
  cachedDefaultBrowser,
  labelOf,
} from "./browser-profile-picker";
import type {
  AppConfig,
  KeyBinding,
  ConflictResult,
  DetectedBrowser,
  DefaultBrowserInfo,
} from "../types.ts";

let _panel: HTMLElement | null = null;
let _backdrop: HTMLElement | null = null;
let _config: AppConfig | null = null;
let _currentKey: string | null = null;
let _onSave: ((key: string, binding: KeyBinding) => void) | null = null;
let _onClosed: (() => void) | null = null;

let _query = "";

/**
 * Page 2 — the browser-profile page — while it is up. It is a child of the
 * panel, absolutely positioned over page 1, so page 1 STAYS IN THE DOM and the
 * panel's height never changes. That is the whole reason it is drawn this way:
 * the panel must not resize, because the keyboard behind it is scaled to fit
 * and any change to the panel's box would make the board re-layout.
 */
let _page: HTMLElement | null = null;
/** Cleared by `closeProfilePage`; the exit tween needs a handle on its timer. */
let _pageTimer = 0;

/**
 * The EXACT value the assigned-value pill loaded back into `#ed-path` for
 * editing, or null when the field's contents are the user's own.
 *
 * It exists to answer one question that nothing else can: "is the text in this
 * field something already saved, or something new?" Before the pill there was
 * no way for the field to hold an already-committed value, so every non-empty
 * field meant "the user typed this" and re-assigning on Done was always right
 * (PROBLEM 199). Clicking the pill body breaks that assumption.
 *
 * WHY THIS MATTERS AND IS NOT DEFENSIVE PADDING: `assignFromPath`'s URL branch
 * deliberately omits the three browser-profile fields so that re-pointing a key
 * CLEARS the pin, and `commit()` normalises them to null. So re-committing an
 * unchanged URL would silently wipe a browser-profile pin — which is verbatim
 * the failure `assignFromPath`'s own comment records: *"the same omission
 * silently wiped a pin the user had just set on a url they were only editing."*
 * Clicking a pill to look at a URL and pressing Done is exactly "only editing".
 */
let _pathSeed: string | null = null;

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

  // PROBLEM 205 — the three warm-ups that USED to run here have moved to
  // `warmPickerData()`, fired on the FIRST openPanel(). Read this before
  // moving them back:
  //
  // `initKeyDetailPanel` is on the critical path to first paint. main.ts
  // calls it during bootstrap(), and bootstrap()'s LAST act is
  // `dashboard_ready`, which is the only thing that shows the window
  // (PROBLEM 74). Every command these warm-ups fire — `list_start_menu_apps`,
  // `list_browser_profiles`, `get_default_browser` — is a NON-async
  // `#[tauri::command]`, so each one runs on the MAIN THREAD and every other
  // IPC call from both webviews queues behind it. Measured on the owner's
  // machine: ~12s + ~2.5s of main-thread block, inside a 15.8s startup during
  // which he saw no window at all, only a tray icon.
  //
  // Nothing is lost by deferring them: every consumer already treats "not
  // landed yet" as its own state rather than as an answer. `drawAppGrid`
  // renders "Scanning this device…", `wireProfileChip` re-checks after
  // `loadBrowsers()` resolves, `knownBrowsers()` falls back to last session's
  // list from localStorage, and the default-browser disc repaints when
  // `loadDefaultBrowser()` lands.
  //
  // This does NOT make the scan cheap — it relocates it. Until
  // `list_start_menu_apps` is safe to make `async` (it calls in-process COM;
  // see its doc comment), the first editor open pays the cost. That is a
  // deliberate trade: a wait the user asked for, with a visible "Scanning…"
  // note, beats the same wait before any window exists.
}

/**
 * Start every picker scan the editor needs, once, on first open.
 *
 * Called from `openPanel`, NOT from `initKeyDetailPanel` — see the note there.
 * Fire-and-forget by design: nothing awaits these, and each of the three
 * loaders is idempotent and caches for the session, so a second call is free.
 */
let _pickerWarmed = false;
function warmPickerData(): void {
  if (_pickerWarmed) return;
  _pickerWarmed = true;
  // The app grid. ~12s on the owner's machine; `drawAppGrid` shows its
  // "Scanning this device…" state until this lands.
  void loadApps();
  // The browser scan (~2.5s: it walks AppData). Starting it as the panel opens
  // means the profile picker is usually populated by the time the user reaches
  // it, and `knownBrowsers()` covers the gap from last session's cached list.
  warmBrowsers();
  // …and the OS default browser, for 4b's leading disc. Cheap and cached on the
  // Rust side (one registry read plus the shared icon cache), so the disc
  // paints its real icon rather than swapping a placeholder for an icon later.
  warmDefaultBrowser();

  // PROBLEM 237 wiring — the picker-refresh listener. Registered here rather
  // than at main.ts bootstrap: `warmPickerData()` already only ever runs once
  // per session (the `_pickerWarmed` guard above), so this is the earliest
  // point the picker is known to be in use, and `initPickerRefreshListener`
  // is itself idempotent besides. Re-renders just the app grid, and only if
  // the panel is still open when a background refresh actually changes the
  // list.
  initPickerRefreshListener(() => {
    if (_currentKey && _panel && !_panel.hidden) renderGrid();
  });
}

export function openPanel(key: string, config: AppConfig, origin?: HTMLElement): void {
  _config = config;
  _currentKey = key;
  _query = "";

  if (!_panel) return;

  // PROBLEM 205 — first open pays for the picker scans, not bootstrap.
  warmPickerData();

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

  // PROBLEM 242 — step 1 of the first-run tour is satisfied by the editor
  // opening for ANY letter, so the report goes out unconditionally and the
  // tour decides whether it cares. The bound/unbound flag is read from the
  // SAME `getBinding` the panel itself paints from, so the tour's "this one's
  // already set" copy can never disagree with what the editor is showing.
  const had = getBinding(key);
  tourEditorOpened(key, !!(had && (had.app || had.web_url)));
}

export function closePanel(): void {
  if (!_panel || _panel.hidden) return;

  // Page 2 goes with the panel, and INSTANTLY: the panel is already collapsing
  // back into the key, so sliding the page out on top of that would be two
  // exits fighting over the same 280ms.
  closeProfilePage(true);

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

  // PROBLEM 242 — this fires for BOTH "closed after saving" and "closed
  // without saving", because `closePanel` cannot tell them apart and should
  // not have to try. The tour makes the distinction itself: a save has already
  // moved it to step 3, so only a close that arrives while it is still waiting
  // on step 2 counts as walking away. Getting that backwards would either nag
  // after every successful bind or never pause at all.
  if (key) tourEditorClosed();
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

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

function renderPanel(key: string): void {
  if (!_panel || !_config) return;

  // `_panel.innerHTML = …` below would orphan page 2's node while leaving
  // `_page` pointing at it. Tear it down first, on the same path everything
  // else does.
  closeProfilePage(true);
  // The seed describes the CONTENTS of a field that is about to be replaced, so
  // it cannot outlive it. Left standing it would compare a fresh paste against
  // the previous key's URL — and a match would silently skip a real bind.
  _pathSeed = null;

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

    <!-- "Open this in a specific browser profile" (2026-08-26). Hidden unless
         this binding is a URL or a detected browser; see wireProfileChip.
         Label wording is the owner's choice ("Browser profile", not "Opens
         in"). Width check, since this label is the row's fixed part
         (flex-shrink: 0): ~110px at 10px/700/.1em uppercase + 8px gap + the
         chip's 260px max still totals ~378px of the panel's 416px usable row
         width (460 - 2x20 padding - 2x2 margin), so no CSS change needed. -->
    <div class="ed-bp-row" id="ed-bp-row" hidden>
      <span class="ed-bp-label">Browser profile</span>
      <span id="ed-bp-chip"></span>
    </div>

    <input class="input" id="ed-search" placeholder="Search apps…" autocomplete="off" spellcheck="false" />

    <div class="ed-section">Apps on this device</div>
    <div id="ed-grid-scroll">
      <div id="ed-grid"></div>
      <div class="ed-empty" id="ed-empty" hidden></div>
    </div>

    <!-- The replace confirm (2026-09-04, OWNER'S FATHER TEST) + the 10s Undo
         that follows it. One box, two uses in sequence: confirmReplace()
         fills it with "Replace 'X' with this? Replace · Keep" the instant a
         paste or an app-grid pick would overwrite an existing binding, and
         offerReplaceUndo() reuses the same box afterwards for "Replaced 'X'
         · Undo". See the doc comment on confirmReplace() for why this exists
         and on paintPathPill() for the gate it replaces.
         (No backticks anywhere in this block — it is inside a template
         literal, and one would terminate the string. See the NOTE below.) -->
    <div id="ed-replace-confirm" hidden></div>

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

    <!-- 4b — "a leading disc in the paste row" (owner's decision, 2026-08-26:
         *"implement 4b now, and 4a later"*). The disc is the SECOND way into
         the profile page, for a URL that has not been committed yet. It sits
         inside the field's own box, so the row's height is unchanged and
         nothing is added below it.

         THE AGREED FALLBACK, recorded here so whoever picks it up later knows
         it was considered rather than forgotten: if this disc proves
         undiscoverable, build 4a instead — Assign/Enter/Done commits the URL
         and the panel then turns AUTOMATICALLY to a page headed "Open
         <url> in…" listing every browser with its profiles, plus "My default
         browser" as the leading, pre-selected row. 4a adds no control at all;
         it costs a forced page turn on every URL commit, which is exactly why
         it is the fallback and not the default. renderProfilePage already takes
         the multi-browser shape 4a needs — only the trigger differs.
         (No backticks anywhere in this block: it is inside a template literal,
         and one would terminate the string. See the NOTE above.) -->
    <!-- THE ASSIGNED-VALUE PILL (2026-08-27). The owner: *"when we have assigned
         an app to a key and we press that same key again, from the dashboard we
         can see that OK this is the app I assigned it to. But it's not the same
         case in case of the links... That pill should show that, or the URL
         assigned, in the URL or file path pasting place, along with crossing
         option for if someone wants to edit."*

         An APP binding is already visible on reopen — its grid tile draws
         .current. A URL binding was visible NOWHERE: this field is built
         fresh every render and nothing ever assigned .value, so a bound key
         looked unbound. #ed-val is the missing half, and it deliberately
         REPLACES the input rather than pre-filling it — a pre-filled field
         invites a stray keystroke into a saved value and gives the crossing
         option nowhere to live.

         The pill is a .bp-chip — literally the browser-profile chip's own
         classes, which is the visual precedent the owner named ("the way they
         make a pill shape for the browser that is being used with the profile
         — that's similar"). Radius 999, disc + text + trailing ✕, the same
         accent tokens, one family by construction rather than by resemblance.

         GEOMETRY, so the 4b rebuild is not disturbed: the pill sits INSIDE
         .ed-path-wrap, is 32px tall and align-self: center in a row whose
         36px comes from #ed-assign — which stays put, disabled, exactly as it
         is over an empty field today. That is not furniture: keeping it means
         there is ZERO layout shift when the pill is clicked and the input takes
         its place, and it is what holds the row at 36px without a new
         min-height rule invented to defend a measurement.
         (No backticks in this block — it is inside a template literal.) -->
    <div class="ed-row-tight">
      <span class="ed-path-wrap">
        <button class="ed-path-disc" id="ed-path-disc" hidden></button>
        <input class="input" id="ed-path" placeholder="…or paste a file path / URL" autocomplete="off" spellcheck="false" />
        <span class="ed-val-host" id="ed-val" hidden></span>
      </span>
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
  } else if (binding?.site_icon) {
    // PROBLEM 267 — a link's favicon (fetched once at bind time).
    const img = document.createElement("img");
    img.src = binding.site_icon;
    img.alt = "";
    img.onerror = () => { img.remove(); cap.textContent = key.toUpperCase(); };
    cap.appendChild(img);
  } else {
    cap.textContent = key.toUpperCase();
  }

  const sub = _panel.querySelector<HTMLElement>("#ed-sub")!;
  sub.textContent = bound ? `Bound to ${boundLabel}` : "Not bound yet";

  // --- wiring ---
  _panel.querySelector("#ed-close")!.addEventListener("click", () => closePanel());
  _panel.querySelector("#ed-browse")!.addEventListener("click", handleBrowse);
  _panel.querySelector("#ed-remove")?.addEventListener("click", handleRemove);

  const search = _panel.querySelector<HTMLInputElement>("#ed-search")!;
  search.value = _query;
  search.addEventListener("input", () => {
    _query = search.value;
    renderGrid();
  });

  const path = _panel.querySelector<HTMLInputElement>("#ed-path")!;
  const disc = _panel.querySelector<HTMLButtonElement>("#ed-path-disc")!;
  wirePathDisc(key, path, disc);
  // `syncAssign` used to be a closure over `path`/`assign`/`disc`. It is now
  // `syncPathRow`, which finds them itself, because the pill has to run the
  // same sync from OUTSIDE this render — the field going from "hidden behind a
  // pill" to "holding the pill's value" is exactly the transition Assign and
  // the 4b disc both key off, and a second, hand-rolled copy of that rule in
  // the pill's click handler is how the two would drift.
  path.addEventListener("input", () => {
    syncPathRow();
    // A stale "Replace 'X' with this?" for whatever was typed a moment ago
    // must not linger once the field's contents have moved on from it.
    hideReplaceConfirm();
  });
  path.addEventListener("keydown", (e) => {
    if (e.key === "Enter") submitPathField(path.value.trim());
  });
  _panel.querySelector<HTMLButtonElement>("#ed-assign")!
    .addEventListener("click", () => submitPathField(path.value.trim()));
  syncPathRow();

  // PROBLEM 199 — pasting a URL/path then clicking "Done" silently discarded
  // it. The owner: *"pressing done instead of assigning after pasting a URL
  // doesn't assign the website to the key, fix it."*
  //
  // `assignFromPath` only ever fired on Enter in the paste field, or on the
  // separate small "Assign" button — never from "Done". So the flow SILENTLY
  // REQUIRED two deliberate actions (assign, then close) when everything
  // about the panel's layout — one text field, one big primary "Done" button
  // right there — reads as one. A pasted value sitting in the field when
  // "Done" is pressed is unambiguous intent; there is no reading of "I typed
  // a URL and clicked the button that closes this panel" other than "save
  // it, then close."
  //
  // Wired here rather than left where `#ed-done` used to be attached (before
  // `path`/`assignFromPath` existed in scope) so there is exactly ONE
  // listener on it, not two silently stacking.
  //
  // MUST await, not fire-and-forget: `assignFromPath`'s FILE-PATH branch
  // `await`s `check_app_path` before it ever calls `commit()`, and
  // `closePanel()` sets the module-level `_currentKey` to `null`
  // SYNCHRONOUSLY. Calling `assignFromPath(pending)` without awaiting it and
  // then immediately closing would null `_currentKey` while that check is
  // still in flight — so by the time `commit()` finally ran, `_currentKey`
  // would already be gone and the bind would silently no-op, reproducing the
  // exact bug this fix exists for, just for a pasted file path instead of a
  // URL. Only the URL branch happens to have no `await` before its own
  // `commit()` call, which is why the bug as REPORTED only showed up on URLs.
  //
  // THE PILL'S INTERACTION WITH THIS, traced rather than assumed, because the
  // pill can now put an ALREADY-SAVED value into the field this reads:
  //   · unbound key, nothing typed      -> pending "" -> falsy -> just closes
  //   · unbound key, something pasted   -> pending = the paste, seed null -> assigns (199 intact)
  //   · bound key, PILL SHOWING, nothing typed -> the field sits beside the
  //     pill, empty (paintPathPill sets it that way; renderPanel never
  //     assigns .value either) -> pending "" -> NO commit, binding untouched
  //   · bound key, pill clicked, value UNCHANGED -> pending === _pathSeed ->
  //     skipped. Without this it would re-commit and, via assignFromPath's
  //     deliberate omission of the three browser fields, wipe the pin.
  //   · bound key, pill clicked and EDITED -> pending !== _pathSeed -> assigns
  //   · bound key, pill clicked then field emptied -> pending "" -> nothing.
  //     Emptying the field is not "clear the binding"; the pill's ✕ is.
  //   · bound key, FRESH paste beside the pill (2026-09-04, `_pathSeed` still
  //     null — this was never loaded from the pill) -> the replace gate:
  //     `confirmReplace` asks first, and the panel stays open until the user
  //     answers (see below) rather than closing out from under the confirm.
  _panel.querySelector("#ed-done")!.addEventListener("click", () => {
    const pending = path.value.trim();
    if (pending && isUnchangedPillValue(pending)) {
      console.info("key-editor: Done with an unedited pill value — nothing committed");
      closePanel();
      return;
    }
    if (pending && key && _pathSeed === null) {
      const existing = getBinding(key);
      if (existing && (existing.app || existing.web_url)) {
        confirmReplace(key, existing, () =>
          void assignFromPath(pending, { keepOpen: true, onSaved: () => finishReplace(key, existing) }),
        );
        return;   // wait for Replace/Keep — Done does not close out from under it
      }
    }
    void (async () => {
      if (pending) await assignFromPath(pending);
      closePanel();
    })();
  });

  wireProfileChip(key, binding);
  renderPathValue(key, binding);

  renderGrid();
}

/**
 * Show the browser-profile chip, but only where it means something.
 *
 * TWO CASES, and nothing else:
 *   1. a URL binding — ANY url can optionally be pinned to a browser+profile;
 *   2. an app binding whose path is EXACTLY one of the detected browsers'
 *      exes ("just open Brave's Studies profile", no url).
 *
 * Case 2 is matched by exact path, never by name: "Chrome" is a name several
 * things answer to, `…\Google\Chrome\Application\chrome.exe` is one program.
 *
 * NOTHING HERE IS ON THE BIND PATH. The panel has already committed the
 * binding through its normal instant-bind flow by the time this runs (PROBLEM
 * 199's `#ed-done` fix is untouched), so this can only ever ADD a control to an
 * already-saved key. If the browser scan has not landed yet the chip simply
 * appears a moment later — it never delays anything and never blocks a bind.
 */
function wireProfileChip(key: string, binding: KeyBinding | undefined): void {
  const row = _panel?.querySelector<HTMLElement>("#ed-bp-row");
  const host = _panel?.querySelector<HTMLElement>("#ed-bp-chip");
  if (!row || !host || !binding) return;

  const isUrl = !!binding.web_url;
  const boundApp = binding.app ?? null;

  const show = (): void => {
    row.hidden = false;
    renderProfileChip(
      host,
      {
        // `browser_exe` is null on an APP binding by design — the exe already
        // IS `binding.app`, and storing it twice is how the two get to
        // disagree. The chip still has to NAME the browser, so the effective
        // exe is resolved HERE, at paint time, from whichever field holds it.
        // Measured before this line existed: an app binding pinned to
        // "ARPON'S STUDIES" drew a chip with an empty browser name and the raw
        // folder name "Default" as the profile, because the lookup was handed
        // null and could not find the browser to read the display name from.
        browserExe: binding.browser_exe ?? boundApp,
        profileDir: binding.browser_profile_dir ?? null,
        profileName: binding.browser_profile_name ?? null,
        // Only a URL binding STORES a browser. An app binding's exe is
        // `binding.app`, which is not a pin and must not be drawn as one.
        exePinned: !!binding.browser_exe,
      },
      {
        // The chip body no longer opens a popover. `.bp-pop` hung off a chip
        // inside a 460px panel and overflowed it by ~19px, which is what gave
        // the editor a horizontal scrollbar. It opens page 2 instead, which is
        // the panel's own width by construction.
        onOpen: (browser) => openProfilePage(key, browser),
        // Three nulls, through the SAME commit path every other change uses.
        // No confirm dialog: it is one press to redo, and the toast carries an
        // Undo. NEVER `""` — null is the only value that means "the OS default
        // browser opens this", which is the owner's hard requirement.
        onClear: () => clearPin(key),
      },
    );
  };

  if (isUrl) { show(); return; }
  if (!boundApp) return;

  // An app binding: only a detected browser gets the chip. On a true first run
  // nothing is known yet, in which case `findBrowserByExe` returns null meaning
  // "not yet known" — so ask again when the scan lands rather than deciding
  // "no" too early. From the second run onward last session's list answers this
  // in the same frame.
  if (findBrowserByExe(boundApp)) { show(); return; }
  void loadBrowsers().then(() => {
    // The editor may have moved to another key, or closed, while we waited.
    if (_currentKey !== key || !row.isConnected) return;
    if (findBrowserByExe(boundApp)) show();
  });
}

/**
 * Write a picked profile into the binding, through the SAME commit() every
 * other change goes through, so the save/toast flow is reused rather than
 * duplicated. Everything else about the binding is carried across verbatim —
 * this edits one property, it does not re-bind.
 *
 * `browserExe` is what the PAGE reports (whose tile was pressed). Whether it is
 * STORED depends on the binding: for a URL it must be, because a URL has no
 * other record of which browser to use; for an app binding the exe already IS
 * `binding.app`, so `browser_exe` stays null and Rust reads the profile off
 * `app`. Two sources of truth for one path is the bug to avoid.
 */
function commitProfile(
  key: string,
  browserExe: string | null,
  profileDir: string | null,
  profileName: string | null,
  opts: { keepOpen: boolean },
): void {
  // Read the binding LIVE rather than trusting a snapshot: with `keepOpen` the
  // editor stays up, so a second pick in the same visit would otherwise
  // re-commit state captured before the first one.
  const live = getBinding(key);
  if (!live) {
    console.error(`bp: commit skipped — no binding for key=${key}`);
    return;
  }
  const isUrl = !!live.web_url;
  // `profileName` is the account label, derived from a signed-in email
  // address — so it is NOT logged. `dir` says which tile was pressed just as
  // precisely and identifies nobody. (Owner's rule 2026-08-31: emails never
  // reach logs or telemetry.) `named` keeps the line able to distinguish
  // "a pin was written" from "a pin was cleared", which is what it was for.
  console.info(
    `bp: commit reached — key=${key} isUrl=${isUrl} exe=${browserExe ?? "(default)"} ` +
    `dir=${profileDir ?? "(none)"} named=${profileName ? "yes" : "no"}`,
  );
  void commit(
    {
      app: live.app ?? null,
      web_url: live.web_url ?? null,
      label: live.label ?? null,
      icon_override: live.icon_override ?? null,
      browser_exe: isUrl ? browserExe : null,
      browser_profile_dir: profileDir,
      browser_profile_name: profileName,
    },
    {
      // The conflict check is about Space+<key> colliding with an OS shortcut.
      // That verdict cannot change by picking a profile, and the user already
      // answered it when this key was bound — re-prompting would be pure noise.
      skipConflict: true,
      // THE WHOLE POINT of the chip path: it edits ONE property of a binding
      // that already exists. Closing the editor on it — which is what every
      // other commit() caller wants and what this one used to inherit — made
      // choosing a profile look exactly like a failed re-bind: the editor
      // vanished and the toast read "Space+Y → Youtube", saying nothing about
      // the browser. The owner's verdict was "so bad, it's non-functional".
      keepOpen: opts.keepOpen,
      // With `keepOpen` the panel is NOT re-rendered, so nothing else would
      // repaint the chip — measured: pressing the ✕ wrote three nulls and saved
      // them correctly while the chip went on reading "Brave · ARPON'S
      // STUDIES", which is a save that worked being indistinguishable from one
      // that did not. Re-wiring the row is enough; the 6s Undo lives on the row
      // itself, not inside the chip host, so it survives.
      onSaved: opts.keepOpen ? () => refreshProfileRow(key) : undefined,
      // …so the confirmation has to name what actually changed, or a save that
      // worked is indistinguishable from one that did not.
      toast: profileDir
        ? `🌐 Space+${key.toUpperCase()} opens in ${profileName ?? profileDir}`
        : `🌐 Space+${key.toUpperCase()} opens in your default browser`,
    },
  );
}

/** Repaint the chip row from the LIVE binding, without touching page 1. */
function refreshProfileRow(key: string): void {
  if (_currentKey !== key) return;
  // A pending Undo belongs to the pin that WAS there. Once a new value has been
  // written it would put back something the user has since replaced, so it goes
  // with the state it described. `clearPin` re-offers a fresh one immediately
  // afterwards.
  _panel?.querySelector("#ed-bp-row .ed-bp-undo")?.remove();
  wireProfileChip(key, getBinding(key));
}

/** The chip's ✕ — three nulls, no confirm, and an Undo for 6 seconds. */
function clearPin(key: string): void {
  const live = getBinding(key);
  if (!live) return;
  const undo = {
    browser_exe: live.browser_exe ?? null,
    browser_profile_dir: live.browser_profile_dir ?? null,
    browser_profile_name: live.browser_profile_name ?? null,
  };
  commitProfile(key, null, null, null, { keepOpen: true });
  offerPinUndo(key, undo);
}

/**
 * The 6-second Undo after clearing a pin.
 *
 * DEVIATION FROM THE HANDOFF, stated so nobody has to re-derive it: the handoff
 * asks for "a toast carrying an Undo for 6s". It cannot live in the toast as
 * things stand. `#toast-container` is `pointer-events: none` (toast.ts's
 * `toastLayer`, set via CSSOM), and toast.ts is the overlay's verbatim drop-in
 * — the same component renders into the transparent CLICK-THROUGH overlay
 * window, where a button is unreachable by definition. Making it clickable
 * would mean changing the shared toast component and the overlay's stylesheet,
 * neither of which this feature owns.
 *
 * So the Undo sits where the ✕ that caused it was, for the same 6 seconds. The
 * user's eyes are already there, and the panel is still open (`keepOpen`), so
 * it is if anything closer to hand than a toast at the bottom of the window.
 */
function offerPinUndo(
  key: string,
  prev: { browser_exe: string | null; browser_profile_dir: string | null; browser_profile_name: string | null },
): void {
  const row = _panel?.querySelector<HTMLElement>("#ed-bp-row");
  if (!row || (!prev.browser_exe && !prev.browser_profile_dir)) return;
  row.querySelector(".ed-bp-undo")?.remove();

  const undo = document.createElement("button");
  undo.type = "button";
  undo.className = "ed-bp-undo";
  undo.textContent = "Undo";
  undo.title = `Put the ${prev.browser_profile_name ?? prev.browser_profile_dir ?? "browser"} pin back`;
  const timer = window.setTimeout(() => undo.remove(), 6000);
  undo.addEventListener("click", () => {
    window.clearTimeout(timer);
    undo.remove();
    if (_currentKey !== key) return;
    console.info(`bp: pin undo — restoring dir=${prev.browser_profile_dir ?? "(none)"}`);
    commitProfile(key, prev.browser_exe, prev.browser_profile_dir, prev.browser_profile_name, {
      keepOpen: true,
    });
  });
  row.appendChild(undo);
}

// ---------------------------------------------------------------------------
// The replace confirm (2026-09-04) — OWNER'S FATHER TEST
// ---------------------------------------------------------------------------

/**
 * ONE inline confirm, shown the instant a paste or an app-grid pick would
 * REPLACE an existing binding — see the doc comment on `paintPathPill` for
 * the full story of why. Every call site that can reach a bound key with a
 * fresh (not click-to-edit) value goes through here before it ever calls
 * `commit()`/`assignFromPath()`: `submitPathField`, the `#ed-done` handler,
 * the 4b disc, and the app grid's `onPick`.
 *
 * `existing` MUST be the live binding read straight off `getBinding(key)`
 * before anything changes — `offerReplaceUndo` hands it back to `commit()`
 * verbatim if the user changes their mind, and `commit()` normalises
 * whatever it is given to a COMPLETE seven-field `KeyBinding` (PROBLEM 204a),
 * so there is no way for this to reproduce "the pin never saved" even though
 * it is passing a snapshot around rather than re-deriving one.
 *
 * `onReplace` is the caller's own commit — this function never calls
 * `commit()`/`assignFromPath()` itself, it only decides whether to ask first.
 * That keeps every call site's own shape (which fields it commits, whether a
 * multi-profile browser page follows) exactly as it already was; only the ONE
 * new gate in front of it is shared.
 */
function confirmReplace(key: string, existing: KeyBinding, onReplace: () => void): void {
  const box = _panel?.querySelector<HTMLElement>("#ed-replace-confirm");
  // Fail OPEN, never silently block a bind the user already committed to by
  // pasting or picking — the same principle as `commit`'s own early-exit
  // logging (PROBLEM 199): a missing confirm box must not read as "nothing
  // happened", it must still bind.
  if (!box) { onReplace(); return; }

  const label = replaceLabel(existing);
  box.hidden = false;
  box.innerHTML = `
    <div class="ed-replace-box">
      <span class="ed-replace-text">Replace "${escapeHtml(label)}" with this?</span>
      <span class="ed-replace-btns">
        <button class="btn btn-sm btn-primary" id="ed-replace-go">Replace</button>
        <button class="btn btn-sm" id="ed-replace-no">Keep</button>
      </span>
    </div>
  `;
  box.querySelector("#ed-replace-go")!.addEventListener("click", () => {
    hideReplaceConfirm();
    console.info(`key-editor: replace confirmed — key=${key} was "${label}"`);
    onReplace();
  });
  box.querySelector("#ed-replace-no")!.addEventListener("click", () => {
    hideReplaceConfirm();
    console.info(`key-editor: replace declined — key=${key} kept "${label}", nothing committed`);
  });
}

/** Dismiss the box, whichever of its two uses (confirm or Undo) currently
 *  occupies it. Safe to call when it is already empty. */
function hideReplaceConfirm(): void {
  const box = _panel?.querySelector<HTMLElement>("#ed-replace-confirm");
  if (!box) return;
  box.hidden = true;
  box.innerHTML = "";
}

/** The name a binding reads by in the confirm/Undo copy — the same rule
 *  `renderPanel`'s "Bound to …" sub-title uses, plus the browser-profile pin
 *  when there is one, so "Replace 'Google Chrome — Arpon' with this?" names
 *  the SPECIFIC thing being lost, not just the app. */
function replaceLabel(binding: KeyBinding): string {
  const base = binding.label ||
    (binding.app ? cleanLabel(binding.app.split(/[\\/]/).pop() || "")
                 : cleanLabel(binding.web_url || ""));
  return binding.browser_profile_name ? `${base} — ${binding.browser_profile_name}` : base;
}

/**
 * Repaint page 1 after a CONFIRMED replace actually commits, then offer the
 * 10-second Undo. Only the replace path calls this — an ordinary bind onto an
 * empty key still assigns and closes instantly, unchanged (the mockup's rule:
 * no Save/Cancel pair). Replacing something that was already there is the one
 * case that now stays open long enough to be walked back.
 */
function finishReplace(key: string, previous: KeyBinding): void {
  if (_currentKey !== key) return;
  renderPanel(key);
  offerReplaceUndo(key, previous);
}

/**
 * The 10-second inline Undo after a CONFIRMED replace, restoring the FULL
 * previous binding — all seven fields, exactly as `previous` was read out of
 * `getBinding()` before the replace touched anything.
 *
 * REUSES `offerPinUndo`'s precedent rather than reinventing it (owner's
 * decision, 2026-09-04): same reason it cannot live in the toast
 * (`#toast-container` is `pointer-events: none` — see `offerPinUndo`'s doc
 * comment for the full explanation), same "sits where the action happened"
 * placement. It lives in `#ed-replace-confirm`, the box the confirm itself
 * just used — `finishReplace` has already repainted page 1 by the time this
 * runs, so the box is empty again.
 *
 * 10s, not `offerPinUndo`'s 6s: replacing a whole binding (app AND label AND
 * icon AND any browser pin) is a bigger thing to walk back than clearing one
 * pin, and the owner's own wording for this feature asked for "~10 s".
 */
function offerReplaceUndo(key: string, previous: KeyBinding): void {
  const box = _panel?.querySelector<HTMLElement>("#ed-replace-confirm");
  if (!box) return;
  const label = replaceLabel(previous);

  box.hidden = false;
  box.innerHTML = `
    <div class="ed-replace-box ed-replace-undo-row">
      <span class="ed-replace-text">Replaced "${escapeHtml(label)}"</span>
      <span class="ed-replace-btns"><button class="btn btn-sm" id="ed-replace-undo">Undo</button></span>
    </div>
  `;
  const timer = window.setTimeout(() => hideReplaceConfirm(), 10_000);
  box.querySelector("#ed-replace-undo")!.addEventListener("click", () => {
    window.clearTimeout(timer);
    hideReplaceConfirm();
    if (_currentKey !== key) return;
    console.info(`key-editor: replace undo — restoring "${label}" for key=${key}`);
    // The SAME commit() every other change goes through — PROBLEM 204a's
    // normalisation guarantees all seven fields land, never a partial object
    // that erases a browser-profile pin by omission.
    void commit(previous, {
      skipConflict: true,
      keepOpen: true,
      onSaved: () => { if (_currentKey === key) renderPanel(key); },
    });
  });
}

// ---------------------------------------------------------------------------
// Page 2 — the browser-profile page
// ---------------------------------------------------------------------------

/**
 * Open page 2 over page 1.
 *
 * `browser` null means "list every detected browser" — the shape a URL binding
 * needs before it has chosen one, and the shape 4a would reuse verbatim.
 *
 * `fromBind` says this page was reached by pressing a tile in the app grid,
 * which is one continuous bind gesture: picking a profile there finishes it and
 * closes the editor, exactly as the design draws it. Reached from the CHIP it
 * is a one-property edit of a key that is already bound, so picking returns to
 * page 1 with the editor still open — that is the defect the `keepOpen` option
 * exists to fix, and it must not come back through this door.
 */
function openProfilePage(
  key: string,
  browser: DetectedBrowser | null,
  fromBind = false,
): void {
  if (!_panel || _panel.hidden) return;
  closeProfilePage(true);

  const binding = getBinding(key);
  const list = browser ? [browser] : (knownBrowsers() ?? []);
  const combo = `Space + ${key.toUpperCase()}`;
  const boundLabel =
    browser?.browser_name ??
    binding?.label ??
    (binding?.web_url ? binding.web_url : "this key");

  const page = document.createElement("div");
  page.className = "bp-page";
  // Anything the panel could scroll must be pinned to the panel's VISIBLE box,
  // and an absolutely-positioned child of a scrolled container scrolls with it.
  // Reset the scroll and freeze it for as long as the page is up.
  _panel.scrollTop = 0;
  _panel.classList.add("bp-paged");
  _panel.appendChild(page);
  _page = page;

  const selection = {
    browserExe: binding?.browser_exe ?? binding?.app ?? null,
    profileDir: binding?.browser_profile_dir ?? null,
  };
  const pinned = !!(binding?.browser_exe || binding?.browser_profile_dir);

  renderProfilePage(page, {
    title: browser ? `Which ${browser.browser_name} profile?` : `Open ${boundLabel} in…`,
    subtitle: browser
      ? `${combo} is already bound to ${browser.browser_name}`
      : `${combo} opens this link`,
    hint: browser
      ? `Skip this and ${combo} opens ${browser.browser_name} the way it always has.`
      : `Skip this and ${combo} opens in your default browser.`,
    browsers: list,
    selection,
    // Only offer "back to normal" when there is something to undo. A row that
    // re-selects the state you are already in is furniture.
    resetLabel: pinned
      ? (browser && !binding?.web_url
          ? `◍  No specific profile — open ${browser.browser_name} normally`
          : "🌐  Open in my default browser")
      : null,
    onPick: (browserExe, dir, name) => {
      // PROBLEM 242 follow-up — BEFORE the commit, not after, and the order is
      // load-bearing: with `fromBind` the commit ends in `closePanel()`, which
      // runs `closeProfilePage(true)` and so trips the catch-all dismissal at
      // the bottom of that function. Reporting the PICK first means the
      // informative call lands and the catch-all is the no-op it should be.
      tourProfilePickerClosed();
      commitProfile(key, browserExe, dir, name, { keepOpen: !fromBind });
      if (fromBind) return;              // commit() closed the whole editor
      closeProfilePage();
      if (_currentKey === key) renderPanel(key);
    },
    onReset: () => {
      tourProfilePickerClosed();         // "open it normally" — the skip shape
      commitProfile(key, null, null, null, { keepOpen: !fromBind });
      if (fromBind) return;
      closeProfilePage();
      if (_currentKey === key) renderPanel(key);
    },
    onBack: () => {
      // ← returns to the app grid with the binding intact. Nothing is committed
      // or reverted here; whatever was saved on the way in stays saved.
      console.info("bp: page back — binding untouched");
      closeProfilePage();
      // REGRESSION SWEEP 2026-09-07 — AND PAGE 1 HAS TO BE REBUILT, exactly as
      // `onPick` and `onReset` above already do it.
      //
      // The bug: reaching this page through the app grid (`fromBind`) commits
      // the browser binding and then slides page 2 over a page 1 that was
      // rendered BEFORE that commit. Pressing ← put the user back on that
      // stale page — measured in the harness on Space+K/Brave, which showed
      // "Not bound yet", no assigned-value pill, no "Browser profile" row and
      // **no "Remove binding" button** for a key the board behind it was
      // already drawing as Brave. So the one exit that is supposed to leave
      // you where you can keep editing was the one exit that took the editing
      // controls away, and the only cure was to close the editor and reopen it.
      //
      // Generalise: a view that another view was layered on top of is stale by
      // default — if anything under the layer could have changed while it was
      // up, EVERY way back has to re-render, not just the ways that changed it.
      if (_currentKey === key) renderPanel(key);
    },
    // ✕ and Done are both "I am finished with this question" — the owner's
    // "Skip this and Space + K opens Brave the way it always has". The tour
    // treats them exactly as the skip row, and says so here rather than
    // relying on the catch-all below to infer it.
    onClose: () => { tourProfilePickerClosed(); closePanel(); },
    onDone: () => { tourProfilePickerClosed(); closePanel(); },
  });

  console.info(
    `bp: page opened — key=${key} browser=${browser?.browser_name ?? "(all)"} ` +
    `profiles=${list.reduce((n, b) => n + b.profiles.length, 0)} fromBind=${fromBind}`,
  );

  // PROBLEM 242 follow-up — LAST, once the page is really on screen and its
  // handlers are wired. The tour dims to step 2b from here; announcing it
  // before `renderProfilePage` had built anything would leave the step's ring
  // hunting a `.bp-scroll` that did not exist yet.
  tourProfilePickerOpened(key, browser?.browser_name ?? null);
}

/** Slide page 2 out. `instant` skips the tween — used when the panel itself is
 *  closing, so two exits are not fighting over the same 280ms. */
function closeProfilePage(instant = false): void {
  const page = _page;
  _page = null;
  window.clearTimeout(_pageTimer);
  _panel?.classList.remove("bp-paged");
  if (!page) return;

  // PROBLEM 242 follow-up — THE CATCH-ALL, and it is here rather than at the
  // five call sites for the reason the tour hooks are in this file at all:
  // page 2 has more ways to disappear than any list of them stays correct
  // about. ← back, `renderPanel`'s teardown, `openProfilePage` replacing a
  // page that was already up, and `closePanel` on Esc or a backdrop click all
  // arrive here and NONE of them reports itself. `backToEditor` is the honest
  // reading of every one: the picker is gone, the editor may not be, and the
  // tour must not jump to "hold Space" on a guess.
  //
  // The informative exits (a pick, the skip row, ✕, Done) call the tour BEFORE
  // reaching here, and `tourProfilePickerClosed` only acts in step 2b, so this
  // is a no-op on all four. A second call cannot overwrite the first.
  tourProfilePickerClosed(true);

  if (instant) { page.remove(); return; }
  page.classList.add("bp-page-out");
  // 195ms = ~65% of the 300ms entrance, the app's standing exit ratio.
  _pageTimer = window.setTimeout(() => page.remove(), 195);
}

// ---------------------------------------------------------------------------
// 4b — the leading disc in the paste row
// ---------------------------------------------------------------------------

/**
 * IS THIS TEXT A WEBSITE OR A THING ON DISK? One rule, one place (1.0.88).
 *
 * THE BUG THIS EXISTS FOR, in the owner's words: *"When I pasted the complete
 * URL of YouTube — with the https and all — it did launch and gave me the
 * option to choose my browser, the circular thing beside it. But when I just
 * typed youtube.com, no option to choose the browser came. Nor for
 * discord.com."*
 *
 * Every URL test in this file used to be `/^https?:\/\//i`. `youtube.com`
 * fails that, so it fell into the FILE-PATH branch: `check_app_path`, then an
 * app binding, which the engine later rescued by Start Menu heuristics
 * (`cascade: resolved youtube.com via Start Menu` in his log). It "worked",
 * and cost him the browser-profile chip, the 4b disc and the correct binding
 * kind — because all three key off this one test.
 *
 * TWO OUTCOMES, NOT THREE. A third "ambiguous" verdict would push the tiebreak
 * back out to each call site, which is exactly the drift this consolidation
 * removes: the disc, Assign, Enter, Done and `assignFromPath` must all reach
 * the same answer for the same text or the panel contradicts itself.
 *
 * THE RULES, in order. The PATH VETOES run first and always win:
 *   1. `https?://` — url. Unchanged from before; this branch is byte-identical
 *      in effect for every binding that already exists.
 *   2. A backslash ANYWHERE, a `X:` drive letter, a `\\` UNC prefix, a leading
 *      `%ENVVAR%`, a leading `./` or `../`, a leading `/`, or any other scheme
 *      (`file:`, `mailto:`, `steam:`) — path. A backslash is never legal in a
 *      hostname, so it is a free and total discriminator.
 *   3. An `.exe` / `.lnk` / `.bat` / `.cmd` suffix — path. This list is short
 *      ON PURPOSE and does not need to grow: a file pasted WITH a directory
 *      component is already caught by rule 2, so this only has to cover a bare
 *      filename typed alone — and .exe/.lnk are the only things Browse files…
 *      will even offer, with .bat/.cmd for the scripts people bind by hand.
 *   4. Hostname SHAPE, no TLD list: labels of [a-z0-9-] joined by dots, at
 *      least two of them, last one 2+ letters. A TLD table would be wrong the
 *      week it was written and is not needed once rule 2 exists.
 *      `youtube.com`, `discord.com`, `notebooklm.google.com`, `x.com` → url.
 *      `notepad` → path, because a single label is a program name — that is a
 *      REAL existing flow (bind `notepad` and let the Start Menu resolve it)
 *      and it must not be turned into `https://notepad`.
 *   5. Anything else — path. The field's other job is still file paths.
 *
 * THE `.com` TRADE-OFF, ACCEPTED DELIBERATELY. `.com` is both the commonest
 * TLD and a DOS-era executable extension, and they collide EXACTLY here.
 * `C:\Tools\a.com` classifies as a path (rule 2, the backslash). A bare
 * `a.com` typed with no path at all classifies as a URL. That is the wrong
 * answer for someone who pastes the bare name of a .com executable and expects
 * it resolved off the Start Menu — and it is the right answer approximately
 * every other time the string `something.com` is typed into this field. The
 * owner asked for precisely this. Anyone tempted to "fix" it: the cure is a
 * file-exists check, not a TLD list (see the note on `check_app_path` below).
 *
 * SYNCHRONOUS, so the "does this file exist?" leg of the veto is NOT here.
 * This runs on every keystroke (`syncPathRow`), and an IPC round trip per
 * keypress to answer a question that only matters at commit time would be a
 * cost paid constantly for a case that is vanishingly rare. The commit path's
 * `check_app_path` still guards the file branch; nothing about that changed.
 */
type PathInputKind = "url" | "path";

/** `.exe` and friends. NOT `.com` — see the trade-off note above. */
const EXECUTABLE_SUFFIX = /\.(?:exe|lnk|bat|cmd)$/i;
/** One hostname label: alphanumeric, inner hyphens allowed. */
const HOST_LABEL = /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/i;

function classifyPathInput(raw: string): PathInputKind {
  const value = raw.trim();
  if (!value) return "path";

  // 1 — an explicit web scheme settles it.
  if (/^https?:\/\//i.test(value)) return "url";

  // 2 — the path vetoes. Any one of these and the hostname test never runs.
  if (value.includes("\\")) return "path";              // C:\… , \\server\… , anything Windows
  if (/^[a-z]:/i.test(value)) return "path";            // C:  C:/  D:\
  if (value.startsWith("%")) return "path";             // %LOCALAPPDATA%\…
  if (/^\.{1,2}[\\/]/.test(value)) return "path";       // ./run   ../bin
  if (value.startsWith("/")) return "path";             // a separator before the first dot
  // Any OTHER scheme is somebody else's business, not a website. The character
  // class deliberately excludes `.` so a host with a port (`youtube.com:8080`)
  // cannot be mistaken for a scheme.
  if (/^[a-z][a-z0-9+-]*:/i.test(value)) return "path";

  // 3 — a bare executable filename.
  if (EXECUTABLE_SUFFIX.test(value)) return "path";

  // 4 — hostname shape. Take the authority only: `youtube.com/watch?v=x` is a
  // url, and the `/` after the first dot is its path, not a file separator.
  const authority = value.split(/[/?#]/)[0] ?? "";
  const host = authority.split(":")[0] ?? "";           // drop any :port
  const labels = host.split(".");
  if (labels.length < 2) return "path";                 // "notepad" stays an app lookup
  if (!labels.every((l) => HOST_LABEL.test(l))) return "path";
  if (!/^[a-z]{2,}$/i.test(labels[labels.length - 1]!)) return "path";
  return "url";
}

/**
 * What actually gets STORED in `web_url`, once classified as a url.
 *
 * ALWAYS SCHEMED. Verified against the consumers rather than assumed:
 *   · `run_browser` (smart_cascade.rs) already prepends `https://` itself and
 *     logs it — so the DEFAULT-browser leg would survive a bare host, but only
 *     by being repaired downstream on every single press;
 *   · `open_binding_url`'s `BrowserRoute::Specific` leg does NOT. It hands the
 *     stored string to `build_launch_params` as a command-line argument to a
 *     browser exe, with no scheme repair anywhere on that path — so a URL
 *     pinned to a browser profile (which is the very feature this fix restores
 *     access to) would be launched as a bare word;
 *   · `url_match_keys` tolerates either, and
 *   · `splitUrlForPill` needs `new URL()` to parse, which a bare host does not.
 * One normalisation here beats three tolerances downstream.
 */
function normaliseUrl(value: string): string {
  const v = value.trim();
  return /^https?:\/\//i.test(v) ? v : `https://${v}`;
}

/**
 * Show the disc only once the field holds a link, and reserve no room for it
 * before then.
 *
 * A permanently-present disc would be a control that can do nothing on the
 * common path (a file path, or an empty field), and this codebase's standing
 * rule is that such a control is worse than its absence. A permanently-reserved
 * 38px of padding would be the same problem wearing a different hat. So both
 * arrive together, on a 90ms padding tween, at the moment a link is recognised
 * — which reads as "I noticed this is a link", not as noise.
 */
function syncPathDisc(path: HTMLInputElement, disc: HTMLElement): void {
  // CALL SITE 1 of 4 for the classifier. Before 1.0.88 this was
  // `looksLikeUrl`, i.e. `^https?://`, which is why typing `youtube.com`
  // raised no disc — the owner's report, half of it.
  const on = classifyPathInput(path.value) === "url";
  disc.hidden = !on;
  path.parentElement?.classList.toggle("has-disc", on);
}

/**
 * Assign + the 4b disc follow whatever `#ed-path` holds. ONE rule, called from
 * the render wiring, from every keystroke, and from the pill's edit path.
 */
function syncPathRow(): void {
  const path = _panel?.querySelector<HTMLInputElement>("#ed-path");
  const assign = _panel?.querySelector<HTMLButtonElement>("#ed-assign");
  const disc = _panel?.querySelector<HTMLElement>("#ed-path-disc");
  if (!path || !assign || !disc) return;
  assign.disabled = path.value.trim().length === 0;
  syncPathDisc(path, disc);
}

/**
 * True when the field is holding the pill's own value, untouched.
 *
 * CALL SITE 2 of 4 for the classifier, and the one that had to be re-traced
 * for 1.0.88 rather than left alone. The seed is whatever the pill loaded back
 * in, which for a URL binding is now ALWAYS the schemed, stored form
 * (`https://youtube.com`) — because `assignFromPath` normalises before it
 * commits. So a NEW way to "change nothing" appeared with normalisation:
 * deleting the `https://` the user never typed in the first place. Byte
 * equality would call that an edit, re-commit an identical `web_url`, and —
 * through `assignFromPath`'s deliberate omission of the three browser-profile
 * fields — silently wipe the pin. That is verbatim the failure `_pathSeed`
 * exists to prevent, arriving through a door this release opened.
 *
 * So a url is compared in its NORMALISED form. Nothing else is loosened: the
 * comparison is still exact apart from a scheme this code added itself, so
 * `youtube.com` vs `youtu.be` is still a real edit, and a path is still
 * compared byte for byte.
 */
function isUnchangedPillValue(value: string): boolean {
  if (_pathSeed === null) return false;
  if (value === _pathSeed) return true;
  return (
    classifyPathInput(value) === "url" &&
    classifyPathInput(_pathSeed) === "url" &&
    normaliseUrl(value) === normaliseUrl(_pathSeed)
  );
}

/**
 * Enter in the paste field, and the Assign button. Both used to be
 * `if (value) assignFromPath(value)`.
 *
 * Two things added since, neither a no-op:
 *  1. The unchanged-pill case — pressing Assign over a value you did not
 *     change is a request to finish, and finishing means "the field goes
 *     back to showing what is assigned", which is the pill. Committing
 *     instead would be a save with nothing to save that also clears the
 *     browser-profile pin (see `_pathSeed`).
 *  2. The replace gate (2026-09-04) — `_pathSeed === null` means this value
 *     was typed or pasted fresh, not loaded from the pill for a precise edit
 *     (that path already skipped this function via case 1, or goes straight
 *     through with `_pathSeed` set and is a deliberate small edit, not a
 *     replace). A fresh value landing on an ALREADY-BOUND key asks first —
 *     see `confirmReplace`.
 */
function submitPathField(value: string): void {
  if (!value) return;
  if (isUnchangedPillValue(value)) { cancelPathEdit(); return; }
  const key = _currentKey;
  if (key && _pathSeed === null) {
    const existing = getBinding(key);
    if (existing && (existing.app || existing.web_url)) {
      confirmReplace(key, existing, () =>
        void assignFromPath(value, { keepOpen: true, onSaved: () => finishReplace(key, existing) }),
      );
      return;
    }
  }
  void assignFromPath(value);
}

/** Abandon an edit and restore the pill, committing nothing. */
function cancelPathEdit(): void {
  const key = _currentKey;
  if (!key) return;
  console.info("bp: path edit cancelled — value unchanged, nothing committed");
  const path = _panel?.querySelector<HTMLInputElement>("#ed-path");
  if (path) path.value = "";
  _pathSeed = null;
  renderPathValue(key, getBinding(key));
  syncPathRow();
}

/**
 * Split a URL so the SCANNABLE half leads: `youtube.com` then `/watch?v=…`.
 *
 * This is the same "one part holds, one part gives" split the browser-profile
 * chip already makes (`.bp-chip-browser` never truncates, `.bp-chip-profile`
 * ellipsizes) — browser·profile there, host·path here. A URL that will not
 * parse is shown whole and given no tail, rather than guessed at.
 */
function splitUrlForPill(url: string): { head: string; tail: string } {
  try {
    const u = new URL(url);
    const host = u.hostname.replace(/^www\./, "");
    const rest = (u.pathname === "/" ? "" : u.pathname) + u.search + u.hash;
    return { head: host || url, tail: rest };
  } catch (_) {
    return { head: url, tail: "" };
  }
}

/**
 * Decide what the paste row shows for this binding, and draw it.
 *
 * THE RULE, read out of the code rather than assumed. The app grid marks a tile
 * `.current` when `getBinding(key).app === app.path` (see `renderGrid`'s
 * `isCurrent`) over `cachedApps()`. So:
 *
 *   · a URL binding      -> pill. The grid can never show a URL.
 *   · an app binding whose path IS in the detected list -> NO pill. The tile
 *     is right there wearing `.current`; a pill would say the same thing twice.
 *   · an app binding whose path is NOT in the list -> pill. This is the case
 *     the owner did not ask about and it is the SAME defect one step worse: an
 *     app bound by pasting a path (a portable exe, a game launcher, anything
 *     off the Start Menu) has no tile to highlight AND an empty field, so it is
 *     invisible twice over.
 *   · nothing bound      -> no pill, and the row is byte-for-byte what it was.
 *
 * TWO deliberate calls, both stated so they are not read as oversights:
 *
 * 1. The membership test uses the UNFILTERED cached list, not what the grid is
 *    rendering this instant. Typing in the search box narrows the grid, and a
 *    pill that appeared and vanished as the query moved would be noise on every
 *    keystroke. The question the pill answers is "does this key's assignment
 *    appear anywhere in this panel", and with the search box cleared it does.
 * 2. `cachedApps()` being null means NOT YET SCANNED, never "not in the list" —
 *    the same distinction `wireProfileChip` makes for `findBrowserByExe`. Draw
 *    nothing and ask again when the scan lands, so a first run adds a pill only
 *    to the genuinely invisible bindings instead of flashing one onto every
 *    app-bound key. The grid beside it is showing "Scanning this device…" for
 *    the same interval, so the two agree about what is not yet known.
 */
function renderPathValue(key: string, binding: KeyBinding | undefined): void {
  const host = _panel?.querySelector<HTMLElement>("#ed-val");
  const input = _panel?.querySelector<HTMLInputElement>("#ed-path");
  if (!host || !input) return;

  const clear = (): void => {
    host.hidden = true;
    host.innerHTML = "";
    input.hidden = false;
    // Back to the unbound placeholder — paintPathPill sets the "replace"
    // wording, and nothing else restores this one.
    input.placeholder = "…or paste a file path / URL";
  };

  const url = binding?.web_url ?? null;
  if (url) {
    const { head, tail } = splitUrlForPill(url);
    console.info(`bp: path pill — key=${key} kind=url host=${head} len=${url.length}`);
    paintPathPill({
      key, host, input, head, tail,
      title: url,
      value: url,
      paintDisc: (d) => {
        // A globe, NOT the default browser's icon. Which browser opens this
        // link is answered by the Browser-profile chip at the top of the panel,
        // for this same binding — a second, quieter answer painted 300px below
        // it could only ever disagree with the first one. This disc says
        // "this assignment is a link", which is the thing the row was missing.
        d.classList.add("ed-val-disc-url");
        d.textContent = "🌐";
      },
    });
    return;
  }

  const app = binding?.app ?? null;
  if (!app) { clear(); return; }

  const apps = cachedApps();
  if (!apps) {
    clear();
    void loadApps().then(() => {
      // The editor may have moved to another key, or closed, while we waited.
      if (_currentKey !== key || !host.isConnected) return;
      renderPathValue(key, getBinding(key));
    });
    return;
  }
  if (apps.some((a) => a.path === app)) {
    console.info(`bp: path pill suppressed — key=${key} app is on a .current grid tile`);
    clear();
    return;
  }

  const name = binding?.label || cleanLabel(app.split(/[\\/]/).pop() || app);
  console.info(`bp: path pill — key=${key} kind=app name=${name} (no grid tile for this path)`);
  paintPathPill({
    key, host, input,
    head: name,
    tail: "",
    title: app,
    value: app,
    paintDisc: (d) => paintAppDisc(d, binding?.icon_override, name, 0),
  });
}

/**
 * Draw the pill and wire its THREE intentions — one more than before.
 *
 * ✕ means "clear this". The body means "let me edit this one precisely"
 * (loads the exact value back into the field, see the chip click handler
 * below). And now, simply typing or pasting into the field that sits right
 * beside the pill means "replace it with something new" — see
 * OWNER'S FATHER TEST below for why that third path had to exist.
 *
 * OWNER'S FATHER TEST (2026-09-04). Until now this function did
 * `o.input.hidden = true`, so a filled slot's paste field did not exist as
 * far as a first-time user could see — only the pill and its ✕. The owner's
 * father could assign an app to an EMPTY key, but on a key that already had a
 * link bound, he could not see how to put a NEW link in: he did not realise
 * the small ✕ had to be pressed first (which CLEARS the binding — not what
 * he wanted) before he could paste. DECIDED: a filled slot offers replace
 * DIRECTLY. The field stays visible and usable right alongside the pill —
 * nothing to discover, nothing to clear first — and what used to be an
 * unguarded overwrite is now guarded by ONE inline confirm instead
 * (`confirmReplace`, wired at every call site that can reach this: `Enter` /
 * `Assign` in `submitPathField`, `Done`, and the 4b disc). The pill itself is
 * unchanged: click its body to load the exact value for a precise edit,
 * click ✕ to clear — this function only stopped hiding the thing beside it.
 */
function paintPathPill(o: {
  key: string;
  host: HTMLElement;
  input: HTMLInputElement;
  head: string;
  tail: string;
  title: string;
  value: string;
  paintDisc: (disc: HTMLElement) => void;
}): void {
  o.host.innerHTML = "";
  o.host.hidden = false;
  // GATE REMOVED — see the doc comment above. The field stays visible so a
  // paste lands the instant it happens, no click required first.
  o.input.hidden = false;
  // The field BEHIND the pill must be empty at rest, or `#ed-done` would read
  // a value nobody typed and re-assign it. Stated here rather than relied
  // upon: this is the invariant the whole `pending` trace above rests on —
  // still true with the field visible, since "empty" is what makes a fresh
  // paste distinguishable from a click-to-edit (`_pathSeed` stays null here).
  o.input.value = "";
  o.input.placeholder = "Paste a new link or path to replace it…";
  _pathSeed = null;
  syncPathRow();

  const chip = document.createElement("button");
  chip.type = "button";
  chip.className = "bp-chip bp-chip-set ed-val-chip";
  chip.title = o.title;                    // the FULL url / path, always
  chip.setAttribute("aria-label", `Assigned: ${o.title}. Press to edit.`);

  const disc = document.createElement("span");
  disc.className = "ed-tile-disc bp-chip-disc";
  o.paintDisc(disc);
  chip.appendChild(disc);

  // With a tail, the head HOLDS and the tail GIVES — the chip's own rule. With
  // no tail the single run is the one that gives, because an app name has no
  // assumable width and must be free to ellipsize (CLAUDE.md).
  const head = document.createElement("span");
  head.className = o.tail ? "bp-chip-browser" : "bp-chip-profile";
  head.textContent = o.head;               // textContent — user data
  chip.appendChild(head);

  if (o.tail) {
    const sep = document.createElement("span");
    sep.className = "bp-chip-sep";
    sep.textContent = "·";
    const tail = document.createElement("span");
    tail.className = "bp-chip-profile";
    tail.textContent = o.tail;
    chip.append(sep, tail);
  }
  o.host.appendChild(chip);

  // Outside the <button>, for the reason `renderProfileChip` records: a button
  // inside a button is invalid HTML and the inner one's clicks are unreliable.
  const x = document.createElement("button");
  x.type = "button";
  x.className = "bp-chip-x";
  x.textContent = "✕";
  x.title = "Clear this assignment";
  x.setAttribute("aria-label", "Clear this assignment");
  x.addEventListener("click", (e) => {
    e.stopPropagation();
    console.info(`bp: path pill ✕ — clearing the binding for key=${o.key}`);
    clearBindingFromPill(o.key);
  });
  o.host.appendChild(x);

  chip.addEventListener("click", () => {
    o.host.hidden = true;
    o.host.innerHTML = "";
    o.input.hidden = false;
    o.input.value = o.value;
    _pathSeed = o.value;
    syncPathRow();                          // Assign lights up; a URL raises the 4b disc
    o.input.focus();
    o.input.select();                       // so a long URL can be replaced in one keystroke
    console.info(
      `bp: path pill opened for editing — key=${o.key} ${o.value.length} chars, ` +
      `4b disc ${classifyPathInput(o.value) === "url" ? "shown" : "hidden"}`,
    );
  });
}

/**
 * The pill's ✕ — the SAME commit "Remove binding" uses, so normalisation, the
 * save and the toast are all reused rather than re-implemented.
 *
 * Two differences from `handleRemove`, both required by the owner's wording
 * ("clears the assignment back to an empty, focused input, ready for a new
 * value"): `keepOpen` stops `commit()` collapsing the editor, and `onSaved`
 * re-renders page 1. The re-render is not decoration — clearing a binding also
 * stales the "Bound to …" sub-title, the Remove button, the grid's `.current`
 * tile and the browser-profile row, and with `keepOpen` nothing else repaints
 * any of them. That is the same failure `commitProfile`'s `onSaved` exists for:
 * *"a save that worked being indistinguishable from one that did not."*
 *
 * No confirm and no Undo, matching `handleRemove`, which this is: one press of
 * work to redo, and the grid is right there.
 */
function clearBindingFromPill(key: string): void {
  void commit(
    { app: null, web_url: null, label: null, icon_override: null },
    {
      // A cleared key cannot collide with an OS shortcut — same reasoning as
      // `handleRemove`, which also skips it.
      skipConflict: true,
      keepOpen: true,
      onSaved: () => {
        if (_currentKey !== key) return;
        renderPanel(key);
        _panel?.querySelector<HTMLInputElement>("#ed-path")?.focus();
      },
    },
  );
}

/** Paint the disc and wire its press. */
function wirePathDisc(key: string, path: HTMLInputElement, disc: HTMLButtonElement): void {
  const paint = (info: DefaultBrowserInfo | null): void => {
    disc.innerHTML = "";
    if (info) {
      // The REAL default browser's own icon, resolved by Rust from the same
      // http/https handler `run_browser` launches through — so the disc cannot
      // name a browser other than the one the key would actually open. Painted
      // through `paintAppDisc`, the app's single icon path, so a malformed
      // payload falls back the way every other missing icon does instead of
      // showing the broken-image glyph.
      disc.classList.remove("ed-path-disc-unset");
      paintAppDisc(disc, info.icon_base64, info.name, 0);
      disc.title = `Opens in ${info.name} — press to choose a different browser or profile`;
    } else {
      // Rust could not resolve a handler (or the lookup has not landed yet).
      // The unset treatment — dashed ring and `◍` — never a guess: showing
      // Brave's icon because Brave happens to be installed would be a lie on
      // exactly the machine that matters, one whose default is Firefox.
      disc.classList.add("ed-path-disc-unset");
      disc.textContent = "◍";
      disc.title = "Opens in your default browser — press to choose a browser or profile";
    }
  };

  paint(cachedDefaultBrowser());
  if (!cachedDefaultBrowser()) {
    void loadDefaultBrowser().then((info) => {
      if (disc.isConnected) paint(info);
    });
  }
  disc.setAttribute("aria-label", "Choose which browser opens this link");

  disc.addEventListener("click", () => {
    const value = path.value.trim();
    // CALL SITE 3 of 4. The disc only appears over a url (`syncPathDisc`), so
    // this guard and that one have to agree by construction — they now read
    // the same function rather than two copies of the same regex.
    if (classifyPathInput(value) !== "url") return;
    // Loaded back out of the pill and not edited: it is ALREADY committed, so
    // there is nothing to save — and saving it would clear the very pin the
    // page is about to set, then leave it cleared if the user backs out with ←.
    if (isUnchangedPillValue(value)) {
      console.info("bp: 4b disc pressed on an unedited pill value — opening the page, committing nothing");
      openProfilePage(key, null);
      return;
    }
    // The replace gate (2026-09-04) — a fresh URL beside an ALREADY-BOUND
    // key's pill still asks first, same as every other path into a replace.
    // `_pathSeed === null` because a click-to-edit already returned above via
    // `isUnchangedPillValue`, or is a deliberate small edit that reaches the
    // committing branch below unguarded — same reasoning as `submitPathField`.
    if (_pathSeed === null) {
      const existing = getBinding(key);
      if (existing && (existing.app || existing.web_url)) {
        confirmReplace(key, existing, () => {
          console.info(`bp: 4b disc pressed — committing ${value} then opening the page`);
          void assignFromPath(value, {
            keepOpen: true,
            onSaved: () => {
              if (_currentKey !== key) return;
              // Page 1 first, so the Undo row this offers is there waiting
              // when the user backs out of the profile page with ←.
              finishReplace(key, existing);
              openProfilePage(key, null);
            },
          });
        });
        return;
      }
    }
    console.info(`bp: 4b disc pressed — committing ${value} then opening the page`);
    // The URL has to be BOUND before there is anything for the page to pin to,
    // so this commits first and turns the page from `onSaved` — the same
    // ordering the app-grid branch uses, for the same reason: a conflict the
    // user cancels must not leave a page open over a binding that was never
    // written. `keepOpen` is what stops commit() collapsing the editor first.
    void assignFromPath(value, {
      keepOpen: true,
      onSaved: () => openProfilePage(key, null),
    });
  });
}

function renderGrid(): void {
  const grid = _panel?.querySelector<HTMLElement>("#ed-grid");
  const empty = _panel?.querySelector<HTMLElement>("#ed-empty");
  if (!grid || !empty) return;

  // The tiles, the icon fallback, the RENDER_CAP notice and the "no apps
  // match" copy all live in components/app-grid.ts now — shared verbatim with
  // the App-exceptions setting.
  const key = _currentKey;
  drawAppGrid(
    grid,
    empty,
    {
      query: _query,
      isCurrent: (app) => (key ? getBinding(key)?.app ?? null : null) === app.path,
      onPick: (app) => {
        // THE DEFECT THIS FEATURE EXISTS TO FIX. This used to be a bare
        // `commit({...})`, and `commit()` ends in `closePanel()` — so pressing
        // a browser tile bound the key and destroyed the panel in the same
        // tick, and `wireProfileChip` never got a chance to show anything. The
        // owner: *"pressing any browser to a letter just assigns it, i expected
        // something to change in the app choosing dialogue after detecting it
        // is a browser to let me choose a profile."*
        //
        // The bind still commits instantly. What changes is what happens after.
        const b = findBrowserByExe(app.path);
        // `findBrowserByExe` returning null means NOT YET KNOWN, not "no". On a
        // true first run that resolves to "bind and close", which is the
        // owner's decision — nobody waits up to 2.2s to be offered something
        // optional. Reopening the key once the scan has landed shows the chip.
        const multi = !!b && b.profiles.length > 1;
        // A browser with exactly ONE profile never opens the page — there is no
        // choice to make — but the profile IS still written, so the launch is
        // explicit instead of relying on Chromium's last-used.
        const only = b && b.profiles.length === 1 ? b.profiles[0] : null;
        const binding = {
          app: app.path,
          web_url: null,
          label: app.name,
          icon_override: app.icon_base64 ?? null,
          // `browser_exe` stays null on an app binding: the exe already IS
          // `app`, and two sources of truth for one path is how this breaks
          // later. The other two are nulled for a non-browser app by
          // commit()'s normalisation, which is what clears a pin that
          // described the target being replaced.
          browser_exe: null,
          browser_profile_dir: only?.directory ?? null,
          // The ACCOUNT LABEL, exactly as a hand-picked tile would store it
          // — never `display_name` directly, or a one-profile browser would
          // label its HUD chip differently from every other pin.
          browser_profile_name: only ? labelOf(only) : null,
        };
        // The ordinary bind — unchanged byte-for-byte from before this
        // feature. `keepOpen`/`onSaved` turn the page for a multi-profile
        // browser exactly as they always did.
        const bindNow = () => void commit(binding, {
          keepOpen: multi,
          onSaved: multi && key ? () => openProfilePage(key, b, true) : undefined,
        });
        // The replace gate (2026-09-04, OWNER'S FATHER TEST) — only for a key
        // that already points somewhere ELSE. Re-pressing the tile that is
        // already `.current` is a no-op in effect and asking about it would be
        // noise, so that case still binds instantly like every empty key does.
        const existing = key ? getBinding(key) : undefined;
        const alreadyBound = !!(existing && (existing.app || existing.web_url));
        const sameTarget = existing?.app === app.path;
        if (alreadyBound && !sameTarget && key) {
          confirmReplace(key, existing!, () => void commit(binding, {
            keepOpen: true,
            onSaved: () => {
              if (_currentKey !== key) return;
              finishReplace(key, existing!);
              if (multi) openProfilePage(key, b, true);
            },
          }));
          return;
        }
        bindNow();
      },
    },
    // Abandon a late scan result if the editor moved to another key or closed.
    () => _currentKey === key && key !== null,
  );
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

/**
 * Shared by the paste row, Enter in that row, Browse files… and the 4b disc.
 *
 * `opts` is forwarded verbatim to `commit`. Every existing caller passes
 * nothing and therefore behaves exactly as before — only the 4b disc uses it,
 * to keep the editor open so it can turn the page onto the URL it just bound.
 */
async function assignFromPath(raw: string, opts: CommitOptions = {}): Promise<void> {
  const value = raw.trim();
  if (!value || !_currentKey) return;

  // CALL SITE 4 of 4, and the one that decides what the binding IS. This was
  // `/^https?:\/\//i` until 1.0.88; `youtube.com` failed it and became an APP
  // binding that only worked because the engine's Start Menu cascade happened
  // to rescue it.
  if (classifyPathInput(value) === "url") {
    // NORMALISE AT COMMIT TIME, not at classify time. The field, the pill and
    // the disc all keep working with exactly what the user typed; only the
    // value that reaches `web_url` is repaired, and it is repaired once. See
    // `normaliseUrl` for which Rust consumer needs it and why.
    const url = normaliseUrl(value);
    let host = url;
    try { host = new URL(url).hostname.replace(/^www\./, ""); } catch (_) { /* keep raw */ }
    // No browser-profile fields here ON PURPOSE: this key is being pointed at
    // a NEW url, so any pin from the old target must go. commit() normalises
    // the three of them to null explicitly — see its header. Until 2026-08-26
    // that clearing was an accident of the full-replace save rather than a
    // decision, which is how the same omission silently wiped a pin the user
    // had just set on a url they were only editing.
    void commit({ app: null, web_url: url, label: cleanLabel(host), icon_override: null }, opts);
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
  // Same rule as the URL branch: a new target clears the pin, via commit()'s
  // normalisation rather than by leaving the fields off and hoping.
  void commit({
    app: value,
    web_url: null,
    label: cleanLabel(name),
    icon_override: icon,
  }, opts);
}

function handleRemove(): void {
  // The three browser-profile fields are nulled by commit()'s normalisation —
  // clearing a key must not leave a pin behind in config.json.
  void commit({ app: null, web_url: null, label: null, icon_override: null }, {
    skipConflict: true,
  });
}

/** The key's CURRENT binding in the live config, if any. */
function currentBinding(key: string): KeyBinding | undefined {
  const profile = _config?.profiles.find((p) => p.name === _config?.active_profile);
  return profile?.bindings[key] ?? profile?.bindings[key.toLowerCase()];
}

/** PROBLEM 267 — see the call in `commit`. Fire-and-forget by design. */
function fetchSiteIconOnce(key: string, url: string): void {
  invoke<string | null>("fetch_site_icon", { url })
    .then((icon) => {
      if (!icon || !_onSave) return;
      const current = currentBinding(key);
      if (!current || current.web_url !== url || current.site_icon) return;
      console.info(`key-editor: site icon fetched for ${key} (${icon.length} chars) — attaching`);
      _onSave(key, { ...current, site_icon: icon });
    })
    .catch(() => { /* an absent icon is the whole failure mode */ });
}

interface CommitOptions {
  /** Skip the Space+<key> conflict prompt. Remove and the profile chip do. */
  skipConflict?: boolean;
  /** Save WITHOUT collapsing the editor. Only the profile chip does. */
  keepOpen?: boolean;
  /** Override the confirmation text. Only the profile chip does. */
  toast?: string;
  /**
   * Fired AFTER `_onSave` has actually run — never when `commit` bails out.
   *
   * It exists because "did this save?" was previously unanswerable from a call
   * site: `commit` returns `Promise<void>` and its two early exits (no key /
   * no handler, and a conflict the user has yet to answer) look identical to
   * success from outside. The app-grid's browser branch has to know, or a
   * cancelled conflict prompt would still turn the page onto a binding that was
   * never written. Carried through `showConflict` too, so "Bind anyway" reaches
   * it and "Cancel" does not.
   */
  onSaved?: () => void;
}

/**
 * Commit a binding: conflict-check first, then save, pop the key, close.
 *
 * An OPTIONS OBJECT, not the positional `skipConflict` boolean this used to
 * take. There are now two independent switches, and `commit(b, true, false)`
 * at a call site is a coin flip about which is which.
 *
 * `binding` REPLACES the stored one outright (main.ts does
 * `profile.bindings[key] = binding`), it does not merge — so an omitted field
 * is a DELETED field, and TypeScript cannot warn about it because
 * `browser_exe?` and its two siblings are optional. That is how the
 * browser-profile pin was being erased: `assignFromPath` calls
 * `commit({ app, web_url, label, icon_override })`, and those four keys were
 * the whole saved binding a moment later.
 *
 * `full` below closes that hole by NORMALISING to a complete KeyBinding here,
 * once, instead of asking six call sites to remember three fields.
 *
 * NOTE THIS IS NOT A MERGE, DELIBERATELY. Re-pointing a key at a new target
 * must CLEAR the pin — the pin described the target being replaced, and a
 * merge would resurrect it so a newly-dropped URL silently kept opening in the
 * old browser profile. That is exactly the bug `BINDING_RESET` exists to
 * prevent in keyboard-matrix.ts, whose `updateBinding` DOES merge. The two
 * save paths therefore have opposite semantics on purpose: matrix = merge +
 * explicit reset, panel = replace + explicit normalise. What changed here is
 * only that the panel's clearing is now stated instead of accidental.
 */
async function commit(binding: KeyBinding, opts: CommitOptions = {}): Promise<void> {
  const key = _currentKey;
  if (!key || !_onSave) {
    // NEVER SILENT AGAIN. This early return is the same `_currentKey`-is-null
    // trap that produced PROBLEM 199 (see the `#ed-done` comment above): it
    // saved nothing, showed nothing and logged nothing, so a save path that
    // had stopped working was indistinguishable from one that had nothing to
    // do. A frontend-only failure leaves no trace in debug.log either, which
    // is why this reports through all three channels.
    const why = !key ? "no key is open (_currentKey is null)" : "no onSave handler";
    const msg = `key-editor: commit ABORTED — ${why}; nothing was saved`;
    console.error(msg, binding);
    showToast("⚠️ Not saved — the key editor lost track of which key this was");
    void invoke("frontend_log", { msg }).catch(() => {});
    return;
  }

  if (!opts.skipConflict) {
    try {
      const conflict = await invoke<ConflictResult>("show_conflict_check", {
        keyCombo: `Space+${key.toUpperCase()}`,
      });
      if (conflict.has_conflict) {
        showConflict(conflict, binding, opts);
        return;
      }
    } catch (_) { /* the check is advisory; never block a binding on it */ }
  }

  // The COMPLETE binding. Every optional field is stated, so a caller that
  // leaves one off can no longer delete it by omission.
  const full: KeyBinding = {
    app: binding.app ?? null,
    web_url: binding.web_url ?? null,
    label: binding.label ?? null,
    icon_override: binding.icon_override ?? null,
    // Never `""` — the owner's hard requirement is that a URL with no specific
    // browser opens in the OS default, and null is the only value that says so
    // (types.ts spells this out; Rust re-checks it in
    // `browser_profiles::should_use_specific_browser`).
    browser_exe: binding.browser_exe ?? null,
    browser_profile_dir: binding.browser_profile_dir ?? null,
    browser_profile_name: binding.browser_profile_name ?? null,
    // PROBLEM 267 — a link's favicon SURVIVES a re-commit of the same URL (a
    // browser-profile pick re-commits the whole binding through this same
    // function) and is DROPPED with the URL when the key is pointed elsewhere:
    // the icon described the target being replaced.
    site_icon: binding.site_icon
      ?? (binding.web_url && currentBinding(key)?.web_url === binding.web_url
        ? currentBinding(key)?.site_icon ?? null
        : null),
  };

  console.info(
    `key-editor: onSave key=${key} app=${full.app ?? "null"} url=${full.web_url ?? "null"} ` +
    `browser_exe=${full.browser_exe ?? "null"} profile_dir=${full.browser_profile_dir ?? "null"}`,
  );
  _onSave(key, full);

  // PROBLEM 267 — "Site icons are fetched once, when you bind the link."
  // THIS is the once. A URL binding with no icon asks Rust for the site's
  // favicon (`/favicon.ico`, then the page's <link rel=icon>, 3 s each) in
  // the background — the save above has already happened and the editor is
  // not kept waiting on the network — and, when it arrives, re-saves the
  // binding with the icon attached, PROVIDED the key still points at that
  // URL and still has no icon. A failure writes nothing; the next edit of
  // the key lands here again, which is the one retry the design allows.
  if (full.web_url && !full.site_icon) {
    fetchSiteIconOnce(key, full.web_url);
  }

  // PROBLEM 242 — the ONE place a save is reported to the first-run tour, and
  // it is deliberately here rather than at any of the seven call sites that
  // reach `commit`. Both of this function's early exits (no key / no handler,
  // and a pending conflict prompt) return above this line, so reaching it
  // means a binding genuinely landed. A CLEAR does not count — `commit` is
  // also how the ✕ empties a key, and step 3 goes on to ask the user to press
  // the combo, which would then do nothing forever.
  //
  // The THIRD argument (2026-09-06) is `full`'s own profile name rather than
  // anything the picker reported, and that is the point: `full` is the binding
  // that is being written, so step 3 can never name a profile the key does not
  // actually open. It arrives here for a profile pick too — `commitProfile`
  // re-commits the whole binding with the pin attached and lands on this same
  // line, which is why `tourBindingSaved` has to accept a second save for a key
  // it is already showing.
  if (full.app || full.web_url) {
    tourBindingSaved(key, full.label ?? key.toUpperCase(), full.browser_profile_name ?? null);
  }

  // The point of no return has passed, so the paste row must not still be
  // holding text that a later "Done" would re-assign over the top of what was
  // just saved. That was harmless while every commit closed the panel; with
  // `keepOpen` the field survives, and re-running `assignFromPath` would wipe
  // the pin the user had just set.
  const pathInput = _panel?.querySelector<HTMLInputElement>("#ed-path");
  if (pathInput) {
    pathInput.value = "";
    const assignBtn = _panel?.querySelector<HTMLButtonElement>("#ed-assign");
    if (assignBtn) assignBtn.disabled = true;
  }
  // The seed described the field's contents, and the field has just been
  // emptied. A stale seed would then match the next paste only by coincidence,
  // and a coincidence that skips a bind is the same silent-no-save class of bug
  // as PROBLEM 199.
  _pathSeed = null;

  const label = full.label ?? key.toUpperCase();
  showToast(
    opts.toast ??
      (full.app || full.web_url
        ? `✅ Space+${key.toUpperCase()} → ${label}`
        : `🗑️ Cleared: Space+${key.toUpperCase()}`),
  );

  // Whatever the caller wanted to do with a save that actually happened. Runs
  // BEFORE the close below, so a caller can cancel that close by opening a page
  // over the top of it (`keepOpen`) rather than racing it.
  opts.onSaved?.();

  // A one-property edit leaves the editor where it was: the user is still
  // looking at this key, and the chip has already repainted itself.
  if (opts.keepOpen) return;

  closePanel();
  // Pop the key after the editor has collapsed back into it.
  window.setTimeout(() => {
    const cell = getKeyCell(key);
    if (cell) animateKeyPop(cell);
  }, 220);
}

function showConflict(
  conflict: ConflictResult,
  binding: KeyBinding,
  opts: CommitOptions = {},
): void {
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
    // Carry the original caller's options through — only the conflict answer
    // changes here, not whether the editor should close afterwards.
    void commit(binding, { ...opts, skipConflict: true });
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
