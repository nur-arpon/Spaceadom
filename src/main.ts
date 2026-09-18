/**
 * main.ts — Spaceadom dashboard bootstrap.
 *
 * The V14 dashboard is a single warm stage with the keyboard as the hero:
 * profiles behind the top-right pill, settings behind the bottom-left gear,
 * special keys behind the bottom-centre pill. No sidebar, header grid or
 * status bar. Transcribed from Dashboard Earthy v2.dc.html.
 *
 * This file owns the stage-level motion (cursor glow, press ripples, board
 * fit) and the popover plumbing; each component still owns its own domain.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import {
  initKeyboardMatrix,
  updateMatrix,
  setKeyboardSound,
  DESIGN_W,
  DESIGN_H,
} from "./components/keyboard-matrix";
import {
  initKeyDetailPanel,
  openPanel,
  closePanel,
  updatePanelConfig,
  getCurrentKey,
} from "./components/key-detail-panel";
import {
  initProfileEditor,
  refreshProfileList,
  syncPill,
  resetNewProfileRow,
} from "./components/profile-editor";
import {
  initSettingsPanel,
  openSettingsPanel,
  closeSettingsPanel,
  isSettingsPanelOpen,
  setPausedState,
} from "./components/settings-panel";
import { showToast } from "./components/toast";
// PROBLEM 253 — the report dialog is its own leaf module because TWO owners
// open it: this file's safe-mode banner, and the settings panel's About
// section. One dialog, one import, no second copy to drift.
import { openReportDialog } from "./components/report-dialog";
// PROBLEM 249 — the "what changed?" sheet. A leaf module for the same reason
// report-dialog.ts is one: main.ts owns WHEN it opens, that file owns what it
// looks like, and neither drags the other into `preview.ts`'s harness.
import { openWhatsNew } from "./components/whats-new-sheet";
import { initTour, maybeStartTour } from "./components/tour";
import { syncStarrySky } from "./components/starry-sky";
import { sfx, bindSfxConfig, wireSfxUnlock } from "./sfx";
import { resolveSpecials, toggleSpecialCard } from "./components/special-cards";
import { dismissAll } from "./dismissable";
import { wireKeyWake, applyKeyWakeMotion } from "./key-wake";
import { installJsErrorReporter } from "./js-error-reporter";
// PROBLEM 259 — the own-window fallback for PROBLEM 257. A LEAF module (it
// imports nothing from here), so the preview harness can drive it. main.ts
// owns only WHEN it is wired and WHICH timings it gets; every decision is in
// that file, and every guard against double-firing is in Rust.
import { initOwnWindowKeys, setOwnWindowKeyTiming } from "./own-window-keys";
// PROBLEM 255 — the ONE "auto" theme rule, shared with the OVERLAY bundle.
// A leaf module (it imports nothing), so `overlay.ts` can take the identical
// rule without importing `main.ts` and pulling the whole dashboard into the
// overlay's bundle. See that file's header for what the duplicate cost.
import { resolveTheme, onSystemThemeChange, THEME_AUTO } from "./theme-resolve";
// REVIEW FIXES 2026-09-05 (H4) — the OS light/dark value that `resolveTheme`
// resolves "auto" against comes from RUST now, not from this webview's
// `prefers-color-scheme`. See os-theme.ts (and theme_watch.rs) for why: a
// window that has been told which colour scheme to prefer cannot report which
// one the USER asked for, and this one had been told "Light".
import { initOsTheme } from "./os-theme";

import type { AppConfig, HookStatus, KeyBinding } from "./types";

// PROBLEM 217 — the dashboard had NO global error handler at all, and
// `frontend_log` (its only bridge to Rust) logs at INFO, which is below the
// crash reporter's floor. So every JavaScript failure in the dashboard stayed
// on the user's machine — the whole UI layer, invisible.
//
// Installed at MODULE level, not inside bootstrap(): an exception thrown while
// bootstrap is still running is the most valuable one there is, and a handler
// registered at the end of bootstrap would miss exactly those. The reporter
// itself cannot throw and cannot re-enter — see js-error-reporter.ts.
installJsErrorReporter("frontend_error");

/** One other keyboard remapper found running (Rust: hook::conflicts::Conflict). */
export interface Conflict {
  process: string;
  product: string;
  detail: string;
  /** PROBLEM 198 — full exe path, resolved live while the process is still
   *  running. Empty when Rust could not read it (a protected process, or it
   *  exited between the scan and the query) — the settings panel's icon
   *  lookup treats that exactly like "no path available" and falls back to
   *  the letter disc, same as every other icon in this app. */
  path: string;
}

/** Cached for the Settings › Conflicts section so it doesn't re-scan on every
 *  gear open. Refreshed by the Re-check button there. */
export let knownConflicts: Conflict[] = [];

// ---------------------------------------------------------------------------
// App-level state
// ---------------------------------------------------------------------------

export let appConfig: AppConfig | null = null;

// PROBLEM 147 — the sound kit (design/sounds.js, kept verbatim). The instance
// lives in sfx.ts; this hands it a live view of the config. A getter, not the
// object: `appConfig` is reassigned on reload and a captured reference would
// go on gating sounds by a config nothing else is reading any more.
bindSfxConfig(() => appConfig);



// ---------------------------------------------------------------------------
// Bootstrap
// ---------------------------------------------------------------------------

/**
 * PROBLEM 134's lesson, applied to decoration: this dashboard composites in
 * SOFTWARE on the owner's machine, and continuous compositing there is what
 * starved the keyboard hook. `body.is-blurred` parks every ambient animation
 * whenever the window is not the one being looked at — which, for a tray app,
 * is nearly always.
 */
function wireAmbientPause(): void {
  const set = (blurred: boolean) => {
    document.body.classList.toggle("is-blurred", blurred);
    // PROBLEM 230 — `animation-play-state: paused` cannot reach a loop that
    // JavaScript drives, and the cursor glow is one: `wireCursorGlow` schedules
    // itself forever and writes a transform to a 380px blurred layer on EVERY
    // frame, whether or not the pointer moved and whether or not anyone can see
    // the window. A CSS class was never going to stop it. Stop it here.
    if (blurred) stopCursorGlow();
    else startCursorGlow();
  };
  window.addEventListener("focus", () => set(false));
  window.addEventListener("blur", () => set(true));
  document.addEventListener("visibilitychange", () =>
    set(document.visibilityState !== "visible"),
  );
  set(!document.hasFocus());
}

/**
 * Esc is the guaranteed way out of sky mode — but the settings gear now stays
 * reachable while the sky is up too (owner, 2026-08-27: a faint arrow alone
 * still stranded people), and its popover has its own Escape-driven dismissal
 * lower down in bootstrap(). Escape must peel, not clear — the same rule
 * dismissable.ts already applies to its own stack of surfaces: closing an open
 * settings popover takes priority, and Esc only leaves the sky once nothing is
 * open. So bail out here and let the ordinary Escape handler close the
 * popover instead; firing both on one press would exit the sky AND drop
 * whatever the popover was doing.
 */
function wireSkyEscape(): void {
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape" || !document.body.classList.contains("sky-mode")) return;
    if (isSettingsPanelOpen()) return;
    e.preventDefault();
    void leaveSkyMode();
  });
}

/**
 * PROBLEM 205 — one greppable startup mark in `debug.log`.
 *
 * `grep "boot:" debug.log` gives the whole bootstrap timeline. Fire-and-
 * forget, and errors are swallowed: telemetry must never be able to break the
 * thing it is measuring.
 */
function mark(what: string): void {
  void invoke("frontend_log", {
    msg: `${what} (+${Math.round(performance.now())}ms)`,
  }).catch(() => {});
}

