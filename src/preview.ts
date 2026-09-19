/**
 * preview.ts — DEV-ONLY visual harness for the dashboard.
 *
 * Renders the real keyboard-matrix component and the real stylesheet against
 * a stub config so the design can be compared to Dashboard Earthy v2.dc.html
 * without the Rust backend. Not a Vite build input; it never ships.
 *
 * Query flags: ?dark  ?profiles  ?gear  ?specials  ?editor  ?double  ?wide
 *              ?fun  ?sky  ?conflict  ?expand  ?tour  ?about-open  ?rollback
 *              ?portable  ?osdark
 *
 * ?portable (H6) stubs `get_packaged_startup`'s PORTABLE tuple, which is the
 * only way to look at the greyed "Run at startup" row and its note without
 * unzipping a portable build. `window.__previewStartupProbe()` walks every
 * tuple the backend can return and prints a pass/fail table.
 *
 * ?osdark (H4) starts the harness on a machine whose Windows app mode is DARK.
 * `window.__previewThemeProbe()` then flips it through the real event bus and
 * checks that the dashboard's rule and the overlay's `applyThemeName` resolve
 * the theme "auto" identically at every value.
 *
 * ?tour flips the stub's `tour_done` to FALSE so the first-run walkthrough
 * (PROBLEM 242) runs — the harness otherwise reports it as already seen, so
 * every other flag above keeps behaving exactly as it did. Clicking a letter
 * really opens the editor here (see initKeyboardMatrix below), so steps 1 and
 * 2 drive end to end; step 3 waits on the engine's `st-launched` event, which
 * has no engine here — fire it by hand from the console:
 *
 *     window.__previewEmit("st-launched", { key: "s", label: "Slack" })
 *
 * That is the same in-memory bus a real `listen()` reaches, so what is being
 * exercised is the tour's real handler, not a shortcut around it.
 *
 * STEP 2b — the browser-profile detour (2026-09-06) — is reachable here too,
 * because `list_browser_profiles` now answers with two stub browsers instead
 * of `[]` (see PREVIEW_BROWSERS below). Three paths worth walking, and the
 * only difference between them is which tile is pressed in step 2:
 *
 *   Brave   (2 profiles) → step 2b over "Which Brave profile?"; picking Arpon
 *                          lands on step 3 reading "Space+K opens Brave
 *                          (Arpon)." Done / ✕ / the skip row land on the same
 *                          step 3 without the parenthesis.
 *   Chrome  (1 profile)  → no picker, straight to step 3.
 *   Slack   (not a browser) → no picker, straight to step 3. Unchanged.
 *
 * ?double selects Double on the ring pill, which is the ONLY way to see the
 * inert treatment of the "Show special keys" switch without the backend — and
 * an inert control is exactly the kind of state that ships broken because
 * nobody looked at it.
 *
 * ?wide selects Wide, the middle shape. (It replaces ?classic, and ?double
 * replaces ?rows2: the switch-plus-pill pair those two flags existed for was
 * folded into one 3-way pill on 2026-09-01.)
 *
 * ?expand opens the panel in its full-screen state, which is the only way to
 * look at that layout without a backend.
 *
 * ?editor opens the key editor on Space+C (an app binding). ?editor=<key>
 * opens it on any other key instead — added 2026-09-04 so the replace-confirm
 * flow (OWNER'S FATHER TEST — pasting/picking OVER an existing binding) has
 * something to watch work: ?editor=y is a bound URL, ?editor=b is an app
 * pinned to a browser profile (its confirm text is the feature's own worked
 * example, "Replace 'Google Chrome — Arpon' with this?"), and any other
 * lowercase letter not in DEMO below is an empty key, for the ordinary
 * instant-bind path this must NOT change.
 */
import { initKeyboardMatrix, updateMatrix, DESIGN_W, DESIGN_H } from "./components/keyboard-matrix";
import { initKeyDetailPanel, openPanel, closePanel, getCurrentKey } from "./components/key-detail-panel";
import { initTour, maybeStartTour, startTour } from "./components/tour";
import type { AppConfig, AppInfo, KeyBinding } from "./types";
import {
  toggleSwitchHtml, sliderShell, segRowHtml, paintInert,
  SPECIALS_INERT_NOTE, RING_OPTS, ringLayoutFor,
  groupHeadingHtml, filterSettings, setPanelExpanded,
  showRingPreview, refreshRingPreview, isRingPreviewShowing, RING_PREVIEW_MS,
  wireSegRowsKeyboard, wireSegIndicators, positionSegIndicator, aboutRowHtml, renderThirdPartyGroups,
  fetchAboutInfo, requestUpdateCheck, openAboutLink,
  // REVIEW FIXES 2026-09-05 (H6) — "Run at startup"'s ownership rules. The
  // SAME functions settings-panel.ts calls; see `?portable` below.
  startupRowIsInert, startupShownAsOn, startupOutcome, startupIsPortable,
  PORTABLE_STARTUP_STATE, type StartupOwnership,
  type AboutLinkKind, type ThirdPartyEntry,
  // PROBLEM 267 — the icon ring's rows and the exception tiles, from the
  // same leaf data the panel renders.
  MIDDLE_STYLE_OPTS, MIDDLE_SCOPE_OPTS, ALL_LAYOUT_OPTS, EXC_SCOPE_OPTS, EIGHT_PICKER_NOTE,
} from "./components/controls";
// PROBLEM 267 — the ring itself, rendered by the SAME leaf renderer the
// overlay uses, from a stub payload (?ring / ?ring=all). See the end of the file.
import { renderMiddleRing, setRingArmed, fitPill, previewWave, previewAim, previewWaveStep, waveSnapshot, waveTargets, type MiddleRingPayload, type RingItem } from "./components/middle-ring";
// ABOUT (feature 1) — the same data import settings-panel.ts uses, so the
// harness's third-party count and grouped list are the real ones, not a
// fabricated stand-in.
import thirdPartyRaw from "./generated/third-party.json";
const THIRD_PARTY = thirdPartyRaw as ThirdPartyEntry[];
import { resolveSpecials, toggleSpecialCard } from "./components/special-cards";
import { openConflictPrompt } from "./components/conflict-prompt";
import { buildStarrySky } from "./components/starry-sky";
import { initProfileEditor } from "./components/profile-editor";
// PROBLEM 259 — the own-window fallback, driven by ?ownwindow at the end of
// this file. A leaf module, so importing it costs the harness nothing.
import { initOwnWindowKeys } from "./own-window-keys";

const q = new URLSearchParams(location.search);

const DEMO: Record<string, string> = {
  c: "Chrome", s: "Slack", n: "Notion", f: "Figma", m: "Mail", t: "Terminal",
  g: "GitHub", d: "Discord", w: "Word", e: "Excel", p: "Photos", o: "Obsidian",
  v: "VS Code", z: "Zoom",
};

const bindings: Record<string, KeyBinding> = {};
"abcdefghijklmnopqrstuvwxyz".split("").forEach((k) => {
  bindings[k] = DEMO[k]
    ? { app: `C:\\Apps\\${DEMO[k]}.exe`, web_url: null, label: DEMO[k] }
    : { app: null, web_url: null, label: null };
});
// 2026-09-04 — the key-editor's replace-confirm harness (?editor=<key>, see
// below) needs at least one bound key of each kind the OWNER'S FATHER TEST's
// flows touch: a bare URL, and an app pinned to a browser profile (this one's
// wording is the confirm's own worked example — "Google Chrome — Arpon").
// Both are 7-field-complete, matching what commit() itself always normalises
// to (key-detail-panel.ts) — neither key nor letter collides with DEMO above.
bindings.y = {
  app: null, web_url: "https://youtube.com", label: "Youtube",
  icon_override: null, browser_exe: null, browser_profile_dir: null, browser_profile_name: null,
};
bindings.b = {
  app: "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  web_url: null, label: "Google Chrome", icon_override: null,
  browser_exe: null, browser_profile_dir: "Profile 1", browser_profile_name: "Arpon",
};
// PHASE A (2026-09-18) — the twelve specials Rust seeds into every profile
// (`schema::DEFAULT_SPECIALS`), on their default keys, so the board's
// non-letter keys, the tray's combos and the editor's "Spaceadom special"
// page all render as they do in the app. Same table, same order.
([
  ["esc", "boss_key"], ["backtick", "pip"], ["tab", "pip_fullscreen"],
  ["backspace", "force_close"], ["ralt", "cycle_profile"], ["comma", "search"],
  ["period", "pause"], ["semicolon", "voice_typing"], ["slash", "screenshot"],
  ["quote", "osk"], ["up", "scroll_top"], ["down", "scroll_bottom"],
  ["left", "move_window_left"], ["right", "move_window_right"],
] as const).forEach(([key, id]) => {
  bindings[key] = {
    app: null, web_url: null, label: null, icon_override: null,
    browser_exe: null, browser_profile_dir: null, browser_profile_name: null,
    site_icon: null, action: { kind: "special", id },
  };
});
// One non-special action on a non-letter key, so ?editor=7 shows the
// "Windows setting" page with a current row.
bindings["7"] = {
  app: null, web_url: null, label: "Display", icon_override: null,
  browser_exe: null, browser_profile_dir: null, browser_profile_name: null,
  site_icon: null, action: { kind: "uri", target: "ms-settings:display" },
};

const config: AppConfig = {
  version: 1,
  active_profile: "Founders",
  rollover_ms: 50,
  guide_hud_delay_ms: 300,
  // PROBLEM 174 — off, matching the shipped default, so the harness shows the
  // switch in the state a new user actually meets.
  hud_toast_flight: false,
  // "auto" is the shipped default; ?double shows the Double state, where the
  // specials switch below goes inert.
  hud_band_count: q.has("double") ? "two" : "auto",
  // ON is the shipped default (schema.rs, the owner's 2026-08-27 decision),
  // so the harness shows the ring pill on Compact — the shape a new user
  // actually meets. ?wide flips it to the middle segment.
  hud_magnetic_layout: !q.has("wide"),
  // PROBLEM 195 — true, matching the shipped default, so the harness shows the
  // "Don't send logs" switch in the OFF state a new user actually meets.
  send_logs: true,
  // PROBLEM 242 — TRUE by default here, which is the opposite of a fresh
  // install, and deliberately so: the harness's job is to show one surface at
  // a time, and a walkthrough that opened over ?gear or ?editor would be in
  // front of whatever was actually being looked at. ?tour asks for it.
  tour_done: !q.has("tour"),
  opacity_floor_pct: 30,
  // PHASE A — ?advanced shows the editor's "Run command" page and the full
  // catalogue; off is the shipped default.
  advanced_mode: q.has("advanced"),
  browser_path: null,
  fullscreen_allowlist: [],
  dark_mode: q.has("dark"),
  sound_enabled: false,
  profiles: [
    // One profile with an emoji and two without, deliberately: the rule
    // `Profile.emoji` states is that a profile WITHOUT one keeps the look it
    // has always had, and that is only checkable when both are on screen at
    // once. The emoji is a ZWJ family (five code points) so the harness shows
    // the case a naive one-character cap would break.
    { name: "Founders", bindings, emoji: "👨‍👩‍👧", specials_seeded: true },
    { name: "Gamers", bindings: {}, emoji: null },
    { name: "Professionals", bindings: {}, emoji: null },
  ],
};

