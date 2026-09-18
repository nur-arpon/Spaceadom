/**
 * settings-panel.ts — the bottom-left gear popover.
 *
 * V14: pill toggles and terracotta sliders instead of the old slide-in with
 * native range inputs (Dashboard Earthy v2.dc.html).
 *
 * DEPARTURE FROM THE MOCKUP, on purpose: the mockup shows a fourth toggle,
 * "Run at startup". There is no backend command for it — startup.rs writes
 * the Run key unconditionally on every launch, which is itself an open bug
 * (WHAT_HAPPENED.md). A toggle that silently does nothing is exactly the
 * hollow-feature problem this rebuild exists to end, so it is left out until
 * the command exists. The three sliders below are V13 settings that DO work
 * and must not be dropped to match a mockup.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
// Only for the THIRD-PARTY list's per-package links (`openAboutLink` in
// controls.ts covers the four fixed ones); same leaf-safe import as there.
import { openUrl } from "@tauri-apps/plugin-opener";
// PROBLEM 242 — the header's second entry restarts the first-run walkthrough.
// tour.ts is a leaf and imports nothing back, so there is no cycle here.
import { startTour } from "./tour";
import {
  appConfig, persistConfig, applySound, applyMotion,
  applyLook, applySkyMode, knownConflicts, refreshConflicts, resolveTheme,
} from "../main";
import { sfx } from "../sfx";
import { openConflictPrompt } from "./conflict-prompt";
import {
  toggleSwitchHtml, sliderShell, segRowHtml, paintInert,
  SPECIALS_INERT_NOTE, RING_OPTS, ringLayoutFor, ringConfigFor,
  groupHeadingHtml, filterSettings, setPanelExpanded,
  showRingPreview, refreshRingPreview, clearRingPreview,
  wireSegRowsKeyboard, wireSegIndicators, positionSegIndicator, aboutRowHtml, renderThirdPartyGroups,
  fetchAboutInfo, requestUpdateCheck, openAboutLink,
  // PROBLEM 249 — the About row's two update controls and the one rule they
  // share: act on the `update-status` EVENT, never on the command's return.
  updateStateIsBusy, fetchRollbackTarget, requestRollback,
  // REVIEW FIXES 2026-09-05 (H6) — "Run at startup"'s ownership rules, moved
  // into the leaf so `preview.ts` exercises the identical decision. The panel
  // no longer decides any of this for itself; see controls.ts for what the
  // old `packaged && !mayChange` got wrong about a portable copy.
  startupRowIsInert, startupShownAsOn as ownershipShownAsOn, startupOutcome,
  startupIsPortable, type StartupOwnership,
  type RingLayout, type ThirdPartyEntry, type AboutLinkKind,
  // PROBLEM 267 — the icon ring's three settings and the App-exceptions
  // scope, as leaf data shared with preview.ts.
  MIDDLE_STYLE_OPTS, MIDDLE_SCOPE_OPTS, ALL_LAYOUT_OPTS, EXC_SCOPE_OPTS,
  middleStyleFor, middleScopeFor, allLayoutFor, normaliseExceptions, effectiveFavourites,
  EIGHT_PICKER_NOTE, FAVOURITES_MAX,
  type MiddleStyle, type MiddleScope, type AllLayout, type ExcScope, type ExcRow,
} from "./controls";
// ABOUT (feature 1) — a plain data import. resolveJsonModule (tsconfig.json)
// makes this a typed array literal at compile time; Vite bundles it into
// dist2 like any other imported asset, so it ships with the app rather than
// being read from disk at runtime.
import thirdPartyRaw from "../generated/third-party.json";
import { showToast } from "./toast";
// The SAME grid the key editor uses - the owner asked for exactly that
// picker here. Shared leaf module, never a fork (see app-grid.ts).
import {
  drawAppGrid, cachedApps, loadApps, exeStem, findAppByStem, paintAppDisc,
} from "./app-grid";
import { registerDismissable } from "../dismissable";

let panelEl: HTMLElement | null = null;
let _paused = false;
/** Two-step confirm state for the destructive buttons: "def" | "clr" | null */
let _armed: "def" | "clr" | null = null;
/** PROBLEM 144 — true only for the first render after the gear is opened, so
 *  the "Show me around" convoy plays once and never re-opens what the user
 *  closed. */
let _freshOpen = false;
let _armTimer: number | undefined;
/** PROBLEM 213 follow-up — the exit timer for closeSettingsPanel's animated
 *  close, so a reopen mid-exit can cancel it instead of racing it. */
let _closeTimer: number | undefined;
/**
 * `renderConflicts`'s inner `draw()`, published so the Maintenance group's
 * "Re-check now" can repaint the list it just re-scanned.
 *
 * A FUNCTION REFERENCE, not a re-entry into `renderConflicts()`: `draw` is a
 * closure over the `#set-conflicts` box that exists RIGHT NOW, and calling
 * `renderConflicts()` again would attach a second set of listeners to the same
 * section. Re-set on every `renderConflicts`, so it always points at the live
 * box; null before the section has ever been drawn, which is why every call
 * site is optional.
 */
let _redrawConflicts: (() => void) | null = null;

let _onResetDefaults: (() => void) | null = null;
let _onClearAll: (() => void) | null = null;

/**
 * 2026-09-01 redesign — the search text and the expanded state.
 *
 * BOTH LIVE AT MODULE SCOPE for the same reason the app-exceptions picker's
 * query does: `render()` replaces the panel's whole `innerHTML` after every
 * toggle, so anything held only in the DOM is destroyed by the next flip of
 * any switch. A user who typed "sound", flipped the switch they found and
 * watched the filter reset itself would be right to call that a bug.
 */
let _query = "";
let _expanded = false;
/** Which descriptions were open at the top of the current render() — see the
 *  assignment there, and `renderRingFixRow`, which is built too late to be
 *  reached by that function's own restore loop. */
let _openDescSnapshot: string[] = [];
/** Registered once; collapses an expanded panel on Escape (see `wireExpandEscape`). */
let _expandEscWired = false;

/**
 * PROBLEM 250 — who owns "Run at startup" in THIS install.
 *
 * `null` until the backend has been asked, and `packaged: false` for every
 * NSIS and MSI install, which is every install that exists today — so the row
 * behaves exactly as it always has unless this app is running from the
 * Microsoft Store's MSIX package.
 *
 * MODULE SCOPE, like `_query` and `_expanded` above, and for the same reason:
 * `render()` replaces the panel's entire innerHTML on every toggle, so the
 * greying has to be re-applied from a value that survives that. It is fetched
 * ONCE when the panel opens (and again right after the switch is used), never
 * per render — an `invoke` on the render path would make every unrelated
 * toggle round-trip to Rust.
 */
let _pkgStartup: StartupOwnership = null;

// ---------------------------------------------------------------------------
// ABOUT (feature 1) — module-scope state, same reasoning as `_pkgStartup`
// above: `render()` replaces the panel's entire innerHTML on every toggle, so
// anything that must survive a render (what was fetched, whether the
// third-party list is expanded) has to live outside the DOM.
// ---------------------------------------------------------------------------
const THIRD_PARTY = thirdPartyRaw as ThirdPartyEntry[];
/** `null` until `fetchAboutInfo()` answers once, or forever in a harness with
 *  no Tauri runtime behind it — `aboutRowHtml` renders that state as the bare
 *  app name, never as a blank row. */
let _aboutInfo: { version: string; installKind: string } | null = null;
/** Whether the third-party list is expanded. Survives render() like every
 *  other "is this open" flag in this file (`_expanded`, `_excPickerOpen`). */
let _thirdPartyOpen = false;
/** The line under "Check for updates" — the last status text, so a check
 *  started and a render triggered by something else (any toggle) do not wipe
 *  it back to blank. */
let _updateStatusText = "";
/** Wired once: the `update-status` event fires for background checks too
 *  (the daily poller), not only for a press of this row's own button, so this
 *  is a top-level listener rather than something re-wired per render. */
let _updateStatusListenerWired = false;
/** REVIEW FIXES 2026-09-05 (LOW) — a `listen()` for it is in flight. See
 *  `wireUpdateStatusListener`: `_updateStatusListenerWired` now flips only
 *  when the registration RESOLVES, so this covers the window in between,
 *  which `render()` walks through on every toggle. */
let _updateStatusListenPending = false;
/** PROBLEM 249 — the version this copy can go back to, or `null` when there
 *  is none (a dev build, a Store package, a portable copy, or the very first
 *  auto-update this machine has taken). Fetched with `_aboutInfo`, on every
 *  panel open rather than once per session: an update installed by the daily
 *  background check while the panel was shut changes the answer. */
let _rollback: { version: string; path: string; kind: string } | null = null;
/** One-tap confirm for the rollback button, local to that button rather than
 *  going through `arm()`/`disarm()` — those re-render the whole panel, which
 *  would replay its entrance wave and, worse, rebuild the very button whose
 *  armed state is being tracked. */
let _rollbackArmed = false;
let _rollbackArmTimer: number | undefined;

export function initSettingsPanel(
  onResetDefaults: () => void,
  onClearAll: () => void,
): void {
  panelEl = document.getElementById("settings-panel");
  _onResetDefaults = onResetDefaults;
  _onClearAll = onClearAll;
  if (!panelEl) return;
  wireExpandEscape();
  render();
}

/**
 * PROBLEM 213 follow-up — a panel mid-exit is not "open" for any caller's
 * purposes. Without `.closing` excluded here, a fast reopen (the gear
 * pressed again while the animated close from entering sky mode is still
 * fading out, ~270ms) read as `wasOpen === true` to the gear's own toggle
 * handler, so the click was treated as "close what's already closing"
 * instead of "reopen" — the panel then finished hiding and the reopen was
 * silently swallowed (measured). Same reasoning applies to `wireSkyEscape`'s
 * bail check: a panel already on its way out should not block Escape from
 * also leaving sky mode.
 */
export function isSettingsPanelOpen(): boolean {
  return !!panelEl && !panelEl.hidden && !panelEl.classList.contains("closing");
}

/**
 * PROBLEM 205 — has the user ever actually opened this panel?
 *
 * `initSettingsPanel` calls `render()`, and `render()` reaches BOTH
 * `renderAppExceptions()` and `renderConflicts()`, each of which used to fire
 * `loadApps()` unconditionally. `initSettingsPanel` runs inside `bootstrap()`,
 * on the critical path to first paint — so the settings panel was a SECOND
 * bootstrap trigger for `list_start_menu_apps`, alongside the key editor's.
 * Deferring only the key editor's would therefore have changed nothing, which
 * is the kind of "fix" that gets measured, found ineffective, and blamed on
 * the wrong hypothesis.
 *
 * `list_start_menu_apps` is a NON-async `#[tauri::command]`, so it runs on the
 * MAIN THREAD (~12s here) and every IPC call from both webviews queues behind
 * it — including the `dashboard_ready` that is the only thing that shows the
 * window.
 */
let _settingsEverOpened = false;

/** Warm the Start-Menu scan, but never before the user has opened the panel. */
function warmAppsIfOpened(): void {
  if (_settingsEverOpened) void loadApps();
}

export function openSettingsPanel(): void {
  if (!panelEl) return;
  // A reopen (e.g. the gear pressed again while sky mode's close is still
  // fading out) must win outright — cancel the pending hide and drop the
  // exit state, or the panel would reappear already mid-fade-out.
  window.clearTimeout(_closeTimer);
  panelEl.classList.remove("closing");
  panelEl.style.animation = "";   // undo closeSettingsPanel's inline kill switch
  _settingsEverOpened = true;   // PROBLEM 205 — before render(), which reads it
  _freshOpen = true;      // PROBLEM 144 — arm the one-shot "Show me around"
  render();
  panelEl.hidden = false;
  document.getElementById("gear-btn")?.setAttribute("aria-expanded", "true");
  // PROBLEM 250 — ask, every open, who owns "Run at startup". Deliberately
  // NOT awaited and deliberately not inside render(): the panel must appear at
  // once, and for the packaged case the answer arrives a few milliseconds later
  // and repaints one row in place. The user can change this in Task Manager
  // while the panel is shut, so a value cached from the first open would go
  // stale — hence every open, not once per session.
  void refreshPackagedStartup();
  // ABOUT (feature 1) — same shape, same reason: fetch every open (the daily
  // updater can have changed the version since the last one), repaint one
  // corner of the row in place rather than blocking the panel's appearance.
  void refreshAboutInfo();
}

/**
 * PROBLEM 213 follow-up (owner, 2026-08-28) — entering sky mode now closes
 * this panel (main.ts's applySkyMode), and a panel that just vanished under
 * a fading sky read as a glitch, not a deliberate close. So this is no
 * longer an instant `hidden = true`: it plays the same ~65%-of-entrance,
 * --ease-in exit as the rest of the app (.conflict-prompt's is-leaving,
 * #key-detail-panel's own .closing — see styles.css), then hides for real.
 * Every caller gets it — outside click, Escape, the gear re-toggling itself
 * closed — there is exactly one way this popover closes.
 *
 * Guarded by `panelEl.hidden` so the (very frequent — every outside click in
 * the whole app runs through closeAllPopovers) no-op case does no work.
 */
export function closeSettingsPanel(): void {
  if (!panelEl || panelEl.hidden) return;
  document.getElementById("gear-btn")?.setAttribute("aria-expanded", "false");
  // A closed panel is never left in the full-screen state. The expand is a
  // "look at it now" gesture, not a preference (see `setPanelExpanded`), and a
  // panel that reopened full-screen minutes later — from the gear, from the
  // tray, from anywhere — would read as the app having got stuck rather than
  // as something the user chose. Collapse BEFORE the exit animation so the
  // fade plays on the shape it was actually shown at.
  if (_expanded) { _expanded = false; setPanelExpanded(panelEl, false); }
  // 1.0.96 review fix — same reasoning as the expand collapse just above: a
  // filter is a "look at it now" gesture too, not a preference. `_query` is
  // module scope, so without this it survived the popover closing and the
  // NEXT open reopened still filtered on whatever was last typed, with no
  // visible search box open to explain why rows were missing. `render()`
  // rebuilds the panel's HTML on every open (unfiltered) and `wireSearch`
  // only re-applies a filter when `_query` is non-empty, so clearing it
  // here is what makes a reopen start clean; the input clear is just so a
  // still-mounted box (the animated exit keeps the DOM around briefly)
  // doesn't flash the stale query first.
  _query = "";
  const searchBox = panelEl.querySelector<HTMLInputElement>("#set-search");
  if (searchBox) searchBox.value = "";
  disarm();
  // The ring-preview clock, forgotten with the panel. It is only ever read to
  // decide whether a CHANGE should re-project, and every control that can make
  // that change lives in this panel — so a clock still ticking after it closes
  // can only ever be wrong. Clearing never fires a command; the ring on screen
  // takes itself down on Rust's own timer exactly as before.
  clearRingPreview();

  // :root.reduced-motion is the in-app setting (config.motion), deliberately
  // not the OS media query alone — PROBLEM 47. No lingering transition: skip
  // the animated branch entirely rather than let CSS race a forced opacity.
  if (document.documentElement.classList.contains("reduced-motion")) {
    panelEl.hidden = true;
    return;
  }

  // .popover's st-pop-in entrance (styles.css) is a `both`-fill animation
  // that holds opacity:1 forever once it finishes — and CSS transitions
  // refuse to engage on a property that is still animation-driven at the
  // moment of change, EVEN IF that same style change is what cancels the
  // animation. Measured: killing the animation and setting the exit
  // transition in one pass jumped straight to opacity:0, no fade at all. So
  // the animation is killed on its OWN frame first (forcing a layout flush
  // commits it as a plain, non-animated value), then `.closing` is applied —
  // the same "let it paint before transitioning" rule as key-detail-panel's
  // backdrop fade a few files over.
  panelEl.style.animation = "none";
  void panelEl.offsetHeight;   // flush — commits opacity:1 as a static value
  panelEl.classList.add("closing");

  window.clearTimeout(_closeTimer);
  _closeTimer = window.setTimeout(() => {
    if (!panelEl || !panelEl.classList.contains("closing")) return; // reopened mid-exit
    panelEl.hidden = true;
    panelEl.classList.remove("closing");
    panelEl.style.animation = "";
  }, 270);
}

/** Keep the Engine toggle honest when the engine is paused from elsewhere. */
export function setPausedState(paused: boolean): void {
  _paused = paused;
  if (isSettingsPanelOpen()) render();
}

/**
 * Escape peels ONE layer: an expanded panel returns to the popover, and only a
 * second Escape closes it.
 *
 * IT HAS TO BE A CAPTURE-PHASE LISTENER, and that is the whole reason this
 * function exists rather than a line in main.ts. Two bubble-phase document
 * listeners already answer Escape — bootstrap's (`closeAllPopovers`, which
 * closes this panel outright) and `wireSkyEscape`'s (which leaves sky mode) —
 * and neither can be taught about a state they do not know exists without
 * editing main.ts. Capture runs first, so stopping propagation here means the
 * expanded panel collapses and NOTHING else happens on that press. Exactly the
 * peel rule `wireSkyEscape`'s own comment describes, applied one layer higher.
 *
 * Wired once, from `initSettingsPanel`. It is a no-op on every press while the
 * panel is not expanded, which is nearly all of them.
 */
function wireExpandEscape(): void {
  if (_expandEscWired) return;
  _expandEscWired = true;
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape" || !_expanded || !panelEl || panelEl.hidden) return;
    e.preventDefault();
    e.stopPropagation();
    toggleExpanded(false);
  }, true);
}

/** The one place the expanded state changes, so the flag and the DOM cannot
 *  disagree. `sfx.bloomOpen/Close` because this IS a bloom — the same surface
 *  growing and shrinking, which is what those two sounds are for. */