async function bootstrap(): Promise<void> {
  try {
    appConfig = await invoke<AppConfig>("get_config");
  } catch (e) {
    console.error("SpaceToggle: failed to load config —", e);
    showFatalError("Could not connect to the Spaceadom backend.");
    return;
  }

  // REVIEW FIXES 2026-09-05 (H4) — ASK WINDOWS BEFORE THE FIRST PAINT.
  //
  // `applyLook()` resolves the theme `"auto"` (the default for every new
  // install) against the value `theme-resolve.ts` holds, and that value starts
  // at its daylight fallback until Rust answers. Awaited here rather than
  // fired and forgotten so a dark machine comes up in Starry night directly
  // instead of painting Earthy and cross-fading a beat later — which is the
  // "nothing paints in the wrong palette then snaps" rule on the line below,
  // now that the answer lives in another process.
  //
  // It cannot fail the boot: `initOsTheme` swallows both of its own failures
  // (an older backend, or no backend at all) and leaves the documented
  // daylight fallback in place, which is exactly what the old `matchMedia`
  // `try`/`catch` did.
  await initOsTheme();

  // Theme first, so nothing paints in the wrong palette then snaps.
  applyLook();
  document.body.classList.toggle("show-around", appConfig.show_me_around === true);

  // Low-power scene (see starry-sky.css). Decided ONCE at boot from the three
  // signals that mean "this machine has nothing spare": Windows asking for
  // reduced effects, the user turning Visual effects off, and the app running
  // in software compositing — which is exactly the case the owner's own laptop
  // is in, and the case the app must survive on any machine.
  // SOFTWARE COMPOSITING IS NOT A TRIGGER. It was, for one build, and it was
  // wrong: the owner's own machine composites in software as its NORMAL state
  // (the overlay self-test set that), so lite mode switched itself on for him
  // and flattened the storm — "you messed up the clouds and storms animation
  // now". Software compositing means "no GPU path", not "no headroom".
  // The two signals that really mean the user wants less are the two the user
  // controls: Windows' reduced-effects setting, and this app's own Visual
  // effects switch.
  const lite = appConfig.motion === "reduced"
    || (appConfig.motion !== "full" && window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  document.body.classList.toggle("lite-scene", lite);
  void invoke("frontend_log", { msg: `scene: lite=${lite} (motion=${appConfig.motion ?? "auto"})` });
  applySkyMode(appConfig.hide_keyboard === true);
  wireAmbientPause();
  wireSkyEscape();
  wireSfxUnlock();
  applySound(!!appConfig.sound_enabled);
  const reduced = applyMotion(appConfig.motion);
  // Log the effective motion state at startup. A tester's "there are no
  // animations" is otherwise indistinguishable from a rendering bug, and this
  // one line answers it without asking him to go read Windows Settings.
  void invoke("frontend_log", {
    msg: `motion: setting=${appConfig.motion ?? "auto"} os-prefers-reduced=` +
         `${window.matchMedia("(prefers-reduced-motion: reduce)").matches} → ` +
         `effective=${reduced ? "REDUCED" : "full"}`,
  }).catch(() => {});

  // ---- keyboard ----
  const matrixEl = document.getElementById("keyboard-matrix");
  if (matrixEl) {
    initKeyboardMatrix(
      matrixEl,
      appConfig,
      (key, cell) => {
        // The ripple + tick fire inside keyboard-matrix for EVERY key
        // (PROBLEM 39) — this callback only handles the editor. The 90ms
        // delay is the mockup's: the ripple is seen before the bloom starts.
        if (getCurrentKey() === key) { closePanel(); return; }
        window.setTimeout(() => {
          if (appConfig) openPanel(key, appConfig, cell);
        }, 90);
      },
      () => void persistConfig(),
    );
  }
  // PROBLEM 205 — startup telemetry. Between `dashboard-js: motion:` and
  // `dashboard-js: frontend ready` there used to be NOT ONE line, and that gap
  // was 14.66s of a 15.8s startup (≈85% of a median one). An operation nobody
  // logs is invisible twice over: you cannot see its cost, and you cannot see
  // what it is blocking. These three marks turn "where did the time go?" into
  // a grep. `performance.now()` is ms since this document started, so the
  // three are directly comparable with each other and with Rust's own marks.
  mark("boot: keyboard matrix wired");

  // ---- key editor ----
  const panelEl = document.getElementById("key-detail-panel");
  if (panelEl) {
    initKeyDetailPanel(
      panelEl,
      appConfig,
      async (key: string, binding: KeyBinding) => {
        if (!appConfig) return;
        const profile = appConfig.profiles.find(
          (p) => p.name === appConfig!.active_profile,
        );
        // A FULL REPLACE, not a merge — and that is only safe because
        // `commit()` in key-detail-panel.ts normalises to a COMPLETE
        // KeyBinding first. It did not, and the three optional
        // browser-profile fields (`browser_exe?` and friends) were therefore
        // deleted by omission every time a caller left them off, which is why
        // a profile pin never once survived to config.json. If you ever make
        // this line merge instead, the panel's callers must go back to nulling
        // those three explicitly — see `BINDING_RESET` in keyboard-matrix.ts,
        // whose `updateBinding` DOES merge and needed exactly that.
        if (profile) profile.bindings[key] = binding;
        await persistConfig();
        refreshBoard();
        refreshProfileList(appConfig);
      },
    );
  }
  mark("boot: key editor wired");

  // ---- profiles + settings ----
  initProfileEditor(appConfig, (name: string) => {
    if (!appConfig) return;
    // set_active_profile has ALREADY saved on the Rust side. Persisting
    // again here was the double config-save bug — this callback is UI only.
    appConfig.active_profile = name;
    refreshBoard();
    syncPill();
    closePanel();
  });

  initSettingsPanel(resetActiveProfileToDefaults, clearActiveProfile);

  // PROBLEM 253 — the tray's "Report a problem" item opens the SAME dialog.
  //
  // An event rather than a second Rust path to the same zip, because a report
  // with no description is barely a report: the tray fronts the dashboard and
  // then asks for the dialog, so the user still gets to say what went wrong.
  // The listener is here rather than in `report-dialog.ts` because that module
  // is a leaf by design (PROBLEM 148) — it imports only `invoke`, and adding
  // `listen` to it would put a Tauri event subscription inside a component
  // `preview.ts` renders without a backend.
  void listen("open-report-dialog", () => openReportDialog());

  // ---- safe mode (PROBLEM 253) ----
  // FIRST, and not on a timer. Every other banner in this file is delayed a
  // few seconds because it waits on a background scan; this one is already
  // decided before the window exists, and it is the only banner that explains
  // why the app the user is looking at has no shortcuts.
  void checkSafeMode();

  // ---- conflicts ----
  // Scanned once at startup and shown as a DISMISSIBLE BANNER, never a toast:
  // a popup on every launch is annoying, and this is reference information,
  // not an event. Always also visible under Settings › Conflicts.
  void refreshConflicts();

  // ---- stale startup task (PROBLEM 75) ----
  // Delayed: the Rust side triages the task on a background thread right
  // after launch, so an immediate query could race it and read false.
  // PROBLEM 141 — two installs is the worse of the two faults and both banners
  // share ONE element, so the rival check runs first and the stale-task check
  // only claims the slot if the rival check did not.
  window.setTimeout(() => {
    void checkRivalInstall().then((shown) => {
      if (!shown) void checkStaleTask();
    });
  }, 3000);

  // PROBLEM 245 — "Updated to 1.0.X", once, on the first dashboard open after
  // the updater relaunched the app. Rust decides whether there is anything
  // to say (it compares the persisted last-run version) AND whether this
  // window is actually visible — an --autostart relaunch boots this page in
  // a hidden window, and the first build of this consumed the toast there
  // (measured 2026-09-04 22:08:34). So: ask at boot, and ask again every
  // time the window becomes visible or takes focus; Rust answers once.
  const askUpdateNotice = (): void => {
    void invoke<string | null>("get_update_notice")
      .then((v) => { if (v) showToast(`Updated to ${v}`, { duration: 6000 }); })
      .catch(() => {});
  };
  window.setTimeout(askUpdateNotice, 2000);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") askUpdateNotice();
  });
  window.addEventListener("focus", askUpdateNotice);

  // PROBLEM 249 — WHAT'S NEW. The other half of the sentence "Updated to
  // 1.0.X" was never able to finish.
  //
  // TWO ROUTES INTO THE SAME FUNCTION, and both are needed. Rust emits
  // `whats-new-available` from the `st-whats-new` thread 25 s after launch,
  // AND parks the payload for `get_whats_new()` — because an `--autostart`
  // relaunch builds this webview HIDDEN and an event emitted before any
  // listener exists simply vanishes (release_notes.rs says so in its own
  // module doc; it is PROBLEM 245's lesson applied before it could be paid
  // for twice). So: listen, and also ask once after `dashboard_ready`.
  //
  // `_whatsNewShown` is what makes two routes safe. Whichever arrives first
  // opens the sheet; the other finds the latch set and does nothing. Without
  // it the common case — a normal launch, where the listener is registered
  // long before the 25-second emit — would show the sheet twice.
  wireWhatsNew();

  // ---- stage chrome ----
  wirePopovers();
  renderSpecials();
  wireCursorGlow();
  wireKeyboardFit();
  // PROBLEM 179 — the board reacts to a cursor SWEEP, not only to dwelling.
  // AFTER wireKeyboardFit: the wake measures key rectangles once, and doing
  // that before the board has been scaled to the window would cache every
  // position wrong. See key-wake.ts.
  {
    const board = document.getElementById("keyboard-scale");
    if (board) wireKeyWake(board);
  }

  // PROBLEM 259 — THE OWN-WINDOW FALLBACK.
  //
  // PROBLEM 257 measured that no keystroke reaches EITHER keyboard hook while
  // this window has the foreground, and that re-hooking does not recover it.
  // This page still gets the `keydown`, so it feeds it to the engine instead.
  // Wired after the config load because the fallback has to run the SAME
  // typing-rollover and ring-delay numbers the hook and the engine run —
  // guessing them would make the dashboard the one place where "hold Space"
  // means something slightly different.
  //
  // Rust refuses the injection unless our window really is in front and the
  // hook has not just delivered the same press, so wiring it on a machine
  // where the hook works costs nothing and changes nothing.
  initOwnWindowKeys({
    rolloverMs: appConfig.rollover_ms,
    holdThresholdMs: appConfig.guide_hud_delay_ms,
  });

  // ---- backend sync ----
  try {
    const status = await invoke<HookStatus>("get_hook_status");
    setPausedState(status.bypass_active);
    applyHookState(status.installed);
  } catch (_) { /* status is informational */ }

  await listen<AppConfig>("config-updated", (event) => {
    appConfig = event.payload;
    // applyLook(), not applyTheme(): a theme pushed from Rust must bring its
    // palette AND its fun-gate with it, or the body keeps the old data-theme
    // and only the nocturne class flips.
    applyLook();
    applySkyMode(appConfig.hide_keyboard === true);
    applySound(!!appConfig.sound_enabled);
    applyMotion(appConfig.motion);
    refreshBoard();
    refreshProfileList(appConfig);
    updatePanelConfig(appConfig);
    // PROBLEM 259 — re-arm the fallback with the new numbers. Rust re-arms the
    // hook from the same two settings on every save; if this line is dropped,
    // moving the "typing speed" or "ring delay" slider would change the hook
    // and leave the dashboard running the old values until the next launch —
    // a divergence nobody would think to look for.
    setOwnWindowKeyTiming({
      rolloverMs: appConfig.rollover_ms,
      holdThresholdMs: appConfig.guide_hud_delay_ms,
    });
  });

  await listen<string>("profile-changed", (event) => {
    if (!appConfig) return;
    appConfig.active_profile = event.payload;
    refreshBoard();
    refreshProfileList(appConfig);
  });

  await listen<boolean>("bypass-toggled", (event) => setPausedState(event.payload));

  await listen("hook-status-update", async () => {
    try {
      const s = await invoke<HookStatus>("get_hook_status");
      setPausedState(s.bypass_active);
      applyHookState(s.installed);
    } catch (_) { /* ignore */ }
  });

  // Space must never "click" a residually-focused button: tapping Space in
  // this window activates whatever holds focus, which used to pop the New
  // Profile dialog open by itself (user report, 2026-08-10).
  document.addEventListener("keydown", (e) => {
    const el = document.activeElement as HTMLElement | null;
    if (e.code === "Space" && el && el.tagName === "BUTTON") {
      e.preventDefault();
      el.blur();
    }
    if (e.key === "Escape") {
      if (getCurrentKey()) closePanel();
      else closeAllPopovers();
    }
  });
  (document.activeElement as HTMLElement | null)?.blur?.();

  console.info("Spaceadom: dashboard initialised");

  // PROBLEM 205 — the last mark before the window is asked for. The delta
  // between this and Rust's `dashboard-js: frontend ready` is pure IPC queue
  // time: if they are seconds apart, something non-async is holding the main
  // thread, not the frontend being slow.
  mark("boot: bootstrap complete, calling dashboard_ready");

  // PROBLEM 242 — the first-run tour. Wired here and STARTED below, after
  // `dashboard_ready`, for one reason: Rust shows the window only in response
  // to that call (PROBLEM 74), so this is the earliest moment the dashboard is
  // genuinely on screen. Starting it any earlier would spend the entry card on
  // a window nobody can see yet, and it shows exactly once.
  //
  // The host is two lambdas rather than a config import, so `tour.ts` stays a
  // leaf module and `preview.ts` can drive it (PROBLEM 148).
  initTour({
    isDone: () => appConfig?.tour_done === true,
    setDone: () => {
      if (!appConfig || appConfig.tour_done === true) return;
      appConfig.tour_done = true;
      void persistConfig();
    },
  });

  // PROBLEM 74 — LAST step, after every component above is wired: tell Rust
  // the frontend is alive. Rust shows the window only now, so the first thing
  // the user ever sees is a dashboard that can paint and respond — never the
  // "(Not Responding)" ghost of a window whose webview is still booting.
  void invoke("dashboard_ready").catch(() => {});

  // PROBLEM 249 — route two into What's New, and it has to be HERE rather
  // than up with `wireWhatsNew()`: `dashboard_ready` is the call in response
  // to which Rust shows the window (PROBLEM 74). Asking before it would put a
  // sheet into a window that is not on screen yet. See `askWhatsNewOnce`.
  askWhatsNewOnce();

  // One frame after the window is asked for, so the entry card blooms onto a
  // painted dashboard instead of arriving with it.
  //
  // It does NOT try to detect whether the window is actually on screen, and it
  // must not: PROBLEM 135's lesson is that a page cannot observe that its own
  // window is hidden. It does not need to. At logon (`--autostart`) Rust never
  // shows the dashboard (PROBLEM 70) but the webview still boots, so the card
  // is built into a window nobody is looking at — and that is harmless,
  // because `tour_done` is written ONLY by Skip or by finishing. The card is
  // still sitting there, unconsumed, the first time the user opens the
  // dashboard from the tray, which is exactly "the first launch where the
  // dashboard is visible after install".
  requestAnimationFrame(() => maybeStartTour());
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