document.body.classList.toggle("nocturne", !!config.dark_mode);

// ?sky — the full Starry night scene without the backend: moon, 20-figure
// bands, crest-field sea, rigged galleon, storm. This is how the scene's
// geometry is MEASURED before it ever reaches the real app.
// ?conflict — the top-centre close offer, without the backend.
if (q.has("conflict")) {
  openConflictPrompt({
    process: "spacedeskservice.exe",
    product: "spacedesk",
    detail: "spacedesk forwards input to a second display and can intercept keys.",
  });
}

if (q.has("sky")) {
  document.body.classList.add("nocturne");
  document.body.dataset.theme = "starry";
  document.body.dataset.fun = "on";
  buildStarrySky();
}

// ---- the real component ----
initKeyboardMatrix(
  document.getElementById("keyboard-matrix")!,
  config,
  // Clicking a letter opens the real editor, mirroring main.ts's callback
  // including its 90ms ripple delay. This used to be a no-op: the ?editor flag
  // was the only way in, so nothing that BEGINS with a click on the board
  // could be watched here — which is most of the first-run tour (PROBLEM 242).
  (key, cell) => {
    if (getCurrentKey() === key) { closePanel(); return; }
    window.setTimeout(() => openPanel(key, config, cell), 90);
  },
  () => {},
);

// ---- fit (same maths as main.ts) ----
//
// REGRESSION SWEEP 2026-09-07 — this comment was a lie in three ways and each
// one made the harness show a board the app never draws:
//
//  1. **No zero guard.** `main.ts`'s `fit` bails on `!r.width || !r.height`;
//     this one did not, so a first measurement taken before layout (or in a
//     zero-sized viewport) evaluated `(0 - 12) / 320` and wrote
//     `scale(-0.0375)` — a board flipped upside down and rendered 3.75% of
//     size. Measured in the browser pane, which starts at 0x0.
//  2. **The old fill rule.** `Math.min(1, (w - 12) / …)` is the 1.0.36 formula
//     the owner rejected twice; the app has shipped `room * 0.75` capped at
//     2.0 since PROBLEM 128. A harness that scales the board differently from
//     the app cannot be used to judge the board's size at all.
//  3. **ResizeObserver only.** `main.ts` also listens on `window.resize`.
//
// All three are now literally `main.ts::wireKeyboardFit`'s body.
const outer = document.getElementById("keyboard-outer")!;
const scale = document.getElementById("keyboard-scale")!;
const FILL = 0.75;
const MAX_SCALE = 2.0;
const fit = () => {
  const r = outer.getBoundingClientRect();
  if (!r.width || !r.height) return;
  const room = Math.min(r.width / DESIGN_W, r.height / DESIGN_H);
  const s = Math.min(MAX_SCALE, room * FILL);
  scale.style.transform = `scale(${s.toFixed(4)})`;
  document.documentElement.style.setProperty("--ui-scale", s.toFixed(4));
};
fit();
new ResizeObserver(fit).observe(outer);
window.addEventListener("resize", fit);

// ---- specials tray ----
// Same list and same card the app uses (special-cards.ts), so the 8 entrance
// animations of spec §4 can be watched here without the backend.
const tray = document.getElementById("specials-tray")!;
resolveSpecials(config).forEach((spec, i) => {
  const item = document.createElement("button");
  item.type = "button";
  item.className = "special-item";
  item.dataset.spec = spec.id;
  item.setAttribute("aria-expanded", "false");
  item.style.animationDelay = `${60 + i * 30}ms`;
  const k = document.createElement("kbd"); k.textContent = spec.combo;
  const t = document.createElement("span"); t.textContent = spec.name;
  item.append(k, t);
  item.addEventListener("click", (e) => { e.stopPropagation(); toggleSpecialCard(item, spec, i); });
  tray.appendChild(item);
});

// ---- profile editor: THE REAL COMPONENT, on a stubbed backend ----
//
// This used to be a hand-rolled copy of four lines of row markup, and it was
// the exact drift this file exists to prevent — the harness would have gone on
// drawing a 1.0.95 row forever while the app grew a drag handle, an emoji disc
// and an edit mode. Two things had to change for the real module to load here:
//
//  1. `profile-editor.ts` imports `offerUndo` from main.ts. That is now a
//     DYNAMIC import, so this file is a LEAF (PROBLEM 148) — a static one
//     would pull main.ts's bootstrap in and it would rebuild the dashboard on
//     top of the harness.
//  2. `invoke` needs somewhere to go. `@tauri-apps/api`'s `invoke` reads
//     `window.__TAURI_INTERNALS__.invoke` AT CALL TIME (v2's core.js), not at
//     import time, so a stub installed anywhere before the first call is
//     enough — module evaluation order does not matter.
//
// The stub is a REAL in-memory backend, not a set of no-ops: reorder actually
// validates the way Rust does and refuses a mismatched list, delete actually
// removes and undo actually restores. A stub that always says "ok" would make
// the harness agree with any bug.
type StubArgs = Record<string, unknown>;
const clone = <T,>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

/**
 * **THE STUB HOLDS ITS OWN COPY. `clone(config)` IS LOAD-BEARING.**
 *
 * It used to be `{ config, … }` — the same object the component was
 * initialised with — and that is not a stub of the backend, it is a stub that
 * has been merged into the frontend. Measured in the browser pane 2026-09-04:
 * clicking Duplicate produced **two** rows called "Professionals 2". The stub
 * spliced the copy in, and then `duplicateProfile()` spliced its own copy into
 * what it thought was a separate local config — the same array.
 *
 * The false POSITIVE is the cheap half. The expensive half is the negative it
 * cannot produce: with one shared object, a frontend that forgot to update its
 * local copy at all would still render a correct list, because the stub had
 * already done it. **A stub that shares state with the component under test
 * can only agree with it** — the same class as CLAUDE.md's "a check that
 * cannot produce a negative result is not a check", one layer up.
 *
 * Rust is a separate store on the other side of `invoke`; so is this. Every
 * command mutates only `stubState.config`, `get_config` hands back a clone,
 * and the component keeps its own copy exactly as it does in the app.
 */
const stubState = { config: clone(config), undo: null as AppConfig | null };
/** PHASE A — the stub recorder's poll counter (see `chord_record_poll`). */
let _previewChordPolls = 0;

/**
 * Grapheme clusters, the same approximation `schema::cluster_count` makes.
 *
 * NOT `Intl.Segmenter`, which would do this properly: tsconfig's `lib` is
 * ES2020 and `Segmenter` needs ES2022, and raising the whole project's lib for
 * a dev harness is the wrong trade. Not `e.length === 1` either — that is the
 * exact bug the real validator exists to avoid, and a stub that reproduced the
 * bug would let the harness agree with it.
 */
function stubClusters(s: string): number {
  let clusters = 0, afterZwj = false, regionalOpen = false;
  for (const c of s) {
    const cp = c.codePointAt(0)!;
    const joins =
      (cp >= 0xfe00 && cp <= 0xfe0f) ||        // variation selectors
      (cp >= 0x1f3fb && cp <= 0x1f3ff) ||      // skin tones
      (cp >= 0xe0020 && cp <= 0xe007f) ||      // tag characters
      cp === 0x20e3 ||                          // keycap
      (cp >= 0x0300 && cp <= 0x036f);           // combining marks
    const regional = cp >= 0x1f1e6 && cp <= 0x1f1ff;
    if (cp === 0x200d) { afterZwj = true; regionalOpen = false; continue; }
    if (joins) { afterZwj = false; continue; }
    if (regional && regionalOpen) { regionalOpen = false; afterZwj = false; continue; }
    if (afterZwj) { afterZwj = false; regionalOpen = regional; continue; }
    clusters += 1;
    regionalOpen = regional;
  }
  return clusters;
}

/**
 * The copy-naming rule, mirroring `commands::unique_copy_name`.
 *
 * This used to be `` `${name} 2` ``, and the harness happily made TWO rows
 * called "Professionals 2" on the second click (measured 2026-09-04). Rust
 * never can: it takes the names already in use. Two rows sharing a name is not
 * a cosmetic difference here — `data-profile-name` is how a row is identified,
 * so the duplicate broke reorder's uniqueness check and made delete ambiguous.
 * The harness would have reported bugs the app does not have.
 *
 * Same two rules as Rust: a trailing " N" is a copy index, so copying "Work 2"
 * gives "Work 3" and not "Work 2 2"; and the result is capped at 24 characters
 * by trimming the stem, not the suffix.
 */
function stubCopyName(existing: string[], base: string): string {
  const m = /^(.*\S)\s(\d+)$/.exec(base);
  const stem = m ? m[1] : base;
  for (let n = 2; n < 1000; n += 1) {
    const suffix = ` ${n}`;
    const room = Math.max(0, 24 - suffix.length);
    const candidate =
      Array.from(stem).slice(0, room).join("").trimEnd() + suffix;
    if (!existing.includes(candidate)) return candidate;
  }
  return `${stem} copy`;
}

/** `list_start_menu_apps`'s stub answer — the same DEMO names the keyboard
 *  itself is bound to, so a tile in the editor's app grid actually matches
 *  something Space+<key> would show on the board. */
const PREVIEW_APPS: AppInfo[] = [
  ...Object.entries(DEMO).map(([, name]) => ({
    name,
    path: `C:\\Apps\\${name}.exe`,
    icon_base64: null,
  })),
  // Brave is NOT in DEMO on purpose — DEMO also seeds the keyboard's bindings,
  // and the tour's step 1 offers EMPTY letters. A pre-bound Brave key would
  // put the one tile this harness exists to exercise behind the
  // replace-confirm gate, which is a different flow.
  { name: "Brave", path: "C:\\Apps\\Brave.exe", icon_base64: null },
];

/**
 * `list_browser_profiles`'s stub answer (2026-09-06, PROBLEM 242 follow-up).
 *
 * This used to be `[]`, and an empty list is the one shape that makes the
 * browser-profile picker UNREACHABLE — `key-detail-panel.ts` only turns page 2
 * for a browser with MORE THAN ONE profile (`multi`), so with no browsers at
 * all every bind looked like a plain app bind and the tour's step 2b could not
 * be watched at all. That is the CLAUDE.md failure mode about a stub that can
 * only ever agree with the component under test.
 *
 * Two browsers, deliberately, because the gate has two sides:
 *
 *   Brave  — TWO profiles, so binding it opens "Which Brave profile?" and the
 *            tour must detour into step 2b. The names are the owner's own
 *            (his screenshot: Arpon / ARPON'S STUDIES); the first is signed in
 *            so `account_label` differs from `display_name` and the tile draws
 *            its second, dimmer line.
 *   Chrome — ONE profile, which the app writes but never asks about. Binding
 *            it must go STRAIGHT to step 3, exactly like a non-browser app.
 *
 * `browser_exe` matches PREVIEW_APPS' path byte for byte: `findBrowserByExe`
 * compares full paths, not names, so a near-miss here would silently look like
 * "this exe is not a browser" and prove nothing.
 */
