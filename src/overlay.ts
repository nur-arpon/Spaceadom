import { invoke } from "@tauri-apps/api/core";
import {
  initToastListener,
  applyTheme,
  applySound,
  markOverlayWindow,
} from "./components/toast";

// THIS is the overlay window. Only here may toast.ts resize/hide the overlay
// via overlay_fit / overlay_toasts_done — the dashboard shares this module for
// its own toasts and must never touch those commands (PROBLEM 45).
markOverlayWindow();

// Surface overlay JS failures in the Rust log — the webview console is
// invisible in production, so without this an exception here just looks
// like "the HUD didn't appear".
window.addEventListener("error", (e) => {
  invoke("overlay_log", { msg: `error: ${e.message} @ ${e.filename}:${e.lineno}` }).catch(() => {});
});
window.addEventListener("unhandledrejection", (e) => {
  invoke("overlay_log", { msg: `unhandled rejection: ${e.reason}` }).catch(() => {});
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
  invoke<{ dark_mode?: boolean; sound_enabled?: boolean; motion?: string }>("get_config")
    .then((cfg) => {
      applyTheme(!!cfg?.dark_mode);
      applySound(!!cfg?.sound_enabled);
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