function toggleExpanded(on: boolean): void {
  if (!panelEl || _expanded === on) return;
  _expanded = on;
  setPanelExpanded(panelEl, on);
  if (on) sfx.bloomOpen(); else sfx.bloomClose();
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------


/**
 * PROBLEM 103 — the button must say what it will actually DO.
 *
 * On a stock profile (Founders/Gamers/Professionals) reset restores the
 * factory bindings. On a profile the USER created there are no factory
 * bindings to restore, so the same button simply empties it. One label, two
 * very different outcomes — and the destructive one was hidden behind the
 * gentler word. The user hit exactly this: four clicks of "Reset this
 * profile" on their own profile 'hi' cleared all 26 bindings.
 */
const STOCK_PROFILES = ["Founders", "Gamers", "Professionals"];
function resetLabel(): string {
  const name = appConfig?.active_profile ?? "";
  return STOCK_PROFILES.includes(name) ? "Reset this profile" : "Clear this profile";
}

function render(): void {
  if (!panelEl || !appConfig) return;

  // The entrance cascade belongs to OPENING the panel, not to re-rendering it.
  // .pop gates every st-pop-in in styles.css; without it, flipping one toggle
  // replayed the whole wave and the character animation drowned in it.
  panelEl.classList.toggle("pop", _freshOpen);

  // Descriptions live in the DOM (.is-open), and innerHTML below replaces the
  // DOM — so every re-render silently closed them all. That is the owner's
  // "I turned on fun mode and the show-me-around descriptions disappeared".
  // Snapshot what is open, restore it after the rebuild, without animation.
  const openDescs = Array.from(
    panelEl.querySelectorAll<HTMLElement>(".set-desc.is-open"),
  ).map((b) => b.dataset.descFor ?? "").filter(Boolean);
  // Published for the rows that do not exist yet when the restore loop below
  // runs — `renderConflicts` builds the ring tool's description box after
  // this function has already replaced the panel's markup, so it has to
  // restore its own open state from the same snapshot.
  _openDescSnapshot = openDescs;

  const sound = !!appConfig.sound_enabled;

  // run_at_startup is optional on configs migrated from V14 — treat missing
  // as ON, matching the backend's serde default.
  const startup = appConfig.run_at_startup !== false;

  // "Visual effects" is shown as a plain ON/OFF, but stores three values:
  // ON  → "full"   (force effects on, even if Windows asks for less)
  // OFF → "reduced"
  // and "auto" (the default) follows Windows. The toggle reflects what is
  // ACTUALLY happening right now, so on a machine with animation effects
  // disabled it correctly shows OFF and flipping it to ON works — that
  // machine is exactly where a tester saw a motionless app (PROBLEM 47).
  const effects = !document.documentElement.classList.contains("reduced-motion");

  // PROBLEM 92's "Software overlay" switch USED TO BE READ HERE.
  //
  // It is gone (2026-09-01, owner). It asked the user to hold an opinion about
  // GPU compositing, and `overlay_compositing` is a MEASUREMENT rather than a
  // preference — a switch is the wrong shape for a measurement. What the user
  // actually has is a symptom ("the ring isn't showing"), so the control now
  // lives in the Conflicts area as a TOOL that re-runs the measurement and
  // reports what it found: `renderRingFixRow` below, `run_overlay_fix` in
  // commands.rs. Nothing was lost — the escape hatch from a false-positive
  // verdict is still reachable, and it no longer requires the user to know
  // what compositing is.

  // PROBLEM 144 — the new settings. `theme` is the 3-way look; `fun` gates the
  // personality layer AND (per the owner) whether Starry night is the new sky
  // or the plain nocturne this app has always had.
  const theme = appConfig.theme || (appConfig.dark_mode ? "starry" : "earthy");
  // === true, not !== false: since 2026-08-20 both personality switches are
  // OFF at first install (the owner's decision) — a missing field means off.
  const fun = appConfig.fun_mode === true;
  const hideBoard = appConfig.hide_keyboard === true;
  const showAround = appConfig.show_me_around === true;
  // PROBLEM 174 — the ring-to-toast flight. `=== true`, same rule as the two
  // above: absent means OFF, and every config written before 1.0.73 lacks it.
  const flight = appConfig.hud_toast_flight === true;
  // PROBLEM 209 — pointer activation on the guide HUD. `!== false`, which
  // BREAKS the run of `=== true` lines directly above, deliberately: the
  // owner flipped this default ON on 2026-08-27 (overriding his own
  // new-behaviour-defaults-off convention), the key is absent from every
  // config written before 1.0.88, and absent must now read ON. `=== true`
  // here would show the switch off for every existing user while Rust ran the
  // feature — the switch and the app disagreeing, which is the one class of
  // bug in this panel nobody can see from the outside.
  const hudPointer = appConfig.pointer_hud_activation !== false;
  // PROBLEM 263 — hold the middle mouse button to raise the ring. `!== false`
  // for the SAME reason as `hudPointer` directly above, not by copying it: Rust
  // ships this `default = "default_true"` and the key is absent from every
  // config on disk, so `=== true` would show the switch off for every existing
  // user while the feature ran. The 3D/CAD safety net is NOT this flag — it is
  // the built-in list in `hook/orbit_apps.rs`, which is not a setting.
  const middleRing = appConfig.middle_button_ring !== false;
  // PROBLEM 267 — WHAT the middle button raises and HOW MUCH the icon ring
  // shows. Both absent-tolerant through the leaf helpers (absent = icon ring,
  // absent = my eight — Rust's serde defaults). Both pills are inert while
  // the middle-button switch is off: a control for a trigger that is switched
  // off is a control that does nothing, and that is worse than a missing one
  // (CLAUDE.md). The scope pill is additionally inert under "Space ring": the
  // Space HUD has its own Compact/Wide/Double and this pill is not it.
  const middleStyle = middleStyleFor(appConfig.middle_ring_style);
  const middleScope = middleScopeFor(appConfig.middle_ring_scope);
  // 2026-09-15 — how the "All" scope is arranged. The pill sits directly
  // under the scope pill and is only relevant while "All" is chosen, the
  // same way "Choose your favourites" only means something under
  // Favourites; absent = rings, which is what every install already draws.
  const allLayout = allLayoutFor(appConfig.all_ring_layout);
  // PROBLEM 267 follow-up — how the icon ring moves. Inert with the other
  // two rows, and additionally under "Space ring" (it has its own motion).
  // PROBLEM 209 — show the specials on the HUD's inner ring. `!== false`
  // again, but for the OTHER reason: this is existing behaviour becoming
  // optional, so an old config must keep the ring it has always had. Same
  // read, different argument — check the field's default before copying
  // either of these lines onto a new setting.
  const hudSpecials = appConfig.hud_show_specials !== false;
  // THE SPACE RING'S SHAPE — one 3-way pill since 2026-09-01, replacing the
  // "New ring layout" switch AND the "Shortcut rows" pill. The mapping onto
  // the two config fields is a pure function in controls.ts so the harness
  // reads it the same way; see `RING_OPTS` there for the table and for why
  // `hud_band_count: "one"` is retired rather than shown.
  const ring = ringLayoutFor(appConfig.hud_magnetic_layout, appConfig.hud_band_count);
  // THE ONE REMAINING DEPENDENCY, in one line: Double gives both rings to
  // apps, so the specials switch has nothing to do and must SAY so (see
  // `specialsRow`). Presentation only — `hud_show_specials` itself is never
  // touched here, so coming back to Compact or Wide restores what the user
  // picked. The rows pill's OWN inert state is gone with the pill: three
  // named shapes have no dead state left to explain.
  const specialsInert = ring === "double";
  // PROBLEM 195 — "Don't send logs" is the NEGATION of the config field.
  //
  // `send_logs: true` means SENDING IS HAPPENING, and that is the default. So
  // the switch is CHECKED when send_logs is FALSE. Read as `!== false` and not
  // `=== true` (the opposite of the `flight` line above, deliberately): the key
  // is absent from every config written before 1.0.82, and absent means ON.
  //
  // If you ever find yourself "simplifying" this to `appConfig.send_logs`, the
  // switch will read backwards and a user who asked for silence will be told
  // they have it while reports keep going out. That is the one bug in this
  // panel nobody could see from the outside.
  const dontSendLogs = appConfig.send_logs === false;
  // PHASE A — Advanced mode. Absent = off (schema.rs `advanced_mode`).
  const advanced = appConfig.advanced_mode === true;
  document.body.classList.toggle("show-around", showAround);

  // THE 2026-09-01 REDESIGN, and the order is the owner's, not a convenience.
  //
  //   header  — the name, the link that opens every description, the expand
  //   engine  — STICKY, so the main switch is reachable from any scroll depth
  //   search  — directly above the first group heading, as specified
  //   groups  — Appearance · Behaviour · The Space ring
  //   sections— App exceptions · Conflicts (which now carries the ring tool)
  //   actions — reset / clear / presets / logs
  //   privacy — LAST, exactly where PROBLEM 195 put it
  //
  // "Privacy stays last" is why its group sits BELOW the action buttons rather
  // than beside the other three: the instruction is about the bottom of the
  // panel, which is where the crash-report opt-out has lived since 1.0.82, and
  // moving it up to join a tidy row of groups would quietly relocate the one
  // control here that concerns what leaves the machine.
  panelEl.innerHTML = `
    <div class="set-head">
      <div class="set-title">Settings</div>
      <button type="button" class="set-head-link" id="set-help-descs"
              aria-pressed="${showAround}">What do these do?</button>
      <button type="button" class="set-head-link" id="set-help-tour"
              title="Replay the first-run walkthrough">Show me the walkthrough</button>
      <button type="button" class="set-expand" id="set-expand"
              aria-pressed="${_expanded}"
              aria-label="${_expanded ? "Shrink settings" : "Expand settings"}"
              title="${_expanded ? "Back to the small panel (Esc)" : "Fill the window"}">⤢</button>
    </div>

    ${engineRow(!_paused)}

    <!-- THE SCROLLER, and it is a DIFFERENT element from the panel ONLY in
         the expanded state. ".set-scroll { display: contents }" is the base
         rule, so in the 280px popover this wrapper is not in the layout at
         all and every measurement, every margin and the panel's own scrollbar
         are byte-identical to what they were before it existed.

         Expanded, it becomes the flex child that scrolls, which is what makes
         the Engine bar RESERVE its height instead of floating over the grid
         (owner, 1.0.96 review: "the sticky bar covers the first row of columns
         2 and 3"). A sticky header inside the scroller can only ever overlay
         the content that scrolls under it — that is what sticky IS — so the
         fix is to stop the top of the panel being part of the scroll at all.
         See #settings-panel.expanded in styles.css. -->
    <div class="set-scroll">
    <!-- THE COLUMN FLOW (owner, 1.0.110 review: "design chaos" — a large
         blank beside the tall Space-ring column while the left one ended
         early). Same contract as .set-scroll above: display:contents in the
         popover, so nothing there changes; in the expanded panel it is the
         multicol container (columns: 2) that the section cards flow through,
         and the App-exceptions / Conflicts sections span it (column-span:
         all). See #settings-panel.expanded .set-cols in styles.css. NOTE:
         no backticks in this comment — it sits inside a template literal. -->
    <div class="set-cols">

    <input class="input set-search" id="set-search" type="text"
           placeholder="Search settings…" autocomplete="off" spellcheck="false"
           aria-label="Search settings" />
    <!-- ACCESSIBILITY PASS (feature 3) — a live region so a screen-reader user
         typing into the search box hears "Nothing here matches that" the
         moment it appears, the same way a sighted user sees it without having
         to go looking. role=status plus aria-live=polite announces a CHANGE
         to this element's content; it says nothing while hidden is true,
         because there is no content change to announce yet. -->
    <div class="set-note" id="set-search-empty" role="status" aria-live="polite" hidden>Nothing here matches that.</div>

    <div id="set-groups">
      <div class="set-group">
        ${groupHeadingHtml("appearance", "Appearance")}
        <div class="set-rows">
          ${themeRow(theme, 0)}
          ${toggleRow("fun",       "Fun mode",          fun,       1)}
          ${toggleRow("sound",     "Sound ticks",       sound,     2)}
          ${toggleRow("motion",    "Visual effects",    effects,   3)}
          ${toggleRow("hideboard", "Hide the keyboard", hideBoard, 4)}
          ${toggleRow("advanced",  "Advanced mode",     advanced,  5)}
        </div>
      </div>

      <div class="set-group">
        ${groupHeadingHtml("behaviour", "Behaviour")}
        <div class="set-rows">
          ${startupRow(startupShownAsOn(startup), 5)}
          ${typingSpeedRow(appConfig.typing_wpm ?? DEFAULT_WPM)}
          ${sliderRow("opacity", "Opacity floor", appConfig.opacity_floor_pct, 10, 90, 5, "%")}
        </div>
      </div>

      <div class="set-group">
        ${groupHeadingHtml("ring", "The Space ring")}
        <div class="set-rows">
          ${toggleRow("hudpointer", "Point to launch", hudPointer, 6)}
          <!-- PROBLEM 263 — the ring's second trigger, and it sits HERE, next
               to "Point to launch", for two reasons. Both rows are about the
               MOUSE's part in the ring, so they read as a pair; and this is
               the only gap in the section that does not come between the pill
               and the specials switch (see the note directly below, which is
               a constraint, not a preference). -->
          ${toggleRow("middlering", "Middle button opens the ring", middleRing, 7)}
          <!-- PROBLEM 267 — the two rows that only mean something while the
               switch above is on, directly under it so the greyed reason is
               visible FROM the control (the same adjacency rule the pill and
               the specials switch obey below). "Middle button shows" is the
               owner's choice between the new icon ring and phase 1's Space
               ring; "Middle-button ring shows" is the icon ring's scope with
               the "Choose your favourites" picker under it (artboards 8 and 9). -->
          ${middleStyleRow(middleStyle, !middleRing, 8)}
          ${middleScopeRow(middleScope, !middleRing || middleStyle === "guide_hud", 9)}
          ${allLayoutRow(allLayout, !middleRing || middleStyle === "guide_hud" || middleScope !== "all", 9)}
          <!-- THE PILL AND THE SPECIALS SWITCH STAY ADJACENT. Double is the
               state in which the specials switch has nothing to do, and a
               reason a control is greyed out has to be visible FROM that
               control — put another row between these two and it stops
               being. (The old three-row chain this replaced needed the same
               rule for two dependencies; there is only one left.) -->
          ${ringRow(ring, 10)}
          ${specialsRow(hudSpecials, specialsInert, 11)}
          ${toggleRow("flight", "Guide-to-toast motion", flight, 12)}
          ${sliderRow("huddelay", "Guide HUD delay", appConfig.guide_hud_delay_ms, 100, 1000, 50, "ms")}
        </div>
      </div>
    </div>

    <!-- .set-section-span: in the expanded panel this section spans both
         columns of .set-cols (its tiles are a wrapping grid of fixed-width
         cards, which wants the whole row). -->
    <div class="set-section set-filterable set-section-span">
      <div class="divider" style="margin:14px 0 10px;"></div>
      <button type="button" class="set-title set-row-label" data-desc="appexceptions"
              aria-expanded="false" style="font-size:13px; margin-bottom:8px;">App exceptions</button>
      ${descBox("appexceptions")}
      <!-- THE SUMMARY ROW (owner, 1.0.110 review): in the popover this
           section is ONE line — the counts and a "Show" button that opens
           the full-screen panel scrolled to the section — and nothing else
           of it renders; the side scroll is about the app's own features.
           Both rows are always in the markup; .set-summary / .set-full are
           shown or hidden by the panel's .expanded class (styles.css), so
           expanding never rebuilds the tiles or closes an open picker. The
           text is written by renderAppExceptions(), which owns the counts.
           NOTE: no backticks in this comment (template literal). -->
      <div class="set-summary" id="set-exc-summary">
        <span class="set-summary-text" id="set-exc-summary-text"></span>
        <button type="button" class="btn btn-sm set-summary-show" data-show-section="set-app-exceptions">Show</button>
      </div>
      <div id="set-app-exceptions" class="set-full"></div>
    </div>

    <div class="set-section set-filterable set-section-span">
      <div class="divider" style="margin:14px 0 10px;"></div>
      <button type="button" class="set-title set-row-label" data-desc="conflicts"
              aria-expanded="false" style="font-size:13px; margin-bottom:8px;">Conflicts</button>
      ${descBox("conflicts")}
      <!-- Same summary/full pair as App exceptions above; the count is
           written by renderConflicts()'s draw(). The hook-health block and
           the ring tool live inside .set-full, so in the popover they are
           reached through "Show" like the rest of the section. -->
      <div class="set-summary" id="set-conflicts-summary">
        <span class="set-summary-text" id="set-conflicts-summary-text"></span>
        <button type="button" class="btn btn-sm set-summary-show" data-show-section="set-conflicts">Show</button>
      </div>
      <div id="set-conflicts" class="set-full"></div>
    </div>

    <!-- THE ACTION BUTTONS, 1.0.97 (owner: "unnaturally long pills, stretched
         for no reason").
         They used to be a stack of full-width pills — two side-by-side on a
         flex row, then two more at width:100% — which is what made a 1200px
         window draw a 1100px "Open log folder". They are now two named groups
         with the same small uppercase headings the settings groups carry, and
         the buttons inside are sized to their own text in a responsive grid.
         The GROUPING is the part that does the real work: pressing "Re-check
         now" and pressing "Clear all" had exactly the same weight on screen,
         and one of them empties a profile. See .set-act-* in styles.css. -->
    <div class="set-section set-filterable">
      <div class="divider" style="margin:14px 0 10px;"></div>

      <div class="set-act-group">
        ${groupHeadingHtml("maintenance", "Maintenance")}
        <div class="set-act-grid">
          <!-- MOVED HERE from the foot of the Conflicts list, where it was
               the third full-width pill in a row of them. It re-runs the
               conflict scan; it is NOT the same action as "Check the ring"
               in the Conflicts area, which runs run_overlay_fix against
               the graphics driver. Both are kept, neither is duplicated. -->
          <button class="btn" id="set-recheck">Re-check now</button>
          <button class="btn" id="set-logs">Open log folder</button>
        </div>
        ${descBox("logs")}
      </div>

      <div class="set-act-group is-danger">
        ${groupHeadingHtml("danger", "Danger zone")}
        <div class="set-act-grid">
          <button class="btn${_armed === "def" ? " is-armed" : ""}" id="set-reset">${_armed === "def" ? "Confirm" : resetLabel()}</button>
          <button class="btn${_armed === "clr" ? " is-armed" : ""}" id="set-clear">${_armed === "clr" ? "Confirm clear" : "Clear all"}</button>
          <button class="btn" id="set-presets">Restore preset profiles</button>
        </div>
        ${descBox("reset")}
        ${descBox("clear")}
        ${descBox("presets")}
      </div>

      <!-- The action buttons cannot BE their own description trigger: their
           press already does something destructive or irreversible. Their
           descriptions ride the header link's convoy and the ⓘ row below
           instead, which is why they have a box but no data-desc label. -->
      <button type="button" class="set-help-all sma-note" id="set-help-all">
        ⓘ What do these buttons do?
      </button>
    </div>

    <!-- PROBLEM 195 — the crash-reporting opt-out, at the VERY BOTTOM of the
         panel by the owner's instruction: below every switch, every slider and
         every button. It is the only control here that concerns what leaves
         the machine, so it gets its own group and its own space rather than
         sitting in the convoy of ordinary preferences. -->
    <div class="set-group">
      <div class="divider" style="margin:14px 0 10px;"></div>
      ${groupHeadingHtml("privacy", "Privacy")}
      <div class="set-rows">
        ${toggleRow("sendlogs", "Don't send logs", dontSendLogs, 13)}
      </div>
    </div>

    <!-- ABOUT (feature 1) — after Privacy, before nothing: the last thing in
         the panel, by the owner's placement. Reference information (version,
         install kind, links, the third-party list), not a preference, so it
         gets its own group rather than joining the convoy of switches above
         it. aboutRowHtml() is the SAME leaf function preview.ts renders —
         see controls.ts for why that matters (PROBLEM 148). -->
    <div class="set-group">
      <div class="divider" style="margin:14px 0 10px;"></div>
      ${groupHeadingHtml("about", "About")}
      <div class="set-rows">
        <div class="set-item set-filterable" id="set-about">
          ${aboutRowHtml(_aboutInfo, THIRD_PARTY.length, _thirdPartyOpen, _updateStatusText, _rollback?.version ?? null)}
        </div>
      </div>
    </div>

    </div><!-- /.set-cols -->
    </div><!-- /.set-scroll -->
  `;

  // One render, one animation. Anything after this point sees a clean slate,
  // so a later render (a toast, a conflict re-check) cannot replay a character
  // the user pressed minutes ago.
  _flipped = null;

  // PROBLEM 267 — the two middle-button rows' inert state, painted through
  // the SAME `paintInert` the specials row uses (opacity, pointer-events,
  // `disabled` on every control, the reason note shown). Done here, after the
  // markup exists, so a first render and a later flip are one code path.
  paintRow("set-middlestyle-wrap", "set-middlestyle-note", !middleRing);
  paintRow("set-middlescope-wrap", "set-middlescope-note", !middleRing || middleStyle === "guide_hud");
  paintRow(
    "set-alllayout-wrap",
    "set-alllayout-note",
    !middleRing || middleStyle === "guide_hud" || middleScope !== "all",
  );

  // THE SEARCH BOX, restored to whatever the user had typed before this
  // render. `render()` re-runs after every toggle, so without this a filtered
  // panel would clear itself the moment the user flipped the switch they had
  // just searched for. The filter is re-applied in the SAME synchronous task
  // as the innerHTML above, so the browser never paints the unfiltered list.
  wireSearch();

  // The expand button. `_expanded` survives a render (module scope), so the
  // class has to be re-applied here — innerHTML replaced the header, not the
  // panel, but `setPanelExpanded` also stamps the new #set-expand button's
  // aria state, which the fresh markup cannot know.
  setPanelExpanded(panelEl, _expanded);
  panelEl.querySelector("#set-expand")?.addEventListener("click", (e) => {
    e.stopPropagation();
    toggleExpanded(!_expanded);
  });
  // The summary rows' "Show" buttons (owner, 1.0.110 review): expand the
  // panel AND land on the section. The scroll is deferred one frame so it
  // runs against the expanded layout — `toggleExpanded` flips the class
  // synchronously, but `.set-scroll` only becomes the scroller under it, and
  // scrolling a box that is still `display: contents` scrolls nothing.
  panelEl.querySelectorAll<HTMLElement>(".set-summary-show").forEach((b) => {
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      sfx.tick();
      const target = panelEl?.querySelector<HTMLElement>(`#${b.dataset.showSection}`)?.closest<HTMLElement>(".set-section");
      toggleExpanded(true);
      requestAnimationFrame(() => {
        target?.scrollIntoView({
          block: "start",
          behavior: document.documentElement.classList.contains("reduced-motion") ? "auto" : "smooth",
        });
      });
    });
  });

  wireToggle("engine", async () => {
    try {
      const paused = await invoke<boolean>("toggle_bypass");
      _paused = paused;
      // Engine ON is the thruster ignition; OFF is a plain flip down.
      if (paused) sfx.toggleOff("engine"); else sfx.toggleOn("engine");
      showToast(paused ? "⏸️ Engine paused" : "▶️ Engine active");
    } catch (_) {
      showToast("⚠️ Could not toggle the engine");
    }
    render();
  });

  // PROBLEM 144 — the 3-way theme pill replaced the Dark mode switch.
  panelEl?.querySelectorAll<HTMLElement>("[data-theme-set]").forEach((b) => {
    b.addEventListener("click", async () => {
      if (!appConfig) return;
      const next = b.dataset.themeSet ?? "earthy";
      if (next === appConfig.theme) return;
      appConfig.theme = next;
      // FEATURE 2 — "auto" is stored literally (schema.rs keeps it, never
      // resolves it away), but every consumer below this line — the theme
      // CHORD, and `dark_mode` for the overlay — needs one of the three real
      // palettes. `resolveTheme` is the one place that decision is made, so
      // the pill and `applyLook()` can never disagree about what "auto" means
      // right now.
      //
      // REVIEW FIXES 2026-09-05 (H4) — THIS LINE IS WHY THE SPLIT PALETTE WAS
      // MORE THAN COSMETIC. `dark_mode` is PERSISTED from this resolution two
      // lines down, and `resolveTheme` used to answer from this webview's
      // `prefers-color-scheme` — which `tauri.conf.json` had pinned to Light.
      // So picking "Auto" on a dark machine wrote `dark_mode: false` into
      // config.json, and Rust and the overlay were then handed the dashboard's
      // wrong reading as fact. It resolves against Rust's own registry read
      // now (theme-resolve.ts ← os-theme.ts ← theme_watch.rs), so what gets
      // written is what Windows actually says.
      const resolved = resolveTheme(next);
      // sounds.js names the middle theme "war", not "warcry".
      sfx.theme(resolved === "warcry" ? "war" : resolved);
      // dark_mode stays the single source of truth for body.nocturne on BOTH
      // windows — the overlay has no idea themes exist (CLAUDE.md theme rule).
      appConfig.dark_mode = resolved !== "earthy";
      applyLook();

      // PROBLEM 157 — update the pill IN PLACE. render() rebuilds the panel,
      // which DESTROYS the indicator and creates a new one already at the new
      // position — and a brand-new element has nothing to transition FROM.
      // That is the whole reason the owner reported "the satisfying animation
      // of the slider sliding smoothly is not there anymore": the CSS never
      // stopped being correct, the element just stopped surviving the change.
      // There are TWO pills in this panel now, so the theme pill can no longer
      // be found as "the .theme-seg". Scope to the one this button lives in.
      const seg = b.closest<HTMLElement>(".theme-seg");
      seg?.querySelector<HTMLElement>(".theme-seg-ind")?.setAttribute("data-seg", next);
      seg?.querySelectorAll<HTMLElement>("[data-theme-set]").forEach((o) => {
        const on = o.dataset.themeSet === next;
        o.classList.toggle("is-on", on);
        o.setAttribute("aria-checked", String(on));
      });
      // PROBLEM 255 follow-up — `.is-on` just moved to a DIFFERENT-WIDTH
      // button ("Auto" and "Starry night" are not the same size), so the
      // indicator's rect has to be re-measured, not just re-indexed.
      if (seg) positionSegIndicator(seg);
      await persistConfig();
      // The ring wears the app's palette, so a theme changed while a
      // projection is up leaves it in the OLD one until it hides.
      refireRingPreview();
    });
  });

  // THE SPACE RING PILL — Compact · Wide · Double. Same in-place update as the
  // theme pill above and for the same reason (PROBLEM 157): render() would
  // destroy the indicator and build a new one already at the destination,
  // which has nothing to transition FROM, and the owner noticed the moment
  // that happened. So this handler moves the pill by hand and then repaints
  // ONLY the row whose meaning changed.
  //
  // The whole feature lives in the OVERLAY page (band count falls out of
  // measured label widths) plus one deterministic gate in Rust, and both learn
  // about the change through config::save — so persistConfig() is the entire
  // backend wiring. Same shape as "flight" and "hudpointer".
  panelEl?.querySelectorAll<HTMLElement>("[data-hudring-set]").forEach((b) => {
    b.addEventListener("click", async () => {
      if (!appConfig) return;
      const next = (b.dataset.hudringSet ?? "compact") as RingLayout;
      const { magnetic, band } = ringConfigFor(next);

      // PRESSING THE OPTION YOU ARE ALREADY ON IS NOT ALWAYS A NO-OP HERE, and
      // that is the difference from the theme pill directly above.
      //
      // `hud_band_count: "one"` is retired: the UI never writes it again, but
      // a config written between 1.0.89 and 1.0.95 can still hold it, and such
      // a config shows COMPACT (forced-one and auto are indistinguishable
      // whenever the labels fit). A bare `if (next === current) return;` would
      // make Compact permanently unpressable for exactly those users and leave
      // the retired value in their file forever. So the guard compares the
      // CONFIG this press would write, not the label it would light up.
      const settled = appConfig.hud_magnetic_layout === magnetic
        && (band === null || appConfig.hud_band_count === band);
      if (settled) return;

      appConfig.hud_magnetic_layout = magnetic;
      // `null` means "leave the row count alone" — Wide has no opinion about
      // it, so a detour through Wide and back returns Double intact.
      if (band !== null) appConfig.hud_band_count = band;

      // No dedicated sound in sounds.js for this pill, and sfx.theme() is the
      // THEME's chord — playing it here would tell the user the look changed.
      // The switch-family tick is the honest one: this is a preference row.
      sfx.toggleOn("hudring");

      const seg = b.closest<HTMLElement>(".theme-seg");
      seg?.querySelector<HTMLElement>(".theme-seg-ind")?.setAttribute("data-seg", next);
      seg?.querySelectorAll<HTMLElement>("[data-hudring-set]").forEach((o) => {
        const on = o.dataset.hudringSet === next;
        o.classList.toggle("is-on", on);
        o.setAttribute("aria-checked", String(on));
      });
      // PROBLEM 255 follow-up — same reasoning as the theme pill above: the
      // three Ring-layout labels are not equal width either, so the indicator
      // has to be re-measured on every selection, not re-indexed.
      if (seg) positionSegIndicator(seg);

      // THE VISIBLE HALF OF THE DEPENDENCY. Under Double both rings belong to
      // apps, so the specials switch has nothing to do — and a control that
      // does nothing is worse than a missing one, so it greys out and says why
      // in the same gesture.
      paintSpecialsInert(next === "double");
      // LIVE PREVIEW — show the shape being chosen, in the real overlay.
      // Fire-and-forget and fully optional: `previewRing` swallows everything,
      // so a build where the command does not exist yet still gets a pill that
      // saves correctly. A settings panel must never break because a preview
      // could not be drawn.
      void previewRing(next);
      await persistConfig();
    });
  });

  // "What do these do?" — the header link, which inherited the removed "Show
  // me around" row's entire behaviour (owner, 2026-09-01). Same config field,
  // same convoy, same two sounds; only the control changed shape. The field is
  // still written because it does two jobs: it is what `body.show-around`
  // gates every `.sma-note` on, and it is what makes the convoy re-open by
  // itself the next time the panel is opened.
  panelEl.querySelector("#set-help-descs")?.addEventListener("click", async (e) => {
    e.stopPropagation();
    if (!appConfig) return;
    const on = !(appConfig.show_me_around === true);
    appConfig.show_me_around = on;
    document.body.classList.toggle("show-around", on);
    (e.currentTarget as HTMLElement).setAttribute("aria-pressed", String(on));
    convoyAll(on);
    if (on) sfx.convoyOn(); else sfx.convoyOff();
    await persistConfig();
    // deliberately NOT render() — a re-render would wipe the convoy mid-flight
  });

  // PROBLEM 242 — the second header entry, sitting beside "What do these do?"
  // because they answer the same question at two different sizes: one explains
  // the switches you are looking at, the other explains the app you are
  // holding. It does NOT touch `show_me_around` and does not disturb the
  // convoy — the handler above is unchanged.
  //
  // It restarts at STEP 1 regardless of `tour_done`: someone who asks for the
  // walkthrough wants the doing part, not the pitch they have already read.
  // And it closes the panel first, because step 1 dims everything except the
  // keyboard and a lit settings popover sitting over it would be the one thing
  // the user is told not to look at.
  panelEl.querySelector("#set-help-tour")?.addEventListener("click", (e) => {
    e.stopPropagation();
    closeSettingsPanel();
    startTour();
  });

  wireToggle("fun", async () => {
    if (!appConfig) return;
    appConfig.fun_mode = !(appConfig.fun_mode === true);
    // toggleOn/Off("fun") is special-cased inside sounds.js: it plays the
    // genie / wind-down in EITHER gate state, so the switch that controls the
    // personality layer is never the one switch you cannot hear.
    if (appConfig.fun_mode) sfx.toggleOn("fun"); else sfx.toggleOff("fun");
    applyLook();          // the living sky exists only while fun is on
    await persistConfig();
    render();
  });

  // PHASE A — Advanced mode: shows "Run command" and the full catalogue in
  // the key editor. UI-only, so persistConfig() is the whole wiring; the key
  // editor reads `advanced_mode` off the config it is handed on every open.
  wireToggle("advanced", async () => {
    if (!appConfig) return;
    appConfig.advanced_mode = !(appConfig.advanced_mode === true);
    if (appConfig.advanced_mode) sfx.toggleOn("advanced"); else sfx.toggleOff("advanced");
    await persistConfig();
    render();
  });

  wireToggle("hideboard", async () => {
    if (!appConfig) return;
    appConfig.hide_keyboard = !(appConfig.hide_keyboard === true);
    // Not a switch sound: this is the one control that clears the whole
    // screen. sounds.js §8a documents spaceRise/spaceFall for exactly this
    // ("big reveals" / "exiting a mode"). They ignore fun() by design, so the
    // gate is ours — in plain mode it stays an ordinary flip.
    const funNow = appConfig.fun_mode === true;
    if (appConfig.hide_keyboard) { if (funNow) sfx.spaceRise(); else sfx.toggleOn("hideboard"); }
    else                         { if (funNow) sfx.spaceFall(); else sfx.toggleOff("hideboard"); }
    applySkyMode(appConfig.hide_keyboard);
    await persistConfig();
    render();
  });

  wireDescriptions();

  // Restore what the user had open BEFORE this render — instantly, no 380ms
  // slide replay: the boxes get their transition suppressed for one frame.
  openDescs.forEach((id) => {
    const box = panelEl?.querySelector<HTMLElement>(`[data-desc-for="${id}"]`);
    if (!box) return;
    // Both, not just the transition: the grid-rows slide lives on the box and
    // the convoy entrance lives on the child, so suppressing one still let the
    // other replay on every re-render.
    const inner = box.querySelector<HTMLElement>(".set-desc-in");
    box.style.transition = "none";
    if (inner) inner.style.animation = "none";
    setDescOpen(id, true);
    requestAnimationFrame(() => {
      box.style.transition = "";
      if (inner) inner.style.animation = "";
    });
  });

  // Auto-open ONCE per panel opening, not on every render.
  //
  // render() runs again after every toggle, so doing this unconditionally
  // would re-open any description the user had just closed by hand — the
  // setting would quietly fight them, which is the sort of thing that reads as
  // a bug rather than a feature.
  if (_freshOpen && appConfig?.show_me_around === true) convoyAll(true);
  _freshOpen = false;

  wireToggle("sound", async () => {
    if (!appConfig) return;
    appConfig.sound_enabled = !appConfig.sound_enabled;
    applySound(appConfig.sound_enabled);
    // toggleOn("sound") forces past the mute gate internally — the switch you
    // just enabled has to confirm itself, and enabled() reads false until the
    // very instant above.
    if (appConfig.sound_enabled) sfx.toggleOn("sound"); else sfx.toggleOff("sound");
    await persistConfig();
    render();
  });

  wireToggle("startup", async () => {
    if (!appConfig) return;

    // REVIEW FIXES 2026-09-05 (H6) — ASK BEFORE WRITING, IF NOBODY HAS ASKED
    // YET.
    //
    // `refreshPackagedStartup()` runs when the panel OPENS, so between that
    // open and its answer arriving `_pkgStartup` is null and the row is fully
    // live — `startupRowIsInert(null)` is false, and it has to be, because a
    // row that greys itself for a second on every open would look broken. But
    // "live" must mean "clickable", not "writable": a press landing in that
    // window used to go straight through to `set_startup_enabled` on a copy
    // that may own nothing at all.
    //
    // One awaited question closes it. It is the same call the panel makes on
    // open, so on the ordinary path (the answer already in hand) this costs
    // nothing and does not run.
    if (_pkgStartup === null) await refreshPackagedStartup();

    // And if the answer is "you do not own this", stop here. `paintInert` has
    // already disabled the input and taken it out of the tab order, so
    // reaching this line means something got past that — a stale DOM node, a
    // synthesised event, a future keyboard path. The note under the row is
    // already on screen and already says who is holding it; a toast points at
    // it rather than repeating it.
    if (startupRowIsInert(_pkgStartup)) {
      showToast(
        startupIsPortable(_pkgStartup)
          ? "📦 Portable copy — see the note under the switch"
          : "🔒 Windows decides this one — see the note under the switch",
      );
      paintPackagedStartup();
      return;
    }

    // PROBLEM 250 — flip away from what is SHOWN, not from what config says.
    // For a packaged install those can differ (the user changed it in Task
    // Manager while the panel was shut), and flipping from a stale config value
    // would make the first press appear to do nothing.
    const next = !startupShownAsOn(appConfig.run_at_startup !== false);
    appConfig.run_at_startup = next;
    try {
      // ONE command persists config AND applies it — the Scheduled Task / Run
      // key for a normal install, the package's startupTask for a Store one
      // (startup.rs branches). Not persistConfig(): that path doesn't apply it.
      await invoke("set_startup_enabled", { enabled: next });

      // PROBLEM 250 — Windows is allowed to say no. `RequestEnableAsync` will
      // not override a user who switched Spaceadom off in Task Manager, so a
      // packaged install has to be ASKED what actually happened rather than
      // told. Re-reading also repaints the row inert with the reason, which is
      // the whole difference between "the switch bounced back" and "Windows is
      // holding this off, and here is where to change it".
      await refreshPackagedStartup();

      // REVIEW FIXES 2026-09-05 (H6) — THREE OUTCOMES, NOT TWO, AND A
      // COMMAND THAT RETURNED `Ok` IS NOT ONE OF THEM ON ITS OWN.
      //
      // This used to be a two-way `refused` bool computed from
      // `_pkgStartup?.packaged`, which is `true` for a PORTABLE copy —
      // `get_packaged_startup` answers `(true, "portable", false, …)`. So the
      // portable case fell into `refused` when switching ON ("Windows decides
      // this one", about a copy Windows has no opinion on) and into SUCCESS
      // when switching OFF ("Won't start with Windows"), announcing a
      // completed action for a write that never happened:
      // `startup.rs::apply_task_enabled` returns early for a portable copy
      // having touched no Run key and no Scheduled Task, and
      // `set_startup_enabled` still returns `Ok(())`.
      //
      // `startupOutcome` (controls.ts) separates the two, and the config is
      // put back for both non-writes — a stored `run_at_startup: true` on a
      // copy that can never honour it is a preference the app would keep
      // showing and never keep.
      switch (startupOutcome(_pkgStartup, next)) {
        case "nothing-written":
          appConfig.run_at_startup = !next;
          // NO `sfx.toggleOn/Off` either: the switch sound is the app saying
          // "done", and nothing was done.
          showToast("📦 Portable copy — put a shortcut in your Startup folder");
          break;
        case "refused":
          appConfig.run_at_startup = !next;
          showToast("🔒 Windows decides this one — see the note under the switch");
          break;
        case "written":
          if (next) sfx.toggleOn("startup"); else sfx.toggleOff("startup");
          showToast(next ? "🚀 Starts with Windows" : "🚀 Won't start with Windows");
          break;
      }
    } catch (_) {
      appConfig.run_at_startup = !next;   // revert on failure
      showToast("⚠️ Could not change the startup task");
    }
    render();
    // render() rebuilt the row from scratch, so the inert treatment and the
    // note have to be re-applied to the NEW element. Cheap and synchronous —
    // `_pkgStartup` is already in hand.
    paintPackagedStartup();
  });

  wireToggle("motion", async () => {
    if (!appConfig) return;
    const nowReduced = document.documentElement.classList.contains("reduced-motion");
    // Flipping writes an EXPLICIT choice, never "auto" — the user has just
    // told us what they want, so stop deferring to Windows for this app.
    appConfig.motion = nowReduced ? "full" : "reduced";
    // Before applyMotion: turning effects DOWN still gets to announce itself.
    if (nowReduced) sfx.toggleOn("motion"); else sfx.toggleOff("motion");
    applyMotion(appConfig.motion);
    await persistConfig();
    showToast(nowReduced ? "✨ Visual effects on" : "🪶 Visual effects reduced");
    render();
  });

  // PROBLEM 174 — the flight lives in the OVERLAY page, which cannot see this
  // config object. persistConfig() saves, and save_config re-emits
  // "flight-changed" from Rust so the overlay learns about it; that global
  // emit is the only arrangement that has ever delivered here (the same rule
  // the theme follows). Nothing to apply locally: the dashboard's own toasts
  // never fly, because they are not in the overlay window.
  wireToggle("flight", async () => {
    if (!appConfig) return;
    appConfig.hud_toast_flight = !appConfig.hud_toast_flight;
    if (appConfig.hud_toast_flight) sfx.toggleOn("flight"); else sfx.toggleOff("flight");
    await persistConfig();
    render();
  });

  // PROBLEM 206 — pointer activation lives entirely in Rust (the mouse hook
  // plus the st-hud-pointer poller), which learns about the change through
  // config::save's publish — so persistConfig() is enough: no dedicated
  // command, and nothing to apply locally. Same shape as "flight" above.
  wireToggle("hudpointer", async () => {
    if (!appConfig) return;
    // `!== false`, matching the read in render() — PROBLEM 209 flipped the
    // default ON, so an ABSENT key is ON and the first click must turn it
    // OFF. `=== true` here would make that first click a no-op.
    appConfig.pointer_hud_activation = !(appConfig.pointer_hud_activation !== false);
    if (appConfig.pointer_hud_activation) sfx.toggleOn("hudpointer");
    else sfx.toggleOff("hudpointer");
    await persistConfig();
    refireRingPreview();
    render();
  });

  // PROBLEM 267 — "Middle button shows": Icon ring / Space ring. Same in-place
  // pill update as the theme and ring pills above (PROBLEM 157 — a re-render
  // would destroy the indicator mid-slide). Rust reads the field on the
  // engine thread at the next middle press (`engine::routed_middle_event`),
  // so persistConfig() is the whole wiring. No preview: this changes WHICH
  // ring opens, not what one looks like.
  panelEl?.querySelectorAll<HTMLElement>("[data-middlestyle-set]").forEach((b) => {
    b.addEventListener("click", async () => {
      if (!appConfig) return;
      const next = middleStyleFor(b.dataset.middlestyleSet) as MiddleStyle;
      if (next === middleStyleFor(appConfig.middle_ring_style)) return;
      appConfig.middle_ring_style = next;
      sfx.toggleOn("middlestyle");
      moveSeg(b, "middlestyleSet", next);
      // The scope pill only means something for the icon ring, and the
      // All-layout pill under it only for the icon ring showing All.
      paintRow("set-middlescope-wrap", "set-middlescope-note", next === "guide_hud");
      paintRow(
        "set-alllayout-wrap",
        "set-alllayout-note",
        next === "guide_hud" || middleScopeFor(appConfig.middle_ring_scope) !== "all",
      );
      await persistConfig();
    });
  });

  // PROBLEM 267 — "Middle-button ring shows": Favourites / All. Read by
  // Rust when it builds the ring's entries (`middle_ring::build_entries`);
  // persistConfig() is the wiring, as above.
  panelEl?.querySelectorAll<HTMLElement>("[data-middlescope-set]").forEach((b) => {
    b.addEventListener("click", async () => {
      if (!appConfig) return;
      const next = middleScopeFor(b.dataset.middlescopeSet) as MiddleScope;
      if (next === middleScopeFor(appConfig.middle_ring_scope)) return;
      appConfig.middle_ring_scope = next;
      sfx.toggleOn("middlescope");
      moveSeg(b, "middlescopeSet", next);
      // The All-layout pill only means something under "All" — greyed from
      // the control that greys it, with no rebuild, exactly as above.
      paintRow("set-alllayout-wrap", "set-alllayout-note", next !== "all");
      await persistConfig();
    });
  });

  // 2026-09-15 — "All layout": Rings / Spiral. Read by Rust when it lays the
  // ring out (`middle_ring::spiral_slots` vs `layout_ring_slots`); the pill
  // itself does nothing but persist the choice, like the two above it.
  panelEl?.querySelectorAll<HTMLElement>("[data-alllayout-set]").forEach((b) => {
    b.addEventListener("click", async () => {
      if (!appConfig) return;
      const next = allLayoutFor(b.dataset.alllayoutSet) as AllLayout;
      if (next === allLayoutFor(appConfig.all_ring_layout)) return;
      appConfig.all_ring_layout = next;
      sfx.toggleOn("alllayout");
      moveSeg(b, "alllayoutSet", next);
      await persistConfig();
    });
  });

  // PROBLEM 267 — "Choose your favourites →" opens the picker under its own row
  // (artboard 9). Drawn into its own container so a pick never re-renders the
  // panel and slams the list shut.
  panelEl?.querySelector<HTMLElement>("#set-eight-open")?.addEventListener("click", (e) => {
    e.stopPropagation();
    sfx.tick();
    if (_eightOpen) { closeEightPicker(); return; }
    _eightOpen = true;
    renderEightPicker();
  });
  renderEightPicker();

  // PROBLEM 263 — the middle-button ring trigger. Same shape as "hudpointer"
  // directly above, and for the same reason: the whole feature lives in Rust
  // (the WM_MBUTTONDOWN branch of the mouse callback, gated on an atomic
  // `hook::MIDDLE_BUTTON_RING` that `config::save` republishes), so
  // persistConfig() is the entire wiring. No dedicated command, nothing to
  // apply locally.
  wireToggle("middlering", async () => {
    if (!appConfig) return;
    // `!== false`, matching the read in render(). Rust ships this
    // `default = "default_true"`, so an ABSENT key is ON and the first click
    // must turn it OFF — `=== true` here would make that first click a no-op
    // for every user who has ever run an older build, which is all of them.
    appConfig.middle_button_ring = !(appConfig.middle_button_ring !== false);
    if (appConfig.middle_button_ring) sfx.toggleOn("middlering");
    else sfx.toggleOff("middlering");
    await persistConfig();
    // NO `refireRingPreview()` HERE, deliberately, and this is the one line
    // that differs from the handler above. The preview shows what the ring
    // LOOKS like; this setting changes how it is OPENED, and re-firing a
    // preview the switch cannot alter would tell the user their change did
    // something to the picture. `flight`'s handler omits it for the same
    // reason. See `refireRingPreview`'s own comment for what does belong.
    render();
  });

  // 2026-08-27's "New ring layout" SWITCH USED TO BE WIRED HERE. It is gone;
  // `hud_magnetic_layout` is now one of the two fields the Compact/Wide/Double
  // pill writes (see the `[data-hudring-set]` handler above). The config field
  // itself is untouched, so `components/hud-layout.ts` in the overlay and the
  // `hud-layout-changed` event `save_config` emits both keep working exactly
  // as they did — only the control that writes it changed shape.

  // PROBLEM 209 — show the specials on the HUD's inner ring. Same shape as
  // "hudpointer" above and for the same reason: the whole feature is a
  // config field Rust reads when it builds the HUD payload (engine/mod.rs
  // sends an empty specials list when this is off), so persistConfig() is
  // the entire wiring. The special KEYS keep working either way.
  wireToggle("hudspecials", async () => {
    if (!appConfig) return;
    // The belt to `specialsRow`'s braces. The input is `disabled` and its
    // wrapper is `pointer-events:none` under Double, so this should be
    // unreachable — but "should be unreachable" is how a control that does
    // nothing gets shipped, and the cost of the guard is one comparison.
    // Asked through the SAME pure mapping the row is painted from, so the
    // guard cannot drift from the greying.
    if (ringLayoutFor(appConfig.hud_magnetic_layout, appConfig.hud_band_count) === "double") return;
    appConfig.hud_show_specials = !(appConfig.hud_show_specials !== false);
    if (appConfig.hud_show_specials) sfx.toggleOn("hudspecials");
    else sfx.toggleOff("hudspecials");
    await persistConfig();
    // AFTER persistConfig, never before: `preview_hud_layout` builds its
    // payload from ConfigState, so a re-fire sent first would project the old
    // specials list and look like the bug it exists to fix.
    refireRingPreview();
    render();
  });

  // PROBLEM 195 — the crash-reporting opt-out.
  //
  // THE NEGATION, ONE MORE TIME, because this is where it is easiest to get
  // wrong: the switch says "Don't send logs", so switch ON means send_logs
  // FALSE. `nextDontSend` is what the user just asked for; the command gets
  // its opposite.
  //
  // Its OWN command, not persistConfig(): the running app checks an atomic on
  // every log line and inside the panic hook, and set_send_logs is what flips
  // it. That is what makes the switch take effect on the next error rather
  // than at the next launch.
  wireToggle("sendlogs", async () => {
    if (!appConfig) return;
    const nextDontSend = !(appConfig.send_logs === false);
    const nextSendLogs = !nextDontSend;
    try {
      await invoke("set_send_logs", { sendLogs: nextSendLogs });
      appConfig.send_logs = nextSendLogs;
      if (nextDontSend) sfx.toggleOn("sendlogs"); else sfx.toggleOff("sendlogs");
      showToast(
        nextDontSend
          ? "🔒 Nothing will leave this machine"
          : "📮 Crash reports will be sent",
      );
    } catch (e) {
      console.error("set_send_logs failed:", e);
      showToast("⚠️ Could not change the log setting");
    }
    render();
  });

  renderAppExceptions();
  renderConflicts();
  wireAboutSection();
  // ACCESSIBILITY PASS (feature 3) — arrow-key navigation for every segmented
  // pill in the panel (Theme, Ring layout). One call for the whole subtree,
  // safe on every render for the same reason `wireDescriptions` is: render()
  // just rebuilt the DOM these listeners attach to.
  wireSegRowsKeyboard(panelEl);
  // PROBLEM 255 follow-up — measures the Theme and Ring-layout pills' active
  // segment and writes `--ind-x`/`--ind-w` (styles.css) so the indicator
  // matches the real label rect instead of an assumed equal fraction. Safe on
  // every render for the same reason as the call above: it re-finds `.theme-
  // seg` fresh and re-arms its own ResizeObserver rather than accumulating one
  // per render.
  wireSegIndicators(panelEl);

  // PROBLEM 109 — the way back from deleting a preset. Additive: it restores
  // only the presets that are MISSING and never overwrites one the user still
  // has, so it is safe to press even after months of customisation.
  panelEl.querySelector("#set-presets")!.addEventListener("click", async (e) => {
    e.stopPropagation();                       // #stage closes popovers (PROBLEM 98)
    try {
      const restored = await invoke<string[]>("restore_preset_profiles");
      sfx.confirm();
      showToast(
        restored.length === 0
          ? "✓ All preset profiles are already here"
          : `↺ Restored ${restored.join(", ")}`,
      );
    } catch (_) {
      showToast("⚠️ Could not restore the presets");
    }
  });

  // The four action buttons explain themselves through this one row, because
  // pressing THEM is already an action (reset, clear, restore, open folder) —
  // a destructive control must never double as its own help trigger.
  panelEl.querySelector("#set-help-all")?.addEventListener("click", (e) => {
    e.stopPropagation();
    const ids = ["reset", "clear", "presets", "logs"];
    const opening = !isDescOpen("reset");
    if (opening) sfx.bloomOpen(); else sfx.bloomClose();
    ids.forEach((id, i) => window.setTimeout(() => setDescOpen(id, opening), i * CONVOY_STAGGER_MS));
  });

  panelEl.querySelector("#set-logs")!.addEventListener("click", () => {
    sfx.tick();
    void invoke("open_log_folder").catch(() => showToast("⚠️ Could not open the log folder"));
  });

  // MAINTENANCE — "Re-check now". Same body it had at the foot of the
  // Conflicts list; only its home changed. The label is restored in a
  // `finally` so a `refreshConflicts` that throws cannot leave the button
  // reading "Checking…" for the rest of the session.
  const recheck = panelEl.querySelector<HTMLButtonElement>("#set-recheck");
  recheck?.addEventListener("click", async (e) => {
    e.stopPropagation();
    sfx.tick();
    recheck.disabled = true;
    recheck.textContent = "Checking…";
    try {
      await refreshConflicts();
      sfx.confirm();
      _redrawConflicts?.();
    } finally {
      recheck.disabled = false;
      recheck.textContent = "Re-check now";
    }
  });

  wireTypingSpeed();
  wireSlider("huddelay", (v) => { if (appConfig) appConfig.guide_hud_delay_ms = v; });
  wireSlider("opacity",  (v) => { if (appConfig) appConfig.opacity_floor_pct = v; });

  // Destructive actions arm on the first click and fire on the second —
  // a window.confirm() dialog over this stage looks like a different app.
  panelEl.querySelector("#set-reset")!.addEventListener("click", () => {
    if (_armed !== "def") { arm("def"); sfx.arm(); return; }
    disarm();
    sfx.confirm();
    _onResetDefaults?.();
  });
  panelEl.querySelector("#set-clear")!.addEventListener("click", () => {
    if (_armed !== "clr") { arm("clr"); sfx.arm(); return; }
    disarm();
    sfx.confirm();
    _onClearAll?.();
  });
}

