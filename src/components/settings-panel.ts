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
import {
  appConfig, persistConfig, applySound, applyMotion,
  applyLook, applySkyMode, knownConflicts, refreshConflicts,
} from "../main";
import { sfx } from "../sfx";
import { openConflictPrompt } from "./conflict-prompt";
import { toggleSwitchHtml, sliderShell } from "./controls";
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

let _onResetDefaults: (() => void) | null = null;
let _onClearAll: (() => void) | null = null;

export function initSettingsPanel(
  onResetDefaults: () => void,
  onClearAll: () => void,
): void {
  panelEl = document.getElementById("settings-panel");
  _onResetDefaults = onResetDefaults;
  _onClearAll = onClearAll;
  if (!panelEl) return;
  render();
}

export function isSettingsPanelOpen(): boolean {
  return !!panelEl && !panelEl.hidden;
}

export function openSettingsPanel(): void {
  if (!panelEl) return;
  _freshOpen = true;      // PROBLEM 144 — arm the one-shot "Show me around"
  render();
  panelEl.hidden = false;
  document.getElementById("gear-btn")?.setAttribute("aria-expanded", "true");
}

export function closeSettingsPanel(): void {
  if (!panelEl) return;
  panelEl.hidden = true;
  document.getElementById("gear-btn")?.setAttribute("aria-expanded", "false");
  disarm();
}