export async function persistConfig(): Promise<void> {
  if (!appConfig) return;
  try {
    await invoke("save_config", { newConfig: appConfig });
  } catch (e) {
    console.error("SpaceToggle: save_config failed —", e);
    showToast("⚠️ Config save failed");
  }
}

function refreshBoard(): void {
  const el = document.getElementById("keyboard-matrix");
  if (el && appConfig) updateMatrix(el, appConfig);
  // PHASE A — the tray's "␣ Esc" combos are derived from the active profile
  // now, so a moved or removed special re-labels its chip with the board.
  renderSpecials();
}

/** ONE setting drives the dashboard AND the overlay (Rust re-emits on save). */
export function applyTheme(dark: boolean): void {
  document.body.classList.toggle("nocturne", dark);
}

/**
 * PROBLEM 144 — put the whole LOOK on <body>: which of the three themes, and
 * whether the personality layer is on.
 *
 * Both live as data-attributes rather than classes because themes.css keys off
 * them in combination — `[data-theme="starry"][data-fun="on"]` is the starry
 * palette, and anything else with a dark theme falls through to plain
 * nocturne. That combination IS the owner's rule: with fun on, Starry night is
 * the new sky; with it off, Starry night is "the previous dark mode we have
 * been using all along".
 *
 * `.nocturne` is still the single switch the overlay reads, so it is kept in
 * lockstep here rather than being derived independently anywhere else.
 */
let _xfadeTimer: number | undefined;

/**
 * FOLLOW SYSTEM THEME (owner decision, feature 2) — resolve the config's raw
 * theme choice, which may now be the literal string `"auto"`, down to one of
 * the three real palettes every OTHER piece of code in this app already
 * understands (`data-theme`, `syncStarrySky`, the theme sound).
 *
 * **THE RULE ITSELF NOW LIVES IN `src/theme-resolve.ts`** and this is a
 * re-export, not a second copy. PROBLEM 255 shipped the rule here only, and
 * the overlay — a separate webview that may not import `main.ts` — kept its
 * own "anything that is not warcry/starry is earthy" version in
 * `toast.ts::applyThemeName`. Handed the literal `"auto"`, the two windows
 * of one app rendered in two different palettes, which is the exact failure
 * CLAUDE.md's "ONE setting drives everything" rule exists to prevent. The
 * shared file is a LEAF module (it imports nothing at all), so both bundles
 * can take it without either dragging the other's dependencies along.
 *
 * Kept exported from here because `settings-panel.ts` already imports it from
 * `main.ts` and that import is correct — the panel lives in the dashboard
 * bundle. Nothing is gained by making every call site learn a new path.
 */
export { resolveTheme };

export function applyLook(): void {
  const rawTheme = appConfig?.theme || (appConfig?.dark_mode ? "starry" : "earthy");
  const theme = resolveTheme(rawTheme);
  // Cross-fade the whole app between palettes (spec §5). Only when the theme
  // ACTUALLY changes — applyLook also runs on boot and on the fun switch, and
  // a 450ms transition on every surface during the first paint would fade the
  // dashboard in from nothing. Compared on the RESOLVED value: an OS flip that
  // moves Auto from Earthy to Starry night is exactly the change this is for.
  if (document.body.dataset.theme && document.body.dataset.theme !== theme
      && !document.documentElement.classList.contains("reduced-motion")) {
    document.body.classList.add("theme-xfade");
    window.clearTimeout(_xfadeTimer);
    _xfadeTimer = window.setTimeout(() => document.body.classList.remove("theme-xfade"), 480);
  }
  const fun = appConfig?.fun_mode === true;   // off-by-default since 2026-08-20
  document.body.dataset.theme = theme;
  document.body.dataset.fun = fun ? "on" : "off";
  applyTheme(theme !== "earthy");
  // The living scene (constellations, ocean, ship) exists exactly when Starry
  // night AND Fun are both on; fun off keeps 1.0.57's quiet drifting sky.
  syncStarrySky(theme, fun);
}

/**
 * LIVE re-apply when Windows' own light/dark setting changes underneath the
 * app (feature 2). Registered ONCE at module load, not inside `bootstrap()` —
 * `bootstrap()` runs exactly once too, but the listener has no dependency on
 * anything bootstrap sets up (it reads `appConfig` fresh on every fire), so
 * tying its lifetime to module load rather than to boot order is one fewer
 * thing to get wrong later.
 *
 * A no-op whenever the theme is NOT "auto": `applyLook()` is cheap (a handful
 * of class/dataset writes plus the starry-sky sync, which itself no-ops when
 * the scene it would build already exists), but there is no reason to pay
 * even that on every OS flip for a user who picked a fixed look.
 *
 * THE OVERLAY DOES NOT HEAR THIS ONE, and it does not need to: it registers
 * its own copy of this listener in `overlay.ts` against the SAME shared
 * helper, so there is one version of the rule rather than two that agree by
 * hand. That was the gap PROBLEM 255 left open.
 *
 * **REVIEW FIXES 2026-09-05 (H4) — WHAT FIRES THIS CHANGED, AND IT MATTERS.**
 * It used to be this webview's own `matchMedia("(prefers-color-scheme:
 * dark)")`, and the sentence that used to be here — "each window has its own
 * matchMedia and the answer is the same in both" — was FALSE.
 * `tauri.conf.json` pinned this window to `"theme": "Light"`, which WebView2
 * takes as `SetPreferredColorScheme(Light)`, so this window's query answered
 * "light" on a dark machine while the overlay's answered "dark": one rule, two
 * inputs, two palettes on one screen. There IS a cross-window signal now, and
 * there has to be — `theme_watch.rs` reads the registry once and emits
 * `os-theme-changed` to both windows (`os-theme.ts`), which is the only way
 * both can be asking the same question.
 */
onSystemThemeChange(() => {
  if ((appConfig?.theme || "earthy") !== THEME_AUTO) return;
  applyLook();
});

/**
 * PROBLEM 144 — "hide the keyboard layout so a person could enjoy the blank
 * sky". The owner chose the full version: everything goes, leaving only the
 * sky.
 *
 * Two things are non-negotiable when the entire UI can vanish, and both are
 * here rather than assumed: Esc always returns, and a small corner control is
 * always painted so a user who never thinks to press Esc is not stranded in an
 * empty window with no way back.
 */