const PREVIEW_BROWSERS = [
  {
    browser_name: "Brave",
    browser_exe: "C:\\Apps\\Brave.exe",
    user_data_dir: "C:\\Users\\beamu\\AppData\\Local\\BraveSoftware\\User Data",
    icon_base64: null,
    profiles: [
      {
        directory: "Default",
        display_name: "Person 1",
        email: "arpon@example.com",
        account_label: "Arpon",
      },
      {
        directory: "Profile 1",
        display_name: "ARPON'S STUDIES",
        email: null,
        account_label: "ARPON'S STUDIES",
      },
    ],
  },
  {
    browser_name: "Chrome",
    browser_exe: "C:\\Apps\\Chrome.exe",
    user_data_dir: "C:\\Users\\beamu\\AppData\\Local\\Google\\Chrome\\User Data",
    icon_base64: null,
    profiles: [
      { directory: "Default", display_name: "Person 1", email: null, account_label: "Person 1" },
    ],
  },
];

const stubBackend: Record<string, (a: StubArgs) => unknown> = {
  get_config: () => clone(stubState.config),
  set_active_profile: (a) => { stubState.config.active_profile = a.name as string; },
  rename_profile: (a) => {
    const p = stubState.config.profiles.find((x) => x.name === a.oldName);
    if (!p) throw "Profile not found";
    if (stubState.config.profiles.some((x) => x.name === a.newName)) throw "Already exists";
    p.name = a.newName as string;
  },
  create_profile: () => {},
  delete_profile: (a) => {
    stubState.undo = clone(stubState.config);
    stubState.config.profiles = stubState.config.profiles.filter((p) => p.name !== a.name);
  },
  duplicate_profile: (a) => {
    const i = stubState.config.profiles.findIndex((p) => p.name === a.name);
    if (i < 0) throw "Profile not found";
    const copy = clone(stubState.config.profiles[i]);
    copy.name = stubCopyName(
      stubState.config.profiles.map((p) => p.name),
      a.name as string,
    );
    stubState.config.profiles.splice(i + 1, 0, copy);
    return copy.name;
  },
  // The validation the real command performs, reproduced here because it is
  // the behaviour worth LOOKING at: drag a row, and a stale list must visibly
  // snap back rather than quietly dropping a profile.
  reorder_profiles: (a) => {
    const names = a.names as string[];
    const have = stubState.config.profiles.map((p) => p.name);
    const ok =
      names.length === have.length &&
      new Set(names).size === names.length &&
      names.every((n) => have.includes(n));
    if (!ok) throw "The profile order does not match. Nothing was reordered.";
    const byName = new Map(stubState.config.profiles.map((p) => [p.name, p]));
    stubState.config.profiles = names.map((n) => byName.get(n)!);
  },
  set_profile_emoji: (a) => {
    const p = stubState.config.profiles.find((x) => x.name === a.name);
    if (!p) throw "Profile not found";
    const e = (a.emoji as string | null) ?? null;
    if (e && stubClusters(e) !== 1) throw "That is not a single emoji.";
    p.emoji = e;
  },
  open_emoji_panel: () => {
    // No Win+. in a browser. Say so where it will be seen, rather than
    // pretending the panel opened.
    console.info("preview: open_emoji_panel — type an emoji into the box by hand");
  },
  export_profile: () => null,       // the user "cancelled" — no file dialogs here
  import_profile: () => null,
  import_profile_commit: () => null,
  undo_last_change: () => {
    if (!stubState.undo) throw "Nothing left to undo";
    stubState.config = stubState.undo;
    stubState.undo = null;
    return "Deleted the profile";
  },
  undo_available: () => null,
  overlay_fit: () => null,
  /**
   * THE PROJECTION, counted rather than drawn.
   *
   * There is no overlay window in the harness, so the only thing that CAN be
   * checked here is the one thing the 2026-09-04 bug was about: how many times
   * the command is sent, and when. `window.__previewHudCalls` is the record —
   * read it from the browser pane after clicking the pill and the switch.
   */
  preview_hud_layout: (a) => {
    const w = window as unknown as { __previewHudCalls: string[] };
    (w.__previewHudCalls ||= []).push(String(a.layout));
    console.info(`preview: preview_hud_layout(${String(a.layout)}) — call #${w.__previewHudCalls.length}`);
    return null;
  },

  // ---- key editor (2026-09-04) — everything initKeyDetailPanel can invoke,
  // so the replace-confirm flow (?editor, see the file doc comment) has a
  // real backend to talk to rather than degrading through every fallback at
  // once. Each answer is the "nothing found yet" shape the real command
  // would give on a slow machine, which is exactly the state this harness
  // wants to be checked against — see the CLAUDE.md rule on stubs that can
  // only ever agree with the component under test.
  list_start_menu_apps: () => PREVIEW_APPS,
  list_browser_profiles: () => PREVIEW_BROWSERS,
  get_default_browser: () => null,
  show_conflict_check: () => ({ has_conflict: false, conflicting_combo: null, description: null }),
  check_app_path: () => null,
  extract_icon_cmd: () => null,
  pick_file: () => null,
  frontend_log: (a) => { console.info("preview: frontend_log —", a.msg); return null; },
  // PHASE A — the chord recorder and "Try it". No hook here: the recorder
  // answers a fixed Win+Shift+S after the first poll so the caps can be
  // seen; "Try it" only reports what the key would say.
  chord_record_start: () => { _previewChordPolls = 0; return null; },
  chord_record_poll: () => (++_previewChordPolls > 3 ? [0x5b, 0x10, 0x53] : []),
  chord_record_stop: () => null,
  run_command_once: (a) => (a.elevated ? `▶ Asked Windows to run as administrator: ${String(a.line)}` : `▶ Ran: ${String(a.line)}`),

  // ---- REVIEW FIXES 2026-09-05 (H4) — the OS light/dark seed ----
  //
  // The value `theme-resolve.ts` resolves the theme `"auto"` against. In the
  // real app it is Rust reading HKCU; here it is one mutable flag, so the
  // harness can be BOTH machines without touching the registry (which it
  // could not do honestly anyway — the agent shell's HKCU is virtualised,
  // CLAUDE.md PROBLEM 143).
  //
  // Seeded from `?osdark`, then flipped at will through
  // `window.__previewSetOsDark(true|false)` below.
  get_os_prefers_dark: () => _previewOsDark,

  // ---- REVIEW FIXES 2026-09-05 (H6) — who owns "Run at startup" ----
  //
  // The four-value tuple, byte-for-byte the shape `commands.rs` returns.
  // `?portable` selects its portable branch; without the flag this is the
  // ordinary unpackaged answer every NSIS and MSI install gets.
  get_packaged_startup: () => (previewStartup
    ? [previewStartup.packaged, previewStartup.state, previewStartup.mayChange, previewStartup.note]
    : [false, "unavailable", true, ""]),
};

/** The harness's stand-in for `HKCU\…\Personalize\AppsUseLightTheme`. */
let _previewOsDark = new URLSearchParams(location.search).has("osdark");

// ---- event listen/emit stub (2026-09-04, PROBLEM 237 wiring) ----
//
// `listen()` from `@tauri-apps/api/event` calls `transformCallback` (stores
// the handler under an id, on `window.__TAURI_INTERNALS__`) and then
// `invoke("plugin:event|listen", {event, target, handler: id})`. Neither
// existed in this harness before app-grid.ts's picker-refresh listener
// (`initPickerRefreshListener`, wired from key-detail-panel.ts's
// `warmPickerData`) needed one — every `listen()` call reached from
// preview.ts's dependency tree would have thrown on
// "transformCallback is not a function". This is a REAL in-memory event bus,
// not a no-op: `window.__previewEmit(event, payload)` fires every handler
// registered for that event name, the same way Rust's global `emit` reaches
// every `listen()` call in a real webview — so a test can dispatch
// `picker-data-updated` and observe `loadApps()` re-run and an open grid
// re-render, exactly as PROBLEM 237's frontend contract describes.
let _cbId = 1;
const _callbacks = new Map<number, (payload: unknown) => void>();
const _eventListeners = new Map<string, Set<number>>();
stubBackend["plugin:event|listen"] = (a) => {
  const event = a.event as string;
  const id = a.handler as number;
  if (!_eventListeners.has(event)) _eventListeners.set(event, new Set());
  _eventListeners.get(event)!.add(id);
  return id;
};
stubBackend["plugin:event|unlisten"] = (a) => {
  _eventListeners.get(a.event as string)?.delete(a.eventId as number);
  return null;
};

interface TauriStub {
  invoke(cmd: string, args?: StubArgs): Promise<unknown>;
  transformCallback(callback: (payload: unknown) => void, once?: boolean): number;
}
(window as unknown as { __TAURI_INTERNALS__: TauriStub }).__TAURI_INTERNALS__ = {
  invoke(cmd: string, args: StubArgs = {}) {
    const fn = stubBackend[cmd];
    if (!fn) {
      console.warn(`preview: no stub for invoke("${cmd}")`);
      return Promise.resolve(null);
    }
    try {
      return Promise.resolve(fn(args));
    } catch (e) {
      return Promise.reject(e);
    }
  },
  transformCallback(callback: (payload: unknown) => void) {
    const id = _cbId++;
    _callbacks.set(id, callback);
    return id;
  },
};

/** Test hook: fire every registered `listen(event, …)` handler with `payload`. */
(window as unknown as { __previewEmit: (event: string, payload: unknown) => void }).__previewEmit =
  (event, payload) => {
    for (const id of _eventListeners.get(event) ?? []) {
      _callbacks.get(id)?.({ event, id, payload });
    }
  };