// ---------------------------------------------------------------------------
// Conflicts section
// ---------------------------------------------------------------------------

/**
 * Lists other keyboard-remapping software that is running.
 *
 * REPORTS ONLY — there is deliberately no "kill it" button. Terminating
 * another running program is not this app's business, a false positive would
 * close something the user wanted, and it is malware behaviour besides. The
 * user is told exactly what to turn off and decides for themselves.
 */
/** The picker’s open state and its search text live at module scope, because
 *  render() replaces the panel’s whole innerHTML and would otherwise slam the
 *  grid shut every time anything else re-rendered. Same reasoning as the
 *  open-descriptions snapshot at the top of render(). */
let _excPickerOpen = false;
let _excQuery = "";

/**
 * PROBLEM 178's picker had to be closed by hand, and the owner reported the
 * same trap a second time, this time for "Add an app": *"Instead of having
 * to press on Done adding — if someone presses another place it shouldn't
 * stay and wait for pressing Done adding. And after a few seconds it should
 * automatically close."* Two ways back now, mirroring profile-editor.ts's
 * new-profile box exactly rather than inventing a third mechanism:
 *   - an outside press (registerDismissable, `dismissable.ts`)
 *   - 12s of no interaction with the picker (re-armed on real use)
 * "Done adding" stays — it is now one way out among several, not the only
 * one, so nobody who already relies on it is stranded.
 */