export function applySkyMode(on: boolean): void {
  document.body.classList.toggle("sky-mode", on);
  // PROBLEM 213 follow-up (owner, 2026-08-28) — the settings popover is what
  // the user was just standing in when they flipped this switch, and it has
  // no reason to keep sitting open over an empty sky: #gear-dock/#gear-btn
  // stay reachable (exempted from the hiding rule below), but the popover
  // itself should close. Route through settings-panel.ts's OWN close
  // function rather than touching #settings-panel here — it now has its own
  // animated exit (see closeSettingsPanel), so this reads as deliberate
  // rather than a glitchy snap. Only on ENTERING sky mode: leaving it must
  // not disturb a panel the user reopened from the gear while the sky was up
  // (the "hide the keyboard" toggle's own applySkyMode(false) call happens
  // from inside exactly that reopened panel).
  if (on) closeSettingsPanel();
  let back = document.getElementById("sky-return");
  if (on && !back) {
    const btn = document.createElement("button");
    btn.id = "sky-return";
    btn.type = "button";
    back = btn;
    back.title = "Show the dashboard again (Esc)";
    back.setAttribute("aria-label", "Show the dashboard again");
    back.textContent = "⤢";
    back.addEventListener("click", () => void leaveSkyMode());
    document.body.appendChild(back);
  } else if (!on && back) {
    back.remove();
  }
}

/** Come back from sky mode, and remember that we did. */
export async function leaveSkyMode(): Promise<void> {
  if (!appConfig?.hide_keyboard) return;
  appConfig.hide_keyboard = false;
  // Escape and the return button leave by this path; the settings switch has
  // its own. Same falling sweep either way (sounds.js §8a, "exiting a mode").
  if (appConfig.fun_mode === true) sfx.spaceFall();
  applySkyMode(false);
  await persistConfig();
}

/**
 * Decide whether to run reduced visual effects, and put the answer on <html>
 * as `.reduced-motion` (PROBLEM 47).
 *
 * WHY A SETTING AND NOT JUST THE MEDIA QUERY: a tester's Windows had
 * Accessibility > Visual effects > Animation effects switched OFF (Battery
 * Saver also does this), so WebView2 reported prefers-reduced-motion: reduce.
 * The old blanket CSS rule then removed EVERY animation and transition, the
 * cursor glow never started and key ripples never spawned — and he reported
 * the app as having "no motion graphics" and being broken. Accessibility must
 * be honoured by default, but it must also be overridable, because on that
 * machine the OS default made the app look defective.
 *
 * Returns the effective value so callers can log it — the single line in the
 * log that would have answered this in one look instead of a round trip.
 */
export function applyMotion(pref: "auto" | "full" | "reduced" | undefined): boolean {
  // DELIBERATELY IGNORES the OS reduced-motion signal (owner decision,
  // 2026-08-12): "I don't want my app to respect reduce animations even if
  // something is on power saving mode." Windows turns animation effects off
  // for Battery Saver and for Accessibility, and either one silently stripped
  // the app of all its motion on a tester's machine. Effects are now ON for
  // everyone unless the user explicitly switches them off in Settings.
  // Anything that is not the literal string "reduced" — undefined, "auto"
  // from an older config, "full" — means full effects.
  const reduced = pref === "reduced";
  document.documentElement.classList.toggle("reduced-motion", reduced);
  // Marks that the setting has been resolved, so toast.ts's REDUCED() stops
  // falling back to the bare OS media query.
  document.documentElement.dataset.motionResolved = "1";
  // PROBLEM 179 — the key wake consults `enabled()` only from inside its rAF
  // loop, so a board caught mid-sweep when effects are switched OFF would keep
  // its inline transforms until something happened to start the loop again.
  // Tell it directly; it is a no-op when nothing is displaced.
  applyKeyWakeMotion();
  return reduced;
}

/** Single source of truth for "should this animate?" — reads the class that
 *  applyMotion() set, so the in-app override is honoured everywhere instead of
 *  each call site re-querying the OS media query behind the setting's back. */
export function motionReduced(): boolean {
  return document.documentElement.classList.contains("reduced-motion");
}

export function applySound(on: boolean): void {
  // Two consumers: the overlay window hears about it via the "sound-changed"
  // event Rust emits on save; the dashboard's own key ticks (520Hz on press,
  // matching the mockup's beep) are fed here directly.
  setKeyboardSound(on);
}

// ---------------------------------------------------------------------------
// Destructive actions (behind the gear's two-step confirm)
// ---------------------------------------------------------------------------

async function clearActiveProfile(): Promise<void> {
  if (!appConfig) return;
  const name = appConfig.active_profile;

  // PROBLEM 99 — goes through Rust so the pre-change config is stashed for
  // undo. Doing it here (blanking each binding, then persistConfig) left NO
  // way back: for a profile the user built themselves, those bindings and
  // their custom icons exist in no other copy. A two-click confirm is not a
  // safety net — it is asked before the user can see what they are losing.
  try {
    const cleared = await invoke<number>("clear_active_profile");
    closePanel();
    showToast(`🗑️ Cleared ${cleared} binding${cleared === 1 ? "" : "s"} in ${name}`);
    offerUndo();
  } catch (e) {
    console.error("clear_active_profile failed:", e);
    showToast("⚠️ Could not clear the profile");
  }
}

/**
 * PROBLEM 99 — the 10-second undo banner.
 *
 * Deliberately a BANNER and not a toast: the toast pill is single-line,
 * transient and cannot hold a button (see .st-toast — nowrap, fixed height).
 * An undo the user cannot click is not an undo.
 */
/**
 * PROBLEM 120 — ONE countdown, held here rather than inside each call.
 *
 * `offerUndo` used to declare `const timer` locally, so every call started a
 * fresh interval and never stopped the previous one. Reported by the owner:
 * delete Gamers (20s offered), then delete Founders (30s offered) — and the
 * Undo button vanished after 20 seconds, because the FIRST interval was still
 * running and its expiry executed `el.hidden = true` on the banner the SECOND
 * one was using. The 30-second undo was still perfectly valid in Rust; there
 * was simply no longer a button to click.
 *
 * PROBLEM 107 made undo a stack in the backend. The countdown in front of it
 * stayed single-instance-by-accident, which is the same shape of bug: a stale
 * thing outliving the thing that replaced it.
 */
let undoTimer: number | null = null;
function stopUndoTimer(): void {
  if (undoTimer !== null) {
    window.clearInterval(undoTimer);
    undoTimer = null;
  }
}

export function offerUndo(): void {
  const el = document.getElementById("undo-banner");
  if (!el) return;
  stopUndoTimer();          // a previous offer must never outlive this one

  void (async () => {
    // PROBLEM 106 — Rust owns the deadline and returns the seconds remaining.
    // This used to hardcode 10, which is now wrong for every action (20s
    // normally, 30s for the fallback profile) and would have hidden the offer
    // while it was still perfectly valid.
    const res = await invoke<[string, number] | null>("undo_available").catch(() => null);
    if (!res) { el.hidden = true; return; }
    const label = res[0];

    let left = res[1];
    el.innerHTML = "";
    const txt = document.createElement("span");
    txt.className = "conflict-text";
    txt.textContent = `${label}. This can be undone for ${left}s.`;

    const undo = document.createElement("button");
    undo.className = "btn btn-sm";
    undo.textContent = "Undo";
    undo.addEventListener("click", async (e) => {
      e.stopPropagation();          // #stage closes popovers on click (PROBLEM 98)
      try {
        const what = await invoke<string>("undo_last_change");
        showToast(`↩ Undone: ${what}`);
      } catch (_) {
        showToast("⚠️ That undo has expired");
      }
      stopUndoTimer();
      el.hidden = true;
      // PROBLEM 107 — undo is a STACK now. Deleting two profiles inside the
      // window leaves two entries, and hiding the banner here would strand the
      // older one: still valid, but with nothing on screen offering it. Ask
      // again — if another undo is pending, the banner comes straight back.
      offerUndo();
    });

    const close = document.createElement("button");
    close.className = "conflict-close";
    close.setAttribute("aria-label", "Dismiss");
    close.textContent = "✕";
    close.addEventListener("click", (e) => {
      e.stopPropagation();
      stopUndoTimer();
      el.hidden = true;
    });

    el.append(txt, undo, close);
    el.hidden = false;

    // Counting down visibly matters: an undo offer that vanishes without
    // warning reads as the app losing the option, not the window closing.
    // Cleared again here, not only at the top of offerUndo: this function
    // awaits `undo_available`, so two rapid calls can both get past that guard
    // and the later one would otherwise leak the earlier interval.
    stopUndoTimer();
    undoTimer = window.setInterval(() => {
      left -= 1;
      if (left <= 0) {
        stopUndoTimer();
        el.hidden = true;
        return;
      }
      txt.textContent = `${label}. This can be undone for ${left}s.`;
    }, 1000);
  })();
}

async function resetActiveProfileToDefaults(): Promise<void> {
  // PROBLEM 92 — reset_config used to be a FACTORY reset despite this
  // function's name and the button's label: one click destroyed every
  // profile, every custom icon, special_keys (which no UI can restore), the
  // fullscreen allowlist, the browser choice, and overlay_compositing — the
  // self-test's measured verdict about this machine's GPU, whose loss made
  // the HUD invisible for 12 minutes on 2026-08-13. It now resets the ACTIVE
  // profile's bindings only, and re-emits config-updated to repaint.
  try {
    await invoke("reset_config");
    showToast(`🔄 Reset ${appConfig?.active_profile ?? "profile"} to defaults`);
    offerUndo();   // PROBLEM 99
  } catch (e) {
    console.error("reset_config failed:", e);
    showToast("⚠️ Reset failed");
  }
}

// ---------------------------------------------------------------------------
// Popovers — profile pill, gear, specials tray
// ---------------------------------------------------------------------------