// ---------------------------------------------------------------------------
// REVIEW FIXES 2026-09-05 (H4) — THE SPLIT-PALETTE PROBE
// ---------------------------------------------------------------------------
//
// WHAT WENT WRONG, so the check has something to be a check OF. The theme
// `"auto"` was resolved by ONE shared rule (`theme-resolve.ts`) that both
// bundles import — and the two bundles still rendered different palettes,
// because they fed the rule different inputs. `tauri.conf.json` pinned the
// dashboard window to `"theme": "Light"`, WebView2 turned that into
// `SetPreferredColorScheme(Light)`, and `prefers-color-scheme` answers with
// what the webview was TOLD to prefer. Dashboard: light. Overlay: dark. Same
// machine, same rule, same instant.
//
// So the input is single-sourced now — Rust reads the registry once and emits
// `os-theme-changed` — and this probe is what demonstrates it end to end
// WITHOUT a backend:
//
//   1. `initOsTheme()` runs the real wiring (`os-theme.ts`) against the stub
//      above and this file's real in-memory event bus — the same bus a real
//      `listen()` reaches.
//   2. Flipping the value goes through `__previewEmit("os-theme-changed", …)`,
//      i.e. the exact path Rust's global `emit` takes.
//   3. Both consumers are then asked what palette they are wearing:
//      - the DASHBOARD's, `resolveTheme(raw)` — what `applyLook()` writes to
//        `body.dataset.theme`;
//      - the OVERLAY's, `applyThemeName(raw)` from the real `toast.ts`, read
//        back off `document.body` — the function whose private
//        "anything not warcry/starry is earthy" rule was PROBLEM 255's bug.
//
// `toast.ts` is pulled in with a DYNAMIC import so the harness's ordinary boot
// never loads the overlay's 4,900-line module. A probe must not be able to
// break the page it is a probe on.
//
// Run it from the browser console:  await window.__previewThemeProbe()
async function themeProbe(): Promise<boolean> {
  const { initOsTheme } = await import("./os-theme");
  const { resolveTheme } = await import("./theme-resolve");
  const { applyThemeName } = await import("./components/toast");
  const emit = (window as unknown as {
    __previewEmit: (e: string, p: unknown) => void;
  }).__previewEmit;

  await initOsTheme();

  // Remembered and restored: this probe writes to the real <body> the harness
  // is drawing the dashboard on, and leaving it in the probe's last state
  // would look exactly like a rendering bug.
  const beforeTheme = document.body.dataset.theme;
  const beforeNocturne = document.body.classList.contains("nocturne");

  const rows: { osDark: boolean; dashboard: string; overlay: string }[] = [];
  let agreed = true;
  for (const osDark of [false, true, false]) {
    _previewOsDark = osDark;
    emit("os-theme-changed", { dark: osDark });
    const dashboard = resolveTheme("auto");
    applyThemeName("auto");
    const overlay = document.body.dataset.theme ?? "";
    rows.push({ osDark, dashboard, overlay });
    if (dashboard !== overlay) agreed = false;
    // The nocturne base has to move with it, or the starry palette lands on a
    // daylight background — the overlay half of PROBLEM 255.
    if ((overlay !== "earthy") !== document.body.classList.contains("nocturne")) agreed = false;
  }

  // A fixed theme must be deaf to the OS in BOTH consumers, or "Earthy" would
  // stop meaning Earthy the moment somebody turned Windows dark.
  _previewOsDark = true;
  emit("os-theme-changed", { dark: true });
  applyThemeName("earthy");
  const fixedOverlay = document.body.dataset.theme;
  const fixedDashboard = resolveTheme("earthy");
  if (fixedOverlay !== "earthy" || fixedDashboard !== "earthy") agreed = false;

  if (beforeTheme === undefined) delete document.body.dataset.theme;
  else document.body.dataset.theme = beforeTheme;
  document.body.classList.toggle("nocturne", beforeNocturne);

  console.table(rows);
  console.info(
    agreed
      ? "preview: H4 OK — the dashboard rule and the overlay's applyThemeName resolved \"auto\" "
        + "identically at every OS value, the nocturne base tracked it, and a fixed \"earthy\" "
        + "ignored the OS in both."
      : "preview: H4 FAILED — the two consumers disagree. See the table above.",
  );
  return agreed;
}
(window as unknown as { __previewThemeProbe: () => Promise<boolean> }).__previewThemeProbe =
  themeProbe;

// Toasts need a container or `showToast` returns silently — and the toasts are
// half the feedback the editor gives (order saved, duplicated, emoji set).
if (!document.getElementById("toast-container")) {
  const c = document.createElement("div");
  c.id = "toast-container";
  document.body.appendChild(c);
}

initProfileEditor(config, (name) => console.info(`preview: switched to ${name}`));

// ---- settings panel ----
//
// The switches come from the component itself (toggleSwitchHtml), so this
// harness cannot drift from the app — it went on showing a "Dark mode" switch
// for three versions after the theme pill replaced it, which is exactly the
// failure a preview is supposed to catch.
//
// ?fun turns the personality layer on, matching applyLook()'s gate, so the §2
// characters can be watched without the backend. Flip a switch and its own
// character plays; the others stay still.
//
// 2026-09-01 — REBUILT to the owner's four-group panel. The harness renders
// the same header, the same sticky Engine row, the same search box, the same
// group headings and the same 3-way ring pill as the app, all through the
// shared leaf helpers (`groupHeadingHtml`, `segRowHtml`, `RING_OPTS`,
// `ringLayoutFor`, `filterSettings`, `setPanelExpanded`, `paintInert`). Every
// one of those is the function the app calls, not a copy of it — this file
// went on showing a "Dark mode" switch for three versions after the theme pill
// replaced it, and a harness that imitates the panel instead of using its
// parts is how that happens.
type Row = [string, string, boolean];
const APPEARANCE: Row[] = [
  ["fun",       "Fun mode",          q.has("fun")],
  ["sound",     "Sound ticks",       false],
  ["motion",    "Visual effects",    true],
  ["hideboard", "Hide the keyboard", false],
];
const BEHAVIOUR: Row[] = [
  ["startup", "Run at startup", true],
];
// The Space ring's three switches are emitted BY HAND below rather than from
// a list: the ring pill has to sit between "Point to launch" and "Show special
// keys", because Double is the state that greys the specials switch and a
// reason has to be visible from the control it explains. The app's own
// `render()` writes them out in the same order and for the same reason.
const PRIVACY: Row[] = [
  ["sendlogs", "Don't send logs", false],
];

/** The ring shape the flags asked for, read through the app's own mapping. */
const previewRing = ringLayoutFor(config.hud_magnetic_layout, config.hud_band_count);
/** ?double — the state where "Show special keys" has nothing to do. */
const previewInert = previewRing === "double";

// ---------------------------------------------------------------------------
// REVIEW FIXES 2026-09-05 (H6) — ?portable
// ---------------------------------------------------------------------------
//
// "Run at startup" has a second state in which it can do nothing, and until
// this flag the harness could not draw it. `?double` exists so the SPECIALS
// row's inert treatment gets looked at; this is the same argument for the
// startup row, and the bug it is a check on is worse — the specials row was
// merely greyed on a wrong condition, whereas this one WROTE and then toasted
// a success for a write that never happened.
//
// **The tuple is copied from `commands.rs::get_packaged_startup`'s portable
// branch, `packaged: true` and all.** Not corrected, not tidied. A stub that
// disagrees with the backend can only ever agree with the component under
// test (CLAUDE.md's rule on stubs), and `packaged: true` for a copy that is
// not packaged is precisely the misleading input the fix has to survive — a
// stub that quietly said `false` here would pass whatever the frontend did.
const previewStartup: StartupOwnership = q.has("portable")
  ? {
      packaged: true,
      state: PORTABLE_STARTUP_STATE,
      mayChange: false,
      note:
        "This is the portable copy, so it never writes to the registry or Task "
        + "Scheduler. To start it with Windows, put a shortcut to spaceadom.exe in "
        + "your Startup folder (Win+R, shell:startup).",
    }
  : null;
document.body.dataset.fun = q.has("fun") ? "on" : "off";

/**
 * PROBLEM 267 — one App-exceptions tile (artboard 8), the SAME markup
 * `renderAppExceptions` builds by DOM: icon disc, name, the Default tag on a
 * built-in row, the three-state `.theme-seg`. Static here; the harness only
 * has to LOOK right, and the measured indicator is positioned by the same
 * `wireSegIndicators` pass the pills get.
 */
function excTileHtml(name: string, scope: string, builtin: boolean): string {
  const idx = Math.max(0, EXC_SCOPE_OPTS.findIndex(([v]) => v === scope));
  return `
    <div class="exc-row" data-stem="${name.toLowerCase().replace(/\s+/g, "")}">
      <div class="exc-row-head">
        <span class="exc-tile-disc" style="background:#e7d8b8;"></span>
        <span class="exc-row-name">${name}</span>
        ${builtin ? `<span class="exc-default-tag">Default</span>` : `<button type="button" class="exc-tile-x exc-row-x" aria-label="Remove ${name} from exceptions">\u2715</button>`}
      </div>
      <div class="theme-seg exc-seg" role="radiogroup" aria-label="${name}: what stands down">
        <span class="theme-seg-ind" data-seg="${scope}" style="background:var(--st-accent);"></span>
        ${EXC_SCOPE_OPTS.map(([v, l], n) => `<button type="button" class="theme-seg-opt${n === idx ? " is-on" : ""}" data-exc-scope="${v}" role="radio" aria-checked="${n === idx}">${l}</button>`).join("")}
      </div>
    </div>`;
}

/**
 * PROBLEM 267 — the "Choose your favourites" picker (artboard 9), from the DEMO
 * bindings: six selected of eight, two link rows with the placeholder circle
 * (`rd`, `gh`), and the disclosure line. The same class names
 * `renderEightPicker` uses, so the stylesheet is exercised end to end.
 */
function eightPickerHtml(): string {
  const rows: Array<[string, string, boolean, boolean]> = [
    ["m", "Mail", true, false], ["b", "Browser", true, false], ["r", "reddit.com", false, true],
    ["c", "Chat", true, false], ["g", "github.com", false, true], ["n", "Notes", true, false],
    ["t", "Terminal", true, false], ["p", "Player", true, false], ["k", "Calendar", false, false],
  ];
  const n = rows.filter((r) => r[2]).length;
  return `
    <div class="eight-picker">
      <div class="eight-head"><span class="eight-title">Choose your favourites</span><span class="eight-count">${n} of 15 selected</span><button type="button" class="btn btn-sm eight-done">Done</button></div>
      <div class="eight-list">
        ${rows.map(([k, name, on, link]) => `
          <label class="eight-row${on ? " is-on" : ""}">
            <span class="eight-chip">${k.toUpperCase()}</span>
            <span class="eight-icon${link ? " is-link" : ""}">${link ? name.slice(0, 2) : ""}</span>
            <span class="eight-name">${name}${link ? `<span class="eight-tag"> \u00b7 link</span>` : ""}</span>
            <input type="checkbox" class="eight-check" ${on ? "checked" : ""} aria-label="${name} in the ring" />
          </label>`).join("")}
      </div>
      <div class="eight-foot">${EIGHT_PICKER_NOTE}</div>
    </div>`;
}

const switchRow = (id: string, label: string, on: boolean, i: number): string => {
  // REVIEW FIXES 2026-09-05 (H6) — the startup row's inert state comes from
  // the SAME `startupRowIsInert` the panel calls, fed the same tuple shape the
  // backend returns. Nothing here re-implements the rule.
  const inert = (id === "hudspecials" && previewInert)
    || (id === "startup" && startupRowIsInert(previewStartup));
  // ACCESSIBILITY PASS (feature 3) — the real panel's `toggleRow` passes the
  // row label into `toggleSwitchHtml` as `aria-label` (the visible text lives
  // on a SEPARATE button, so the checkbox itself has no accessible name
  // without it); mirrored here so the harness's accessibility tree matches
  // what settings-panel.ts actually ships instead of showing an unlabelled
  // checkbox that the real panel never has.
  const sw = toggleSwitchHtml(id, on, undefined, label);
  // The pill is rendered LIVE, with the same wrapper and the same hidden note
  // the panel emits; `paintInert` — the SAME function the panel calls, from
  // the same leaf module — greys it below. The harness must not own a second
  // copy of the treatment: the copy is what drifts, and what it drops is
  // `disabled`.
  //
  // ONLY the specials row carries the note. It used to be emitted for every
  // switch with `display:none`, which looked harmless and was not: the search
  // matcher reads a row's whole `textContent`, so a hidden copy of the note on
  // twelve rows would have made "compact" and "wide" match all of them.
  const note = id === "hudspecials"
    ? `<div class="set-note" id="set-hudspecials-note" style="margin-top:6px; display:none;">${SPECIALS_INERT_NOTE}</div>`
    // The startup note is emitted ONLY under ?portable, for the same
    // search-matcher reason the specials note gives: `filterSettings` reads a
    // row's whole `textContent`, so a hidden note on a row that has nothing to
    // explain would make its words match a search they have no business
    // matching. `startupRow` in the panel emits it always because it has an
    // async state to flip into; the harness's answer is decided at load.
    : (id === "startup" && previewStartup)
      ? `<div class="set-note" id="set-startup-note" style="margin-top:6px; display:none;">${previewStartup.note}</div>`
      : "";
  // The switch is drawn from what would be SHOWN, not from the caller's `on`:
  // a portable copy's switch is OFF whatever its config says, and that is
  // `startupShownAsOn`'s job in the panel too.
  const shown = id === "startup" ? startupShownAsOn(previewStartup, on) : on;
  const swHtml = id === "startup" ? toggleSwitchHtml(id, shown, undefined, label) : sw;
  return `
    <div class="set-item set-filterable" style="animation-delay:${60 + i * 45}ms">
      <div class="set-row" aria-disabled="${inert}">
        <button type="button" class="set-row-label">${label}</button>
        <span id="set-${id}-wrap">${swHtml}</span>
      </div>
      ${note}
    </div>`;
};