const EXC_PICKER_IDLE_MS = 12_000;
let _excIdleTimer: number | undefined;
/** Unregisters the picker's dismissable entry. Set while open, cleared on close. */
let _excUnregisterDismiss: (() => void) | null = null;
/**
 * The picker's own container element, and when it was armed. Needed because
 * registerDismissable's document-level "outside press" listener CANNOT see a
 * click that lands inside the settings panel but outside the picker — see
 * `wireExcPanelOutsideClick` below for why, and what covers that gap instead.
 */
let _excPickerWrap: HTMLElement | null = null;
let _excArmedAt = 0;
let _excPanelClickWired = false;

function armExcIdle(): void {
  window.clearTimeout(_excIdleTimer);
  _excIdleTimer = window.setTimeout(closeExcPicker, EXC_PICKER_IDLE_MS);
}

function openExcPicker(): void {
  _excPickerOpen = true;
  _excQuery = "";
  renderAppExceptions();
}

function closeExcPicker(): void {
  if (!_excPickerOpen) return;
  _excPickerOpen = false;
  _excQuery = "";
  window.clearTimeout(_excIdleTimer);
  _excIdleTimer = undefined;
  _excUnregisterDismiss?.();
  _excUnregisterDismiss = null;
  _excPickerWrap = null;
  renderAppExceptions();
}

/**
 * `main.ts` registers `panelEl.addEventListener("click", e =>
 * e.stopPropagation())` at bootstrap (PROBLEM 98 — required so the settings
 * panel survives clicks inside itself). That means a click that lands INSIDE
 * the panel but OUTSIDE the picker (another settings row, a slider, blank
 * space in the box) never reaches `document`, so registerDismissable's
 * document-level listener never fires for it — it only ever sees presses
 * that land outside the whole panel, which never pass through panelEl at
 * all. This second listener, scoped to the panel itself rather than
 * document, closes the gap in between. Wired once; a no-op while the picker
 * is closed.
 */
function wireExcPanelOutsideClick(): void {
  if (_excPanelClickWired || !panelEl) return;
  _excPanelClickWired = true;
  panelEl.addEventListener("click", (e) => {
    if (!_excPickerOpen || !_excPickerWrap) return;
    if (e.timeStamp <= _excArmedAt) return;                 // the click that opened it
    if (_excPickerWrap.contains(e.target as Node)) return;  // handled inside the picker
    closeExcPicker();
  });
}

/** The user's own rows, always lowercase stems, in ONE shape whatever the
 *  config on disk still looks like (PROBLEM 267 — `normaliseExceptions`). */
function excludedList(): ExcRow[] {
  return normaliseExceptions(appConfig?.excluded_apps);
}

async function setExcluded(list: ExcRow[]): Promise<void> {
  if (!appConfig) return;
  const seen = new Set<string>();
  appConfig.excluded_apps = list
    .map((r) => ({ exe: r.exe.toLowerCase(), scope: r.scope }))
    .filter((r) => r.exe && !seen.has(r.exe) && seen.add(r.exe));
  await persistConfig();
  renderAppExceptions();
}

/**
 * PROBLEM 267 — the BUILT-IN rows (SolidWorks, Fusion 360, Blender, …) that
 * the section shows pre-seeded at "Space only" with a "Default" tag, and how
 * many more the full table holds. Asked of Rust once per session; until the
 * answer lands the section draws the user's own rows alone, and repaints.
 */
let _builtin: { rows: Array<[string, string]>; more: number } | null = null;
let _builtinAsked = false;
function ensureBuiltinRows(): void {
  if (_builtinAsked) return;
  _builtinAsked = true;
  invoke<[Array<[string, string]>, number]>("get_builtin_exceptions")
    .then(([rows, more]) => { _builtin = { rows, more }; renderAppExceptions(); })
    .catch(() => { _builtin = { rows: [], more: 0 }; });
}

/** Change one app's scope. A built-in row moved BACK to "Space only" is
 *  removed from the list rather than stored — a built-in is never
 *  duplicated; a changed one is stored (PROBLEM 267). */
async function setScope(stem: string, scope: ExcScope, builtin: boolean): Promise<void> {
  const list = excludedList().filter((r) => r.exe !== stem);
  if (builtin && scope === "space_only") {
    await setExcluded(list);
    return;
  }
  await setExcluded([...list, { exe: stem, scope }]);
}

/** The three-state control on one exception tile, built as DOM (user data
 *  sits beside it) in the SAME `.theme-seg` shape the pills use, so it gets
 *  the measured indicator, the keyboard rules and the theme colours for free. */
function buildScopeSeg(scope: ExcScope, label: string, onPick: (s: ExcScope) => void): HTMLElement {
  const seg = document.createElement("div");
  seg.className = "theme-seg exc-seg";
  seg.setAttribute("role", "radiogroup");
  seg.setAttribute("aria-label", `${label}: what stands down`);
  const ind = document.createElement("span");
  ind.className = "theme-seg-ind";
  ind.setAttribute("data-seg", scope);
  ind.style.background = "var(--st-accent)";
  seg.appendChild(ind);
  EXC_SCOPE_OPTS.forEach(([v, l]) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "theme-seg-opt" + (v === scope ? " is-on" : "");
    b.setAttribute("role", "radio");
    b.setAttribute("aria-checked", String(v === scope));
    b.dataset.excScope = v;
    b.textContent = l;
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      if (v === scope) return;
      sfx.toggleOn("excscope");
      onPick(v as ExcScope);
    });
    seg.appendChild(b);
  });
  wireSegRowsKeyboard(seg);
  return seg;
}

/**
 * "App exceptions" — the apps Spaceadom stands down inside.
 *
 * A WRAPPING GRID of compact tiles (icon + name beneath, side by side), the
 * same visual vocabulary as the app-grid picker below it, rather than one
 * full-width row per app — the owner: *"the apps excepted can be side by
 * side"; a full name on every row "isn't worth it" for the space it costs.
 *
 * Built with createElement, not innerHTML: app names come off this machine and
 * the tile text is the user’s data. It also renders into its OWN container, so
 * adding or removing an entry never triggers the panel-wide render() that
 * would close the picker mid-use.
 */
function renderAppExceptions(): void {
  const box = panelEl?.querySelector<HTMLElement>("#set-app-exceptions");
  if (!box) return;

  // Warm the scan so pressing "Add an app" is not a blank grid — but only
  // once the panel has been opened. See `warmAppsIfOpened` (PROBLEM 205):
  // this function also runs from `render()` during bootstrap, and the scan is
  // ~12s of MAIN-THREAD work. `drawAppGrid` shows "Scanning this device…"
  // if the picker is opened before it lands, so nothing here is left blank.
  warmAppsIfOpened();

  const draw = () => {
    box.innerHTML = "";
    const list = excludedList();
    const known = cachedApps() ?? [];
    // Stem -> the detected app, so a tile can show the real icon and the real
    // display name instead of the bare stem we store.
    const byStem = new Map<string, { name: string; icon: string | null }>();
    known.forEach((a) => {
      const stem = exeStem(a.path);
      if (stem && !byStem.has(stem)) {
        byStem.set(stem, { name: a.name, icon: a.icon_base64 ?? null });
      }
    });

    // PROBLEM 267 — every row is a TILE with a three-state control (artboard
    // 8): the built-in 3D/CAD/design rows first, pre-seeded at "Space only"
    // and tagged "Default" (a user's own row for the same stem overrides the
    // seed and is what the control shows), then the user's own apps.
    ensureBuiltinRows();
    const userByStem = new Map(list.map((r) => [r.exe, r] as const));
    const builtinRows = _builtin?.rows ?? [];
    const builtinStems = new Set(builtinRows.map(([stem]) => stem));
    const ownRows = list.filter((r) => !builtinStems.has(r.exe));

    // The popover's one-line summary ("8 built-in · 1 yours · 71 more"),
    // written from the same three numbers the tiles below are built from so
    // the two can never disagree. textContent — the counts are numbers, but
    // the habit is the point.
    const summary = panelEl?.querySelector<HTMLElement>("#set-exc-summary-text");
    if (summary) {
      const parts: string[] = [];
      if (builtinRows.length) parts.push(`${builtinRows.length} built-in`);
      if (ownRows.length) parts.push(`${ownRows.length} yours`);
      if (_builtin && _builtin.more > 0) parts.push(`${_builtin.more} more`);
      summary.textContent = parts.length ? parts.join(" · ") : "No exceptions yet";
    }

    const intro = document.createElement("div");
    intro.className = "set-note";
    intro.style.marginTop = "0";
    intro.textContent = "Built-in defaults for apps that use the middle button to orbit. Change anytime.";
    box.appendChild(intro);

    const tiles = document.createElement("div");
    tiles.className = "exc-list";

    const makeTile = (
      stem: string,
      label: string,
      scope: ExcScope,
      builtin: boolean,
      i: number,
      removable: boolean,
    ): HTMLElement => {
      const hit = byStem.get(stem);
      const tile = document.createElement("div");
      tile.className = "exc-row";
      tile.dataset.stem = stem;
      tile.title = hit?.name ?? label;

      const head = document.createElement("div");
      head.className = "exc-row-head";
      const disc = document.createElement("span");
      disc.className = "exc-tile-disc";
      paintAppDisc(disc, hit?.icon, hit?.name ?? label, i);
      const name = document.createElement("span");
      name.className = "exc-row-name";
      name.textContent = hit?.name ?? label;   // textContent — user data
      head.append(disc, name);
      if (builtin) {
        const tag = document.createElement("span");
        tag.className = "exc-default-tag";
        tag.textContent = "Default";
        head.appendChild(tag);
      }
      if (removable) {
        const remove = document.createElement("button");
        remove.type = "button";
        remove.className = "exc-tile-x exc-row-x";
        remove.setAttribute("aria-label", `Remove ${label} from exceptions`);
        remove.textContent = "\u2715";
        remove.addEventListener("click", async (e) => {
          e.stopPropagation();
          sfx.tick();
          await setExcluded(excludedList().filter((r) => r.exe !== stem));
        });
        head.appendChild(remove);
      }
      tile.appendChild(head);
      const seg = buildScopeSeg(scope, hit?.name ?? label, (next) => { void setScope(stem, next, builtin); });
      tile.appendChild(seg);
      return tile;
    };

    builtinRows.forEach(([stem, display], i) => {
      const own = userByStem.get(stem);
      tiles.appendChild(makeTile(stem, display, own?.scope ?? "space_only", true, i, false));
    });
    ownRows.forEach((r, i) => {
      tiles.appendChild(makeTile(r.exe, r.exe, r.scope, false, builtinRows.length + i, true));
    });
    box.appendChild(tiles);
    // The pills' indicators are MEASURED after insertion (PROBLEM 255
    // follow-up); a tile built by hand needs the same pass.
    tiles.querySelectorAll<HTMLElement>(".theme-seg").forEach(positionSegIndicator);

    if (_builtin && _builtin.more > 0) {
      const more = document.createElement("div");
      more.className = "set-note";
      more.textContent = `\u2026and ${_builtin.more} more 3D, CAD and design programs are built in at Space only. Add one below to change it.`;
      box.appendChild(more);
    }
    if (ownRows.length === 0 && builtinRows.length === 0) {
      const none = document.createElement("div");
      none.className = "set-note";
      none.style.marginTop = "0";
      none.textContent = "No exceptions yet \u2014 Spaceadom works everywhere.";
      box.appendChild(none);
    }

    // PROBLEM 239 follow-up — this used to be `btn btn-sm` stretched with an
    // inline `width:100%`, which is exactly the "full-width stretched pill"
    // the owner flagged on the Settings review screenshot ("Add an app" is
    // still a full-width stretched pill while everything else in Settings
    // was compacted). `.exc-add-btn` (styles.css) gives it the same
    // content-sized, 13px-radius, secondary-style language as the action
    // grid's `.set-act-inline` buttons — 36px high, `0 16px` padding — and
    // ordinary block flow left-aligns it under the note/tile grid with no
    // centring rule to fight, the same way `.dashed-btn`'s `align-self:
    // flex-start` opted "Import a profile" out of a stretch in PROBLEM 256.
    const add = document.createElement("button");
    add.type = "button";
    add.className = "btn exc-add-btn";
    add.textContent = _excPickerOpen ? "Done adding" : "Add an app";
    add.addEventListener("click", (e) => {
      e.stopPropagation();
      sfx.tick();
      if (_excPickerOpen) closeExcPicker(); else openExcPicker();
    });
    box.appendChild(add);

    if (!_excPickerOpen) return;

    const wrap = document.createElement("div");
    wrap.className = "exc-picker";

    const search = document.createElement("input");
    search.className = "input";
    search.style.marginTop = "8px";
    search.placeholder = "Search apps…";
    search.autocomplete = "off";
    search.spellcheck = false;
    search.value = _excQuery;
    wrap.appendChild(search);

    const label = document.createElement("div");
    label.className = "ed-section";
    label.textContent = "Apps on this device";
    wrap.appendChild(label);

    const scroll = document.createElement("div");
    scroll.className = "ed-grid-scroll";
    const grid = document.createElement("div");
    grid.className = "ed-grid";
    const empty = document.createElement("div");
    empty.className = "ed-empty";
    empty.hidden = true;
    scroll.append(grid, empty);
    wrap.appendChild(scroll);
    box.appendChild(wrap);

    const paint = () => {
      const current = new Set(excludedList().map((r) => r.exe));
      drawAppGrid(
        grid,
        empty,
        {
          query: _excQuery,
          isCurrent: (app) => current.has(exeStem(app.path)),
          onPick: (app) => { armExcIdle(); void addException(app.path, app.name); },
        },
        // A scan that lands after the picker was closed must not repaint a
        // grid that is no longer on screen.
        () => _excPickerOpen && !!panelEl && !panelEl.hidden,
      );
    };

    search.addEventListener("input", () => { _excQuery = search.value; armExcIdle(); paint(); });
    // The panel reacts to stray keys; keep typing inside the box. Still counts
    // as real use of the picker, so it re-arms the idle timer too.
    search.addEventListener("keydown", (e) => { e.stopPropagation(); armExcIdle(); });
    wrap.addEventListener("pointermove", armExcIdle);
    scroll.addEventListener("scroll", armExcIdle, { passive: true });
    paint();

    // Outside-press + Escape (dismissable.ts), and the idle countdown — both
    // (re-)armed fresh on every draw, since the wrap element is rebuilt each
    // time.
    _excUnregisterDismiss = registerDismissable(wrap, closeExcPicker);
    _excPickerWrap = wrap;
    _excArmedAt = performance.now();
    wireExcPanelOutsideClick();
    armExcIdle();
  };

  draw();
}

/** Add one app to the exception list, from whatever path the grid gave us. */
async function addException(path: string, label: string): Promise<void> {
  const stem = exeStem(path);
  if (!stem) return;
  // Excluding the app that DRAWS this panel would be a trap: Spaceadom would
  // stand down whenever its own dashboard had focus, and the setting that
  // caused it would look like it had simply done nothing.
  if (stem === "spaceadom") {
    showToast("Spaceadom cannot exclude itself");
    sfx.toggleOff("engine");
    return;
  }
  const list = excludedList();
  if (list.some((r) => r.exe === stem)) {
    showToast(`${label} is already an exception`);
    return;
  }
  sfx.confirm();
  // PROBLEM 267 — a freshly added app starts at "Off entirely", the one
  // meaning the list had before scopes existed; the tile's control changes it.
  await setExcluded([...list, { exe: stem, scope: "off_entirely" }]);
  showToast(`Spaceadom will pause inside ${label}`);
}

function renderConflicts(): void {
  const box = panelEl?.querySelector<HTMLElement>("#set-conflicts");
  if (!box) return;

  const draw = () => {
    box.innerHTML = "";

    // The popover's one-line summary: the count, or "none found".
    const summary = panelEl?.querySelector<HTMLElement>("#set-conflicts-summary-text");
    if (summary) {
      const n = knownConflicts.length;
      summary.textContent = n === 0 ? "none found" : `${n} found`;
    }

    if (knownConflicts.length === 0) {
      const ok = document.createElement("div");
      ok.className = "set-note";
      ok.style.marginTop = "0";
      ok.textContent = "Nothing else is remapping your keyboard.";
      box.appendChild(ok);
    } else {
      // PROBLEM 239 second follow-up (2026-09-07) — the owner's complaint
      // repeated because the first follow-up compacted padding/disc/CTA but
      // left every row FULL PANEL WIDTH, one per line. This wrapper is what
      // turns them into a responsive grid of cards (`.conflict-grid`, same
      // `repeat(auto-fit, minmax(...))` language as `.set-act-grid`), so two
      // conflicts sit side by side in the expanded panel instead of stacking
      // as two tall bars.
      const grid = document.createElement("div");
      grid.className = "conflict-grid";

      knownConflicts.forEach((c, i) => {
        const row = document.createElement("div");
        row.className = "conflict-row";

        // The conflicting program's icon. Two tiers, cheapest first:
        //
        // 1. `findAppByStem` — a stem lookup against the SAME Start-Menu scan
        //    the exceptions grid already warms. Free (no IPC), and correct
        //    for anything that actually HAS a Start Menu shortcut.
        //
        // 2. PROBLEM 198 — some conflicts never can match tier 1, no matter
        //    what: spacedesk's background service (`spacedeskService.exe`,
        //    the thing actually flagged here) ships with no Start Menu
        //    shortcut at all. Only its separate "spacedesk DRIVER Console"
        //    GUI has one, under a different exe and therefore a different
        //    stem — verified on this machine (Start Menu holds exactly one
        //    spacedesk .lnk, targeting spacedeskConsole.exe; spacedeskService
        //    .exe and spacedeskServiceTray.exe have none). So instead of
        //    depending on a match that structurally cannot exist, fall back
        //    to extracting the icon straight from the running process's own
        //    exe file on disk — `c.path`, resolved live by Rust while the
        //    process is still running (hook/conflicts.rs) — via the SAME
        //    `extract_icon_cmd` + icon cache the key editor already uses for
        //    a manually-typed path.
        //
        // Both tiers land on `paintAppDisc`, which is the one place a broken
        // or missing icon becomes the letter disc — a row is never left
        // waiting and never throws. The letter disc paints IMMEDIATELY as the
        // synchronous starting state; if tier 2's async call lands with a
        // real icon it replaces the disc's contents in place, and if it
        // fails (process already gone, path empty, extraction comes back
        // empty) the letter disc it already painted simply stays.
        const disc = document.createElement("span");
        disc.className = "conflict-row-disc";
        const known = findAppByStem(exeStem(c.process));
        if (known) {
          paintAppDisc(disc, known.icon_base64, known.name, i);
        } else {
          paintAppDisc(disc, null, c.product, i);
          if (c.path) {
            invoke<string | null>("extract_icon_cmd", { exePath: c.path })
              .then((icon) => { if (icon) paintAppDisc(disc, icon, c.product, i); })
              .catch(() => { /* letter disc already painted — nothing to do */ });
          }
        }

        const name = document.createElement("span");
        name.className = "conflict-row-name";
        name.textContent = c.product;          // textContent — read off the machine

        const proc = document.createElement("span");
        proc.className = "conflict-row-proc";
        proc.textContent = c.process;
        // The chip is ellipsised at card widths as low as 240px (CSS); the
        // full exe name still reaches the user via the native tooltip.
        proc.title = c.process;

        // The one-liner is back UNGATED (owner, 2026-08-20: "the previous
        // small one-liner description of the app, what it does and what it
        // was conflicting, was good — bring it back"). It is also what makes
        // the long Conflicts description unnecessary.
        const why = document.createElement("span");
        why.className = "conflict-row-why";
        why.textContent = c.detail;
        // Clamped to 2 lines in CSS (`-webkit-line-clamp`) so a compact card
        // can't be forced tall by a long description; the title carries the
        // full text past the clamp.
        why.title = c.detail;

        // PROBLEM 239 follow-up — the ROW goes back to being a CONTAINER, not
        // the control. PROBLEM 157 made the whole row the click target to cut
        // two permanent buttons down to one; the owner's 1.0.10x Settings
        // review flagged the result as an "extended pill, still not made
        // proper shaped" and asked for the same "rows, not pills — and rows
        // are not buttons" language the App-exceptions tiles already carry
        // (a tile isn't a button either; its ✕ is). This undoes only the
        // "whole row" half of PROBLEM 157 — `openConflictPrompt` and its
        // two-step confirm are untouched — onto ONE real `<button>`, so a
        // keyboard user gets a single focus stop per conflict instead of the
        // row AND a same-purpose control both claiming Tab.
        const close = document.createElement("button");
        close.type = "button";
        close.className = "conflict-row-close";
        close.textContent = "Close it";
        // Matches `openConflictPrompt`'s own dialog aria-label ("Close
        // spacedesk") one line away in conflict-prompt.ts — same wording,
        // so a screen reader announces the same target twice, not two names
        // for one thing.
        close.setAttribute("aria-label", `Close ${c.product}`);
        close.addEventListener("click", (e) => {
          e.stopPropagation();
          openConflictPrompt(c, draw);
        });

        row.append(disc, name, proc, why, close);
        grid.appendChild(row);
      });

      box.appendChild(grid);

      const hint = document.createElement("div");
      hint.className = "set-note sma-note";
      // The old text said "Spaceadom never closes other programs for you" —
      // which the Conflicts description above ALSO said, and which stopped
      // being true on 2026-08-20 when the owner asked for the button below.
      hint.textContent = "Press one to have Spaceadom close it for you.";
      box.appendChild(hint);
    }

    void drawHookHealth(box, draw);

    // THE RING TOOL — always here, always quiet (owner, 2026-09-01).
    //
    // It is drawn whether or not anything is wrong, unlike the hook-health
    // block above it, which appears only when there is a fault to report. That
    // asymmetry is deliberate: hook health is an ALARM (Windows has cut us off
    // N times), and an alarm that is always on screen stops being read. This
    // is a TOOL — the answer to "the ring isn't showing" — and a tool you can
    // only find while the thing is already broken is a tool nobody finds.
    renderRingFixRow(box);

    // "Re-check now" USED TO BE APPENDED HERE, as a fourth full-width pill
    // under the ring tool. It moved to the Maintenance group in the action
    // section (1.0.97) — same action, same `refreshConflicts`, no second copy.
    // What it needs from here is the redraw, which is a closure over `box`,
    // so `draw` is published to module scope on the way past.
  };

  _redrawConflicts = draw;
  draw();

  // The exceptions section (rendered just before this one) already warms the
  // Start-Menu scan; if it's still running when conflicts first draw, redraw
  // once it lands so a row that opened on a letter fallback picks up the
  // real icon instead of staying stuck on it for the rest of the session.
  // Guarded for the same reason (PROBLEM 205): unguarded, THIS line alone
  // kept the ~12s main-thread scan on the bootstrap path.
  if (_settingsEverOpened) {
    void loadApps().then(() => { if (panelEl && !panelEl.hidden) draw(); });
  }
}