/** Keep the Engine toggle honest when the engine is paused from elsewhere. */
export function setPausedState(paused: boolean): void {
  _paused = paused;
  if (isSettingsPanelOpen()) render();
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

  // PROBLEM 92 — the escape hatch for the invisible-overlay bug. This is
  // normally set BY the app (the pixel self-test measures that this machine's
  // driver cannot composite the transparent overlay and writes "software"),
  // but it must be reachable by hand: the verdict is never reverted
  // automatically, so a false positive would otherwise be permanent.
  const software = appConfig.overlay_compositing === "software";

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
  document.body.classList.toggle("show-around", showAround);

  panelEl.innerHTML = `
    <div class="set-title">Settings</div>

    <div class="set-rows">
      ${toggleRow("around",  "Show me around", showAround, 0)}
      ${themeRow(theme, 1)}
      ${toggleRow("engine",  "Engine active",  !_paused, 2)}
      ${toggleRow("fun",     "Fun mode",       fun,      3)}
      ${toggleRow("sound",   "Sound ticks",    sound,    4)}
      ${toggleRow("startup", "Run at startup", startup,  5)}
      ${toggleRow("motion",  "Visual effects", effects,  6)}
      ${toggleRow("hideboard", "Hide the keyboard", hideBoard, 7)}
      ${toggleRow("software", "Software overlay", software, 8)}
      ${toggleRow("flight",   "Guide-to-toast motion", flight, 9)}
    </div>

    <div class="divider" style="margin:14px 0 10px;"></div>

    ${typingSpeedRow(appConfig.typing_wpm ?? DEFAULT_WPM)}
    ${sliderRow("huddelay", "Guide HUD delay", appConfig.guide_hud_delay_ms, 100, 1000, 50, "ms")}
    ${sliderRow("opacity",  "Opacity floor",   appConfig.opacity_floor_pct, 10, 90, 5, "%")}


    <div class="divider" style="margin:14px 0 10px;"></div>
    <button type="button" class="set-title set-row-label" data-desc="appexceptions"
            aria-expanded="false" style="font-size:13px; margin-bottom:8px;">App exceptions</button>
    ${descBox("appexceptions")}
    <div id="set-app-exceptions"></div>

    <div class="divider" style="margin:14px 0 10px;"></div>
    <button type="button" class="set-title set-row-label" data-desc="conflicts"
            aria-expanded="false" style="font-size:13px; margin-bottom:8px;">Conflicts</button>
    ${descBox("conflicts")}
    <div id="set-conflicts"></div>

    <div class="set-actions">
      <button class="btn" id="set-reset">${_armed === "def" ? "Confirm" : resetLabel()}</button>
      <button class="btn btn-danger" id="set-clear">${_armed === "clr" ? "Confirm clear" : "Clear all"}</button>
    </div>
    ${descBox("reset")}
    ${descBox("clear")}
    <button class="btn" id="set-presets" style="width:100%; justify-content:center; margin-top:7px; height:34px; font-size:12px;">Restore preset profiles</button>
    ${descBox("presets")}
    <button class="btn" id="set-logs" style="width:100%; justify-content:center; margin-top:7px; height:34px; font-size:12px;">Open log folder</button>
    ${descBox("logs")}
    <!-- The four action buttons cannot BE their own description trigger: their
         press already does something destructive or irreversible. Their
         descriptions ride the "Show me around" convoy and the ⓘ row below
         instead, which is why they have a box but no data-desc label. -->
    <button type="button" class="set-help-all sma-note" id="set-help-all">
      ⓘ What do these buttons do?
    </button>
  `;

  // One render, one animation. Anything after this point sees a clean slate,
  // so a later render (a toast, a conflict re-check) cannot replay a character
  // the user pressed minutes ago.
  _flipped = null;

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
      // sounds.js names the middle theme "war", not "warcry".
      sfx.theme(next === "warcry" ? "war" : next);
      // dark_mode stays the single source of truth for body.nocturne on BOTH
      // windows — the overlay has no idea themes exist (CLAUDE.md theme rule).
      appConfig.dark_mode = next !== "earthy";
      applyLook();

      // PROBLEM 157 — update the pill IN PLACE. render() rebuilds the panel,
      // which DESTROYS the indicator and creates a new one already at the new
      // position — and a brand-new element has nothing to transition FROM.
      // That is the whole reason the owner reported "the satisfying animation
      // of the slider sliding smoothly is not there anymore": the CSS never
      // stopped being correct, the element just stopped surviving the change.
      const seg = panelEl?.querySelector<HTMLElement>(".theme-seg");
      const idx = ["earthy", "warcry", "starry"].indexOf(next);
      if (seg && idx >= 0) seg.style.setProperty("--seg-i", String(idx));
      seg?.querySelector<HTMLElement>(".theme-seg-ind")?.setAttribute("data-seg", next);
      panelEl?.querySelectorAll<HTMLElement>("[data-theme-set]").forEach((o) => {
        const on = o.dataset.themeSet === next;
        o.classList.toggle("is-on", on);
        o.setAttribute("aria-checked", String(on));
      });
      await persistConfig();
    });
  });

  // "Show me around" — open or close every description as a convoy.
  wireToggle("around", async () => {
    if (!appConfig) return;
    const on = !(appConfig.show_me_around === true);
    appConfig.show_me_around = on;
    document.body.classList.toggle("show-around", on);
    convoyAll(on);
    if (on) sfx.convoyOn(); else sfx.convoyOff();
    await persistConfig();
    // deliberately NOT render() — a re-render would wipe the convoy mid-flight
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
    const next = !(appConfig.run_at_startup !== false);
    appConfig.run_at_startup = next;
    try {
      // ONE command persists config AND flips the Scheduled Task, so the two
      // can never drift apart. Not persistConfig() — that path doesn't touch
      // the task.
      await invoke("set_startup_enabled", { enabled: next });
      if (next) sfx.toggleOn("startup"); else sfx.toggleOff("startup");
      showToast(next ? "🚀 Starts with Windows" : "🚀 Won't start with Windows");
    } catch (_) {
      appConfig.run_at_startup = !next;   // revert on failure
      showToast("⚠️ Could not change the startup task");
    }
    render();
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

  // PROBLEM 92 — goes through its OWN command, not persistConfig(). The
  // dashboard's config is a snapshot taken at bootstrap; if the pixel
  // self-test flips this field while the panel is open, a normal save would
  // write the stale value straight back over the app's own measurement.
  wireToggle("software", async () => {
    if (!appConfig) return;
    const next = appConfig.overlay_compositing === "software" ? "auto" : "software";
    try {
      await invoke("set_overlay_compositing", { mode: next });
      appConfig.overlay_compositing = next;
      if (next === "software") sfx.toggleOn("software"); else sfx.toggleOff("software");
      showToast(
        next === "software"
          ? "🖥️ Software overlay on — restart Spaceadom to apply"
          : "⚡ Software overlay off — restart Spaceadom to apply",
      );
    } catch (e) {
      console.error("set_overlay_compositing failed:", e);
      showToast("⚠️ Could not change overlay rendering");
    }
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

  renderAppExceptions();
  renderConflicts();

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

/** The stems currently excluded, always lowercase. */
function excludedList(): string[] {
  return (appConfig?.excluded_apps ?? []).map((s) => s.toLowerCase());
}

async function setExcluded(list: string[]): Promise<void> {
  if (!appConfig) return;
  appConfig.excluded_apps = Array.from(new Set(list.map((s) => s.toLowerCase())));
  await persistConfig();
  renderAppExceptions();
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

  // Warm the scan now so pressing "Add an app" is not a blank grid.
  void loadApps();

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

    if (list.length === 0) {
      const none = document.createElement("div");
      none.className = "set-note";
      none.style.marginTop = "0";
      none.textContent = "No exceptions yet — Spaceadom works everywhere.";
      box.appendChild(none);
    } else {
      const grid = document.createElement("div");
      grid.className = "exc-grid";

      list.forEach((stem, i) => {
        const hit = byStem.get(stem);
        const label = hit?.name ?? stem;

        const tile = document.createElement("div");
        tile.className = "exc-tile";
        tile.title = label;   // full name discoverable on hover for truncated ones

        const disc = document.createElement("span");
        disc.className = "exc-tile-disc";
        paintAppDisc(disc, hit?.icon, label, i);

        const name = document.createElement("span");
        name.className = "exc-tile-name";
        name.textContent = label;   // textContent — user data

        const remove = document.createElement("button");
        remove.type = "button";
        remove.className = "exc-tile-x";
        remove.setAttribute("aria-label", `Remove ${label} from exceptions`);
        remove.textContent = "✕";
        remove.addEventListener("click", async (e) => {
          e.stopPropagation();
          sfx.tick();
          await setExcluded(excludedList().filter((s) => s !== stem));
        });

        tile.append(disc, name, remove);
        grid.appendChild(tile);
      });

      box.appendChild(grid);
    }

    const add = document.createElement("button");
    add.type = "button";
    add.className = "btn btn-sm";
    add.style.cssText = "width:100%; justify-content:center; margin-top:8px;";
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
      const current = new Set(excludedList());
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
  if (list.includes(stem)) {
    showToast(`${label} is already an exception`);
    return;
  }
  sfx.confirm();
  await setExcluded([...list, stem]);
  showToast(`Spaceadom will pause inside ${label}`);
}

function renderConflicts(): void {
  const box = panelEl?.querySelector<HTMLElement>("#set-conflicts");
  if (!box) return;

  const draw = () => {
    box.innerHTML = "";

    if (knownConflicts.length === 0) {
      const ok = document.createElement("div");
      ok.className = "set-note";
      ok.style.marginTop = "0";
      ok.textContent = "Nothing else is remapping your keyboard.";
      box.appendChild(ok);
    } else {
      knownConflicts.forEach((c, i) => {
        const row = document.createElement("div");
        row.className = "conflict-row";

        // The conflicting program's icon, when we already know it. The cheap
        // path: `c.process` is a bare exe filename ("autohotkey64.exe"), and
        // exeStem() strips a path separator only when one is present, so it
        // works unchanged on a bare filename too — no new Rust command, just
        // a stem lookup against the SAME Start-Menu scan the exceptions grid
        // already warms. Degrades to the letter disc exactly like every
        // other app icon in this app if the scan hasn't found a match.
        const disc = document.createElement("span");
        disc.className = "conflict-row-disc";
        const known = findAppByStem(exeStem(c.process));
        paintAppDisc(disc, known?.icon_base64, known?.name ?? c.product, i);

        const name = document.createElement("span");
        name.className = "conflict-row-name";
        name.textContent = c.product;          // textContent — read off the machine

        const proc = document.createElement("span");
        proc.className = "conflict-row-proc";
        proc.textContent = c.process;

        // The one-liner is back UNGATED (owner, 2026-08-20: "the previous
        // small one-liner description of the app, what it does and what it
        // was conflicting, was good — bring it back"). It is also what makes
        // the long Conflicts description unnecessary.
        const why = document.createElement("span");
        why.className = "conflict-row-why";
        why.textContent = c.detail;

        // PROBLEM 157 — the ROW is the button now. Two permanent buttons per
        // conflict was clutter for something you act on once ("this thing
        // always staying there in the settings isn't worth it"); pressing the
        // program raises the offer at the top of the screen instead.
        row.setAttribute("role", "button");
        row.tabIndex = 0;
        const open = (e: Event) => { e.stopPropagation(); openConflictPrompt(c, draw); };
        row.addEventListener("click", open);
        row.addEventListener("keydown", (e) => {
          if (e.key === "Enter" || e.key === " ") open(e);
        });

        const hint = document.createElement("span");
        hint.className = "conflict-row-cta";
        hint.textContent = "Press to close it →";
        row.append(disc, name, proc, why, hint);
        box.appendChild(row);
      });

      const hint = document.createElement("div");
      hint.className = "set-note sma-note";
      // The old text said "Spaceadom never closes other programs for you" —
      // which the Conflicts description above ALSO said, and which stopped
      // being true on 2026-08-20 when the owner asked for the button below.
      hint.textContent = "Press one to have Spaceadom close it for you.";
      box.appendChild(hint);
    }

    void drawHookHealth(box, draw);

    const again = document.createElement("button");
    again.className = "btn btn-sm";
    again.style.cssText = "width:100%; justify-content:center; margin-top:8px;";
    again.textContent = "Re-check now";
    again.addEventListener("click", async () => {
      sfx.tick();
      again.textContent = "Checking…";
      await refreshConflicts();
      sfx.confirm();
      draw();
    });
    box.appendChild(again);
  };

  draw();

  // The exceptions section (rendered just before this one) already warms the
  // Start-Menu scan; if it's still running when conflicts first draw, redraw
  // once it lands so a row that opened on a letter fallback picks up the
  // real icon instead of staying stuck on it for the rest of the session.
  void loadApps().then(() => { if (panelEl && !panelEl.hidden) draw(); });
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
async function drawHookHealth(box: HTMLElement, redraw: () => void): Promise<void> {
  let h: { timeout_ms: number | null; raised: boolean; evictions: number; rivals: string[] };
  try {
    h = await invoke("get_hook_health");
  } catch {
    return; // an older backend, or the command is unavailable — say nothing
  }
  if (!h || (h.evictions === 0 && !h.raised)) return;

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
  const said = h.rivals.length
    ? ` The likely cause is ${h.rivals.join(" and ")}, which watch the keyboard too.`
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
  box.appendChild(wrap);

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
  box.appendChild(caveat);

  const btn = document.createElement("button");
  btn.className = "btn btn-sm";
  btn.style.cssText = "width:100%; justify-content:center; margin-top:8px;";
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
  box.appendChild(btn);

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
  software:
    "A backup way of drawing the pop-ups. Turn on only if the guide or toasts stop appearing while sounds still play. Applies at the next launch.",
  flight:
    "When a shortcut fires while the Space ring is open, the little message flies out of the ring instead of simply appearing. It looks good and it takes about a second. Off is quicker and quieter.",
  theme:
    "Three looks for the whole app, pop-ups included: Earthy daylight, a Warcry of iron and war-banners, or a Starry night sky.",
  // Not in the spec — this setting is new, so the copy is written to match its
  // voice: what you get, and how to come back.
  hideboard:
    "Clears the whole dashboard away and leaves just the sky. Your shortcuts keep working exactly as they are — press Esc, or the small arrow in the corner, to bring everything back.",
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
    "Spaceadom pauses itself while any of these apps is in front. Space works exactly as it normally would there — Photoshop’s hold-Space panning, a game’s Space key, anything. Shortcuts come back the moment you switch away.",
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
function descBox(id: string, openByDefault = false): string {
  const copy = DESC[id];
  if (!copy) return "";
  if (openByDefault) {
    return `<div class="set-desc is-open" data-desc-for="${id}"><div class="set-desc-in"><div class="set-desc-body">${copy}</div></div></div>`;
  }
  // The visual box is a CHILD of the clipped wrapper, never the wrapper
  // itself — see the .set-desc-in note in styles.css for why.
  return `<div class="set-desc" data-desc-for="${id}"><div class="set-desc-in"><div class="set-desc-body">${copy}</div></div></div>`;
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
  panelEl?.querySelectorAll<HTMLElement>("[data-desc]").forEach((label) => {
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
    <div class="set-item" style="animation-delay:${60 + i * 45}ms">
      <div class="set-row">
        <button type="button" class="set-row-label" data-desc="${id}"
                aria-expanded="false">${label}</button>
        ${toggleSwitchHtml(id, on,
          _flipped?.id === id && _flipped.on === on ? (on ? "on" : "off") : undefined)}
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
  const opts: [string, string][] = [
    ["earthy", "Earthy"],
    ["warcry", "Warcry"],
    ["starry", "Starry night"],
  ];
  const idx = Math.max(0, opts.findIndex(([v]) => v === theme));
  return `
    <div class="set-item" style="animation-delay:${60 + i * 45}ms">
      <div class="set-row set-row-stack">
        <button type="button" class="set-row-label" data-desc="theme"
                aria-expanded="false">Theme</button>
        <div class="theme-seg" style="--seg-i:${idx}" role="radiogroup" aria-label="Theme">
          <span class="theme-seg-ind" data-seg="${opts[idx][0]}"></span>
          ${opts
            .map(
              ([v, l], n) => `<button type="button" class="theme-seg-opt${n === idx ? " is-on" : ""}"
                     data-theme-set="${v}" role="radio"
                     aria-checked="${n === idx}">${l}</button>`,
            )
            .join("")}
        </div>
      </div>
      ${descBox("theme")}
    </div>`;
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

  return `
    <div class="set-row" style="flex-direction:column; align-items:stretch; gap:4px; cursor:default; margin-bottom:10px;">
      <div style="display:flex; align-items:baseline; gap:8px;">
        <button type="button" class="set-row-label" data-desc="wpm"
                aria-expanded="false">Typing speed</button>
        <span style="font-size:11px; font-weight:700; color:var(--st-accent-deep);"
              id="set-wpm-val">${typingTierName(wpm)} · ${wpm} wpm</span>
      </div>
      <div class="wpm-ticks" id="set-wpm-ticks">${ticks}</div>
      ${sliderShell("wpm", `
        <input type="range" id="set-wpm" min="${WPM_MIN}" max="${WPM_MAX}" step="5" value="${wpm}" />`,
        WPM_MIN, WPM_MAX, wpm)}
      <span class="sma-note" style="font-size:10.5px; color:var(--st-ink-soft); line-height:1.35; margin-top:2px;">
        If apps launch by accident while you type, choose a SLOWER speed —
        Spaceadom then waits longer before treating Space+key as a shortcut.
      </span>
      ${descBox("wpm")}
    </div>`;
}

function sliderRow(
  id: string, label: string, value: number,
  min: number, max: number, step: number, unit: string,
): string {
  return `
    <div class="set-row" style="flex-direction:column; align-items:stretch; gap:4px; cursor:default; margin-bottom:10px;">
      <div style="display:flex; align-items:baseline; gap:8px;">
        <button type="button" class="set-row-label" data-desc="${id}"
                aria-expanded="false">${label}</button>
        <span style="font-size:11px; font-weight:700; color:var(--st-accent-deep);" id="set-${id}-val">${value}${unit}</span>
      </div>
      ${sliderShell(id, `
        <input type="range" id="set-${id}" min="${min}" max="${max}" step="${step}" value="${value}"
               data-unit="${unit}" />`, min, max, value)}
      ${descBox(id)}
    </div>`;
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