const sliderRow = (id: string, label: string, lo: number, hi: number, v: number): string => `
  <div class="set-item set-filterable">
    <div class="set-row" style="flex-direction:column; align-items:stretch; gap:4px; cursor:default; margin-bottom:10px;">
      <div style="display:flex; align-items:baseline; gap:8px;">
        <span class="set-row-label">${label}</span>
        <span style="font-size:11px; font-weight:700; color:var(--st-accent-deep);">${v}</span>
      </div>
      ${sliderShell(id, `<input type="range" id="set-${id}" min="${lo}" max="${hi}" value="${v}" aria-label="${label}" />`, lo, hi, v)}
    </div>
  </div>`;

document.getElementById("settings-panel")!.innerHTML = `
  <div class="set-head">
    <div class="set-title">Settings</div>
    <button type="button" class="set-head-link" id="set-help-descs" aria-pressed="false">What do these do?</button>
    <button type="button" class="set-head-link" id="set-help-tour" title="Replay the first-run walkthrough">Show me the walkthrough</button>
    <button type="button" class="set-expand" id="set-expand" aria-pressed="false" aria-label="Expand settings">⤢</button>
  </div>

  <div class="set-item set-filterable set-engine">
    <div class="set-row">
      <button type="button" class="set-row-label">Engine active</button>
      ${toggleSwitchHtml("engine", true)}
    </div>
    <div class="set-engine-note">Off means Space is just a space again.</div>
  </div>

  <div class="set-scroll">
  <!-- Mirrors settings-panel.ts: the multicol wrapper of the expanded
       layout (display:contents in the popover). No backticks here. -->
  <div class="set-cols">

  <input class="input set-search" id="set-search" type="text"
         placeholder="Search settings…" autocomplete="off" spellcheck="false"
         aria-label="Search settings" />
  <!-- ACCESSIBILITY PASS (feature 3) — mirrors settings-panel.ts's own
       search-empty markup exactly (role=status + aria-live=polite), since
       this harness hand-rolls the panel shell rather than calling render(). -->
  <div class="set-note" id="set-search-empty" role="status" aria-live="polite" hidden>Nothing here matches that.</div>

  <div id="set-groups">
    <div class="set-group">
      ${groupHeadingHtml("appearance", "Appearance")}
      <div class="set-rows">
        <div class="set-item set-filterable">
          <div class="set-row set-row-stack">
            <button type="button" class="set-row-label">Theme</button>
            <!-- FEATURE 2 — "Auto" joins as the first segment, mirroring
                 THEME_OPTS in settings-panel.ts exactly (a second literal list
                 here is the drift PROBLEM 148 exists to prevent, but the pill
                 markup itself already comes from the shared segRowHtml — only
                 the OPTIONS array would drift, so it is written out the same
                 way in both places). ?theme=<value> picks any of the four
                 directly; ?dark is kept as a shorthand for ?theme=starry. -->
            ${segRowHtml("theme",
              [["auto", "Auto"], ["earthy", "Earthy"], ["warcry", "Warcry"], ["starry", "Starry night"]],
              q.get("theme") ?? (q.has("dark") ? "starry" : "earthy"), "", "Theme")}
          </div>
        </div>
        ${APPEARANCE.map(([id, label, on], i) => switchRow(id, label, on, i)).join("")}
      </div>
    </div>

    <div class="set-group">
      ${groupHeadingHtml("behaviour", "Behaviour")}
      <div class="set-rows">
        ${BEHAVIOUR.map(([id, label, on], i) => switchRow(id, label, on, i)).join("")}
        ${sliderRow("wpm", "Typing speed", 30, 150, 65)}
        ${sliderRow("opacity", "Opacity floor", 10, 90, 30)}
      </div>
    </div>

    <div class="set-group">
      ${groupHeadingHtml("ring", "The Space ring")}
      <div class="set-rows">
        ${switchRow("hudpointer", "Point to launch", true, 0)}
        <!-- PROBLEM 263 — mirrored from settings-panel.ts: the middle-button
             row sits directly under "Point to launch" (the two mouse rows
             read as a pair) and ON by default, because Rust ships the field
             default_true. NOT in RING_AFFECTING below: the switch changes how
             the ring is OPENED, not what it looks like, so the panel's own
             handler deliberately fires no preview and neither does this. -->
        ${switchRow("middlering", "Middle button opens the ring", true, 1)}
        <!-- PROBLEM 267 — mirrored from settings-panel.ts, directly under the
             middle-button switch: "Middle button shows" (Icon ring / Space
             ring, the owner's 2026-09-13 addition) and "Middle-button ring
             shows" (Favourites / All + "Choose your favourites") with the
             picker artboard under it when ?eight is set. ?middleoff paints
             both inert with the same paintInert the panel uses. -->
        <div class="set-item set-filterable">
          <div class="set-row set-row-stack">
            <button type="button" class="set-row-label">Middle button shows</button>
            <span id="set-middlestyle-wrap">${segRowHtml("middlestyle", MIDDLE_STYLE_OPTS,
              q.get("middlestyle") === "guide_hud" ? "guide_hud" : "icon_ring",
              "background:var(--st-accent);", "Middle button shows")}</span>
          </div>
          <div class="set-sub">Sized by the golden ratio.</div>
          <div class="set-note" id="set-middlestyle-note" style="margin-top:6px;display:none;">Turn on \u201cMiddle button opens the ring\u201d to use this.</div>
        </div>
        <div class="set-item set-filterable">
          <div class="set-row set-row-stack">
            <button type="button" class="set-row-label">Middle-button ring shows</button>
            <span id="set-middlescope-wrap" class="mscope-line">${segRowHtml("middlescope", MIDDLE_SCOPE_OPTS,
              q.get("ring") === "all" ? "all" : "my_eight",
              "background:var(--st-accent);", "Middle-button ring shows")}<button type="button" class="mscope-choose" id="set-eight-open" aria-expanded="${q.has("eight")}">Choose your favourites \u2192</button></span>
          </div>
          <div class="set-sub">A Fibonacci cap, for density.</div>
          <div class="set-note" id="set-middlescope-note" style="margin-top:6px;display:none;">Turn on \u201cMiddle button opens the ring\u201d to use this.</div>
          <div id="set-eight-picker">${q.has("eight") ? eightPickerHtml() : ""}</div>
        </div>
        <!-- 2026-09-15 — mirrored from settings-panel.ts: "All layout"
             (Rings / Spiral) sits directly under the scope pill and is inert
             unless the scope is All, the same way "Choose your favourites"
             only means something under Favourites. ?ring=all|spiral picks it. -->
        <div class="set-item set-filterable">
          <div class="set-row set-row-stack">
            <button type="button" class="set-row-label">All layout</button>
            <span id="set-alllayout-wrap">${segRowHtml("alllayout", ALL_LAYOUT_OPTS,
              q.get("ring") === "spiral" ? "spiral" : "rings",
              "background:var(--st-accent);", "All layout")}</span>
          </div>
          <div class="set-sub">Packed like a sunflower’s seeds.</div>
          <div class="set-note" id="set-alllayout-note" style="margin-top:6px;display:none;">Only for “All” — Favourites arranges itself around the screen edge.</div>
        </div>
        <div class="set-item set-filterable">
          <div class="set-row set-row-stack">
            <button type="button" class="set-row-label">Ring layout</button>
            ${segRowHtml("hudring", RING_OPTS, previewRing, "background:var(--st-accent);", "Ring layout")}
          </div>
        </div>
        ${switchRow("hudspecials", "Show special keys", true, 4)}
        ${switchRow("flight", "Guide-to-toast motion", false, 5)}
        ${sliderRow("huddelay", "Guide HUD delay", 100, 1000, 300)}
      </div>
    </div>
  </div>

  <!-- PROBLEM 239 follow-up — App exceptions had no mirror here at all before
       this pass (the section simply did not exist in the harness), which is
       exactly why the "Add an app" pill regression wasn't caught earlier:
       nothing rendered it for a screenshot to be taken of. Empty state only
       (no .exc-grid tiles) — settings-panel.ts's own renderAppExceptions()
       is what to check for the populated tile grid; this mirror exists to
       measure the ONE thing PROBLEM 239's follow-up changed here.
       NOTE: no backticks in these comments — this whole block is one JS
       template literal (see the assignment above), and a literal backtick
       here closes it early. -->
  <div class="set-section set-filterable set-section-span">
    <div class="divider" style="margin:14px 0 10px;"></div>
    <div class="set-title" style="font-size:13px; margin-bottom:8px;">App exceptions</div>
    <!-- Owner's 1.0.110 review — the popover's one-line summary + "Show";
         the full section (.set-full) only shows expanded. Same markup shape
         as settings-panel.ts's render(); the counts there are live. -->
    <div class="set-summary" id="set-exc-summary">
      <span class="set-summary-text" id="set-exc-summary-text">3 built-in${q.has("excuser") ? " · 1 yours" : ""} · 87 more</span>
      <button type="button" class="btn btn-sm set-summary-show" data-show-section="set-app-exceptions">Show</button>
    </div>
    <div id="set-app-exceptions" class="set-full">
    <!-- PROBLEM 267 — the three built-in tiles artboard 8 shows, pre-seeded
         at "Space only" with the Default tag and the three-state control, in
         the same .exc-row / .theme-seg markup renderAppExceptions() builds. -->
    <div class="set-note" style="margin-top:0;">Built-in defaults for apps that use the middle button to orbit. Change anytime.</div>
    <div class="exc-list">
      ${excTileHtml("SolidWorks", "space_only", true)}
      ${excTileHtml("Fusion 360", "space_only", true)}
      ${excTileHtml("Blender", "space_only", true)}
      ${q.has("excuser") ? excTileHtml("Photoshop", "off_entirely", false) : ""}
    </div>
    <div class="set-note">\u2026and 87 more 3D, CAD and design programs are built in at Space only. Add one below to change it.</div>
    <button type="button" class="btn exc-add-btn" id="exc-add-btn">Add an app</button>
    </div><!-- /.set-full -->
  </div>

  <!-- Two stub conflicts (spacedesk, PowerToys) — the exact markup shape
       renderConflicts()'s draw() produces per row, so this harness can be
       measured for the same regression: the .conflict-grid wrapper, card
       radius, row padding, the 28px disc, the exe-name chip (with its
       title attribute), the clamped description, and the "Close it" button,
       without a live conflict on the machine running the harness.
       NOTE: no backticks in these comments — see the note above this block. -->
  <div class="set-section set-filterable set-section-span">
    <div class="divider" style="margin:14px 0 10px;"></div>
    <div class="set-title" style="font-size:13px; margin-bottom:8px;">Conflicts</div>
    <div class="set-summary" id="set-conflicts-summary">
      <span class="set-summary-text" id="set-conflicts-summary-text">2 found</span>
      <button type="button" class="btn btn-sm set-summary-show" data-show-section="set-conflicts">Show</button>
    </div>
    <div id="set-conflicts" class="set-full">
    <div class="conflict-grid">
      <div class="conflict-row">
        <span class="conflict-row-disc" style="background:#c67139;">S</span>
        <span class="conflict-row-name">spacedesk</span>
        <span class="conflict-row-proc" title="spacedeskservice.exe">spacedeskservice.exe</span>
        <span class="conflict-row-why" title="spacedesk forwards input to a second display and can intercept keys.">spacedesk forwards input to a second display and can intercept keys.</span>
        <button type="button" class="conflict-row-close" aria-label="Close spacedesk">Close it</button>
      </div>
      <div class="conflict-row">
        <span class="conflict-row-disc" style="background:#b08a3e;">P</span>
        <span class="conflict-row-name">PowerToys</span>
        <span class="conflict-row-proc" title="powertoys.exe">powertoys.exe</span>
        <span class="conflict-row-why" title="PowerToys' Keyboard Manager can remap keys system-wide, the same layer this app uses.">PowerToys' Keyboard Manager can remap keys system-wide, the same layer this app uses.</span>
        <button type="button" class="conflict-row-close" aria-label="Close PowerToys">Close it</button>
      </div>
    </div>
    <div class="set-note sma-note">Press one to have Spaceadom close it for you.</div>
    <div class="ring-fix">
      <div class="set-row">
        <button type="button" class="set-row-label">The ring isn't showing?</button>
        <button type="button" class="btn btn-sm">Check the ring</button>
      </div>
    </div>
    </div><!-- /.set-full -->
  </div>

  <div class="set-section set-filterable">
    <div class="divider" style="margin:14px 0 10px;"></div>
    <div class="set-act-group">
      ${groupHeadingHtml("maintenance", "Maintenance")}
      <div class="set-act-grid">
        <button class="btn" id="set-recheck">Re-check now</button>
        <button class="btn" id="set-logs">Open log folder</button>
      </div>
    </div>
    <div class="set-act-group is-danger">
      ${groupHeadingHtml("danger", "Danger zone")}
      <div class="set-act-grid">
        <button class="btn" id="set-reset">Reset this profile</button>
        <button class="btn" id="set-clear">Clear all</button>
        <button class="btn" id="set-presets">Restore preset profiles</button>
      </div>
    </div>
  </div>

  <div class="set-group">
    <div class="divider" style="margin:14px 0 10px;"></div>
    ${groupHeadingHtml("privacy", "Privacy")}
    <div class="set-rows">
      ${PRIVACY.map(([id, label, on], i) => switchRow(id, label, on, i)).join("")}
    </div>
  </div>

  <!-- ABOUT (feature 1) — same leaf function the real panel renders, off the
       real third-party list, so this harness catches the same drift every
       other rebuilt row here is meant to. fetchAboutInfo/requestUpdateCheck
       both degrade gracefully with no Tauri runtime behind this page (see
       their doc comments in controls.ts), so the row simply shows the bare
       app name until the wiring below resolves. -->
  <div class="set-group">
    <div class="divider" style="margin:14px 0 10px;"></div>
    ${groupHeadingHtml("about", "About")}
    <div class="set-rows">
      <div class="set-item set-filterable" id="set-about">
        ${/* PROBLEM 249 — ?rollback puts the "Roll back to 1.0.99" button on
              screen. It exists for the same reason ?double does (see the
              header): the button appears ONLY when Rust's
              `rollback_available()` answers with a version, which needs two
              consecutive real auto-updates to have happened, so without a
              flag it is a control nobody can look at until the day it
              matters. `aboutRowHtml` is the same leaf function the real panel
              calls, so what is on screen here is what ships. */ ""}
        ${aboutRowHtml(null, THIRD_PARTY.length, q.has("about-open"), "",
                       q.has("rollback") ? "1.0.99" : null)}
      </div>
    </div>
  </div>

  </div><!-- /.set-cols -->
  </div><!-- /.set-scroll -->`;