/**
 * "The ring isn't showing?" — the Conflicts-area tool that replaced the
 * "Software overlay" switch (owner, 2026-09-01).
 *
 * WHY IT IS NOT A SWITCH ANY MORE. `overlay_compositing` is a MEASUREMENT the
 * app takes about this machine's graphics driver (PROBLEM 80/92/93/122/171),
 * not a preference — and a switch asks the user to hold an opinion about GPU
 * compositing, which nobody outside this repo has. What a user HAS is the
 * symptom: the ring does not appear. So the row asks the symptom, the button
 * re-runs the measurement, and the app says what it found and what it did.
 * The escape hatch PROBLEM 92 added is intact — a false-positive "software"
 * verdict is still reversible from the UI — it just no longer requires the
 * user to know what it is reversing.
 *
 * WHY IT LIVES IN THE CONFLICTS AREA. That section is already the place this
 * app puts "something on this machine is interfering with Spaceadom", and
 * people who have hunted for the ring have already looked there. It is drawn
 * QUIETLY: no colour, no badge, no note until the button is pressed.
 *
 * Built with createElement rather than an innerHTML string because it renders
 * into `#set-conflicts`, which `draw()` rebuilds on its own schedule — the
 * same reason every other row in this section is built this way.
 */
function renderRingFixRow(box: HTMLElement): void {
  const wrap = document.createElement("div");
  wrap.className = "ring-fix";

  const row = document.createElement("div");
  row.className = "set-row";

  const label = document.createElement("button");
  label.type = "button";
  label.className = "set-row-label";
  label.dataset.desc = "ringfix";
  label.setAttribute("aria-expanded", "false");
  label.textContent = "The ring isn't showing?";

  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "btn btn-sm";
  btn.textContent = "Check the ring";
  row.append(label, btn);
  wrap.appendChild(row);

  // The description box, by hand: `descBox()` returns markup for an innerHTML
  // assignment and this subtree is built node by node. Same shape, so
  // `setDescOpen` / `convoyAll` / the CSS all reach it unchanged.
  const desc = document.createElement("div");
  desc.className = "set-desc";
  desc.dataset.descFor = "ringfix";
  const descIn = document.createElement("div");
  descIn.className = "set-desc-in";
  const descBody = document.createElement("div");
  descBody.className = "set-desc-body";
  descBody.textContent = DESC.ringfix;
  descIn.appendChild(descBody);
  desc.appendChild(descIn);
  wrap.appendChild(desc);

  const out = document.createElement("div");
  out.className = "set-note";
  out.style.cssText = "margin-top:8px; white-space:pre-line;";
  out.hidden = true;

  const restart = document.createElement("button");
  restart.type = "button";
  // NOT width:100% any more (1.0.97). It stays HERE rather than moving to the
  // Maintenance group with the other actions, and that is a decision, not an
  // oversight: it appears only after "Check the ring" has changed a setting,
  // and it is the second half of the sentence `out` just wrote above it. A
  // conditional button that explains itself through the paragraph it sits
  // under cannot be filed somewhere else without becoming unexplained.
  restart.className = "btn btn-sm set-act-inline";
  restart.style.cssText = "margin-top:8px;";
  restart.textContent = "Restart now";
  restart.hidden = true;

  wrap.append(out, restart);
  box.appendChild(wrap);

  // `wireDescriptions()` has already run by the time this section is built
  // (render() wires the panel's own markup, THEN calls renderConflicts), so
  // this label has to be wired by hand — through the same function, never a
  // second copy of the hover/click behaviour.
  wireDescLabel(label);
  // …and re-opened if it was open before the render that rebuilt it. render()
  // restores the boxes it can see at the moment it runs, which is before this
  // one exists. See `_openDescSnapshot`.
  if (_openDescSnapshot.includes("ringfix")) setDescOpen("ringfix", true);

  btn.addEventListener("click", async (e) => {
    e.stopPropagation();
    sfx.tick();
    btn.disabled = true;
    btn.textContent = "Checking…";
    out.hidden = false;
    restart.hidden = true;
    out.textContent = "Drawing the ring and watching the screen…";

    // THE BEFORE VALUE, read from the config the panel already holds, and the
    // AFTER value read back from Rust. `run_overlay_fix` returns the human
    // sentence; whether the stored verdict actually CHANGED is a fact about
    // the config, and asking the config is the only way to be sure of it —
    // parsing the sentence for a keyword would be a second source of truth
    // that drifts the first time the copy is edited.
    const before: "auto" | "software" = appConfig?.overlay_compositing ?? "auto";
    try {
      const msg = await invoke<string>("run_overlay_fix");
      let after = before;
      try {
        // `set_overlay_compositing` in Rust rejects anything that is not
        // exactly "auto" or "software", so narrowing here is a re-statement of
        // a rule the backend already enforces — not a guess about the data.
        const fresh = await invoke<{ overlay_compositing?: string }>("get_config");
        after = fresh?.overlay_compositing === "software" ? "software" : "auto";
        if (appConfig) appConfig.overlay_compositing = after;
      } catch (_) { /* keep `before`; the message still stands on its own */ }

      out.textContent = msg;
      if (after !== before) {
        sfx.confirm();
        restart.hidden = false;
        showToast("🔎 The ring check changed a setting — restart to apply");
      } else {
        sfx.tick();
      }
    } catch (err) {
      console.error("run_overlay_fix failed:", err);
      out.textContent = "The check could not run just now. Try again in a moment.";
      showToast("⚠️ Could not check the ring");
    }
    btn.disabled = false;
    btn.textContent = "Check again";
  });

  restart.addEventListener("click", (e) => {
    e.stopPropagation();
    sfx.confirm();
    restart.disabled = true;
    restart.textContent = "Restarting…";
    void invoke("restart_app").catch(() => {
      restart.disabled = false;
      restart.textContent = "Restart now";
      showToast("⚠️ Could not restart — close and reopen Spaceadom");
    });
  });
}

/**
 * PROBLEM 173 — the eviction report, and the one control that fixes the cause.
 *
 * Windows enforces a HARD deadline on low-level keyboard hooks
 * (`LowLevelHooksTimeout`, 300 ms by default). Overrun it and Windows silently
 * unhooks you: no error, no event, the hook just stops firing. Our callback is
 * microseconds of work, but the deadline is measured across the whole CHAIN —
 * so another slow hook ahead of us in the queue evicts US.
 *
 * The owner's 2026-08-24 log has that happening 17 times in one day, with
 * spacedesk and PowerToys both resident. Every one of those is a stretch of
 * several seconds where holding Space does nothing whatsoever, which is what
 * he described as "space hud doesn't appear all the time".
 *
 * Two things are shown, and only when there is something to say:
 *
 *   * The COUNT, so an intermittent fault stops being invisible. A user who
 *     can see "shortcuts stopped 17 times" has a bug report; a user who cannot
 *     has a flaky app.
 *   * The BUTTON, which raises the timeout in the user's own HKCU hive. Never
 *     automatic, and it states the sign-out requirement up front — a setting
 *     that appears to do nothing for an hour is worse than no setting.
 *
 * Hidden entirely when the count is 0 and the timeout has not been raised:
 * there is no point offering a registry change to someone whose hook has never
 * been evicted.
 */
/**
 * PROBLEM 194 — the duplicate "Raise Windows' limit" button.
 *
 * `renderConflicts`'s `draw()` clears `box` synchronously and then fires this
 * function, which is ASYNC (`await invoke("get_hook_health")`). `draw()` is
 * called from two places that race: once when the section first renders, and
 * again from `loadApps().then(() => draw())` once the Start-Menu icon scan
 * lands. `box.innerHTML = ""` only ever runs at the START of `draw()` — so if
 * the FIRST `drawHookHealth` call is still awaiting its invoke when the
 * SECOND `draw()` clears and repopulates the box, both calls eventually
 * append their own copy of this block once their own `invoke` resolves, and
 * neither knows the other exists. That is the screenshot: two identical
 * "Raise Windows' limit" buttons stacked under one "Re-check now".
 *
 * Fix: remove any earlier instance of this block by its marker class before
 * appending a fresh one. Whichever call resolves LAST wins cleanly — correct
 * here (unlike the Guide HUD's epoch race) because both calls are reading the
 * same live health data, so "last write wins" is not a staleness bug, only a
 * cosmetic one if left unguarded.
 */
async function drawHookHealth(box: HTMLElement, redraw: () => void): Promise<void> {
  let h: { timeout_ms: number | null; raised: boolean; evictions: number; rivals: string[] };
  try {
    h = await invoke("get_hook_health");
  } catch {
    return; // an older backend, or the command is unavailable — say nothing
  }
  if (!h || (h.evictions === 0 && !h.raised)) return;

  // PROBLEM 194 — remove any earlier copy of this whole block before adding a
  // fresh one, so two racing draw() calls converge on exactly one instead of
  // stacking. Marked on a wrapping container (not on `wrap` itself) because
  // `.sma-note` is a shared, generic class used elsewhere in this panel —
  // querying for it here would risk deleting notes this function never wrote.
  box.querySelectorAll(":scope > .hook-health-block").forEach((el) => el.remove());
  const container = document.createElement("div");
  container.className = "hook-health-block";

  const wrap = document.createElement("div");
  wrap.className = "set-note sma-note";
  wrap.style.marginTop = "10px";

  // PROBLEM 186 — this used to say "Give shortcuts more time (recommended)"
  // and nothing else. The owner, 2026-08-25: *"the explanation is not good
  // enough — even I don't understand what that means… what would happen if
  // more time is not given, and what is happening by giving more time? And why
  // keep it as an option rather than the default?"*
  //
  // He is right on all three counts, and the third is the important one: a
  // control whose only justification is the word "(recommended)" is asking for
  // trust it has not earned. So the copy now answers, in order: what goes
  // wrong, what the button changes, and why the app will not just do it.
  // 2026-08-31 — this used to name the rivals as "the likely cause". The
  // owner's own three weeks of logs REFUTED that: deafness was WORSE with
  // PowerToys/spacedesk closed (21.0 vs 9.9 deaf-minutes per 100 active).
  // Blaming a named app the user then uninstalls for nothing is worse than
  // no explanation, so the copy now states only the fact (they also watch
  // the keyboard) without the causal claim the data does not support.
  const said = h.rivals.length
    ? ` ${h.rivals.join(" and ")} also watch${h.rivals.length === 1 ? "es" : ""} the keyboard, which can add to the queue.`
    : "";
  const why =
    "Windows gives every app that watches the keyboard 0.3 seconds to handle each " +
    "keypress. If Spaceadom is still busy when that runs out — a slow moment, or " +
    "another keyboard app ahead of it in the queue — Windows stops sending it keys " +
    "altogether. Spaceadom notices and reconnects within a second, but until it does, " +
    "holding Space does nothing at all.";
  wrap.textContent = h.evictions > 0
    ? `Windows has cut Spaceadom off ${h.evictions} time${h.evictions === 1 ? "" : "s"} ` +
      `since it started.${said}\n\n${why}`
    : `Windows is currently allowing 1 second instead of the usual 0.3, so a busy ` +
      `moment is no longer enough for it to cut Spaceadom off.\n\n${why}`;
  wrap.style.whiteSpace = "pre-line";
  container.appendChild(wrap);

  // WHY IT IS A BUTTON AND NOT THE DEFAULT. Stated plainly, because the honest
  // answer is also the reassuring one — and because a user who is not told the
  // trade-off cannot consent to it.
  const caveat = document.createElement("div");
  caveat.className = "set-note sma-note";
  caveat.style.cssText = "margin-top:8px; opacity:.82;";
  caveat.textContent = h.raised
    ? "This is a Windows setting, not a Spaceadom one — it applies to every app on " +
      "this PC that watches the keyboard. Undoing it takes effect after you sign out " +
      "and back in."
    : "Spaceadom will not change this for you. It is a Windows setting that applies to " +
      "every app on this PC that watches the keyboard, and it needs a sign-out to take " +
      "effect — so it is your call, not the app's. The trade-off: a keyboard app that " +
      "genuinely hangs could hold your keys for up to 1 second before Windows steps " +
      "in, instead of 0.3.";
  container.appendChild(caveat);

  const btn = document.createElement("button");
  // Sized to its own label (1.0.97), and kept HERE for the same reason the
  // ring tool's "Restart now" is: the three paragraphs above it are what the
  // owner asked for when he said "(recommended)" was not good enough, and a
  // consent control filed away from the consent text is a control nobody
  // consented to.
  btn.className = "btn btn-sm set-act-inline";
  btn.style.cssText = "margin-top:8px;";
  // The label says what it CHANGES, with the numbers in it. "Give shortcuts
  // more time (recommended)" told the owner nothing — he could not tell what
  // it did, what it cost, or why he should trust "(recommended)".
  btn.textContent = h.raised
    ? "Put back Windows' 0.3 second limit"
    : "Raise Windows' limit from 0.3 to 1 second";
  btn.addEventListener("click", async () => {
    sfx.tick();
    btn.disabled = true;
    try {
      const msg = await invoke<string>("set_hook_timeout", { raise: !h.raised });
      sfx.confirm();
      showToast(msg);
      redraw();
    } catch (e) {
      console.error("set_hook_timeout failed:", e);
      showToast("⚠️ Could not change that Windows setting");
      btn.disabled = false;
    }
  });
  container.appendChild(btn);
  box.appendChild(container);

  const fine = document.createElement("div");
  fine.className = "set-note sma-note";
  fine.style.marginTop = "6px";
  fine.textContent = h.raised
    ? "This changes a Windows setting for your account only. Sign out and back in to apply."
    : "Changes one Windows setting for your account only — no admin needed — so Windows waits " +
      "1 second instead of 0.3 before giving up on a shortcut. Sign out and back in to apply.";
  box.appendChild(fine);
}

// ---------------------------------------------------------------------------
// PRESS-TO-EXPAND DESCRIPTIONS (PROBLEM 144)
//
// The owner's brief: "a user who didn't use this ever doesn't know how to use
// this, or what those settings do... at the same time the place doesn't look
// clumsy." So nothing is added to a row until it is asked for — press a
// setting's label and its description slides open underneath it.
//
// Copy is transcribed VERBATIM from design/design-system-overhaul-3.md §1.
// It is deliberately plain-spoken ("your spacebar is just a spacebar again"),
// which was an explicit instruction: real language, not artificial language.
// Do not "improve" these into product-speak.
// ---------------------------------------------------------------------------
const DESC: Record<string, string> = {
  engine:
    "The main switch. Turn it off and your spacebar is just a spacebar again — nothing launches until you flip it back on.",
  fun:
    "All the personality — character switches, swirling cards, flame convoys and space sounds. Off swaps everything for plain, quiet controls.",
  sound:
    "Tiny clicks when keys are pressed and switches flip. Just for feel — off means silence.",
  startup:
    "Spaceadom opens quietly in the background when your PC turns on, so your shortcuts work from the first minute.",
  motion:
    "All the movement — keys popping, panels gliding. Turn off if the app ever feels heavy on your machine.",
  // 2026-09-01 — the "Software overlay" switch is gone and so is its entry.
  // Its replacement is `ringfix` further down: the same escape hatch, asked as
  // the symptom the user actually has.
  flight:
    "When a shortcut fires while the Space ring is open, the little message flies out of the ring instead of simply appearing. It looks good and it takes about a second. Off is quicker and quieter.",
  hudpointer:
    "While the Space guide is open, move your cursor out towards an app — you don't have to reach it, just point that way — and it lights up. Let go of Space, or click, and that app opens. Stay near the middle of the ring and nothing is picked, so letting go there types a space as usual.",
  // PROBLEM 263 — three sentences, in the order a worried user asks the
  // questions: what does it do, have you broken my middle click, and what
  // about my CAD program. The third one is not a footnote — it is the reason
  // this switch can be on by default, and a user who works in SolidWorks needs
  // to read it here rather than discover it.
  middlering:
    "Hold the middle mouse button — the scroll wheel, pressed down — and a ring of your apps opens: the icon ring right at your cursor, or the same centred ring as holding Space (choose below). Aim at an app and let go to open it. A normal quick middle click still works exactly as before: links still open in a new tab, tabs still close. 3D and CAD programs are left alone, because middle-drag already spins the model there — SolidWorks, Fusion 360, Blender, AutoCAD and the rest are on a built-in list, along with drawing apps like Photoshop and Figma where it pans the canvas. Your Space shortcuts keep working in all of them; App exceptions below lets you change any of that per app.",
  // PROBLEM 267 — the two rows under the middle-button switch. Same order of
  // questions as `middlering`: what does it do, then what it does NOT change.
  middlestyle:
    "What the middle button opens. Icon ring is a small ring of your apps' real icons that blooms out of the cursor wherever it is — point at one and let go. Space ring opens the same big centred ring you get from holding Space, exactly as before. Your Space ring itself is never changed by this.",
  // 2026-09-15 — the third middle-button row. Same order of questions as the
  // two above: what each choice draws, then what it does NOT change.
  alllayout:
    "How the ring arranges itself when it is showing All of your apps. Rings keeps them on neat circles around your cursor, five on the first, eight on the next and thirteen on the one after — the counts that pack a circle evenly. Spiral puts every app on one winding line instead, turning the same fraction of a circle between each, which is how a sunflower packs its seeds: no rings to line up, no gaps to leave. Aiming works the same either way — whichever icon your cursor is nearest lights up. Favourites is unaffected; it arranges itself around the screen edge instead.",
  middlescope:
    "How many apps the icon ring shows. Favourites keeps it to the apps you tick below \u2014 up to fifteen, six on the inner ring and the rest on a second one \u2014 with the name in the middle. All adds every other key you have bound plus the special keys on further rings, so nothing is more than a flick away. Choose your favourites below.",
  // The ring pill and the specials switch are ONE system, so their two
  // descriptions have to tell the same story from both ends — each says the
  // inner ring is the shared resource, and each says what to change to get the
  // other outcome. Same plain-spoken rule as everything above.
  //
  // 2026-09-01 — this ONE entry replaces `hudlayout` and `hudrows`, which
  // described a switch and a pill that had to be read together. Three named
  // shapes can be described in three sentences, which is the point.
  hudring:
    "The shape of the ring you see while Space is held. Compact keeps everything close and clips long names until you aim at one. Wide is the roomier ring this app used before, with the names written out. Double adds a second ring of apps for when you have bound a lot of keys — it uses all the room, so the special keys step aside.",
  hudspecials:
    "Puts Boss Key, PiP and the rest on the Space ring as a reminder — the keys themselves work either way. They sit in the inner ring, so they can only show when the apps are using a single ring. Choose Double and they step aside.",
  // The Conflicts-area tool that replaced the "Software overlay" switch. It
  // says what the button will DO, in the order a worried person needs it: what
  // it checks, what it might change, and that nothing happens without them.
  ringfix:
    "Draws the ring once and watches whether anything actually reaches the screen. Some graphics drivers stop drawing the pop-ups while everything else keeps working — sounds play, apps launch, and nothing appears. If that is what is happening, Spaceadom switches to a backup way of drawing and asks you to restart. If the backup is already on and the ring is fine, it hands drawing back to your graphics card and re-checks by itself. Nothing changes unless the check finds something.",
  theme:
    "Four looks for the whole app, pop-ups included: Auto follows Windows' own light/dark setting (Earthy in light mode, Starry night in dark), or pick a fixed look yourself — Earthy daylight, a Warcry of iron and war-banners, or a Starry night sky.",
  // Not in the spec — this setting is new, so the copy is written to match its
  // voice: what you get, and how to come back.
  advanced:
    "Shows Run command and the full catalogue in the key editor. Run command lets a key run any command line, with no window and never as administrator; the full catalogue adds Control Panel pages and system shortcuts to the Windows-setting search. Off keeps the editor to apps, links, everyday settings, key chords and Spaceadom's own specials.",
  hideboard:
    "Clears the whole dashboard away and leaves just the sky. Your shortcuts keep working exactly as they are — press Esc, the small arrow in the corner, or the settings gear, which stays on screen, to bring everything back.",
  wpm:
    "If apps launch by accident while you type, pick a slower speed — Spaceadom then waits longer before treating Space+key as a shortcut.",
  huddelay:
    "How long you hold Space before the shortcut guide appears. Shorter shows help sooner; longer keeps it out of your way.",
  opacity:
    "The limit for Space+Scroll window fading. The floor stops a window from ever turning fully invisible.",
  // NOT gated behind "Show me around" like the other teaching prose: a
  // conflict is a live fault on this machine, and the owner wants its
  // explanation there whenever it is (2026-08-20).
  appexceptions:
    "What Spaceadom does while each of these apps is in front. Off entirely pauses everything — Space works exactly as it normally would there, and the middle button too. Space only keeps your Space shortcuts and hands the middle button back to the app, which is what the built-in rows do for 3D, CAD and drawing programs, where middle-drag orbits or pans. Middle only is the other way round: the middle-button ring stays, Space is left alone. Shortcuts come back the moment you switch away.",
  conflicts:
    "Only one program can own the spacebar. Press one below to close it.",
  reset:
    "Puts a preset profile back to its factory bindings. On a profile you created, it clears it instead — you confirm first.",
  clear:
    "Empties every binding in this profile. Asks you to confirm first.",
  presets:
    "Brings back any missing preset (Founders, Gamers, Professionals). Never overwrites one you still have.",
  logs:
    "Opens the folder with Spaceadom's log files — handy when reporting a bug.",
  // PROBLEM 195. Same plain-spoken rule as everything above: say what actually
  // happens, name the company, and do not soften it. "Crash and error" is the
  // literal scope — ERROR-level lines and crashes, nothing quieter.
  sendlogs:
    "Leave this off and Spaceadom sends a report when it crashes or hits an error — the message, where in the code it happened, and your Windows version. It goes to Sentry, a crash-reporting service, so bugs on other people's machines can be fixed without asking anyone to dig out a log file. Nothing about your normal use is sent: not what you type, not which shortcuts you press, not which apps you open. Turn this on and nothing leaves your machine at all.",
};