function wirePopovers(): void {
  const pill = document.getElementById("profile-pill")!;
  const pop = document.getElementById("profile-popover")!;
  const gear = document.getElementById("gear-btn")!;
  const specialsBtn = document.getElementById("specials-btn")!;
  const specialsTray = document.getElementById("specials-tray")!;

  const setProfileOpen = (open: boolean) => {
    pop.hidden = !open;
    pill.setAttribute("aria-expanded", String(open));
  };
  const setSpecialsOpen = (open: boolean) => {
    specialsTray.hidden = !open;
    specialsBtn.setAttribute("aria-expanded", String(open));
    specialsBtn.textContent = open ? "Close" : "Special keys";
  };

  pill.addEventListener("click", (e) => {
    e.stopPropagation();
    const open = pop.hidden;
    closeAllPopovers();
    setProfileOpen(open);
  });

  gear.addEventListener("click", (e) => {
    e.stopPropagation();
    const wasOpen = isSettingsPanelOpen();
    closeAllPopovers();
    if (!wasOpen) openSettingsPanel();
  });

  specialsBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    const open = specialsTray.hidden;
    closeAllPopovers();
    setSpecialsOpen(open);
  });

  // Clicks inside a popover must not close it.
  [pop, specialsTray, document.getElementById("settings-panel")!].forEach((el) =>
    el.addEventListener("click", (e) => e.stopPropagation()),
  );

  // Click anywhere OUTSIDE a popover closes them all. On DOCUMENT, not on
  // #stage: in Starry night with Fun on, #stage is pointer-events:none (the
  // PROBLEM 146 carve-out), so a click on empty sky never reached a #stage
  // listener and the panels simply would not close — the owner's exact report.
  // Everything that must survive its own click already stops propagation
  // (PROBLEM 98), so the only clicks that arrive here are genuine "elsewhere".
  document.addEventListener("click", () => closeAllPopovers());
}

function closeAllPopovers(): void {
  const pop = document.getElementById("profile-popover");
  const pill = document.getElementById("profile-pill");
  if (pop) pop.hidden = true;
  pill?.setAttribute("aria-expanded", "false");
  // PROBLEM 178 — an abandoned "new profile" name box must not survive the
  // popover being closed and reopened, or the ＋ button is still missing the
  // next time you look for it.
  resetNewProfileRow();

  const tray = document.getElementById("specials-tray");
  const trayBtn = document.getElementById("specials-btn");
  if (tray) tray.hidden = true;
  if (trayBtn) {
    trayBtn.setAttribute("aria-expanded", "false");
    trayBtn.textContent = "Special keys";
  }

  closeSettingsPanel();

  // Everything that registered itself with `dismissable` — the conflict
  // prompt today, and whatever is added next without anyone having to edit
  // this function. The owner reported the same "it won't close when I press
  // elsewhere" bug twice, for two different surfaces, because this function
  // was the only place that knew how to close anything and it names each one
  // by id. New surfaces opt in from their own file now; see dismissable.ts.
  dismissAll();
}

/**
 * Ask Rust which other keyboard remappers are running, cache them for the
 * Settings section, and show the banner if any turned up.
 *
 * Dismissal is per-session and remembered per detected set: dismissing a
 * warning about AutoHotkey should not silence a DIFFERENT program appearing
 * later. Keyed on the sorted product list.
 */
export async function refreshConflicts(): Promise<Conflict[]> {
  try {
    knownConflicts = await invoke<Conflict[]>("get_conflicts");
  } catch (_) {
    knownConflicts = [];
  }
  renderConflictBanner();
  return knownConflicts;
}

function conflictKey(list: Conflict[]): string {
  return list.map((c) => c.product).sort().join("|");
}

// ---------------------------------------------------------------------------
// Stale startup task (PROBLEM 75)
// ---------------------------------------------------------------------------
// The self-elevating 1.0.0–1.0.2 builds left a Scheduled Task that opens the
// dashboard at every logon, and a non-elevated process cannot delete, rewrite
// or even disable it (all Access denied — measured). The only honest fix is
// ONE user-initiated elevated deletion. This banner is that offer. It is NOT
// once-per-set like the conflict banner: it stays until the machine is
// actually repaired, because until then every reboot misbehaves.
// ---------------------------------------------------------------------------
// PROBLEM 141 — a SECOND copy of Spaceadom is installed.
//
// The .msi installed per-machine into Program Files; the setup.exe installs
// per-user into %LOCALAPPDATA%. Windows sees two unrelated programs, both
// register autostart, and at logon two processes each hook the keyboard and
// fight over the spacebar. Nobody experiences that as "two apps are running" —
// they experience Space+D opening Discord twice, or settings that keep
// reverting because two processes write one config.json. So it has to be
// NAMED, not left to be diagnosed.
//
// Dropping the .msi in 1.0.41 protects NEW installs. This protects machines
// that are already wrong — which is every friend given an older build. Same
// shape as the stale-task banner below, deliberately: one sentence of plain
// English, one button, one permission prompt.
//
// Returns whether it claimed the banner, so the stale-task check can stand
// down — they share one element and two installs is the worse fault.
// ---------------------------------------------------------------------------
// PROBLEM 253 — THE SAFE-MODE BANNER.
//
// Three launches in a row started and never stayed alive thirty seconds, so
// this one came up with no keyboard hook and no overlay. The dashboard is the
// only thing the user can see, and this banner is the only thing that explains
// why Space is just a space again.
//
// It gets its OWN element and its own injected stylesheet rather than sharing
// `#conflict-banner`, and both of those are deliberate. Sharing the strip would
// mean the rival-install check (which runs three seconds later and claims that
// element unconditionally) could overwrite the one message the user has to
// read. And injecting the CSS from here keeps this whole feature inside the
// files that own it — `styles.css` belongs to the design transcription, and a
// state that only ever appears after a crash does not belong in it.
//
// The `st-safe-mode` body class pushes the OTHER two banners down for the
// duration, so a conflict warning and this can be on screen together. Nothing
// outside this block reads that class, and nothing about the normal dashboard
// changes when it is absent.
// ---------------------------------------------------------------------------
const SAFE_MODE_STYLE_ID = "st-safe-mode-styles";

const SAFE_MODE_CSS = `
#safe-mode-banner {
  position: absolute;
  top: 64px;
  left: 0; right: 0;
  margin-inline: auto;
  width: fit-content;
  max-width: min(820px, calc(100% - 96px));
  z-index: 7;
  display: flex;
  align-items: center;
  gap: 10px;
  box-sizing: border-box;
  padding: 10px 12px 10px 18px;
  border-radius: var(--radius-full, 999px);
  background: var(--st-danger-tint, #f6dfc9);
  border: 1px solid var(--st-accent-brd, #e0ac80);
  color: var(--st-accent-800, #6e3a15);
  box-shadow: var(--shadow-sm);
  font-family: var(--st-font-body, system-ui, sans-serif);
  animation: st-pop-in 420ms var(--ease-spring, cubic-bezier(.34,1.3,.4,1)) both;
}
#safe-mode-banner[hidden] { display: none; }
#safe-mode-text { flex: 1; min-width: 0; font-size: 12px; font-weight: 600; line-height: 1.35; }
body.nocturne #safe-mode-banner {
  background: #3a2118;
  border-color: #8a4a22;
  color: #f0d8c4;
}
/* Only while safe mode is on screen: give the two shared-strip banners their
   own row instead of hiding behind this one. */
body.st-safe-mode #conflict-banner,
body.st-safe-mode #undo-banner { top: 124px; }
@media (prefers-reduced-motion: reduce) { #safe-mode-banner { animation: none } }
`;

interface SafeModeState {
  active: boolean;
  failed_starts: number;
  threshold: number;
}

/**
 * Ask Rust whether this is a safe-mode launch and, if it is, say so.
 *
 * Answers `active: false` on every normal launch, which is why this costs
 * nothing when nothing is wrong: one invoke, one early return, no element
 * created and no stylesheet injected.
 */
async function checkSafeMode(): Promise<void> {
  let state: SafeModeState | null = null;
  try {
    state = await invoke<SafeModeState>("get_safe_mode");
  } catch (_) { /* an old backend has no such command — nothing to say */ }
  if (!state?.active) return;

  if (!document.getElementById(SAFE_MODE_STYLE_ID)) {
    const style = document.createElement("style");
    style.id = SAFE_MODE_STYLE_ID;
    style.textContent = SAFE_MODE_CSS;
    document.head.appendChild(style);
  }
  document.body.classList.add("st-safe-mode");

  // The guard in `applyHookState` only helps for calls made AFTER this answer
  // arrived, and the bootstrap's own `get_hook_status` is a race with this one.
  // In safe mode `HOOK_INSTALLED` is false, so that call may already have put
  // the PROBLEM 161 "not receiving key presses" banner up, blaming another
  // keyboard program for something Spaceadom decided. Take the strip back.
  const strip = document.getElementById("conflict-banner");
  if (strip && strip.dataset.owner === "hook") {
    strip.hidden = true;
    strip.dataset.owner = "";
    strip.innerHTML = "";
  }

  const stage = document.getElementById("stage") ?? document.body;
  let el = document.getElementById("safe-mode-banner");
  if (!el) {
    el = document.createElement("div");
    el.id = "safe-mode-banner";
    // `alert`, not `status`: this is the one message on the dashboard a screen
    // reader must not wait for a quiet moment to announce.
    el.setAttribute("role", "alert");
    stage.appendChild(el);
  }
  el.innerHTML = "";

  const txt = document.createElement("span");
  txt.id = "safe-mode-text";
  // The count comes from Rust rather than being hardcoded as "3": if the
  // threshold ever changes, a banner that still says three would be the app
  // telling the user something it does not believe.
  txt.textContent =
    `Spaceadom started in safe mode — it crashed ${state.failed_starts} times in a row ` +
    "at startup. Your shortcuts are off until you press Turn back on. (Send a report)";

  const on = document.createElement("button");
  on.className = "btn btn-sm";
  on.textContent = "Turn back on";
  on.addEventListener("click", async () => {
    on.disabled = true;
    on.textContent = "Turning on…";
    try {
      await invoke("safe_mode_turn_back_on");
      el.hidden = true;
      document.body.classList.remove("st-safe-mode");
      // Honest about what did and did not come back. The hook is live now; the
      // overlay window is created at startup only, so the Guide HUD and toasts
      // return at the next restart. Claiming otherwise would have the user
      // holding Space and watching for a ring that cannot appear.
      showToast(
        "✅ Shortcuts are back on. The Space ring returns after you restart Spaceadom.",
        { duration: 7000 },
      );
      // The class comes off FIRST, then the hook state is re-read: while it is
      // on, `applyHookState` deliberately stays silent (see the guard there),
      // and asking before removing it would leave the PROBLEM 161 banner
      // permanently suppressed for this session.
      //
      // Delayed, because `spawn_hook_thread` returns before the thread has
      // called `SetWindowsHookExW` — reading `HOOK_INSTALLED` immediately would
      // report false and put up "Spaceadom is not receiving key presses" on top
      // of a success toast.
      window.setTimeout(() => {
        void invoke<HookStatus>("get_hook_status")
          .then((s) => applyHookState(s.installed))
          .catch(() => {});
      }, 800);
    } catch (err) {
      on.disabled = false;
      on.textContent = "Turn back on";
      showToast(`⚠️ ${String(err)}`, { duration: 7000 });
    }
  });

  const report = document.createElement("button");
  report.className = "btn btn-sm";
  report.textContent = "Report a problem";
  report.addEventListener("click", () => openReportDialog());

  el.append(txt, on, report);
  el.hidden = false;
}