// THE DEPENDENCY, LIVE IN THE HARNESS. Same call the panel makes once per
// render(), same function, same leaf module — so what is measured here is the
// shipping treatment and not an imitation of it. Pressing Double on the ring
// pill greys the specials switch exactly as it does in the app, which is the
// only way to LOOK at an inert control before shipping it.
//
// Nothing here writes `config.hud_band_count`: the pill keeps whichever shape
// was selected, so returning from Wide restores it. That is the whole claim
// the panel makes, and it is checkable right here.
const specialsWrap = () => document.getElementById("set-hudspecials-wrap");
const specialsNote = () => document.getElementById("set-hudspecials-note");
paintInert(specialsWrap(), specialsNote(), previewInert);

// PROBLEM 267 — ?middleoff: both middle-button rows dead, through the same
// call. Without the flag this is the no-op that restores two live rows.
paintInert(
  document.getElementById("set-middlestyle-wrap"),
  document.getElementById("set-middlestyle-note"),
  q.has("middleoff"),
);
paintInert(
  document.getElementById("set-middlescope-wrap"),
  document.getElementById("set-middlescope-note"),
  q.has("middleoff") || q.get("middlestyle") === "guide_hud",
);

// REVIEW FIXES 2026-09-05 (H6) — the SECOND row that can be dead, painted by
// the same call. `?portable` greys "Run at startup" and shows the sentence
// under it; without the flag this is `paintInert(…, false)`, which is the
// no-op that restores an ordinary live row.
paintInert(
  document.getElementById("set-startup-wrap"),
  document.getElementById("set-startup-note"),
  startupRowIsInert(previewStartup),
);

// ---------------------------------------------------------------------------
// REVIEW FIXES 2026-09-05 (H6) — THE STARTUP-OWNERSHIP PROBE
// ---------------------------------------------------------------------------
//
// What went wrong: `settings-panel.ts` decided "is this row dead" with
// `packaged && !mayChange` and read `packaged` as "Microsoft Store".
// `commands.rs::get_packaged_startup` answers the PORTABLE copy first, with
// `packaged: true` — so a portable copy's greying rode on a field that means
// something else, and the TOGGLE path, guarded by the same read, toasted
// "Won't start with Windows" for a `set_startup_enabled` that returns `Ok(())`
// having written nothing at all (`startup.rs::apply_task_enabled` returns
// early for a portable copy). A completed action announced for a write that
// never happened.
//
// The probe walks all four tuples the backend can produce and pins what each
// one is allowed to claim. It is a table, not an eyeball test, because the
// portable row's failure was invisible: the switch moved, the toast appeared,
// and nothing on screen said the registry had not been touched.
//
// Run it from the browser console:  window.__previewStartupProbe()
function startupProbe(): boolean {
  const NOTE = "…";
  const cases: {
    what: string;
    p: StartupOwnership;
    inert: boolean;
    /** [wanting ON, wanting OFF] */
    outcome: [string, string];
  }[] = [
    {
      what: "not asked yet / older backend (null)",
      p: null,
      inert: false,
      // A normal install really did write; claiming success is correct here.
      outcome: ["written", "written"],
    },
    {
      what: "ordinary NSIS or MSI install",
      p: { packaged: false, state: "unavailable", mayChange: true, note: "" },
      inert: false,
      outcome: ["written", "written"],
    },
    {
      what: "PORTABLE copy (commands.rs' own tuple, packaged:true and all)",
      p: { packaged: true, state: PORTABLE_STARTUP_STATE, mayChange: false, note: NOTE },
      inert: true,
      // NEITHER direction may claim success. This is the whole fix.
      outcome: ["nothing-written", "nothing-written"],
    },
    {
      what: "Store package, Windows holding it off (disabledbyuser)",
      p: { packaged: true, state: "disabledbyuser", mayChange: false, note: NOTE },
      inert: true,
      // Windows says off: asking for ON is refused, asking for OFF agrees.
      outcome: ["refused", "written"],
    },
    {
      what: "Store package, app may change it, currently enabled",
      p: { packaged: true, state: "enabled", mayChange: true, note: "" },
      inert: false,
      outcome: ["written", "refused"],
    },
  ];

  const rows = cases.map((c) => {
    const inert = startupRowIsInert(c.p);
    const on = startupOutcome(c.p, true);
    const off = startupOutcome(c.p, false);
    const ok = inert === c.inert && on === c.outcome[0] && off === c.outcome[1];
    return {
      case: c.what,
      inert,
      "expected inert": c.inert,
      "switch ON": on,
      "switch OFF": off,
      expected: `${c.outcome[0]} / ${c.outcome[1]}`,
      portable: startupIsPortable(c.p),
      ok,
    };
  });

  // A portable copy's switch must read OFF even from a config that says true —
  // the case a config folder carried over from an installed copy produces.
  const portable = cases[2]!.p;
  const shownOk = startupShownAsOn(portable, true) === false
    && startupShownAsOn(portable, false) === false;

  const agreed = rows.every((r) => r.ok) && shownOk;
  console.table(rows);
  console.info(
    agreed
      ? "preview: H6 OK — the portable tuple is inert in both directions, its switch reads OFF "
        + "whatever config holds, and NEITHER direction reports a write. Load ?portable to see "
        + "the greyed row and its note."
      : `preview: H6 FAILED — see the table above (shownAsOn ok: ${shownOk}).`,
  );
  return agreed;
}
(window as unknown as { __previewStartupProbe: () => boolean }).__previewStartupProbe =
  startupProbe;
document.querySelectorAll<HTMLElement>("[data-hudring-set]").forEach((b) => {
  b.addEventListener("click", () => {
    const next = b.dataset.hudringSet ?? "compact";
    const seg = b.closest<HTMLElement>(".theme-seg");
    seg?.querySelector<HTMLElement>(".theme-seg-ind")?.setAttribute("data-seg", next);
    seg?.querySelectorAll<HTMLElement>("[data-hudring-set]").forEach((o) => {
      const on = o.dataset.hudringSet === next;
      o.classList.toggle("is-on", on);
      o.setAttribute("aria-checked", String(on));
    });
    // PROBLEM 255 follow-up — mirrors settings-panel.ts: the selected button
    // may now be a different width, so the indicator is re-measured, not
    // re-indexed.
    if (seg) positionSegIndicator(seg);
    paintInert(specialsWrap(), specialsNote(), next === "double");
    // THE REAL FUNCTION, from the leaf module the panel calls — not a copy.
    // This is what puts a projection "on screen" as far as the gate below is
    // concerned, and it is what makes the count meaningful.
    harnessRing = next === "wide" ? "wide" : next === "double" ? "double" : "compact";
    void showRingPreview(harnessRing);
  });
});

