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
  appConfig, persistConfig, applyTheme, applySound, applyMotion,
  knownConflicts, refreshConflicts,
} from "../main";
import { showToast } from "./toast";

let panelEl: HTMLElement | null = null;
let _paused = false;
/** Two-step confirm state for the destructive buttons: "def" | "clr" | null */
let _armed: "def" | "clr" | null = null;
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

  const dark = !!appConfig.dark_mode;
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

  panelEl.innerHTML = `
    <div class="set-title">Settings</div>

    <div class="set-rows">
      ${toggleRow("engine",  "Engine active",  !_paused, 0)}
      ${toggleRow("dark",    "Dark mode",      dark,     1)}
      ${toggleRow("sound",   "Sound ticks",    sound,    2)}
      ${toggleRow("startup", "Run at startup", startup,  3)}
      ${toggleRow("motion",  "Visual effects", effects,  4)}
      ${toggleRow("software", "Software overlay", software, 5)}
    </div>
    <div class="set-note" style="margin:-2px 2px 0; font-size:11px; opacity:.62; line-height:1.45;">
      Turn on if the Guide HUD or toasts stop appearing while sounds still
      play. Applies at the next launch.
    </div>

    <div class="divider" style="margin:14px 0 10px;"></div>

    ${typingSpeedRow(appConfig.typing_wpm ?? DEFAULT_WPM)}
    ${sliderRow("huddelay", "Guide HUD delay", appConfig.guide_hud_delay_ms, 100, 1000, 50, "ms")}
    ${sliderRow("opacity",  "Opacity floor",   appConfig.opacity_floor_pct, 10, 90, 5, "%")}

    <div class="divider" style="margin:14px 0 10px;"></div>
    <div class="set-title" style="font-size:13px; margin-bottom:8px;">Conflicts</div>
    <div id="set-conflicts"></div>

    <div class="set-actions">
      <button class="btn" id="set-reset">${_armed === "def" ? "Confirm" : resetLabel()}</button>
      <button class="btn btn-danger" id="set-clear">${_armed === "clr" ? "Confirm clear" : "Clear all"}</button>
    </div>
    <button class="btn" id="set-presets" style="width:100%; justify-content:center; margin-top:7px; height:34px; font-size:12px;">Restore preset profiles</button>
    <button class="btn" id="set-logs" style="width:100%; justify-content:center; margin-top:7px; height:34px; font-size:12px;">Open log folder</button>
  `;

  wireToggle("engine", async () => {
    try {
      const paused = await invoke<boolean>("toggle_bypass");
      _paused = paused;
      showToast(paused ? "⏸️ Engine paused" : "▶️ Engine active");
    } catch (_) {
      showToast("⚠️ Could not toggle the engine");
    }
    render();
  });

  wireToggle("dark", async () => {
    if (!appConfig) return;
    appConfig.dark_mode = !appConfig.dark_mode;
    applyTheme(appConfig.dark_mode);      // dashboard + overlay, one setting
    await persistConfig();
    render();
  });

  wireToggle("sound", async () => {
    if (!appConfig) return;
    appConfig.sound_enabled = !appConfig.sound_enabled;
    applySound(appConfig.sound_enabled);
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

  renderConflicts();

  // PROBLEM 109 — the way back from deleting a preset. Additive: it restores
  // only the presets that are MISSING and never overwrites one the user still
  // has, so it is safe to press even after months of customisation.
  panelEl.querySelector("#set-presets")!.addEventListener("click", async (e) => {
    e.stopPropagation();                       // #stage closes popovers (PROBLEM 98)
    try {
      const restored = await invoke<string[]>("restore_preset_profiles");
      showToast(
        restored.length === 0
          ? "✓ All preset profiles are already here"
          : `↺ Restored ${restored.join(", ")}`,
      );
    } catch (_) {
      showToast("⚠️ Could not restore the presets");
    }
  });

  panelEl.querySelector("#set-logs")!.addEventListener("click", () => {
    void invoke("open_log_folder").catch(() => showToast("⚠️ Could not open the log folder"));
  });

  wireTypingSpeed();
  wireSlider("huddelay", (v) => { if (appConfig) appConfig.guide_hud_delay_ms = v; });
  wireSlider("opacity",  (v) => { if (appConfig) appConfig.opacity_floor_pct = v; });

  // Destructive actions arm on the first click and fire on the second —
  // a window.confirm() dialog over this stage looks like a different app.
  panelEl.querySelector("#set-reset")!.addEventListener("click", () => {
    if (_armed !== "def") { arm("def"); return; }
    disarm();
    _onResetDefaults?.();
  });
  panelEl.querySelector("#set-clear")!.addEventListener("click", () => {
    if (_armed !== "clr") { arm("clr"); return; }
    disarm();
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
      knownConflicts.forEach((c) => {
        const row = document.createElement("div");
        row.className = "conflict-row";

        const name = document.createElement("span");
        name.className = "conflict-row-name";
        name.textContent = c.product;          // textContent — read off the machine

        const proc = document.createElement("span");
        proc.className = "conflict-row-proc";
        proc.textContent = c.process;

        const why = document.createElement("span");
        why.className = "conflict-row-why";
        why.textContent = c.detail;

        row.append(name, proc, why);
        box.appendChild(row);
      });

      const hint = document.createElement("div");
      hint.className = "set-note";
      hint.textContent =
        "Close one of them — either the program above, or Spaceadom — so only " +
        "one owns the spacebar. Spaceadom never closes other programs for you.";
      box.appendChild(hint);
    }

    const again = document.createElement("button");
    again.className = "btn btn-sm";
    again.style.cssText = "width:100%; justify-content:center; margin-top:8px;";
    again.textContent = "Re-check now";
    again.addEventListener("click", async () => {
      again.textContent = "Checking…";
      await refreshConflicts();
      draw();
    });
    box.appendChild(again);
  };

  draw();
}

// ---------------------------------------------------------------------------
// Row builders
// ---------------------------------------------------------------------------

function toggleRow(id: string, label: string, on: boolean, i: number): string {
  return `
    <label class="set-row" style="animation-delay:${60 + i * 45}ms">
      <span class="set-row-label">${label}</span>
      <span class="toggle-switch">
        <input type="checkbox" id="set-${id}" ${on ? "checked" : ""} />
        <span class="toggle-track"><span class="toggle-thumb"></span></span>
      </span>
    </label>`;
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
        <span class="set-row-label">Typing speed</span>
        <span style="font-size:11px; font-weight:700; color:var(--st-accent-deep);"
              id="set-wpm-val">${typingTierName(wpm)} · ${wpm} wpm</span>
      </div>
      <div class="wpm-ticks" id="set-wpm-ticks">${ticks}</div>
      <input type="range" id="set-wpm" min="${WPM_MIN}" max="${WPM_MAX}" step="5" value="${wpm}"
             style="width:100%; accent-color:var(--st-accent);" />
      <span style="font-size:10.5px; color:var(--st-ink-soft); line-height:1.35;">
        If apps launch by accident while you type, choose a SLOWER speed —
        Spaceadom then waits longer before treating Space+key as a shortcut.
      </span>
    </div>`;
}

function sliderRow(
  id: string, label: string, value: number,
  min: number, max: number, step: number, unit: string,
): string {
  return `
    <div class="set-row" style="flex-direction:column; align-items:stretch; gap:4px; cursor:default; margin-bottom:10px;">
      <div style="display:flex; align-items:baseline; gap:8px;">
        <span class="set-row-label">${label}</span>
        <span style="font-size:11px; font-weight:700; color:var(--st-accent-deep);" id="set-${id}-val">${value}${unit}</span>
      </div>
      <input type="range" id="set-${id}" min="${min}" max="${max}" step="${step}" value="${value}"
             data-unit="${unit}" style="width:100%; accent-color:var(--st-accent);" />
    </div>`;
}

function wireToggle(id: string, onChange: () => void | Promise<void>): void {
  const el = panelEl?.querySelector<HTMLInputElement>(`#set-${id}`);
  el?.addEventListener("change", () => void onChange());
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
}
