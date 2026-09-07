import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { installJsErrorReporter } from "./js-error-reporter";
import { applyBandCount } from "./components/hud-band-count";
import { applyHudLayout } from "./components/hud-layout";
import {
  initToastListener,
  applyTheme,
  applyThemeName,
  applySound,
  applyFlight,
  markOverlayWindow,
  reapplyResolvedTheme,
} from "./components/toast";
import { onSystemThemeChange } from "./theme-resolve";
// REVIEW FIXES 2026-09-05 (H4) — the shared OS light/dark value, seeded from
// Rust and kept current by the `os-theme-changed` event. The dashboard takes
// the identical import; that is the point.
import { initOsTheme } from "./os-theme";

// THIS is the overlay window. Only here may toast.ts resize/hide the overlay
// via overlay_fit / overlay_toasts_done — the dashboard shares this module for
// its own toasts and must never touch those commands (PROBLEM 45).
markOverlayWindow();

// PROBLEM 185 regression fix — a marker to scope CSS to THIS body only.
// overlay.html's <body> had no id/class of its own (it holds only
// #toast-container), so nothing distinguished it from the dashboard's body
// for CSS purposes. applyThemeName() (toast.ts) now sets the same
// `data-theme` attribute the dashboard uses, and themes.css has full-bleed
// `body.nocturne[data-theme="warcry"/"starry"]::before` decoration keyed off
// exactly that attribute — see the reset in styles/overlay-earthy.css, which
// needs this class to target the overlay body without touching the
// dashboard's. Set before DOMContentLoaded so it is present for the very
// first paint.
document.body.classList.add("st-overlay");

// Surface overlay JS failures in the Rust log — the webview console is
// invisible in production, so without this an exception here just looks
// like "the HUD didn't appear".
//
// PROBLEM 217 — these two handlers used to be written out here and reported
// through `overlay_log`, which is `log::warn!` and therefore BELOW the crash
// reporter's floor: an exception in this file reached the local debug.log and
// nowhere else. They are now the shared reporter, which routes to
// `overlay_error` (ERROR, so it can be reported), adds the column and the
// stack, and rate-limits itself so a HUD that throws on every hold cannot
// flood. This REPLACES the pair — there is still exactly one `error` and one
// `unhandledrejection` listener on this window.
installJsErrorReporter("overlay_error");

/**
 * PROBLEM 255 — FOLLOW SYSTEM THEME, the overlay's own half.
 *
 * The theme pill can store the literal `"auto"`, and this window has to
 * resolve it exactly the way the dashboard does — `applyThemeName` now does,
 * through the shared `theme-resolve.ts`. But a setting that follows Windows
 * and only notices at launch is wrong for most of the day, so the overlay
 * needs a listener of its own as well as the seed above.
 *
 * **REVIEW FIXES 2026-09-05 (H4) — IT IS A CROSS-WINDOW SIGNAL NOW, AND THAT
 * IS THE FIX.** This used to say "its own, and deliberately not a cross-window
 * signal … a separate webview with a separate `matchMedia` that answers the
 * same question". The two webviews did NOT answer the same question:
 * `tauri.conf.json` pinned the DASHBOARD to `"theme": "Light"`, and
 * `prefers-color-scheme` reports the scheme a webview was told to prefer, so
 * the dashboard read "light" on a dark machine while this window read "dark" —
 * `"auto"` in Earthy here and Starry night there, on one screen. Rust now
 * reads the registry once (`theme_watch.rs`) and emits `os-theme-changed` to
 * every window; `os-theme.ts` feeds it into the shared value both
 * `onSystemThemeChange` listeners hang off. The dashboard registers the
 * identical listener in `main.ts` against the identical helper.
 *
 * At module scope, not inside `DOMContentLoaded`: it touches no DOM of its
 * own (`applyThemeName` writes to `document.body`, which exists as soon as
 * this module runs — `document.body.classList.add("st-overlay")` above
 * already depends on that), and registering it here means an OS flip during
 * the window's first paint is not silently missed.
 *
 * A no-op for a fixed theme: `reapplyResolvedTheme` re-runs the resolution on
 * the RAW value, and a raw `"starry"` resolves to `"starry"` whatever Windows
 * says.
 */
onSystemThemeChange(() => {
  reapplyResolvedTheme();
});