/**
 * THE RE-FIRE, wired to the ring-affecting switches (owner, 2026-09-04).
 *
 * `refreshRingPreview` is the app's own gate: it re-sends `preview_hud_layout`
 * only while a projection this app raised is still up, and fires nothing at
 * all when none is. Flipping "Show special keys" in a quiet panel must NOT
 * throw a full-screen ring in front of the user, and that negative is the half
 * worth checking — so the harness wires the same function the panel does and
 * counts what comes out of it.
 */
let harnessRing = previewRing;
const RING_AFFECTING = ["hudspecials", "hudpointer"];
document.getElementById("settings-panel")!.addEventListener("change", (e) => {
  const box = e.target as HTMLInputElement;
  if (!box.matches?.(".toggle-switch input")) return;
  const id = box.id.replace(/^set-/, "");
  if (!RING_AFFECTING.includes(id)) return;
  void refreshRingPreview(harnessRing);
});
// Readable from the browser pane, so a measurement can assert the GATE and not
// just the count: `__ringPreviewShowing()` is the app's own predicate.
(window as unknown as { __ringPreviewShowing: () => boolean }).__ringPreviewShowing =
  () => isRingPreviewShowing();
(window as unknown as { __ringPreviewMs: number }).__ringPreviewMs = RING_PREVIEW_MS;

// SEARCH AND EXPAND, through the app's own functions. Typing here filters the
// real rows with the real matcher, and ?expand shows the full-screen layout.
const previewPanel = document.getElementById("settings-panel")!;
const previewSearch = document.getElementById("set-search") as HTMLInputElement | null;
previewSearch?.addEventListener("input", () => filterSettings(previewPanel, previewSearch.value));
let previewExpanded = q.has("expand");
setPanelExpanded(previewPanel, previewExpanded);
document.getElementById("set-expand")?.addEventListener("click", (e) => {
  e.stopPropagation();
  previewExpanded = !previewExpanded;
  setPanelExpanded(previewPanel, previewExpanded);
});
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape" || !previewExpanded) return;
  e.preventDefault();
  previewExpanded = false;
  setPanelExpanded(previewPanel, false);
}, true);
// The summary rows' "Show" (owner's 1.0.110 review) — mirrors the panel's
// own wiring: expand, then scroll the section into view a frame later.
previewPanel.querySelectorAll<HTMLElement>(".set-summary-show").forEach((b) => {
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    const target = document.getElementById(b.dataset.showSection ?? "")?.closest<HTMLElement>(".set-section");
    previewExpanded = true;
    setPanelExpanded(previewPanel, true);
    requestAnimationFrame(() => target?.scrollIntoView({ block: "start" }));
  });
});

// ACCESSIBILITY PASS (feature 3) — the same arrow-key wiring the real panel
// calls after every render(); here it only needs calling once, since this
// harness never rebuilds the panel's markup.
wireSegRowsKeyboard(previewPanel);
// PROBLEM 255 follow-up — same measured-indicator wiring the real panel does
// after every render(); this harness never rebuilds the panel markup, so one
// call covers it (the ResizeObserver it sets up still fires on ?expand).
wireSegIndicators(previewPanel);

// ABOUT (feature 1) — the real backend calls, through the same leaf helpers
// settings-panel.ts calls. Both degrade to "nothing to show" with no Tauri
// runtime behind this page, which this harness deliberately does not paper
// over: seeing the fallback state here is how the fallback state gets looked
// at at all.
void fetchAboutInfo().then((info) => {
  const verEl = document.getElementById("set-about-version");
  if (verEl) verEl.textContent = info?.version ? `Spaceadom · v${info.version}` : "Spaceadom";
});
document.getElementById("set-about-check-update")?.addEventListener("click", async (e) => {
  e.stopPropagation();
  const btn = e.currentTarget as HTMLButtonElement;
  const statusEl = document.getElementById("set-about-update-status");
  btn.disabled = true;
  if (statusEl) statusEl.textContent = "Checking…";
  const message = await requestUpdateCheck();
  if (statusEl) statusEl.textContent = message;
  btn.disabled = false;
});
document.querySelectorAll<HTMLButtonElement>("[data-about-link]").forEach((b) => {
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    const kind = b.dataset.aboutLink as AboutLinkKind | undefined;
    if (kind) void openAboutLink(kind).catch(() => {});
  });
});
const tpToggle = document.getElementById("set-about-tp-toggle");
const tpList = document.getElementById("set-about-tp-list");
tpToggle?.addEventListener("click", (e) => {
  e.stopPropagation();
  const open = tpToggle.getAttribute("aria-expanded") !== "true";
  tpToggle.setAttribute("aria-expanded", String(open));
  if (!tpList) return;
  tpList.hidden = !open;
  if (open && !tpList.dataset.built) {
    tpList.dataset.built = "1";
    tpList.innerHTML = renderThirdPartyGroups(THIRD_PARTY);
  }
});
if (q.has("about-open") && tpList && !tpList.dataset.built) {
  tpList.dataset.built = "1";
  tpList.innerHTML = renderThirdPartyGroups(THIRD_PARTY);
}

// The one-render-one-animation rule, mirrored: stamp the switch the user just
// flipped and clear every other. The CHARACTER mapping is not duplicated here
// — it rides along inside toggleSwitchHtml's data-char.
document.getElementById("settings-panel")!.addEventListener("change", (e) => {
  const box = e.target as HTMLInputElement;
  if (!box.matches?.('.toggle-switch input')) return;
  document.querySelectorAll(".toggle-switch[data-anim]").forEach((sw) => sw.removeAttribute("data-anim"));
  box.closest(".toggle-switch")?.setAttribute("data-anim", box.checked ? "on" : "off");
});

// The sliders' live behaviour, mirrored from wireSliderChar(): --p drives the
// fill and every decoration, data-dir points the comet's tail backwards.
document.querySelectorAll<HTMLElement>(".sld").forEach((shell) => {
  const el = shell.querySelector<HTMLInputElement>("input[type=range]");
  if (!el) return;
  const lo = parseFloat(el.min), hi = parseFloat(el.max);
  let last = parseFloat(el.value);
  const paint = () => {
    const v = parseFloat(el.value);
    const f = (v - lo) / (hi - lo);
    shell.style.setProperty("--p", f.toFixed(4));
    if (v !== last) shell.dataset.dir = v > last ? "1" : "-1";
    last = v;
  };
  paint();
  el.addEventListener("input", paint);
  el.addEventListener("pointerdown", () => shell.classList.add("is-drag"));
  window.addEventListener("pointerup", () => shell.classList.remove("is-drag"));
});

// ---- key editor: THE REAL COMPONENT, on a stubbed backend (2026-09-04) ----
//
// This used to be static hand-rolled markup — no wiring, no invoke stub — so
// nothing about the editor's actual BEHAVIOUR could be watched here, only its
// resting look. That was fine while the panel had no logic worth checking
// this way; it stopped being fine the moment the replace-confirm flow needed
// somewhere real to run (?editor, see the file doc comment). Same pattern as
// the profile editor above: the real module, a stub backend it cannot tell
// from the truth.
// `config`, NOT `stubState.config` — the SAME object initKeyboardMatrix above
// was given. main.ts's real bootstrap wires both the matrix and the panel to
// ONE shared `appConfig`, mutated in place on save; using the stub's separate
// clone here would let the panel "save" into a copy the keyboard never reads,
// which is exactly the kind of false negative CLAUDE.md warns a disconnected
// stub produces (a replace could silently stop reaching the board and this
// harness would still look right).
initKeyDetailPanel(
  document.getElementById("key-detail-panel")!,
  config,
  (key, binding) => {
    // Mirrors main.ts's onSave EXACTLY — a full replace, not a merge, which
    // is only safe because commit() in key-detail-panel.ts normalises to a
    // complete seven-field KeyBinding first (PROBLEM 204a). A stub that
    // merged here would hide a regression of that fix instead of reproducing
    // it.
    const profile = config.profiles.find((p) => p.name === config.active_profile);
    if (profile) profile.bindings[key] = binding;
    // main.ts's onSave also calls refreshBoard() — without it the panel would
    // show a replace that the keyboard behind it never repaints.
    updateMatrix(document.getElementById("keyboard-matrix")!, config);
  },
);

// ---- the first-run tour: THE REAL MODULE, on the stub config (PROBLEM 242) --
//
// Same host shape main.ts passes, pointed at the stub instead of the live
// config, so what runs here is the shipped state machine and not a rehearsal
// of it. `setDone` writes into `config` — which is the SAME object the matrix
// and the editor hold — so "Skip, then reload with ?tour" genuinely re-arms it
// and "Skip twice in one page" genuinely does not.
initTour({
  isDone: () => config.tour_done === true,
  setDone: () => { config.tour_done = true; },
});
document.getElementById("set-help-tour")?.addEventListener("click", (e) => {
  e.stopPropagation();
  (document.getElementById("settings-panel") as HTMLElement).hidden = true;
  startTour();
});

// ---- open whichever surface was asked for ----
const show = (id: string, expandBtn?: string) => {
  (document.getElementById(id) as HTMLElement).hidden = false;
  if (expandBtn) document.getElementById(expandBtn)!.setAttribute("aria-expanded", "true");
};
if (q.has("profiles")) show("profile-popover", "profile-pill");
if (q.has("gear")) show("settings-panel", "gear-btn");
if (q.has("specials")) show("specials-tray", "specials-btn");
if (q.has("editor")) {
  // openPanel() does everything the hand-rolled version above used to do by
  // hand (--fx/--fy, .open, the backdrop, .editing) PLUS actually renders the
  // real panel content for this key — see the file doc comment for what each
  // ?editor=<key> is there to show.
  const key = q.get("editor") || "c";
  openPanel(key, config);
}
// LAST, like main.ts: the entry card belongs on top of a dashboard that has
// finished assembling itself. Steps 1→2 are reached the way a user reaches
// them — Show me, then click a letter — not by pairing ?tour with ?editor,
// which opens the panel before the tour is armed and so advances nothing.
maybeStartTour();

// ---- cursor glow ----
const stage = document.getElementById("stage")!;
const glow = document.getElementById("cursor-glow")!;
let tx = stage.clientWidth / 2, ty = stage.clientHeight / 2, gx = tx, gy = ty;
stage.addEventListener("mousemove", (e) => {
  const r = stage.getBoundingClientRect();
  tx = e.clientX - r.left; ty = e.clientY - r.top;
  glow.style.opacity = "1";
});
const loop = () => {
  gx += (tx - gx) * 0.09; gy += (ty - gy) * 0.09;
  glow.style.transform = `translate(${gx - 190}px, ${gy - 190}px)`;
  requestAnimationFrame(loop);
};
requestAnimationFrame(loop);