/** How long a label must be hovered before its description opens itself. */
const HOVER_LINGER_MS = 2000;
/** Gap between rows when "Show me around" opens them all as a convoy. */
const CONVOY_STAGGER_MS = 80;
/** Total stagger budget for OPENING the convoy, however many rows exist. */
const CONVOY_IN_MS = 420;

/** Descriptions that were opened by hovering, so they can close on leave.
 *  One opened by a CLICK stays put — that was a deliberate act. */
const _hoverOpened = new Set<string>();
let _hoverTimer: number | undefined;

/** The collapsing box under a row. Empty when there is no copy for the id. */
/** @param openByDefault  starts expanded and stays expanded through renders —
 *  only Conflicts uses it, because a live fault should explain itself without
 *  being asked (owner, 2026-08-20). */
/**
 * `id="desc-${id}"` — accessibility pass. It is what lets the row's control
 * (the switch, the slider, the pill) carry `aria-describedby="desc-${id}"`
 * and have a screen reader read this text as the control's description,
 * whether or not it is visually expanded — the sighted "press the label to
 * reveal it" interaction and the screen-reader description are two different
 * audiences and do not have to wait on each other.
 */
function descBox(id: string, openByDefault = false): string {
  const copy = DESC[id];
  if (!copy) return "";
  if (openByDefault) {
    return `<div class="set-desc is-open" id="desc-${id}" data-desc-for="${id}"><div class="set-desc-in"><div class="set-desc-body">${copy}</div></div></div>`;
  }
  // The visual box is a CHILD of the clipped wrapper, never the wrapper
  // itself — see the .set-desc-in note in styles.css for why.
  return `<div class="set-desc" id="desc-${id}" data-desc-for="${id}"><div class="set-desc-in"><div class="set-desc-body">${copy}</div></div></div>`;
}

/** `aria-describedby` value for a row's DESC box, or `undefined` when the id
 *  has no copy — pointing a control at a description box that will never
 *  render would be worse than pointing at nothing. */
function descId(id: string): string | undefined {
  return DESC[id] ? `desc-${id}` : undefined;
}

function setDescOpen(id: string, open: boolean): void {
  const box = panelEl?.querySelector<HTMLElement>(`[data-desc-for="${id}"]`);
  if (!box) return;
  box.classList.toggle("is-open", open);
  panelEl
    ?.querySelectorAll<HTMLElement>(`[data-desc="${id}"]`)
    .forEach((l) => l.setAttribute("aria-expanded", String(open)));
  if (!open) _hoverOpened.delete(id);
}

function isDescOpen(id: string): boolean {
  return !!panelEl?.querySelector(`[data-desc-for="${id}"].is-open`);
}

/**
 * Open or close every description at once, staggered.
 *
 * The stagger is the whole point of the "convoy" — they arrive in order rather
 * than all snapping at once. Closing runs the stagger REVERSED so the panel
 * folds up from the bottom, which reads as the same gesture played backwards.
 */
function convoyAll(open: boolean): void {
  const boxes = Array.from(
    panelEl?.querySelectorAll<HTMLElement>("[data-desc-for]") ?? [],
  );
  const order = open ? boxes : boxes.slice().reverse();
  const reduced = document.documentElement.classList.contains("reduced-motion");
  // The stagger is a flourish on the way IN and a wait on the way OUT. There
  // are sixteen descriptions now, so a flat 80ms each meant 1.3s of stagger
  // plus the slide before the panel was clear — the owner's "minimising takes
  // too much time; it wasn't the problem in other builds" (there were fewer
  // rows then). Closing is now BUDGETED: the whole convoy is out inside
  // CONVOY_OUT_MS however many rows there are, which is also the design's
  // "exits run at ~65% of entrance time".
  // Closing has NO stagger (owner, 2026-08-20: "when closing the wait is still
  // too long"). Collapsing all sixteen together is the same total layout work
  // as staggering them — one pass per frame either way — spread over 240ms
  // instead of a second of waiting. The stagger stays on the way IN, where it
  // is the flourish rather than a delay before the panel is usable.
  const step = open ? Math.min(CONVOY_STAGGER_MS, CONVOY_IN_MS / Math.max(1, order.length)) : 0;
  order.forEach((box, i) => {
    const id = box.dataset.descFor ?? "";
    if (reduced) { setDescOpen(id, open); return; }
    window.setTimeout(() => setDescOpen(id, open), i * step);
  });
}

/**
 * Wire every label: click toggles, a 2s hover opens.
 *
 * Re-wired on every render, which is safe because render() replaces the whole
 * subtree — the old listeners go with the old nodes.
 */
function wireDescriptions(): void {
  panelEl?.querySelectorAll<HTMLElement>("[data-desc]").forEach(wireDescLabel);
}

/**
 * ONE label's worth of that wiring, split out so the rows built LATER — the
 * ring tool inside `#set-conflicts`, which `renderConflicts` draws on its own
 * schedule after `wireDescriptions` has already run — get the identical
 * behaviour instead of a second copy that forgets the hover half.
 */
function wireDescLabel(label: HTMLElement): void {
  const id = label.dataset.desc ?? "";
  if (!id || !DESC[id]) return;

  label.addEventListener("click", (e) => {
    // A label inside a <label for=…> would otherwise flip the switch too.
    e.preventDefault();
    e.stopPropagation();
    const next = !isDescOpen(id);
    setDescOpen(id, next);
    if (next) sfx.bloomOpen(); else sfx.bloomClose();
    if (!next) _hoverOpened.delete(id);
  });

  label.addEventListener("pointerenter", () => {
    if (isDescOpen(id)) return;
    window.clearTimeout(_hoverTimer);
    _hoverTimer = window.setTimeout(() => {
      setDescOpen(id, true);
      sfx.whisper();                 // barely-there: it opened by itself
      _hoverOpened.add(id);          // opened by hover -> closes on leave
    }, HOVER_LINGER_MS);
  });

  label.addEventListener("pointerleave", () => {
    window.clearTimeout(_hoverTimer);
    // Only retract what hover opened. Anything the user clicked open stays.
    if (_hoverOpened.has(id)) setDescOpen(id, false);
  });
}

// ---------------------------------------------------------------------------
// Row builders
// ---------------------------------------------------------------------------

/**
 * The row the user just flipped, and which way — consumed by the NEXT render
 * and then forgotten.
 *
 * The spec's rule is "first render = no animation (static position); animate
 * only after user interaction". render() re-runs after every single toggle,
 * so without this latch, flipping one switch would replay all eight
 * characters at once — eight knobs hopping because one was pressed.
 */
let _flipped: { id: string; on: boolean } | null = null;

/**
 * Stamped centrally by wireToggle on the checkbox's own `change`, so a new
 * switch cannot be added without a character — and so the value recorded is
 * the checkbox's real state, not what a handler intended.
 *
 * It marks BOTH the live element and the next render. The live stamp exists
 * for "Show me around", which deliberately never re-renders (that would wipe
 * the convoy mid-flight) and would otherwise be the one silent switch.
 */
function markFlipped(id: string, on: boolean): void {
  _flipped = { id, on };
  panelEl?.querySelectorAll<HTMLElement>(".toggle-switch[data-anim]")
    .forEach((sw) => sw.removeAttribute("data-anim"));
  panelEl?.querySelector<HTMLElement>(`#set-${id}`)
    ?.closest<HTMLElement>(".toggle-switch")
    ?.setAttribute("data-anim", on ? "on" : "off");
}

function toggleRow(id: string, label: string, on: boolean, i: number): string {
  // PROBLEM 144 — this row used to be one big <label>, so a click anywhere on
  // it flipped the switch. Press-to-expand needs the TEXT to mean "explain
  // this" and only the switch to mean "change this", so the label is now a
  // button and the track carries the `for=`. The CSS keeps working because
  // `input:checked + .toggle-track` is still an adjacent sibling.
  return `
    <div class="set-item set-filterable" style="animation-delay:${60 + i * 45}ms">
      <div class="set-row">
        <button type="button" class="set-row-label" data-desc="${id}"
                aria-expanded="false" aria-controls="desc-${id}">${label}</button>
        ${toggleSwitchHtml(id, on,
          _flipped?.id === id && _flipped.on === on ? (on ? "on" : "off") : undefined,
          label, descId(id))}
      </div>
      ${descBox(id)}
    </div>`;
}

/**
 * The 3-way Theme pill (PROBLEM 144) — Earthy / Warcry / Starry night.
 *
 * Replaces the old "Dark mode" switch. A sliding indicator hops between three
 * equal segments; `--seg-i` drives its translateX so the movement is a single
 * transform rather than three elements changing background.
 */
function themeRow(theme: string, i: number): string {
  // The pill markup itself now lives in controls.ts (`segRowHtml`) so the dev
  // harness can draw one and so a SECOND pill — "Shortcut rows" below — cannot
  // drift from this one. The theme pill passes no indicator style: its three
  // segments are coloured per theme by CSS (`.theme-seg-ind[data-seg=...]`),
  // which is meaningful for this control and for no other.
  return `
    <div class="set-item set-filterable" style="animation-delay:${60 + i * 45}ms">
      <div class="set-row set-row-stack">
        <button type="button" class="set-row-label" data-desc="theme"
                aria-expanded="false" aria-controls="desc-theme">Theme</button>
        ${segRowHtml("theme", THEME_OPTS, theme, "", "Theme", descId("theme"))}
      </div>
      ${descBox("theme")}
    </div>`;
}

/**
 * The theme pill's segments, in order. Also drives the in-place index update
 * in the click handler, so the two cannot disagree.
 *
 * "Auto" is FIRST (owner decision, feature 2) — it is the new-install default
 * (schema.rs), so it is the option a first-time user meets before any of the
 * three fixed looks. It resolves to Earthy or Starry night via
 * `resolveTheme()` in main.ts (never Warcry — see that function's doc
 * comment for why); the pill itself just stores and shows the literal string
 * "auto", exactly like it shows "earthy"/"warcry"/"starry" for the other
 * three.
 */
const THEME_OPTS: ReadonlyArray<readonly [string, string]> = [
  ["auto", "Auto"],
  ["earthy", "Earthy"],
  ["warcry", "Warcry"],
  ["starry", "Starry night"],
];

/**
 * THE SPACE RING — Compact · Wide · Double, one pill (owner, 2026-09-01).
 *
 * A 3-WAY PILL, NOT A SWITCH PLUS A PILL, and it follows `themeRow` above
 * because that is the precedent this panel already has for a three-state
 * setting. It is rendered IMMEDIATELY BEFORE the "Show special keys" switch on
 * purpose: the two are one system — under Double both rings belong to apps, so
 * the specials have nowhere to sit — and a dependency the user cannot see is a
 * dependency they will read as a bug.
 *
 * NO ALWAYS-VISIBLE DESCRIPTION, by explicit instruction. The old pair of rows
 * carried permanent notes explaining why one of them was dead; three named
 * shapes have no dead state, so the explanation goes where every other row in
 * this panel keeps it — behind the LABEL, which expands `DESC.hudring` when
 * pressed. The only note that survives is the specials one, and it appears
 * only in the state it explains.
 *
 * The indicator takes `var(--st-accent)` inline rather than a new CSS rule:
 * the accent is already themed, so the pill re-tints in Warcry and Starry for
 * free, and no token is introduced for a control that needs one colour.
 */
function ringRow(ring: RingLayout, i: number): string {
  return `
    <div class="set-item set-filterable" style="animation-delay:${60 + i * 45}ms">
      <div class="set-row set-row-stack">
        <button type="button" class="set-row-label" data-desc="hudring"
                aria-expanded="false" aria-controls="desc-hudring">Ring layout</button>
        ${segRowHtml("hudring", RING_OPTS, ring, "background:var(--st-accent);", "Ring layout", descId("hudring"))}
      </div>
      ${descBox("hudring")}
    </div>`;
}

/**
 * THE ENGINE ROW, which is the only row in this panel that is not in a group.
 *
 * It is STICKY (`.set-engine` in styles.css) so the main switch stays on
 * screen at any scroll depth — the owner's instruction, and the right one: the
 * panel is now long enough that "turn it off for a minute" could mean
 * scrolling back up past four groups to find the switch that does it.
 *
 * The one-liner under the label is NOT an `.sma-note`. Every other piece of
 * teaching prose here hides unless "What do these do?" is on, and that is
 * correct for prose that explains a preference — but this switch turns the
 * whole app off, and a user who is about to press it should not have to have
 * asked for help first to learn what it does. The copy is the owner's own.
 */
function engineRow(on: boolean): string {
  // 1.0.96 review fix — NOT `set-filterable`. This row is `.set-engine`
  // sticky (pinned on screen at any scroll depth, see the doc comment
  // above), so it never needed to be foundable by search in the first
  // place — but being `.set-filterable` meant a query that didn't match it
  // got `filterSettings` to hide it anyway, defeating the whole point of
  // pinning it: the one row that must always be reachable was the one row
  // search could take off screen.
  return `
    <div class="set-item set-engine">
      <div class="set-row">
        <button type="button" class="set-row-label" data-desc="engine"
                aria-expanded="false" aria-controls="desc-engine">Engine active</button>
        ${toggleSwitchHtml("engine", on,
          _flipped?.id === "engine" && _flipped.on === on ? (on ? "on" : "off") : undefined,
          "Engine active", `set-engine-note${descId("engine") ? ` ${descId("engine")}` : ""}`)}
      </div>
      <div class="set-engine-note" id="set-engine-note">Off means Space is just a space again.</div>
      ${descBox("engine")}
    </div>`;
}

/**
 * "Show special keys" — the switch that CANNOT be a plain `toggleRow`, because
 * on **Double** it has nothing to do.
 *
 * (This used to say "at 2 rows". The rows pill that phrase named was retired on
 * 2026-09-01 into the Compact · Wide · Double pill; the condition is the same
 * config state — `hud_band_count: "two"` — under the name the user now sees.)
 *
 * CLAUDE.md: *"A control that does nothing is worse than a missing control."*
 * With two app bands the inner ring is spoken for and Rust sends an empty
 * specials list whatever this says (engine/mod.rs `specials_for_hud`), so
 * leaving the switch live would let a user flip it, hear the sound, watch it
 * animate — and see no change at all, forever, with nothing to explain why.
 *
 * So it renders INERT WITH A VISIBLE REASON:
 *
 *   · the switch is `disabled` and `pointer-events:none` at reduced opacity,
 *     so neither the pointer nor the keyboard can reach it;
 *   · a plain `.set-note` under the row says why and how to get it back —
 *     `.set-note`, NOT `.sma-note`, which is hidden unless "Show me around"
 *     is on and would make the explanation invisible to exactly the user who
 *     needs it;
 *   · the LABEL stays live, because pressing it opens the description that
 *     explains the whole ring-shape/specials system.
 *
 * **THE STORED VALUE IS NOT TOUCHED.** This is presentation, not a config
 * mutation: the switch keeps rendering the user's own preference, so choosing
 * Compact or Wide again restores exactly what they had. Writing `false` here
 * to "make the UI honest" would silently destroy a preference the user never
 * asked to change — and they would only find out much later.
 *
 * `toggleSwitchHtml` is still the source of the markup, so this row cannot
 * drift from the other nine switches or lose its Fun-mode character. (Nine, not
 * eleven: "Show me around" became the header link and "Software overlay" became
 * the Conflicts-area tool on 2026-09-01. `TOGGLE_CHAR` in controls.ts is the
 * list that has to agree with this number.)
 */
/**
 * PROBLEM 267 — move a segmented pill's indicator IN PLACE (PROBLEM 157: a
 * re-render destroys the indicator and builds a new one already at the
 * destination, which has nothing to transition FROM). Shared by the two
 * middle-button pills; the theme and ring pills predate it and keep their own
 * inline copies of the same four lines.
 */
function moveSeg(b: HTMLElement, dataKey: string, next: string): void {
  const seg = b.closest<HTMLElement>(".theme-seg");
  if (!seg) return;
  seg.querySelector<HTMLElement>(".theme-seg-ind")?.setAttribute("data-seg", next);
  seg.querySelectorAll<HTMLElement>(".theme-seg-opt").forEach((o) => {
    const on = o.dataset[dataKey] === next;
    o.classList.toggle("is-on", on);
    o.setAttribute("aria-checked", String(on));
  });
  positionSegIndicator(seg);
}

/* ---- THE MATH SUBTITLES (owner, 2026-09-15) -------------------------------
   One short line under each of the three ring rows, in the same register as
   the `set-note` copy above them: a plain statement of where the number came
   from, for a user curious enough to notice. Deliberately NOT the `DESC`
   text \u2014 these are always visible, so they have to earn their line in four or
   five words. The wording is the owner's, verbatim. */
const SUB_MIDDLESTYLE = "Sized by the golden ratio.";
const SUB_MIDDLESCOPE = "A Fibonacci cap, for density.";
const SUB_ALLLAYOUT = "Packed like a sunflower\u2019s seeds.";

/** The always-visible subtitle under a row's label. */
function subLine(text: string): string {
  return `<div class="set-sub">${text}</div>`;
}

/** The reason line under a middle-button row while the switch above is off. */
const MIDDLE_OFF_NOTE = "Turn on \u201cMiddle button opens the ring\u201d to use this.";
/** \u2026and under the All-layout pill while the scope is Favourites. */
const ALL_LAYOUT_NOTE = "Only for \u201cAll\u201d \u2014 Favourites arranges itself around the screen edge.";
/** …and under the scope pill while the middle button opens the Space ring. */
const MIDDLE_SCOPE_NOTE = "Only for the icon ring \u2014 the Space ring has its own layout above.";

/**
 * PROBLEM 267 — "Middle button shows": Icon ring / Space ring (the owner's
 * addition of 2026-09-13). Same shape as `ringRow`; the wrap + note pair is
 * what lets `paintRow` grey it without a rebuild, exactly as `specialsRow`.
 */
function middleStyleRow(style: MiddleStyle, inert: boolean, i: number): string {
  return `
    <div class="set-item set-filterable" style="animation-delay:${60 + i * 45}ms">
      <div class="set-row set-row-stack" aria-disabled="${inert}">
        <button type="button" class="set-row-label" data-desc="middlestyle"
                aria-expanded="false" aria-controls="desc-middlestyle">Middle button shows</button>
        <span id="set-middlestyle-wrap">${
          segRowHtml("middlestyle", MIDDLE_STYLE_OPTS, style, "background:var(--st-accent);", "Middle button shows", descId("middlestyle"))
        }</span>
      </div>
      ${subLine(SUB_MIDDLESTYLE)}
      <div class="set-note" id="set-middlestyle-note" style="margin-top:6px;display:none;">${MIDDLE_OFF_NOTE}</div>
      ${descBox("middlestyle")}
    </div>`;
}

/**
 * PROBLEM 267 — "Middle-button ring shows": the Favourites / All pill and
 * the "Choose your favourites \u2192" link on one line (artboard 8), with the
 * picker's container under it (artboard 9, drawn by `renderEightPicker`).
 */
function middleScopeRow(scope: MiddleScope, inert: boolean, i: number): string {
  return `
    <div class="set-item set-filterable" style="animation-delay:${60 + i * 45}ms">
      <div class="set-row set-row-stack" aria-disabled="${inert}">
        <button type="button" class="set-row-label" data-desc="middlescope"
                aria-expanded="false" aria-controls="desc-middlescope">Middle-button ring shows</button>
        <span id="set-middlescope-wrap" class="mscope-line">${
          segRowHtml("middlescope", MIDDLE_SCOPE_OPTS, scope, "background:var(--st-accent);", "Middle-button ring shows", descId("middlescope"))
        }<button type="button" class="mscope-choose" id="set-eight-open" aria-expanded="${_eightOpen}">Choose your favourites \u2192</button></span>
      </div>
      ${subLine(SUB_MIDDLESCOPE)}
      <div class="set-note" id="set-middlescope-note" style="margin-top:6px;display:none;">${MIDDLE_OFF_NOTE} ${MIDDLE_SCOPE_NOTE}</div>
      <div id="set-eight-picker"></div>
      ${descBox("middlescope")}
    </div>`;
}