// ---------------------------------------------------------------------------
// PROBLEM 249 — What's New
// ---------------------------------------------------------------------------

/** Rust's `whats-new-available` payload (`release_notes.rs::WhatsNew`). */
type WhatsNewPayload = {
  version: string;
  has_notes: boolean;
  /** The version went BACKWARDS: this launch follows a rollback, not an update. */
  rolled_back: boolean;
};

/**
 * The latch that makes two delivery routes safe. Set the first time either
 * route produces a decision — including the `has_notes: false` decision, which
 * is a real answer ("PROBLEM 245's plain toast stands") and not a reason to
 * keep asking.
 */
let _whatsNewShown = false;

/**
 * Act on one What's New payload, whichever route it arrived by.
 *
 * `has_notes` is the field that decides the presentation, and Rust emits it
 * even when it is `false` on purpose — the alternative, emitting only on
 * success, would leave this page racing a network request it cannot see with
 * no way to know whether to keep waiting for a sheet or fall back to the
 * plain toast. `false` here means exactly that fallback: `askUpdateNotice`'s
 * `Updated to 1.0.X` toast is left to do its job untouched.
 */
function handleWhatsNew(p: WhatsNewPayload | null | undefined): void {
  if (_whatsNewShown || !p || !p.version) return;
  _whatsNewShown = true;
  if (!p.has_notes) return;
  openWhatsNew(p.version, p.rolled_back === true);
}

/** Route one: the event. Registered during bootstrap, long before the
 *  `st-whats-new` thread's 25-second emit on an ordinary launch. */
function wireWhatsNew(): void {
  void listen<WhatsNewPayload>("whats-new-available", (e) => handleWhatsNew(e.payload))
    .catch(() => { /* an older build emits nothing — the toast still runs */ });
}

/**
 * Route two: ask.
 *
 * Called ONCE, after `dashboard_ready`, because that is the call in response
 * to which Rust shows the window (PROBLEM 74) — the earliest moment a sheet
 * would land on a dashboard somebody can see. It exists for the launch shape
 * the listener cannot cover: an `--autostart` relaunch builds this webview
 * HIDDEN and the page may be created AFTER the 25-second emit has already
 * been and gone, in which case `listen` never fires at all. Rust parks the
 * payload for exactly this (`release_notes.rs::get_whats_new`).
 *
 * A single ask rather than the `askUpdateNotice` pattern of re-asking on
 * every visibility change: the parked payload is not consumed by reading it,
 * so re-asking would re-open the sheet every time the user came back to the
 * window until they restarted. The one-shot latch plus one ask is the whole
 * lifetime this needs.
 */
function askWhatsNewOnce(): void {
  void invoke<WhatsNewPayload | null>("get_whats_new")
    .then(handleWhatsNew)
    .catch(() => { /* older build, or no version change this launch */ });
}

async function checkRivalInstall(): Promise<boolean> {
  let found = false, path = "", version = "", kind = "";
  try {
    [found, path, version, kind] = await invoke<[boolean, string, string, string]>("get_rival_install");
  } catch (_) { /* backend unavailable — nothing to offer */ }
  if (!found) return false;

  // PROBLEM 254 — a FOURTH shape: WE are the portable copy that somebody
  // unzipped beside an installed one. Asked only once a rival has actually
  // been found, so the ordinary installed-copy path pays nothing for it.
  //
  // Unlike the packaged case below, NOTHING about the remedy changes — a
  // portable copy is an ordinary unpackaged, non-elevated process and
  // `rival_install::repair` can still elevate via `runas` exactly as an
  // installed copy's can. What changes is only the WORDING, and it matters
  // for one concrete reason: the default sentence says "another copy is
  // installed", which invites a user to go looking for the wrong one of the
  // two in Apps & features. The portable copy is the one with no entry there
  // at all, and saying so is the difference between a banner that can be
  // acted on and one that sends somebody hunting.
  //
  // `catch → false` because an older build has no such command, and "not
  // portable" is the answer that produces the wording this banner has always
  // had — the safe degrade, not a silent new one.
  let portable = false;
  try {
    portable = await invoke<boolean>("is_portable_install");
  } catch (_) { /* older backend — the installed-copy wording is the fallback */ }

  const el = document.getElementById("conflict-banner");
  if (!el) return false;
  el.innerHTML = "";

  const txt = document.createElement("span");
  txt.className = "conflict-text";
  const orphan = kind === "orphaned_entry";
  // PROBLEM 250 — the third variant: WE are the Microsoft Store copy.
  //
  // Everything the other two variants say about the fault is still true — two
  // copies, both starting at logon, both installing a keyboard hook, both
  // wanting the spacebar. What is NOT true is the remedy. `repair()` works by
  // elevating (`runas` → msiexec / Remove-Item), and a packaged app must not
  // elevate itself to delete a product outside its own package: it is a Store
  // policy problem, and it is the same shape as PROBLEM 244, which deleted the
  // running app. Rust refuses on this condition too (`rival_install::repair`),
  // so a stale frontend cannot get past it — this branch is what makes the
  // refusal into an instruction the user can act on instead of a dead button.
  //
  // The button therefore OPENS the place rather than doing the thing. Naming a
  // Settings page is four clicks and a search box for someone who has never
  // been there, and "go and uninstall it yourself" with no door is how a banner
  // gets dismissed rather than acted on.
  const packagedHost = kind === "packaged_host";
  // PROBLEM 250 FOLLOW-UP — the MIRROR of `packaged_host`, and the fourth
  // shape `status_kind()` can return: WE are the ordinary unpackaged copy and
  // a Microsoft Store package is registered for the same user.
  //
  // Two things make this its own arm rather than a wording tweak on the
  // default:
  //
  // 1. **There is no button at all.** `rival_install::repair` refuses this
  //    kind outright and says why — `Program Files\WindowsApps` is ACL'd
  //    against this user by design, there is no ProductCode and no
  //    `uninstall.exe`, so every removal path an elevated helper could take
  //    ends in "access denied". A `runas` that cannot work is worse than no
  //    button: it teaches the user to accept a UAC prompt from this app for
  //    an action that never succeeds. `packaged_host` at least has a door to
  //    open; this one does not, because the remedy is a Settings page that
  //    the OTHER copy is listed on, not a window we can usefully aim at from
  //    here.
  // 2. **`path` is a SENTENCE naming the package, not a file path.**
  //    `detect_cross_kind` fills it from the package full name, so the
  //    default text's "installed at ${path}" would render prose inside a
  //    sentence about a location. This arm interpolates neither `path` nor
  //    `version` — see `rival_install.rs`'s own test,
  //    `a_store_copy_finding_can_never_yield_a_deletable_directory`.
  const storeCopy = kind === "store_copy";
  // PROBLEM 238 — an orphaned HKLM MSI uninstall entry is a different shape
  // from a real second copy: nothing is running twice, so "fight over the
  // spacebar" would be misleading. `kind` comes from Rust's status_kind().
  //
  // PROBLEM 244 (2026-09-04) — the second sentence is not decoration. The
  // owner clicked this button, the old code ran `msiexec /X{GUID}`, and
  // Windows Installer deleted that product's registered files — which were
  // the LIVE app. The backend now does a registry-only cleanup for this
  // shape, and the banner has to say so, because "Remove the old copy" over
  // an entry whose recorded InstallLocation IS the live folder is exactly
  // the sentence that made deleting the app look safe.
  txt.textContent = storeCopy
    ? "A Microsoft Store copy of Spaceadom is also installed. Keep one: " +
      "uninstall the other from Settings > Apps."
    : packagedHost
    ? `This is the Microsoft Store version of Spaceadom, and another copy ` +
      `(v${version}) is also installed at ${path}. Both start with Windows and ` +
      "fight over the spacebar, so one has to go. The Store version cannot " +
      "uninstall the other one for you — open Installed apps, find Spaceadom " +
      "with the older version number, and remove it. Your settings stay where " +
      "they are."
    : portable && !orphan
    // PROBLEM 254 — the portable shape. Two facts the default sentence gets
    // wrong for it: "another copy is INSTALLED at …" is true, but it is the
    // OTHER one that is installed and this one that is not, and a user told
    // to look in Apps & features will find exactly one entry and conclude the
    // banner is confused. And "one has to go" is not the whole truth here —
    // closing the portable copy, or deleting its folder, is a complete remedy
    // that needs no uninstaller and no permission prompt at all, which is the
    // remedy most people running an unzipped copy actually want. The button's
    // one-click removal of the installed copy still works and is still
    // offered; it is simply no longer the only door named.
    ? `You're running the PORTABLE copy of Spaceadom (unzipped, nothing ` +
      `installed), and an installed copy (v${version}) is also on this PC at ` +
      `${path}. Both put a keyboard hook on the spacebar, so only one can run ` +
      "at a time. Closing this portable copy — or deleting its folder — settles " +
      "it with no uninstaller. Or remove the installed one below (Windows will " +
      "ask for permission once)."
    : orphan
    ? "An old installer entry is left over. Nothing is running twice, but " +
      "Programs and Features lists Spaceadom twice. " +
      "This only removes the leftover entry from Programs and Features. " +
      "Your app and settings are not touched."
    : `Another copy of Spaceadom (v${version}) is installed at ${path}. ` +
      "Both start with Windows and fight over the spacebar. " +
      "One click removes the old one (Windows will ask for permission once).";

  // The dismiss control is built BEFORE the repair button on purpose: it lets
  // the `store_copy` arm finish the banner below without ever constructing a
  // repair button — not a disabled one, not a hidden one, not one that exists
  // with a click handler nothing appends. The rule the whole banner is built
  // on is that a control's presence is a promise; the cheapest way to keep
  // that promise is to have no control to keep it about.
  const close = document.createElement("button");
  close.className = "conflict-close";
  close.setAttribute("aria-label", "Dismiss for now");
  close.textContent = "✕";
  close.addEventListener("click", () => {
    el.hidden = true; // this session only — it returns until the copy is gone
  });

  if (storeCopy) {
    el.append(txt, close);
    el.hidden = false;
    return true;
  }

  const fix = document.createElement("button");
  fix.className = "btn btn-sm";
  // The label has to match the action. "Remove the old copy" on an entry that
  // has no copy behind it is what made a registry cleanup read as a deletion —
  // and on a Store install there is no removal to promise at all, so the label
  // promises the only thing this button actually does: it opens a window.
  const fixLabel = packagedHost
    ? "Open Installed apps"
    : orphan ? "Remove the leftover entry" : "Remove the old copy";
  fix.textContent = fixLabel;
  fix.addEventListener("click", async () => {
    if (packagedHost) {
      // No disabling and no "Removing…": nothing is being removed, and the
      // banner deliberately STAYS UP. It is the reminder of what to do in the
      // window that just opened, and the next launch's scan is what decides
      // whether it was done.
      try {
        await invoke<boolean>("open_installed_apps");
      } catch (_) {
        showToast("⚠️ Could not open Settings — it is under Apps ▸ Installed apps");
      }
      return;
    }
    fix.disabled = true;
    fix.textContent = "Removing…";
    let ok = false;
    try {
      ok = await invoke<boolean>("repair_rival_install");
    } catch (_) { /* fall through to the retry state */ }
    if (ok) {
      el.hidden = true;
      showToast(orphan
        ? "✅ Leftover entry removed — Programs and Features now lists one Spaceadom"
        : "✅ Old copy removed — one Spaceadom left, no more spacebar conflict");
    } else {
      fix.disabled = false;
      fix.textContent = fixLabel;
      showToast("⚠️ Not removed — the permission prompt was declined");
    }
  });

  el.append(txt, fix, close);
  el.hidden = false;
  return true;
}