// ---------------------------------------------------------------------------
// PROBLEM 259 — the own-window fallback, drivable without a backend
// ---------------------------------------------------------------------------
//
// The state machine has its own tests (`scripts/own-window-keys.test.ts`); what
// those cannot reach is the DOM half — whether the listeners really attach on
// focus and detach on blur, whether `preventDefault` actually stops the key,
// and whether the space the browser inserted is really taken back out of a real
// `<input>` at the real caret. That half needs a document, so it is verified
// here.
//
// RECIPE (`?ownwindow` on preview.html, then read the console):
//
//   1. Click the page background, hold Space ~1s, release.
//      → ONE `own_window_space_down`, then `own_window_space_up hadCombo=false`.
//   2. Hold Space and tap K.
//      → `own_window_key vk=75`, and no "k" typed anywhere.
//   3. Click into the probe's text box, tap Space quickly.
//      → the box shows a space; NO `own_window_key`. The space stays.
//   4. In the same box, hold Space past the ring delay, then release.
//      → the space vanishes at the threshold and does not come back.
//   5. Click another window (blur).
//      → `own_window_space_up` fires if a hold was open, and the next Space
//        typed into the box produces NO invoke at all until focus returns.
//
// `window.__ownWindowCalls` is the recorded list, so a run can be asserted
// rather than eyeballed.
if (q.has("ownwindow")) {
  const calls: { cmd: string; args: StubArgs }[] = [];
  (window as unknown as { __ownWindowCalls: typeof calls }).__ownWindowCalls = calls;
  for (const cmd of ["own_window_space_down", "own_window_key", "own_window_space_up"]) {
    stubBackend[cmd] = (a) => {
      calls.push({ cmd, args: a });
      console.log(`preview: ${cmd}`, a);
      // `true` = "Rust took the hold". The real guards (foreground, dedupe,
      // ownership) live in Rust and are tested there; the page's behaviour
      // does not branch on the answer, which is exactly why it can be stubbed.
      return true;
    };
  }

  const probe = document.createElement("div");
  probe.style.cssText =
    "position:fixed;left:16px;top:16px;z-index:9999;display:flex;gap:8px;align-items:center";
  const field = document.createElement("input");
  field.type = "text";
  field.placeholder = "type here — tap vs hold Space";
  field.style.cssText = "padding:6px 10px;border-radius:999px;border:1px solid #0003;width:260px";
  const btn = document.createElement("button");
  btn.textContent = "a focusable button";
  btn.style.cssText = "padding:6px 12px;border-radius:999px";
  btn.addEventListener("click", () => console.log("preview: BUTTON CLICKED (Space must not do this)"));
  probe.append(field, btn);
  document.body.appendChild(probe);

  // The same call main.ts makes, with the stub config's own numbers.
  initOwnWindowKeys({
    rolloverMs: config.rollover_ms,
    holdThresholdMs: config.guide_hud_delay_ms,
  });
  console.log(
    `preview: own-window fallback armed (rollover ${config.rollover_ms}ms, ` +
      `hold threshold ${config.guide_hud_delay_ms}ms). Calls are recorded in ` +
      `window.__ownWindowCalls.`,
  );
}

// ---------------------------------------------------------------------------
// PROBLEM 267 — ?ring / ?ring=all: the middle button's ICON RING, drawn by the
// SAME `renderMiddleRing` the overlay page runs, from a stub payload.
// ---------------------------------------------------------------------------
//
// The overlay cannot be validated in a browser harness (its failure mode is
// the OS compositor — CLAUDE.md), so this is the ring's LOOK, not its window:
// geometry, tokens for all three themes (?theme=starry / ?theme=warcry via
// data-theme on the stage), the hovered tile (index 4 = the bottom spoke,
// "Terminal", as artboards 1–3), fun off (?flat), reduced (?reduced).
// Favourites and All both, per the owner's addition #2, so the dense layout
// can be looked at before it ships.
//
// The layout below is a STUB MIRROR of `middle_ring::layout_ring_slots`
// (round 3: 6 on the inner ring at r=125, 9 on the next at r=201, then as
// many as fit at r=277, all 70 px) — Rust's is the real one and has the
// tests. Icons are inline SVG data URLs standing in for real app icons, plus
// one link with NO icon so the letter disc shows. `?name=App&acct=x` puts a
// long two-line name on the hovered tile for the pill's auto-fit.
if (q.has("ring")) {
  // `?ring=spiral` — the 2026-09-15 phyllotaxis layout for the "All" scope,
  // with the SAME constants Rust uses (`middle_ring::spiral_slots`): golden
  // angle per tile, r_i = sqrt(RING_R1^2 + k^2 i), k = ARC_STEP*sqrt(sqrt(3)/2pi).
  // It is the same item list "All" shows, so the two can be compared by
  // flipping one query parameter.
  const spiral = q.get("ring") === "spiral";
  const all = q.get("ring") === "all" || spiral;
  const stage = document.createElement("div");
  stage.className = "mr-stage";
  stage.style.cssText = "position:fixed;inset:0;z-index:200;background:var(--st-bg);";
  const theme = q.get("theme") ?? (q.has("dark") ? "starry" : "earthy");
  stage.dataset.theme = theme;
  stage.classList.toggle("nocturne", theme !== "earthy");
  document.body.appendChild(stage);

  const glyph = (hue: number, letter: string): string => {
    const svg = `<svg xmlns='http://www.w3.org/2000/svg' width='48' height='48' viewBox='0 0 48 48'>`
      + `<rect x='4' y='4' width='40' height='40' rx='10' fill='hsl(${hue} 55% 55%)'/>`
      + `<text x='24' y='31' font-family='Outfit,sans-serif' font-size='20' font-weight='700' fill='#fff' text-anchor='middle'>${letter}</text></svg>`;
    return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
  };
  const eight: Array<[string, string, string | null, RingItem["kind"]]> = [
    ["M", "Mail", glyph(20, "M"), "app"], ["B", "Browser", glyph(210, "B"), "app"],
    ["C", "Chat", glyph(150, "C"), "app"], ["N", "Notes", glyph(45, "N"), "app"],
    ["T", "Terminal", glyph(0, ">"), "app"], ["P", "Player", glyph(280, "P"), "app"],
    ["K", "Calendar", glyph(190, "K"), "app"], ["D", "Downloads", glyph(35, "D"), "folder"],
  ];
  const rest: Array<[string, string, string | null, RingItem["kind"]]> = [];
  "EFGHIJLOQRSVWXYZ".split("").forEach((l, i) => {
    // Two links without an icon yet: the letter disc, as the design's
    // placeholder rule says.
    const link = l === "R" || l === "G";
    rest.push([l, link ? `${l.toLowerCase()}link.com` : `App ${l}`, link ? null : glyph((i * 47) % 360, l), link ? "link" : "app"]);
  });
  const specials: Array<[string, string, string | null, RingItem["kind"]]> = [
    ["Esc", "Boss Key", null, "special"], ["`", "PiP", null, "special"], ["Tab", "Fullscreen PiP", null, "special"],
    ["\u232B", "Force Close", null, "special"], ["RAlt", "Cycle Profiles", null, "special"],
    [",", "Search / Input", null, "special"], [".", "Pause Spaceadom", null, "special"],
    [";", "Voice Typing", null, "special"], ["/", "Screenshot", null, "special"], ["'", "Keyboard", null, "special"],
  ];
  const src = all ? [...eight, ...rest, ...specials] : eight;
  const n = src.length;
  const caps = [6, 9, 22];
  const GOLDEN_ANGLE = 360 * (1 - 1 / 1.618);
  const SPIRAL_K = 71 * Math.sqrt(Math.sqrt(3) / (2 * Math.PI));
  const items: RingItem[] = src.map(([key, name, icon, kind], i) => {
    if (spiral) {
      const it: RingItem = {
        key, code: key.toLowerCase(), name, icon, kind,
        ring: 0,
        angle_deg: ((GOLDEN_ANGLE * i) % 360 + 360) % 360,
        radius: Math.sqrt(108 * 108 + SPIRAL_K * SPIRAL_K * i),
        tile: 44,
        pitch_deg: 360 / n,
      };
      if (i === 4 && q.has("name")) { it.name = q.get("name") ?? name; it.account = q.get("acct"); }
      return it;
    }
    let ring = 0, start = 0;
    while (i - start >= Math.min(caps[ring], n - start)) { start += Math.min(caps[ring], n - start); ring++; }
    const count = Math.min(caps[ring], n - start);
    const radius = 125 + ring * 76;
    const angle = (i - start) * (360 / count);
    const it: RingItem = { key, code: key.toLowerCase(), name, icon, kind, ring, angle_deg: angle, radius, tile: 70, pitch_deg: 360 / count };
    if (i === 4 && q.has("name")) { it.name = q.get("name") ?? name; it.account = q.get("acct"); }
    return it;
  });
  const rings = spiral ? 1 : new Set(items.map((it) => it.ring)).size;
  // A spiral's extent is its outermost tile; it draws no guide circles (the
  // same rule `middle_ring::guide_diameters` applies — no rings, no guides).
  const spiralExtent = spiral
    ? Math.max(...items.map((it) => it.radius)) + 22
    : 0;
  const payload: MiddleRingPayload = {
    items,
    scope: all ? "all" : "my_eight",
    cx: window.innerWidth / 2,
    cy: window.innerHeight / 2,
    scrim: spiral
      ? Math.max(640, 2 * spiralExtent + 80)
      : rings > 1 ? Math.max(640, 2 * (125 + (rings - 1) * 76 + 35) + 80) : 600,
    guides: spiral ? [] : Array.from({ length: rings }, (_, r) => 2 * (125 + r * 76) + 14),
    fun: !q.has("flat"),
    reduced: q.has("reduced"),
    pill_max: 170,
    shape: spiral ? "spiral" : undefined,
  };
  const armed = q.has("none") ? null : 4;
  const el = renderMiddleRing(stage, payload, armed);
  setRingArmed(el, armed, payload.items, payload.pill_max); // the live path's arming (push + pill name)
  // The browser pane can load this page at 0×0 and size it afterwards, so
  // the centre is re-read on resize; the overlay never needs this — Rust
  // sizes its window before the payload is sent.
  const recentre = () => {
    el.style.setProperty("--mr-cx", `${window.innerWidth / 2}px`);
    el.style.setProperty("--mr-cy", `${window.innerHeight / 2}px`);
  };
  recentre();
  window.addEventListener("resize", recentre);
  // The same forced style read the overlay uses (1.0.110 findings): a class
  // added from a rAF before the first style recalc creates no transition.
  void el.getBoundingClientRect();
  el.classList.add("in");
  if (payload.fun && !payload.reduced) previewWave(el, payload.items);
  // `window.__ringAim(deg | null)` feeds the wave a bearing as Rust would;
  // `window.__ringWave()` reads the per-tile scales back.
  (window as unknown as { __ringAim: typeof previewAim }).__ringAim = previewAim;
  (window as unknown as { __ringWave: typeof waveSnapshot }).__ringWave = waveSnapshot;
  (window as unknown as { __ringWaveStep: typeof previewWaveStep }).__ringWaveStep = previewWaveStep;
  (window as unknown as { __ringWaveTargets: typeof waveTargets }).__ringWaveTargets = waveTargets;
  (window as unknown as { __fitPill: typeof fitPill }).__fitPill = fitPill;
}