/**
 * 2026-09-15 — "All layout": Rings / Spiral, the owner's toggle for how the
 * `All` scope arranges itself. It sits DIRECTLY under the scope pill and is
 * inert unless "All" is chosen, mirroring the way "Choose your favourites"
 * only means anything under Favourites — a row whose reason for being greyed
 * has to be visible from the control that greys it (the same adjacency rule
 * the ring pill and the specials switch obey).
 */
function allLayoutRow(layout: AllLayout, inert: boolean, i: number): string {
  return `
    <div class="set-item set-filterable" style="animation-delay:${60 + i * 45}ms">
      <div class="set-row set-row-stack" aria-disabled="${inert}">
        <button type="button" class="set-row-label" data-desc="alllayout"
                aria-expanded="false" aria-controls="desc-alllayout">All layout</button>
        <span id="set-alllayout-wrap">${
          segRowHtml("alllayout", ALL_LAYOUT_OPTS, layout, "background:var(--st-accent);", "All layout", descId("alllayout"))
        }</span>
      </div>
      ${subLine(SUB_ALLLAYOUT)}
      <div class="set-note" id="set-alllayout-note" style="margin-top:6px;display:none;">${ALL_LAYOUT_NOTE}</div>
      ${descBox("alllayout")}
    </div>`;
}

/** The picker's open state survives a render, like the exceptions picker's. */
let _eightOpen = false;
/** The pending auto-close after the eighth tick (owner, 1.0.110 review):
 *  ~400 ms so the user sees the eighth row tint before the card folds.
 *  Cleared on every redraw and on "Done", so a close can never fire twice
 *  or land on a picker the user has already reopened. */
let _eightCloseTimer: number | undefined;
const EIGHT_AUTO_CLOSE_MS = 400;

/** The one place the picker closes — "Done", the auto-close, and the
 *  "Choose your favourites" toggle all land here. */
function closeEightPicker(): void {
  window.clearTimeout(_eightCloseTimer);
  _eightCloseTimer = undefined;
  _eightOpen = false;
  renderEightPicker();
}

/** The active profile's bound keys, sorted, with what the picker shows. */
function boundKeysForPicker(): Array<{ key: string; name: string; icon: string | null; link: boolean }> {
  const prof = appConfig?.profiles.find((p) => p.name === appConfig?.active_profile);
  if (!prof) return [];
  return Object.keys(prof.bindings)
    .filter((k) => /^[a-z]$/i.test(k))
    .map((k) => k.toLowerCase())
    .sort()
    .map((k) => {
      const b = prof.bindings[k] ?? prof.bindings[k.toUpperCase()];
      const link = !!b?.web_url && !b?.app;
      let name = b?.label ?? b?.app ?? b?.web_url ?? k.toUpperCase();
      if (link && b?.web_url) {
        try { name = b.label ?? new URL(b.web_url).hostname.replace(/^www\./, ""); } catch (_) { /* keep */ }
      }
      // A link shows its favicon (a complete data: URL) or the placeholder
      // circle; an app shows its picker PNG.
      const icon = link ? (b?.site_icon ?? null) : (b?.icon_override ? `data:image/png;base64,${b.icon_override}` : null);
      return { key: k, name, icon, link };
    })
    .filter((row) => {
      const b = prof.bindings[row.key] ?? prof.bindings[row.key.toUpperCase()];
      return !!(b && (b.app || b.web_url));
    });
}

/**
 * PROBLEM 267 — the "Choose your favourites" picker (artboard 9): every bound
 * key of the active profile as letter chip + icon + name + checkbox, selected
 * rows tinted, "N of 15 selected" at the top, the disclosure line at the foot.
 * No new app search — it reuses what the user has already bound.
 *
 * Built with createElement: app names and hostnames are the user's data.
 * Draws into its OWN container so a pick never re-renders the panel. Writes
 * `middle_ring_favourites` in the order picked; an EMPTY list is never
 * written by a pick — clearing the last one writes the empty list, which
 * Rust reads as "the first six bound letters" (the same default the picker
 * showed before anyone touched it).
 */
function renderEightPicker(): void {
  const box = panelEl?.querySelector<HTMLElement>("#set-eight-picker");
  if (!box) return;
  box.innerHTML = "";
  // A redraw supersedes any pending auto-close: the tick that armed it is
  // re-evaluated below against the list as it now stands.
  window.clearTimeout(_eightCloseTimer);
  _eightCloseTimer = undefined;
  const open = panelEl?.querySelector<HTMLElement>("#set-eight-open");
  open?.setAttribute("aria-expanded", String(_eightOpen));
  if (!_eightOpen || !appConfig) return;

  const rows = boundKeysForPicker();
  const bound = rows.map((r) => r.key);
  // What the ring WILL show: the stored list, or the same first-six default
  // Rust computes for an empty one — so the first pick keeps the five
  // defaults beside it rather than leaving a ring of one.
  const chosen = effectiveFavourites(appConfig.middle_ring_favourites, bound);

  const wrap = document.createElement("div");
  wrap.className = "eight-picker";

  const head = document.createElement("div");
  head.className = "eight-head";
  const title = document.createElement("span");
  title.className = "eight-title";
  title.textContent = "Choose your favourites";
  const count = document.createElement("span");
  count.className = "eight-count";
  count.textContent = `${chosen.length} of ${FAVOURITES_MAX} selected`;
  // "Done" (owner, 1.0.110 review) — right of the count. The same close the
  // "Choose your favourites →" toggle performs; a picker with no visible way out
  // read as a list that was stuck open.
  const done = document.createElement("button");
  done.type = "button";
  done.className = "btn btn-sm eight-done";
  done.textContent = "Done";
  done.addEventListener("click", (e) => {
    e.stopPropagation();
    sfx.tick();
    closeEightPicker();
  });
  head.append(title, count, done);
  wrap.appendChild(head);

  if (rows.length === 0) {
    const none = document.createElement("div");
    none.className = "set-note";
    none.style.margin = "6px 14px 10px";
    none.textContent = "Bind a few keys first \u2014 the ring is made of them.";
    wrap.appendChild(none);
  }

  const list = document.createElement("div");
  list.className = "eight-list";
  rows.forEach((r) => {
    const on = chosen.includes(r.key);
    const row = document.createElement("label");
    row.className = "eight-row" + (on ? " is-on" : "");

    const chip = document.createElement("span");
    chip.className = "eight-chip";
    chip.textContent = r.key.toUpperCase();

    const disc = document.createElement("span");
    disc.className = "eight-icon" + (r.link ? " is-link" : "");
    if (r.icon) {
      const img = document.createElement("img");
      img.alt = "";
      img.src = r.icon;
      img.onerror = () => { disc.innerHTML = ""; disc.textContent = placeholderPair(r.name); disc.classList.add("is-link"); };
      disc.appendChild(img);
    } else {
      // The placeholder circle: two letters of the host/name (artboard 9's
      // "rd" for reddit.com, "gh" for github.com).
      disc.textContent = placeholderPair(r.name);
    }

    const name = document.createElement("span");
    name.className = "eight-name";
    name.textContent = r.name;
    if (r.link) {
      const tag = document.createElement("span");
      tag.className = "eight-tag";
      tag.textContent = " \u00b7 link";
      name.appendChild(tag);
    }

    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.className = "eight-check";
    cb.checked = on;
    cb.disabled = !on && chosen.length >= FAVOURITES_MAX;
    cb.setAttribute("aria-label", `${r.name} in the ring`);
    cb.addEventListener("change", async () => {
      if (!appConfig) return;
      let next = [...chosen];
      if (cb.checked) {
        if (next.length >= FAVOURITES_MAX) { cb.checked = false; return; }
        if (!next.includes(r.key)) next.push(r.key);
      } else {
        next = next.filter((k) => k !== r.key);
      }
      appConfig.middle_ring_favourites = next;
      sfx.tick();
      await persistConfig();
      renderEightPicker();
      // The fifteenth tick completes the ring, so the card folds itself
      // ~400 ms later (owner, 1.0.110 review) — after the redraw above,
      // which is what shows the last row tinted and every other box greyed
      // first. Only a TICK arms it: un-ticking and re-ticking is a fresh
      // fifteenth, and a pick that was refused (`cb.checked = false` above)
      // never reaches this line.
      if (cb.checked && next.length === FAVOURITES_MAX) {
        _eightCloseTimer = window.setTimeout(() => {
          _eightCloseTimer = undefined;
          if (_eightOpen) closeEightPicker();
        }, EIGHT_AUTO_CLOSE_MS);
      }
    });

    row.append(chip, disc, name, cb);
    list.appendChild(row);
  });
  wrap.appendChild(list);

  const foot = document.createElement("div");
  foot.className = "eight-foot";
  foot.textContent = EIGHT_PICKER_NOTE;
  wrap.appendChild(foot);
  box.appendChild(wrap);
}

/** Two lowercase letters standing in for a favicon that has not arrived. */
function placeholderPair(name: string): string {
  const letters = name.replace(/[^a-z0-9]/gi, "");
  return (letters.slice(0, 2) || name.slice(0, 2)).toLowerCase();
}

function specialsRow(on: boolean, inert: boolean, i: number): string {
  const sw = toggleSwitchHtml(
    "hudspecials", on,
    _flipped?.id === "hudspecials" && _flipped.on === on ? (on ? "on" : "off") : undefined,
    "Show special keys", `set-hudspecials-note${descId("hudspecials") ? ` ${descId("hudspecials")}` : ""}`,
  );
  // The wrapper and the note are ALWAYS in the markup, shown or hidden by
  // inline style. That is what lets `paintSpecialsInert` flip this row without
  // replacing a node: the rows pill updates in place so its indicator can
  // slide, so there is no re-render to rebuild this row — and rebuilding it by
  // hand would drop the description listeners, which are wired per element
  // (`wireDescriptions`).
  return `
    <div class="set-item set-filterable" style="animation-delay:${60 + i * 45}ms">
      <div class="set-row" aria-disabled="${inert}">
        <button type="button" class="set-row-label" data-desc="hudspecials"
                aria-expanded="false" aria-controls="desc-hudspecials">Show special keys</button>
        <span id="set-hudspecials-wrap"${inert ? ' style="opacity:.45; pointer-events:none;"' : ""}>${
          inert ? sw.replace("<input ", "<input disabled ") : sw
        }</span>
      </div>
      <div class="set-note" id="set-hudspecials-note" style="margin-top:6px;${inert ? "" : "display:none;"}">${SPECIALS_INERT_NOTE}</div>
      ${descBox("hudspecials")}
    </div>`;
}

/**
 * Flip the specials row between live and inert WITHOUT rebuilding it.
 *
 * `.set-note` and not `.sma-note`: the latter is hidden unless "Show me
 * around" is on, which would hide the explanation from exactly the person who
 * just greyed the switch out. No new class and no new token — reduced opacity
 * and a display flip, both inline, both undone by passing `false`.
 *
 * Nothing here writes to `appConfig`. The switch keeps showing the user's own
 * `hud_show_specials`, so going back to 1 row restores their preference.
 */
function paintSpecialsInert(inert: boolean): void {
  paintRow("set-hudspecials-wrap", "set-hudspecials-note", inert);
}

// ---------------------------------------------------------------------------
// PROBLEM 250 — "Run at startup" when Windows owns it
// ---------------------------------------------------------------------------

/**
 * The "Run at startup" row. Identical to `toggleRow("startup", …)` for an
 * ordinary NSIS or MSI install, and greyed with a reason for the two copies
 * that do not own their own autostart: a Microsoft Store package (Windows
 * owns it) and the PORTABLE zip (nothing owns it — there is no Run key and no
 * Scheduled Task, by design).
 *
 * REVIEW FIXES 2026-09-05 (H6) — that second case is the correction. This
 * comment used to say "for every install that exists today" the row was
 * ordinary, which stopped being true when PROBLEM 254 shipped the portable
 * build, and the code agreed with the comment rather than with the build: the
 * greying rode on `get_packaged_startup` reporting `packaged: true` for a
 * portable copy, which it only does to reuse a tuple. The decision lives in
 * `controls.ts::startupRowIsInert` now and asks the honest question.
 *
 * WHY THIS ROW NEEDED ITS OWN FUNCTION. In an MSIX package the app does not
 * own autostart — Windows does. The user can switch Spaceadom off in Task
 * Manager ▸ Startup apps at any moment and the app is never told, and
 * `RequestEnableAsync` is documented to refuse to override that choice. So
 * there are states in which this switch genuinely cannot do anything, and
 * CLAUDE.md's rule applies exactly as it does to "Show special keys": *a
 * control that does nothing is worse than a missing control.* The treatment is
 * the same one, from the same leaf module (`paintInert`), so the two cannot
 * drift and `preview.ts` can draw either.
 *
 * The wrap span and the note div are ALWAYS in the markup, hidden by inline
 * style, for the reason `specialsRow` gives: the state arrives from an async
 * `invoke` AFTER this markup exists, so the paint has to be able to flip the
 * row without rebuilding it — rebuilding would drop the per-element
 * description listeners.
 *
 * **THE STORED VALUE IS NOT TOUCHED.** Greying is presentation. `run_at_startup`
 * keeps whatever the user chose, so an install that later stops being packaged
 * (they move to the setup.exe) finds their preference intact.
 */
function startupRow(on: boolean, i: number): string {
  const sw = toggleSwitchHtml(
    "startup", on,
    _flipped?.id === "startup" && _flipped.on === on ? (on ? "on" : "off") : undefined,
    "Run at startup", `set-startup-note${descId("startup") ? ` ${descId("startup")}` : ""}`,
  );
  // REVIEW FIXES 2026-09-05 (H6) — `startupRowIsInert`, not
  // `packaged && !mayChange`. The old expression made the PORTABLE copy's
  // greying depend on `get_packaged_startup` claiming `packaged: true` for it,
  // which it only does to reuse a tuple shape; the honest question is "may
  // this app change it", and `mayChange` is the field that answers it. See
  // controls.ts for the full account.
  const inert = startupRowIsInert(_pkgStartup);
  const note = _pkgStartup?.note ?? "";
  return `
    <div class="set-item set-filterable" style="animation-delay:${60 + i * 45}ms">
      <div class="set-row" aria-disabled="${inert}">
        <button type="button" class="set-row-label" data-desc="startup"
                aria-expanded="false" aria-controls="desc-startup">Run at startup</button>
        <span id="set-startup-wrap"${inert ? ' style="opacity:.45; pointer-events:none;"' : ""}>${
          inert ? sw.replace("<input ", "<input disabled ") : sw
        }</span>
      </div>
      <div class="set-note" id="set-startup-note" style="margin-top:6px;${inert ? "" : "display:none;"}">${note}</div>
      ${descBox("startup")}
    </div>`;
}

/**
 * Is the switch showing ON?
 *
 * A one-line binding of this panel's `_pkgStartup` to the shared rule in
 * `controls.ts` — REVIEW FIXES 2026-09-05 (H6). The rule itself moved out so
 * `preview.ts` can exercise it, and because the portable case it now covers
 * (always OFF, whatever config holds — a portable copy has no Run key and
 * never will) is the kind of thing that gets fixed in one file and not the
 * other.
 */
function startupShownAsOn(fromConfig: boolean): boolean {
  return ownershipShownAsOn(_pkgStartup, fromConfig);
}

/**
 * Ask Rust who owns the switch, then repaint the row in place.
 *
 * Called when the panel opens and again after the switch is used — NOT on the
 * render path. Silent on failure: an older backend has no `get_packaged_startup`
 * command, and an unpackaged install is the answer "nothing to do", so neither
 * case deserves a toast. `_pkgStartup` simply stays null and the row is the row
 * it has always been.
 */
async function refreshPackagedStartup(): Promise<void> {
  try {
    const [packaged, state, mayChange, note] =
      await invoke<[boolean, string, boolean, string]>("get_packaged_startup");
    _pkgStartup = { packaged, state, mayChange, note };
  } catch (_) {
    _pkgStartup = null;
    return;
  }
  paintPackagedStartup();
}

/**
 * Apply `_pkgStartup` to the live row without rebuilding it: the note's text,
 * the inert treatment, and the switch's own checked state — which for a
 * packaged install is Windows' answer, so a change made in Task Manager while
 * the panel was shut is visible the next time it opens.
 */
function paintPackagedStartup(): void {
  if (!panelEl) return;
  // REVIEW FIXES 2026-09-05 (H6) — the shared rule, not a second copy of
  // `packaged && !mayChange`. This was the OTHER half of the same expression
  // and it had to move with it: two places computing "is this row dead"
  // independently is how a row ends up greyed but writable, or writable but
  // greyed.
  const inert = startupRowIsInert(_pkgStartup);
  const noteEl = panelEl.querySelector<HTMLElement>("#set-startup-note");
  if (noteEl) noteEl.textContent = _pkgStartup?.note ?? "";
  const input = panelEl.querySelector<HTMLInputElement>("#set-startup");
  // `_pkgStartup !== null`, not `?.packaged`: the portable tuple's `packaged`
  // is a fiction, and a portable copy is the case that most needs the switch
  // forced OFF — a `run_at_startup: true` carried over from a config folder
  // copied out of an installed copy would otherwise draw a switch that is on,
  // dead, and wrong. `startupShownAsOn` decides; this only asks.
  if (input && _pkgStartup) input.checked = startupShownAsOn(input.checked);
  paintRow("set-startup-wrap", "set-startup-note", inert);
}

/**
 * `paintRowsInert` USED TO LIVE HERE, greying the "Shortcut rows" pill
 * whenever "New ring layout" was off. Both controls are gone (2026-09-01) and
 * so is it: the Compact/Wide/Double pill has no state in which it means
 * nothing, which is the whole reason the owner asked for one pill instead of
 * two dependent rows. `paintSpecialsInert` above is the only inert treatment
 * this panel still needs.
 *
 * The wrapper is one line on purpose: the treatment itself is `paintInert` in
 * `controls.ts`, the LEAF module, so `preview.ts` renders the identical dead
 * control without a backend. A second copy here would drift from the harness
 * the first time either was edited — and the half that goes missing is always
 * `disabled`, which looks perfect and leaves the control fully operable from
 * the keyboard.
 */
function paintRow(wrapId: string, noteId: string, inert: boolean): void {
  paintInert(
    panelEl?.querySelector<HTMLElement>(`#${wrapId}`),
    panelEl?.querySelector<HTMLElement>(`#${noteId}`),
    inert,
  );
}

// ---------------------------------------------------------------------------
// ABOUT (feature 1)
// ---------------------------------------------------------------------------

/**
 * Ask for the version/install-kind once, then repaint the row IN PLACE.
 *
 * Called from `openSettingsPanel()`, not from `render()` — same rule as
 * `refreshPackagedStartup`: the panel must appear at once, and the answer
 * (a Tauri round trip) arrives a few milliseconds later and repaints one
 * corner of one row rather than blocking the open. Every open, not once per
 * session: an update installed by the daily background check changes the
 * version this row should show the next time the user looks.
 */
async function refreshAboutInfo(): Promise<void> {
  _aboutInfo = await fetchAboutInfo();
  paintAboutInfo();
  // PROBLEM 249 — the way back, asked for in the same pass and for the same
  // reason: the answer changes when the daily background check installs
  // something while this panel is shut. `render()` is what actually puts the
  // button on screen (`aboutRowHtml` takes the version), so a change from
  // "none" to "1.0.99" needs one — but only when it is a CHANGE, because a
  // render here would otherwise replay the panel's entrance wave on every
  // single open, a few milliseconds after it had already played.
  const before = _rollback?.version ?? null;
  _rollback = await fetchRollbackTarget();
  if ((_rollback?.version ?? null) !== before) render();
}

/** Apply `_aboutInfo` to the live row without rebuilding it. */
function paintAboutInfo(): void {
  if (!panelEl) return;
  const verEl = panelEl.querySelector<HTMLElement>("#set-about-version");
  if (verEl) verEl.textContent = _aboutInfo?.version ? `Spaceadom · v${_aboutInfo.version}` : "Spaceadom";
  let kindEl = panelEl.querySelector<HTMLElement>("#set-about-kind");
  const idEl = panelEl.querySelector<HTMLElement>("#set-about-id") ?? verEl?.parentElement;
  if (_aboutInfo?.installKind) {
    if (!kindEl && idEl) {
      kindEl = document.createElement("div");
      kindEl.className = "set-note";
      kindEl.id = "set-about-kind";
      kindEl.style.marginTop = "2px";
      idEl.appendChild(kindEl);
    }
    if (kindEl) kindEl.textContent = _aboutInfo.installKind;
  } else {
    kindEl?.remove();
  }
}

/**
 * Wire everything in the About row: the update-check button, the four
 * external links, and the third-party list's expand/collapse. Called once
 * per `render()`, exactly like `wireDescriptions()` and the other per-render
 * wiring functions in this file — safe, because `render()` just replaced the
 * whole subtree these listeners attach to.
 */