async function checkStaleTask(): Promise<void> {
  let stale = false;
  try {
    stale = await invoke<boolean>("get_stale_task");
  } catch (_) { /* backend unavailable — nothing to offer */ }
  if (!stale) return;

  const el = document.getElementById("conflict-banner");
  if (!el) return;
  el.innerHTML = "";

  const txt = document.createElement("span");
  txt.className = "conflict-text";
  txt.textContent =
    "A leftover startup entry from an older version opens this window at every " +
    "logon. One click fixes it (Windows will ask for permission once).";

  const fix = document.createElement("button");
  fix.className = "btn btn-sm";
  fix.textContent = "Fix it";
  fix.addEventListener("click", async () => {
    fix.disabled = true;
    fix.textContent = "Fixing…";
    let ok = false;
    try {
      ok = await invoke<boolean>("repair_stale_task");
    } catch (_) { /* fall through to the retry state */ }
    if (ok) {
      el.hidden = true;
      showToast("✅ Startup entry fixed — next logon starts quietly in the tray");
    } else {
      fix.disabled = false;
      fix.textContent = "Fix it";
      showToast("⚠️ Not fixed — the permission prompt was declined");
    }
  });

  const close = document.createElement("button");
  close.className = "conflict-close";
  close.setAttribute("aria-label", "Dismiss for now");
  close.textContent = "✕";
  close.addEventListener("click", () => {
    el.hidden = true; // this session only — it returns until repaired
  });

  el.append(txt, fix, close);
  el.hidden = false;
}

function renderConflictBanner(): void {
  const el = document.getElementById("conflict-banner");
  if (!el) return;

  if (knownConflicts.length === 0) {
    // Only clear the strip if it is OURS — the dead-hook banner (PROBLEM 161)
    // shares this element and outranks nothing, but must not be wiped by a
    // conflicts refresh that found nothing to say.
    if (el.dataset.owner !== "hook") { el.hidden = true; el.dataset.owner = ""; }
    return;
  }
  el.dataset.owner = "conflicts";
  // Shown ONCE per distinct set of programs, then never again — the user was
  // explicit: "no need to warn all the time, only on first install", and the
  // full list lives permanently in Settings › Conflicts.
  //
  // localStorage, NOT sessionStorage: sessionStorage resets every launch, so
  // the banner came back on every single start. Keyed on the sorted product
  // list so a DIFFERENT program appearing later still gets one warning.
  const key = conflictKey(knownConflicts);
  if (localStorage.getItem("st-conflict-seen") === key) {
    el.hidden = true;
    return;
  }
  // Mark seen at render time, not on dismiss — closing the dashboard without
  // clicking ✕ must not re-arm it for the next launch.
  localStorage.setItem("st-conflict-seen", key);

  const names = knownConflicts.map((c) => c.product).join(", ");
  el.innerHTML = "";

  const txt = document.createElement("span");
  txt.className = "conflict-text";
  // textContent — process/product names are read off the user's machine.
  txt.textContent =
    `${names} ${knownConflicts.length === 1 ? "is" : "are"} running and can capture ` +
    `Space before Spaceadom sees it. If shortcuts do nothing, close it and try again.`;

  const details = document.createElement("button");
  details.className = "btn btn-sm";
  details.textContent = "Details";
  details.addEventListener("click", (e) => {
    // PROBLEM 98 — stopPropagation is LOAD-BEARING, not tidiness. #stage has
    // a click handler that closes every popover (main.ts, wirePopovers), and
    // this banner lives inside #stage. Without this the settings panel opened
    // and the SAME click then bubbled up and closed it again within one frame,
    // so the button looked completely dead. Reported by the user as "pressing
    // Details does nothing".
    e.stopPropagation();
    closeAllPopovers();
    openSettingsPanel();
  });

  const close = document.createElement("button");
  close.className = "conflict-close";
  close.setAttribute("aria-label", "Dismiss");
  close.textContent = "✕";
  close.addEventListener("click", () => {
    el.hidden = true; // already marked seen above; ✕ just hides it now
  });

  el.append(txt, details, close);
  el.hidden = false;
}

/**
 * The bottom-centre reference tray (PROBLEM 148).
 *
 * These were inert <span>s: they named the keys and left the user to guess
 * what "PiP Cycle" meant. Each is now a button that opens its card (spec §4).
 * The list itself comes from special-cards.ts, so the tray, the board and the
 * cards can never disagree about what a special is called.
 */
/**
 * PROBLEM 161 — say so when the keyboard hook is not installed.
 *
 * Rust has always known (`HOOK_INSTALLED`, surfaced as `HookStatus.installed`
 * since PROBLEM 66) and the dashboard has always thrown the value away. The
 * failure it hides is total and silent: no shortcut works, every part of the
 * UI looks perfectly healthy, and the only evidence is a line in a log file
 * the user does not know exists. On someone else's laptop that reads as "this
 * app just doesn't do anything".
 *
 * Deliberately a BANNER and not a toast: a toast is a notification of an
 * event, and this is a persistent state. It stays until the state changes.
 */
function applyHookState(installed: boolean): void {
  const el = document.getElementById("conflict-banner");
  if (!el) return;

  // PROBLEM 253 — in safe mode the hook is off BY DECISION, and this banner
  // would explain it wrongly.
  //
  // Its text blames another keyboard program or Windows and offers "Try again",
  // which reinstalls a hook that was never installed. All of that is right for
  // the failure it was written for and all of it is wrong here: the safe-mode
  // banner directly above says what actually happened and offers the button
  // that actually helps. Two banners contradicting each other about the same
  // symptom is worse than one.
  //
  // Keyed on the body class the safe-mode banner sets, so the suppression ends
  // the instant "Turn back on" removes it — there is no second piece of state
  // that can be left behind.
  if (!installed && document.body.classList.contains("st-safe-mode")) return;

  // Never fight the conflicts banner for the same strip: a detected conflict
  // is the LIKELIER explanation of a dead hook and its text is more useful.
  if (installed) {
    if (el.dataset.owner === "hook") { el.hidden = true; el.dataset.owner = ""; el.innerHTML = ""; }
    return;
  }
  if (el.dataset.owner && el.dataset.owner !== "hook") return;

  el.dataset.owner = "hook";
  el.innerHTML = "";
  const txt = document.createElement("span");
  txt.className = "conflict-text";
  txt.textContent =
    "Spaceadom is not receiving key presses, so no shortcut will work. " +
    "This usually means another keyboard program took the spacebar first, " +
    "or Windows blocked the connection. Restarting Spaceadom from its tray " +
    "icon fixes it most of the time.";

  const retry = document.createElement("button");
  retry.className = "btn btn-sm";
  retry.textContent = "Try again";
  retry.addEventListener("click", async (e) => {
    e.stopPropagation();
    retry.disabled = true;
    retry.textContent = "Trying…";
    try {
      await invoke("reinstall_hook");
      const s = await invoke<HookStatus>("get_hook_status");
      applyHookState(s.installed);
      if (!s.installed) { retry.disabled = false; retry.textContent = "Try again"; }
    } catch (_) {
      retry.disabled = false;
      retry.textContent = "Try again";
    }
  });

  const logs = document.createElement("button");
  logs.className = "btn btn-sm";
  logs.textContent = "Open log folder";
  logs.addEventListener("click", (e) => {
    e.stopPropagation();
    void invoke("open_log_folder");
  });

  el.append(txt, retry, logs);
  el.hidden = false;
}

