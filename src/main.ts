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
import { initProfileEditor, refreshProfileList, syncPill } from "./components/profile-editor";
import {
  initSettingsPanel,
  openSettingsPanel,
  closeSettingsPanel,
  isSettingsPanelOpen,
  setPausedState,
} from "./components/settings-panel";
import { showToast } from "./components/toast";

import type { AppConfig, HookStatus, KeyBinding } from "./types";

/** One other keyboard remapper found running (Rust: hook::conflicts::Conflict). */
export interface Conflict {
  process: string;
  product: string;
  detail: string;
}

/** Cached for the Settings › Conflicts section so it doesn't re-scan on every
 *  gear open. Refreshed by the Re-check button there. */
export let knownConflicts: Conflict[] = [];

// ---------------------------------------------------------------------------
// App-level state
// ---------------------------------------------------------------------------

export let appConfig: AppConfig | null = null;

/** Reference list shown in the bottom-centre tray. */
const SPECIALS: [string, string][] = [
  ["␣ Esc", "Boss Key"],
  ["␣ `", "PiP Cycle"],
  ["␣ ⌫", "Force Close"],
  ["␣ ↑↑", "Scroll Top"],
  ["␣ ,", "Smart Search"],
  ["␣ .", "Pause"],
  ["␣ RAlt", "Cycle Profile"],
  ["␣ Scroll", "Opacity"],
];

// ---------------------------------------------------------------------------
// Bootstrap
// ---------------------------------------------------------------------------

async function bootstrap(): Promise<void> {
  try {
    appConfig = await invoke<AppConfig>("get_config");
  } catch (e) {
    console.error("SpaceToggle: failed to load config —", e);
    showFatalError("Could not connect to the Spaceadom backend.");
    return;
  }

  // Theme first, so nothing paints in the wrong palette then snaps.
  applyTheme(!!appConfig.dark_mode);
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
        if (profile) profile.bindings[key] = binding;
        await persistConfig();
        refreshBoard();
        refreshProfileList(appConfig);
      },
    );
  }

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

  // ---- conflicts ----
  // Scanned once at startup and shown as a DISMISSIBLE BANNER, never a toast:
  // a popup on every launch is annoying, and this is reference information,
  // not an event. Always also visible under Settings › Conflicts.
  void refreshConflicts();

  // ---- stale startup task (PROBLEM 75) ----
  // Delayed: the Rust side triages the task on a background thread right
  // after launch, so an immediate query could race it and read false.
  window.setTimeout(() => void checkStaleTask(), 3000);

  // ---- stage chrome ----
  wirePopovers();
  renderSpecials();
  wireCursorGlow();
  wireKeyboardFit();

  // ---- backend sync ----
  try {
    const status = await invoke<HookStatus>("get_hook_status");
    setPausedState(status.bypass_active);
  } catch (_) { /* status is informational */ }

  await listen<AppConfig>("config-updated", (event) => {
    appConfig = event.payload;
    applyTheme(!!appConfig.dark_mode);
    applySound(!!appConfig.sound_enabled);
    applyMotion(appConfig.motion);
    refreshBoard();
    refreshProfileList(appConfig);
    updatePanelConfig(appConfig);
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

  // PROBLEM 74 — LAST step, after every component above is wired: tell Rust
  // the frontend is alive. Rust shows the window only now, so the first thing
  // the user ever sees is a dashboard that can paint and respond — never the
  // "(Not Responding)" ghost of a window whose webview is still booting.
  void invoke("dashboard_ready").catch(() => {});
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
}

/** ONE setting drives the dashboard AND the overlay (Rust re-emits on save). */
export function applyTheme(dark: boolean): void {
  document.body.classList.toggle("nocturne", dark);
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

  // Click anywhere on the stage closes them all.
  document.getElementById("stage")!.addEventListener("click", () => closeAllPopovers());
}

function closeAllPopovers(): void {
  const pop = document.getElementById("profile-popover");
  const pill = document.getElementById("profile-pill");
  if (pop) pop.hidden = true;
  pill?.setAttribute("aria-expanded", "false");

  const tray = document.getElementById("specials-tray");
  const trayBtn = document.getElementById("specials-btn");
  if (tray) tray.hidden = true;
  if (trayBtn) {
    trayBtn.setAttribute("aria-expanded", "false");
    trayBtn.textContent = "Special keys";
  }

  closeSettingsPanel();
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
    el.hidden = true;
    return;
  }
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

function renderSpecials(): void {
  const tray = document.getElementById("specials-tray");
  if (!tray) return;
  tray.innerHTML = "";
  SPECIALS.forEach(([combo, what], i) => {
    const item = document.createElement("span");
    item.className = "special-item";
    item.style.animationDelay = `${60 + i * 30}ms`;
    const k = document.createElement("kbd");
    k.textContent = combo;
    const t = document.createElement("span");
    t.textContent = what;
    item.append(k, t);
    tray.appendChild(item);
  });
}

// ---------------------------------------------------------------------------
// Stage motion
// ---------------------------------------------------------------------------

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

  const loop = () => {
    gx += (tx - gx) * 0.09;
    gy += (ty - gy) * 0.09;
    glow.style.transform = `translate(${gx - 190}px, ${gy - 190}px)`;
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
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

  const fit = () => {
    const r = outer.getBoundingClientRect();
    if (!r.width || !r.height) return;
    const s = Math.min(1, (r.width - 12) / DESIGN_W, (r.height - 12) / DESIGN_H);
    scale.style.transform = `scale(${s.toFixed(4)})`;
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