function wireAboutSection(): void {
  if (!panelEl) return;

  // PROBLEM 249 — DISARM ON EVERY RENDER. `render()` rebuilds this subtree,
  // so the armed button ("Confirm — go back to 1.0.99") is destroyed and a
  // fresh one reading "Roll back to 1.0.99" takes its place — while
  // `_rollbackArmed`, which lives at module scope precisely so it can survive
  // a render, would still say `true`. The next click would then skip the
  // confirm the label is still promising and roll the machine back on one
  // press. This is the same class of bug `disarm()` was given its own
  // `render()` for in 2026-08-20 ("after confirming it still shows Confirm"),
  // arriving from the opposite direction: there the state outlived the label,
  // here the label outlives the state.
  _rollbackArmed = false;
  window.clearTimeout(_rollbackArmTimer);

  const checkBtn = panelEl.querySelector<HTMLButtonElement>("#set-about-check-update");
  checkBtn?.addEventListener("click", (e) => {
    e.stopPropagation();
    if (!checkBtn) return;
    sfx.tick();
    // Paint the busy state BEFORE the call, and let the `update-status`
    // listener take it from here.
    //
    // THE BUTTON IS NOT RE-ENABLED IN A `finally`, and that is the fix rather
    // than an oversight. `check_for_updates_now` does not resolve when an
    // update actually installs: PROBLEM 233's sequence is stop the hook, run
    // Tauri's exit cleanup, spawn the installer, exit — the process is gone
    // before any continuation runs. A `finally` there executes in exactly the
    // case where nothing happened and never in the case that matters, which
    // makes it a re-enable that lies about the app's state. `setUpdateBusy`
    // is driven by the event instead, and `emit_always` guarantees a terminal
    // status arrives once `downloading` has been emitted, manual or daily.
    setUpdateBusy(true);
    _updateStatusText = "Checking…";
    paintUpdateStatus();
    // Not awaited: the promise is a bonus, not the mechanism. When it does
    // resolve it carries the same `message` the event already carried, and
    // repainting it costs nothing; when it never resolves the event has
    // already said everything there is to say.
    void requestUpdateCheck().then((msg) => {
      _updateStatusText = msg;
      paintUpdateStatus();
    });
  });

  // PROBLEM 249 — the way back. Present only when `rollback_available()`
  // answered with a version, so there is no disabled state to explain.
  const rollBtn = panelEl.querySelector<HTMLButtonElement>("#set-about-rollback");
  rollBtn?.addEventListener("click", (e) => {
    e.stopPropagation();
    const version = rollBtn.dataset.rollbackVersion ?? _rollback?.version ?? "";
    // ONE-TAP CONFIRM, the same shape "Reset to defaults" and "Clear all"
    // use, and for a stronger reason than either: this one replaces the
    // running program with a different build of it, and on the NSIS leg it
    // does so by killing this process. A `window.confirm()` over this stage
    // looks like a different app (the rule this panel has followed since
    // 2026-08-20), so the button arms itself and disarms after a moment.
    if (!_rollbackArmed) {
      _rollbackArmed = true;
      rollBtn.textContent = `Confirm — go back to ${version}`;
      sfx.arm();
      window.clearTimeout(_rollbackArmTimer);
      _rollbackArmTimer = window.setTimeout(() => {
        _rollbackArmed = false;
        if (rollBtn.isConnected) rollBtn.textContent = `Roll back to ${version}`;
      }, 2600);
      return;
    }
    _rollbackArmed = false;
    window.clearTimeout(_rollbackArmTimer);
    sfx.confirm();
    // Painted BEFORE the call for the same reason the update button is:
    // `rollback_to_previous` normally never returns either — it stops the
    // hook, exits, and lets the archived installer replace the exe. A
    // resolved promise here means the rollback did NOT happen, and its string
    // is the explanation.
    rollBtn.disabled = true;
    rollBtn.textContent = "Going back…";
    setUpdateBusy(true);
    _updateStatusText = `Going back to ${version}… Spaceadom will restart itself.`;
    paintUpdateStatus();
    void requestRollback().then((msg) => {
      _updateStatusText = msg;
      paintUpdateStatus();
      setUpdateBusy(false);
      if (rollBtn.isConnected) {
        rollBtn.disabled = false;
        rollBtn.textContent = `Roll back to ${version}`;
      }
    });
  });

  panelEl.querySelectorAll<HTMLButtonElement>("[data-about-link]").forEach((b) => {
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      const kind = b.dataset.aboutLink as AboutLinkKind | undefined;
      if (!kind) return;
      sfx.tick();
      void openAboutLink(kind).catch(() => showToast("⚠️ Could not open that link"));
    });
  });

  const tpToggle = panelEl.querySelector<HTMLButtonElement>("#set-about-tp-toggle");
  const tpList = panelEl.querySelector<HTMLElement>("#set-about-tp-list");
  tpToggle?.addEventListener("click", (e) => {
    e.stopPropagation();
    _thirdPartyOpen = !_thirdPartyOpen;
    tpToggle.setAttribute("aria-expanded", String(_thirdPartyOpen));
    if (tpList) {
      tpList.hidden = !_thirdPartyOpen;
      // LAZY, per the owner's instruction: the grouped markup for several
      // hundred packages is built once, on first expand, and cached in the
      // DOM from then on — never at panel-open time, when nobody has asked
      // for it yet.
      if (_thirdPartyOpen && !tpList.dataset.built) {
        tpList.dataset.built = "1";
        tpList.innerHTML = renderThirdPartyGroups(THIRD_PARTY);
        tpList.querySelectorAll<HTMLAnchorElement>("[data-tp-link]").forEach((a) => {
          // REVIEW FIXES 2026-09-05 (LOW) — `auxclick` as well as `click`,
          // the same correction made in `whats-new-sheet.ts` and for the same
          // reason. These anchors carry a real `href` (so the context menu's
          // "Copy link address" works), and a middle-click or Ctrl+click does
          // not fire `click` — it fires `auxclick`, and the webview then
          // follows the href itself. There is no tab to open it in, so the
          // DASHBOARD navigates to the package's homepage and the app is gone
          // until it is restarted. Found here while fixing the What's New
          // sheet; the class is "an intercepted <a> needs both events, or the
          // href it keeps for polish becomes an escape hatch out of the app".
          const openInBrowser = (ev: Event) => {
            ev.preventDefault();
            void openUrl(a.dataset.tpLink!).catch(() => {});
          };
          a.addEventListener("click", openInBrowser);
          a.addEventListener("auxclick", openInBrowser);
        });
      }
    }
    if (_thirdPartyOpen) sfx.bloomOpen(); else sfx.bloomClose();
  });

  wireUpdateStatusListener();
}

/** Paint just the update-status line, without touching anything else in the
 *  row — the button click handler above calls this twice (immediately, and
 *  again when the check settles) and a full `render()` for either would
 *  replay the whole panel's entrance wave for one line of text. */
function paintUpdateStatus(): void {
  const el = panelEl?.querySelector<HTMLElement>("#set-about-update-status");
  if (el) el.textContent = _updateStatusText;
}

/**
 * The `update-status` event fires for the DAILY background check too, not
 * only for a press of this row's own button — a check that starts on its own
 * schedule while the panel happens to be open should still move this line.
 * Wired ONCE (module scope survives `render()`'s teardown of the DOM it
 * reads from — the query at listener time always finds whatever the CURRENT
 * row is, since it re-queries `panelEl` on every event rather than closing
 * over a node).
 */
function wireUpdateStatusListener(): void {
  // TWO flags, and the second is what makes the first one's move safe. This
  // function is called at the end of every `render()`, i.e. after every
  // toggle, so between the `listen()` call and its resolution there can be
  // several more calls — and a bare "not wired yet" guard would register a
  // second, third and fourth listener, each of which would repaint the status
  // line on every event. `_updateStatusListenPending` covers exactly that gap:
  // in flight counts as taken, resolved counts as done, and only a REJECTION
  // frees it to be tried again.
  if (_updateStatusListenerWired || _updateStatusListenPending) return;
  _updateStatusListenPending = true;
  // REVIEW FIXES 2026-09-05 (LOW) — THE FLAG IS SET WHEN `listen()` RESOLVES,
  // NOT WHEN IT IS CALLED.
  //
  // `listen()` returns a promise and the registration is not live until it
  // settles. Setting the flag first meant that if the first call REJECTED —
  // the window missing from `capabilities/default.json`, which CLAUDE.md
  // records as the failure that makes a window silently deaf, or a plugin not
  // yet initialised when the panel is opened very early — every later call
  // returned at the line above without trying again. One transient rejection
  // and the About row's status line was dead for the life of the process,
  // including for the DAILY background check this listener exists to catch.
  //
  // Set in `.then` and cleared in `.catch`, so a failure is retried the next
  // time the panel is opened. The `void` is deliberate: nothing awaits this,
  // exactly as before.
  void listen<{ state?: string; message?: string }>("update-status", (e) => {
    // THE CONTRACT (updater.rs, PROBLEM 249): switch on the states you
    // handle, and show `message` VERBATIM for everything else. `message` is
    // always a finished English sentence, never a code and never a fragment,
    // which is what makes that rule safe — a state added in a later build
    // shows its own sentence here instead of producing a blank line.
    //
    // So this listener switches on exactly one thing, "is the app still
    // working on it", and prints the sentence for all seven states.
    _updateStatusText = e.payload?.message || _updateStatusText;
    setUpdateBusy(updateStateIsBusy(e.payload?.state));
    paintUpdateStatus();
  })
    .then(() => {
      _updateStatusListenerWired = true;
      _updateStatusListenPending = false;
    })
    .catch(() => {
      // Both left FALSE, so the next panel open tries again. An older build
      // without the updater emits nothing and this simply never succeeds —
      // which costs one rejected promise per open and no toast, the same
      // silence it had before.
      _updateStatusListenerWired = false;
      _updateStatusListenPending = false;
    });
}

/**
 * Enable or disable the About row's two update controls together.
 *
 * Together, because they are two directions of one journey and neither is
 * safe while the other is running: a rollback started mid-download would race
 * two installers over the same exe.
 *
 * Re-queried from `panelEl` on every call rather than closed over, exactly
 * like `paintUpdateStatus` — `render()` replaces this subtree, and the
 * `update-status` listener outlives any particular button node.
 */
function setUpdateBusy(busy: boolean): void {
  const check = panelEl?.querySelector<HTMLButtonElement>("#set-about-check-update");
  if (check) check.disabled = busy;
  const roll = panelEl?.querySelector<HTMLButtonElement>("#set-about-rollback");
  if (roll) roll.disabled = busy;
}

// ---------------------------------------------------------------------------
// Typing speed (PROBLEM 69)
// ---------------------------------------------------------------------------
// Replaces the old raw "Rollover window (ms)" slider. Same underlying knob,
// asked in a question a human can answer. A FASTER typist needs a WIDER
// window: fast typists press the next letter before releasing Space, and an
// overlap longer than the window reads as a deliberate Space+key command —
// an app launching in the middle of a sentence.
//
// PROBLEM 72 — THIS MAPPING WAS BACKWARDS AND SHIPPED (1.0.6/1.0.7). It read
// `wpm * 1.4 + 20`, so "Slow" produced a 62ms window and ordinary typing fired
// commands. The hook measures the delay from Space-down to the next letter,
// which tracks the inter-key interval (12000/wpm) and gets SHORTER as speed
// rises — so the window is INVERSELY proportional to speed. 8400/wpm is
// anchored so the default 70 wpm lands on exactly 120ms, the value this app
// shipped with for months before the slider existed.
//
// The 110ms floor is the load-bearing part: it makes the 62ms failure
// unreachable from anywhere on the slider.
//
// Mirrors rollover_ms_for_wpm() in src-tauri/src/config/schema.rs.
// If you change one, change the other.
const WPM_MIN = 30;
const WPM_MAX = 150;
export const DEFAULT_WPM = 70; // -> exactly 120ms

/**
 * PROBLEM 95 — MIRRORS `rollover_ms_for_wpm` in src-tauri/src/config/schema.rs.
 * Change both together; the Rust version carries the full reasoning.
 *
 * Was `8400 / wpm`, which is 0.7x the typist's own inter-key interval
 * (12000 / wpm) — i.e. the window sat BELOW ordinary typing at every setting.
 * Measured: at 70 wpm / 120 ms, a 180 ms spacebar hold turned 18 of 18 words
 * into commands.
 */
export function rolloverMsForWpm(wpm: number): number {
  return Math.min(300, Math.max(200, Math.round(16800 / Math.max(1, wpm))));
}

/** The four tiers, positioned across the slider by their midpoint WPM. */
const TYPING_TIERS: ReadonlyArray<{ name: string; from: number; to: number }> = [
  { name: "Slow",      from: WPM_MIN, to: 44 },
  { name: "Regular",   from: 45,      to: 74 },
  { name: "Fast",      from: 75,      to: 104 },
  { name: "Very fast", from: 105,     to: WPM_MAX },
];

export function typingTierName(wpm: number): string {
  return TYPING_TIERS.find((t) => wpm >= t.from && wpm <= t.to)?.name ?? "Regular";
}

function typingSpeedRow(wpm: number): string {
  // Labels sit ABOVE the slider, each at the position of its band's midpoint,
  // so the name lines up with the part of the track it covers.
  const ticks = TYPING_TIERS.map((t) => {
    const mid = (t.from + t.to) / 2;
    const pct = ((mid - WPM_MIN) / (WPM_MAX - WPM_MIN)) * 100;
    return `<span class="wpm-tick" data-tier="${t.name}"
              style="left:${pct.toFixed(1)}%">${t.name}</span>`;
  }).join("");

  // WRAPPED IN `.set-item.set-filterable` since 2026-09-01. The slider rows
  // were the only rows in this panel that were a bare `.set-row`, which meant
  // the search would have filtered every switch and skipped all three
  // sliders — the kind of half-working filter that reads as "search is
  // broken" rather than "these three are special". The wrapper also moves the
  // entrance animation from `.set-row` to `.set-item`, matching every other
  // row (`#settings-panel.pop .set-item`, and `.set-item .set-row` cancels
  // the inner one).
  return `
    <div class="set-item set-filterable">
    <div class="set-row" style="flex-direction:column; align-items:stretch; gap:4px; cursor:default; margin-bottom:10px;">
      <div style="display:flex; align-items:baseline; gap:8px;">
        <button type="button" class="set-row-label" data-desc="wpm"
                aria-expanded="false" aria-controls="desc-wpm">Typing speed</button>
        <span style="font-size:11px; font-weight:700; color:var(--st-accent-deep);"
              id="set-wpm-val">${typingTierName(wpm)} · ${wpm} wpm</span>
      </div>
      <div class="wpm-ticks" id="set-wpm-ticks">${ticks}</div>
      ${sliderShell("wpm", `
        <input type="range" id="set-wpm" min="${WPM_MIN}" max="${WPM_MAX}" step="5" value="${wpm}"
               aria-label="Typing speed"${descId("wpm") ? ` aria-describedby="${descId("wpm")}"` : ""} />`,
        WPM_MIN, WPM_MAX, wpm)}
      <span class="sma-note" style="font-size:10.5px; color:var(--st-ink-soft); line-height:1.35; margin-top:2px;">
        If apps launch by accident while you type, choose a SLOWER speed —
        Spaceadom then waits longer before treating Space+key as a shortcut.
      </span>
      ${descBox("wpm")}
    </div>
    </div>`;
}

function sliderRow(
  id: string, label: string, value: number,
  min: number, max: number, step: number, unit: string,
): string {
  const desc = descId(id);
  return `
    <div class="set-item set-filterable">
    <div class="set-row" style="flex-direction:column; align-items:stretch; gap:4px; cursor:default; margin-bottom:10px;">
      <div style="display:flex; align-items:baseline; gap:8px;">
        <button type="button" class="set-row-label" data-desc="${id}"
                aria-expanded="false" aria-controls="desc-${id}">${label}</button>
        <span style="font-size:11px; font-weight:700; color:var(--st-accent-deep);" id="set-${id}-val">${value}${unit}</span>
      </div>
      ${sliderShell(id, `
        <input type="range" id="set-${id}" min="${min}" max="${max}" step="${step}" value="${value}"
               data-unit="${unit}" aria-label="${label}"${desc ? ` aria-describedby="${desc}"` : ""} />`, min, max, value)}
      ${descBox(id)}
    </div>
    </div>`;
}

// ---------------------------------------------------------------------------
// SEARCH + LIVE RING PREVIEW (2026-09-01)
// ---------------------------------------------------------------------------

/**
 * Wire the search box and re-apply whatever was already typed.
 *
 * ZERO COST WHEN UNUSED, and that is the constraint the owner set. Nothing
 * here walks the DOM unless the box has text: `filterSettings` is called from
 * this function only when `_query` is non-empty, and thereafter only from the
 * `input` event. The per-row haystack is built lazily inside `filterSettings`
 * and cached on the element, so an untouched panel never reads a single
 * `textContent`.
 *
 * `keydown` is stopped for the same reason the app-exceptions search stops it:
 * this panel reacts to stray keys (Escape collapses the expand, Space blurs
 * buttons), and typing "space" into a search box must not trigger any of it.
 */
function wireSearch(): void {
  const box = panelEl?.querySelector<HTMLInputElement>("#set-search");
  if (!box || !panelEl) return;
  box.value = _query;
  if (_query) filterSettings(panelEl, _query);

  box.addEventListener("input", () => {
    _query = box.value;
    if (panelEl) filterSettings(panelEl, _query);
  });
  box.addEventListener("keydown", (e) => {
    e.stopPropagation();
    // Escape inside the box clears the filter first — one more peel, and the
    // one a person reaches for while looking at a filtered list.
    if (e.key === "Escape" && _query) {
      e.preventDefault();
      _query = "";
      box.value = "";
      if (panelEl) filterSettings(panelEl, "");
    }
  });
  box.addEventListener("click", (e) => e.stopPropagation());
}

/**
 * Show the ring shape being chosen, in the real overlay, as it is chosen.
 *
 * THE BODY MOVED TO `controls.ts` (1.0.97) and this is now a one-line
 * forward. It had to move because the thing the owner asked for is not "fire
 * the command" but "fire it only while one is already on screen", and that
 * needs a piece of STATE — when the last projection is due to hide — which
 * `render()` would destroy if it lived on an element here. Putting the state
 * and both entry points in the leaf module also means `preview.ts` exercises
 * the real gate rather than a copy of it; a harness that owns a second copy of
 * a decision can only ever agree with itself.
 *
 * Every failure is still swallowed, for the reason it always was: a settings
 * panel that threw because a preview command was missing would be a worse
 * outcome than no preview at all.
 */
function previewRing(layout: RingLayout): Promise<void> {
  return showRingPreview(layout);
}

/**
 * The current ring shape, read through the SAME pure mapping every other
 * caller uses — never re-derived from the two config fields by hand.
 */
function currentRingLayout(): RingLayout {
  return ringLayoutFor(appConfig?.hud_magnetic_layout, appConfig?.hud_band_count);
}

/**
 * PROBLEM — the preview that would not re-fire (owner, 2026-09-04):
 *
 *   *"while the ring preview is showing, toggling 'Show special keys' leaves
 *   the old preview up; the change only appears next time."*
 *
 * Rust builds the projection ONCE, at `preview_hud_layout` time, and nothing
 * re-sends it — so a setting changed while the ring is up is a setting the
 * ring cannot know about. Every control that feeds the payload calls this
 * after its own `persistConfig()`, so the command reads the NEW config:
 * "Show special keys", the Compact/Wide/Double pill, the theme pill (the ring
 * wears the app's palette) and "Point to launch".
 *
 * `refreshRingPreview` fires NOTHING when no projection is on screen. That is
 * the half of this that matters: a switch flipped in a quiet settings panel
 * must not throw a full-screen overlay up in front of the user.
 */
function refireRingPreview(): void {
  void refreshRingPreview(currentRingLayout());
}

function wireToggle(id: string, onChange: () => void | Promise<void>): void {
  const el = panelEl?.querySelector<HTMLInputElement>(`#set-${id}`);
  el?.addEventListener("change", () => { markFlipped(id, el.checked); void onChange(); });
}

/**
 * The slider's personality (spec §3) and its two sounds — one function for all
 * three sliders, so a new slider cannot arrive silent and undecorated.
 *
 * Everything here writes to the WRAPPER, never to the input: `--p` for the
 * fill and the decorations' positions, `data-dir` so the comet's tail trails
 * the direction of travel rather than leading it, and `.is-drag` for the
 * glow and the orbit's faster spin. The input keeps its own semantics.
 */
function wireSliderChar(id: string): void {
  const el = panelEl?.querySelector<HTMLInputElement>(`#set-${id}`);
  const shell = panelEl?.querySelector<HTMLElement>(`#sld-${id}`);
  if (!el || !shell) return;

  const min = parseFloat(el.min || "0");
  const max = parseFloat(el.max || "100");
  let last = parseFloat(el.value);

  const paint = (): void => {
    const v = parseFloat(el.value);
    // --p is the ONLY thing JS writes for position; --x and every decoration
    // derive from it in CSS. A measured pixel --x was tried and reverted: it
    // was chasing a drift that did not exist. See characters.css §3.
    if (max > min) shell.style.setProperty("--p", ((v - min) / (max - min)).toFixed(4));
    // 0 is not a direction — hold the last one so the tail does not flip to a
    // default every time the handle pauses.
    if (v !== last) shell.dataset.dir = v > last ? "1" : "-1";
    last = v;
  };

  el.addEventListener("input", paint);
  // pointerdown, not mousedown: this is a touchscreen laptop.
  el.addEventListener("pointerdown", () => { shell.classList.add("is-drag"); sfx.sliderGrab(); });
  // Keyboard adjustment is a real adjustment: it must move the fill too.
  el.addEventListener("change", paint);
  paint();
  wireSliderRelease();
}

/**
 * The end of a drag, wired ONCE for the whole panel.
 *
 * It has to live on `window`: a drag that ends with the pointer off the track
 * — which is most of them — never delivers pointerup to the input. But
 * render() rebuilds this panel after every setting change, so a per-slider
 * window listener would stack up a new pair on every render and outlive the
 * elements they close over. One listener, one flag, no accumulation.
 */
let _sliderUpWired = false;
function wireSliderRelease(): void {
  if (_sliderUpWired) return;
  _sliderUpWired = true;
  const up = (): void => {
    const dragging = document.querySelectorAll<HTMLElement>(".sld.is-drag");
    if (dragging.length === 0) return;
    dragging.forEach((sh) => sh.classList.remove("is-drag"));
    sfx.sliderRelease();
  };
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", up);
}

/** Live tier readout while dragging; persists both wpm and the derived ms. */
function wireTypingSpeed(): void {
  const el = panelEl?.querySelector<HTMLInputElement>("#set-wpm");
  const out = panelEl?.querySelector<HTMLElement>("#set-wpm-val");
  const ticks = panelEl?.querySelector<HTMLElement>("#set-wpm-ticks");
  if (!el) return;

  const paint = (wpm: number) => {
    const tier = typingTierName(wpm);
    if (out) out.textContent = `${tier} · ${wpm} wpm`;
    // Highlight the label for the band the handle is currently in.
    ticks?.querySelectorAll<HTMLElement>(".wpm-tick").forEach((t) => {
      t.classList.toggle("is-active", t.dataset.tier === tier);
    });
  };
  paint(parseInt(el.value, 10));
  wireSliderChar("wpm");

  el.addEventListener("input", () => paint(parseInt(el.value, 10)));
  el.addEventListener("change", async () => {
    if (!appConfig) return;
    const wpm = parseInt(el.value, 10);
    appConfig.typing_wpm = wpm;
    // Derived, never entered by hand — one knob, two representations.
    appConfig.rollover_ms = rolloverMsForWpm(wpm);
    await persistConfig();
    showToast(`⌨️ ${typingTierName(wpm)} typing · ${appConfig.rollover_ms}ms window`);
  });
}

function wireSlider(id: string, apply: (v: number) => void): void {
  const el = panelEl?.querySelector<HTMLInputElement>(`#set-${id}`);
  const out = panelEl?.querySelector<HTMLElement>(`#set-${id}-val`);
  if (!el) return;
  const unit = el.dataset.unit ?? "";
  wireSliderChar(id);
  el.addEventListener("input", () => { if (out) out.textContent = el.value + unit; });
  el.addEventListener("change", async () => {
    apply(parseInt(el.value, 10));
    await persistConfig();
    showToast("⚙️ Settings saved");
  });
}

function arm(which: "def" | "clr"): void {
  _armed = which;
  window.clearTimeout(_armTimer);
  _armTimer = window.setTimeout(() => { _armed = null; render(); }, 2600);
  render();
}

function disarm(): void {
  _armed = null;
  window.clearTimeout(_armTimer);
  // render(), same as arm(). Without it the button kept reading "Confirm"
  // after the action had already fired — the owner's "after confirming it
  // still shows Confirm, which feels like a bug" (2026-08-20). arm() always
  // re-rendered; its opposite never did.
  render();
}