function renderSpecials(): void {
  const tray = document.getElementById("specials-tray");
  if (!tray) return;
  tray.innerHTML = "";
  // PHASE A — combo/how come from the active profile (`resolveSpecials`):
  // "␣ F1" for a Boss Key moved to F1, "␣ —" for one bound nowhere.
  resolveSpecials(appConfig).forEach((spec, i) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "special-item";
    item.dataset.spec = spec.id;
    item.setAttribute("aria-expanded", "false");
    item.style.animationDelay = `${60 + i * 30}ms`;
    const k = document.createElement("kbd");
    k.textContent = spec.combo;
    const t = document.createElement("span");
    t.textContent = spec.name;
    item.append(k, t);
    item.addEventListener("click", (e) => {
      // #stage closes every popover on click (PROBLEM 98); this card is not
      // a popover and must survive its own opening press.
      e.stopPropagation();
      toggleSpecialCard(item, spec, i);
    });
    tray.appendChild(item);
  });

  // Teaching prose, visible only while "Show me around" is on (the owner's
  // choice between "a button that opens all the cards" and "just tell them
  // the chips are pressable" — this is the second, simpler one).
  const note = document.createElement("div");
  note.className = "sma-note specials-note";
  note.textContent = "Press any of these to read what it does — and try them out.";
  tray.appendChild(note);
}

// ---------------------------------------------------------------------------
// Stage motion
// ---------------------------------------------------------------------------

/**
 * PROBLEM 230 — the cursor glow's frame loop, hoisted out of `wireCursorGlow`
 * so `wireAmbientPause` can stop it.
 *
 * MEASURED, 2026-09-01, 1.0.95 on the owner's machine, with BOTH Spaceadom
 * windows minimised and nothing on screen: the app's own WebView2 tree burned
 * ~1.0 of 16 logical cores continuously — `gpu-process` 59.5% of a core (the
 * SOFTWARE compositor, `--disable-gpu`) and the dashboard renderer 36.4%. The
 * project's own budget for a backgrounded tray utility is ~0% and "investigate"
 * at >1% (references/performance-budget.md).
 *
 * `is-blurred` already parks the CSS animations. It cannot park a `requestAnim-
 * ationFrame` chain, and this one re-schedules itself unconditionally, forever,
 * from first paint — writing a transform to a 380px blurred radial every frame
 * even when the pointer has not moved since the window was hidden. Nothing on
 * screen, full frame cost.
 */
let _glowRaf = 0;
let _glowFrame: (() => void) | null = null;

function startCursorGlow(): void {
  if (_glowRaf || !_glowFrame) return;
  _glowRaf = requestAnimationFrame(_glowFrame);
}

function stopCursorGlow(): void {
  if (!_glowRaf) return;
  cancelAnimationFrame(_glowRaf);
  _glowRaf = 0;
}

/** Cursor-follow glow: 380px blurred radial, RAF lerp factor .09. */
function wireCursorGlow(): void {
  const stage = document.getElementById("stage");
  const glow = document.getElementById("cursor-glow");
  if (!stage || !glow) return;
  // motionReduced(), NOT the raw media query: the user's "Visual effects"
  // setting can override the OS, and querying the OS here would ignore it.
  if (motionReduced()) return;

  let tx = stage.clientWidth / 2, ty = stage.clientHeight / 2;
  let gx = tx, gy = ty;

  stage.addEventListener("mousemove", (e) => {
    const r = stage.getBoundingClientRect();
    tx = e.clientX - r.left;
    ty = e.clientY - r.top;
    glow.style.opacity = "1";
  });
  stage.addEventListener("mouseleave", () => { glow.style.opacity = "0"; });

  _glowFrame = () => {
    _glowRaf = 0;
    gx += (tx - gx) * 0.09;
    gy += (ty - gy) * 0.09;
    glow.style.transform = `translate(${gx - 190}px, ${gy - 190}px)`;
    // Re-arm through startCursorGlow so a `stop` that lands between two frames
    // cannot be undone by a frame that was already in flight.
    startCursorGlow();
  };
  // Bootstrap wires the ambient pause BEFORE this runs and the window is
  // usually blurred at that moment (a tray app starts unfocused), so ask the
  // class rather than assuming: start only if someone is actually looking.
  if (!document.body.classList.contains("is-blurred")) startCursorGlow();
}

// (The press ripple lives in keyboard-matrix.ts now, attached to EVERY key —
// spawning it from the letter-select callback here is exactly how the
// non-letter keys lost their feedback. PROBLEM 39.)

/**
 * Scale the fixed-geometry board to fit BOTH axes of whatever space it has.
 * The previous attempt scaled on width only and the keyboard ran off the
 * bottom/right of the display — this is that failure's fix, so do not
 * "simplify" it back to a single-axis scale.
 */
function wireKeyboardFit(): void {
  const outer = document.getElementById("keyboard-outer");
  const scale = document.getElementById("keyboard-scale");
  if (!outer || !scale) return;

  /**
   * PROBLEM 123 — the board may now grow, not only shrink.
   *
   * This was `Math.min(1, …)`. The 1 is a hard 1:1 ceiling: however much room
   * the window had, the keyboard stopped at its design size of
   * 1048x320 CSS px and the rest of the screen stayed empty. Together with the
   * window's own 1220x880 ceiling in lib.rs that made the dashboard a
   * fixed-size island on any large monitor — reported by the owner as the
   * keyboard looking small and the space being wasted.
   *
   * Scaling ABOVE 1 is safe because this is a CSS `transform: scale()` on the
   * whole board: every key, gap, radius, shadow and label scales by the same
   * factor, so the design's proportions are preserved exactly. It is the same
   * mechanism that already handled shrinking; only the ceiling changed.
   *
   * MAX_SCALE is a safety valve, not a design limit. Taking the MIN across
   * both axes already bounds this on any sane display; the cap only exists so
   * that a pathological viewport (a very tall narrow window, a mis-reported
   * monitor) cannot produce absurd geometry. 2.5x covers a 4K panel at 100%.
   */
  /**
   * PROBLEM 128, second attempt — the board takes a fixed PROPORTION of the
   * room, so the breathing room is a proportion too.
   *
   * History, because this one line has now been wrong in opposite directions:
   *   - 1.0.36/37 filled the room to a fixed 12px margin. Owner, twice:
   *     "scaled too much, no breathing room".
   *   - The first fix (GROWTH=0.5, built as 1.0.39) NEVER REACHED HIS SCREEN:
   *     the MSI deferred the upgrade while the app was running (PROBLEM 127),
   *     so his "still the same" verdict was about a binary that did not
   *     contain it. Do not judge this formula by that report.
   *   - He then specified the design himself: "make the keyboard 0.75 times
   *     of what is running right now" — 75% of the fill, at EVERY size.
   *
   * FILL is that number. board = room * FILL, so the margin is always
   * (1 - FILL) of the available space: a quarter of a small screen, a quarter
   * of a 4K panel. That is what "proportionate breathing room" means, and the
   * board scales continuously with the display instead of being pinned at
   * either extreme.
   *
   * NOTE: this also applies BELOW the design size — small screens get the
   * same 25% margin instead of filling to the old 12px. Deliberate reading of
   * "it should scale up or down depending on the size of the display", and
   * flagged to the owner rather than slipped in.
   *
   * TUNING: FILL is the one knob, and 0.75 is the owner's own number, not a
   * guess. MAX_SCALE stays as a backstop for pathological viewports only.
   */
  const FILL = 0.75;
  const MAX_SCALE = 2.0;
  const fit = () => {
    const r = outer.getBoundingClientRect();
    if (!r.width || !r.height) return;
    const room = Math.min(r.width / DESIGN_W, r.height / DESIGN_H);
    const s = Math.min(MAX_SCALE, room * FILL);
    scale.style.transform = `scale(${s.toFixed(4)})`;
    // Published for anything else that should grow with the board. Nothing
    // consumes it yet — the popovers are the obvious candidate, but their
    // entry animation already owns `transform`, so scaling them needs `zoom`
    // and a check on how their absolute offsets behave under it. That is a
    // judgement to make from a screenshot, not from reasoning.
    document.documentElement.style.setProperty("--ui-scale", s.toFixed(4));
  };

  fit();
  new ResizeObserver(fit).observe(outer);
  window.addEventListener("resize", fit);
}

// ---------------------------------------------------------------------------
// Fatal error
// ---------------------------------------------------------------------------

function showFatalError(msg: string): void {
  document.body.innerHTML = "";
  const wrap = document.createElement("div");
  wrap.id = "fatal";

  const mark = document.createElement("div");
  mark.style.cssText = "font-size:40px";
  mark.textContent = "⌾";

  const title = document.createElement("div");
  title.style.cssText = "font-family:var(--st-font-heading); font-size:20px";
  title.textContent = "Spaceadom";

  const body = document.createElement("div");
  body.style.cssText = "font-size:14px; color:var(--st-text-dim); max-width:420px";
  body.textContent = msg;

  wrap.append(mark, title, body);
  document.body.appendChild(wrap);
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

window.addEventListener("DOMContentLoaded", () => {
  bootstrap().catch((e) => {
    console.error("SpaceToggle bootstrap failed:", e);
    showFatalError("A critical error occurred during startup. Check the debug log.");
  });
});