window.addEventListener("DOMContentLoaded", () => {
  // This webview is the ONLY registered listener for backend toast/HUD events.
  // The dashboard deliberately does not register (see main.ts step 9) —
  // otherwise Tauri's target-Any listen() would render every HUD/toast inside
  // the settings window too.
  //
  // Failures go to the Rust log, NOT console.error — a silent listen()
  // rejection here (e.g. this window missing from capabilities/default.json)
  // is exactly how the HUD shipped as an empty box (2026-08-10).
  initToastListener()
    .then(() => invoke("overlay_log", { msg: "listeners registered OK" }).catch(() => {}))
    .catch((e) =>
      invoke("overlay_log", { msg: `listener init FAILED: ${e}` }).catch(() => {}),
    );

  // Seed the theme from the persisted config. The "theme-changed" event only
  // fires when the setting CHANGES, so without this the overlay would start
  // in the light palette every launch and only correct itself the next time
  // the user touched the toggle — a split-theme app, which the design rules
  // out explicitly ("ONE setting drives everything").
  // HALF ONE of the band-count wiring: the LISTENER. `save_config` emits this
  // globally (`emit`, never `emit_to` — the only arrangement that has ever
  // delivered here), and it is registered from THIS file rather than from
  // `initToastListener` because the value lives in its own leaf module.
  listen<string>("hud-band-count-changed", (e) => {
    applyBandCount(e.payload);
  }).catch((e) =>
    invoke("overlay_log", { msg: `band-count listen FAILED: ${e}` }).catch(() => {}),
  );

  // HALF ONE of the ring-LAYOUT wiring, and the same shape as the band count
  // directly above: `save_config` emits this globally (`emit`, never
  // `emit_to`), and it is registered from THIS file rather than from
  // `initToastListener` because the value lives in its own leaf module.
  //
  // The payload is the raw config BOOL. `applyHudLayout` is the only place it
  // is ever turned into a name, so nothing downstream compares it.
  listen<boolean>("hud-layout-changed", (e) => {
    applyHudLayout(e.payload);
  }).catch((e) =>
    invoke("overlay_log", { msg: `hud-layout listen FAILED: ${e}` }).catch(() => {}),
  );

  // REVIEW FIXES 2026-09-05 (H4) — ASK WINDOWS FIRST, THEN SEED THE PALETTE.
  //
  // `applyThemeName` below resolves the raw theme — which may be the literal
  // `"auto"` — through the shared rule, and that rule needs the OS value the
  // dashboard is using. Chained AHEAD of `get_config` rather than fired
  // alongside it so this window cannot paint Earthy and correct itself a
  // moment later, and so the two windows cannot be seeding from different
  // answers during the seconds after a launch.
  //
  // `.catch(() => {})` between the two: `initOsTheme` already swallows its own
  // failures, and this guarantees that even a future change to it can never
  // stop the config seed — the palette matters, but the sound, motion, band
  // count and ring layout below matter as much and have nothing to do with
  // the theme.
  initOsTheme()
    .catch(() => {})
    .then(() => invoke<{
      dark_mode?: boolean;
      theme?: string;
      sound_enabled?: boolean;
      motion?: string;
      hud_toast_flight?: boolean;
      hud_band_count?: string;
      hud_magnetic_layout?: boolean;
    }>("get_config"))
    .then((cfg) => {
      applyTheme(!!cfg?.dark_mode);
      // PROBLEM 185 — seeded here for the same reason as the theme bool:
      // "theme-name-changed" only fires on a CHANGE, so a freshly created
      // overlay (first launch, or after a display-change rebuild) would
      // otherwise wear the wrong palette until the user next switched theme.
      // PROBLEM 255 — the fallback is `dark_mode`, not the bare string
      // "earthy", and it mirrors `main.ts::applyLook()`'s first line exactly:
      // `appConfig?.theme || (appConfig?.dark_mode ? "starry" : "earthy")`.
      // `applyThemeName` now also owns the nocturne class (it has to, so an
      // OS flip under `"auto"` can move it with nothing from Rust), which
      // means it runs AFTER the `applyTheme` seed above and gets the last
      // word. A `?? "earthy"` here would give a config with an EMPTY theme
      // string the last word as daylight and quietly undo the dark seed one
      // line up. `||` catches the empty string; `??` does not.
      applyThemeName(cfg?.theme || (cfg?.dark_mode ? "starry" : "earthy"));
      applySound(!!cfg?.sound_enabled);
      // PROBLEM 174 — seed the guide-to-toast motion from the saved config for
      // the same reason the theme is seeded here: "flight-changed" only fires
      // on a CHANGE, so an overlay that has just been created (first launch,
      // or after a display-change rebuild) would otherwise run with the
      // module's own default until the user next touched the switch.
      // `=== true`: the key is absent from every config written before 1.0.73
      // and absent must mean OFF.
      applyFlight(cfg?.hud_toast_flight === true);
      // HALF TWO, and the one that gets skipped: the SEED. Without it the
      // listener above only corrects the overlay the next time the user
      // touches the setting, so a first launch — or the overlay Rust rebuilds
      // when the display setup changes — would lay the ring out on this
      // module's default while the dashboard showed something else. Exactly
      // the split-state failure the theme rule exists to prevent.
      //
      // No `=== true` / `!== false` question to get wrong here: it is a string
      // enum, and `applyBandCount` normalises anything that is not "one" or
      // "two" — `undefined` included — to "auto".
      applyBandCount(cfg?.hud_band_count);
      // HALF TWO for the ring layout, for exactly the reason spelled out
      // above: without the seed, a first launch — or the overlay Rust
      // rebuilds when the display setup changes — would draw whichever
      // layout the module defaults to until the user next flipped the
      // switch. Both halves, every time.
      //
      // The raw bool is handed straight in: `applyHudLayout` applies the
      // `!== false` rule (absent means the NEW ring, because the key is
      // missing from every config written before 1.0.89). Do not "help" by
      // writing `cfg?.hud_magnetic_layout === true` here — that would ship
      // the flip to nobody.
      applyHudLayout(cfg?.hud_magnetic_layout);
      // Same "Visual effects" resolution as the dashboard (PROBLEM 47). The
      // overlay is a SEPARATE document, so it must set the class on its own
      // <html> — the dashboard's copy is invisible to it. Without this the
      // HUD bloom and toast entrances would ignore the user's override.
      // Same rule as the dashboard: the OS reduced-motion signal is IGNORED
      // (owner decision, 2026-08-12). Only an explicit in-app "reduced"
      // switches effects off. Battery Saver / Accessibility must never strip
      // the app's motion on their own.
      const reduced = cfg?.motion === "reduced";
      document.documentElement.classList.toggle("reduced-motion", reduced);
      document.documentElement.dataset.motionResolved = "1";
    })
    .catch(() => { /* default is the light palette, which is the default theme */ });

  // Beacon so the Rust log can prove this webview's JS actually booted.
  // (Added 2026-08-10 while diagnosing the invisible-overlay bug; cheap
  // enough to keep as a permanent health check.)
  invoke("overlay_ready").catch(() => {});
});
