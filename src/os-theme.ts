/**
 * os-theme.ts — REVIEW FIXES 2026-09-05 (H4). The Tauri half of "does Windows
 * want dark app surfaces?", kept in its own file so `theme-resolve.ts` can
 * stay a leaf module.
 *
 * WHAT THIS IS FOR. `theme-resolve.ts` holds the RULE (raw theme → one of the
 * three palettes) and the VALUE it resolves `"auto"` against. It deliberately
 * imports nothing, so both bundles and `preview.ts` can take it without
 * dragging a backend along. This file is the two lines of plumbing that fill
 * that value in from Rust, and it is imported by exactly the two entry points
 * that own a window: `main.ts` and `overlay.ts`.
 *
 * WHY IT COMES FROM RUST AND NOT FROM `matchMedia`. Inside a Tauri window,
 * `prefers-color-scheme` reports the colour scheme the WEBVIEW was told to
 * prefer, not the one the USER chose. `tauri.conf.json` pinned the `settings`
 * window to `"theme": "Light"`, so the dashboard's query answered "light" on a
 * dark machine, permanently — while the overlay, with no such pin, answered
 * "dark". That is the whole of H4: one rule, two inputs, two palettes on one
 * screen, and a `dark_mode: false` written into `config.json` from the wrong
 * half. `config/mod.rs::os_prefers_dark()` reads
 * `HKCU\…\Themes\Personalize\AppsUseLightTheme` and cannot be lied to by a
 * window configuration.
 *
 * BOTH HALVES, EVERY TIME — the same rule the overlay's band-count and ring
 * layout follow (see `overlay.ts`): an event that only fires on CHANGE leaves
 * a freshly-opened window wearing whatever the module defaulted to, so there
 * is a SEED (`get_os_prefers_dark`) as well as a LISTENER
 * (`os-theme-changed`). A first launch on a dark machine with "Auto" selected
 * is exactly the case a seedless wiring gets wrong, and it is also the
 * default for every new install (schema.rs).
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { setOsPrefersDark } from "./theme-resolve";

/**
 * Set once the LISTENER is genuinely registered — not when this function is
 * entered.
 *
 * `listen()` returns a promise and the registration is not live until it
 * resolves; flipping a "wired" flag before that means a second call returns
 * early while nothing is listening, which is a window that never notices an
 * OS theme change for the rest of its life. Same correction as the
 * `update-status` listener in `settings-panel.ts`.
 */
let _wired = false;

/**
 * The in-flight run, if there is one. Two windows call this once each, so in
 * the shipping app there is never a second caller — but `preview.ts`'s probe
 * calls it too, and a guard that only closes when the promise RESOLVES leaves
 * a window in which a concurrent call registers a second listener. Returning
 * the same promise is the cheapest way to make the function idempotent under
 * concurrency as well as under repetition; the same correction as
 * `settings-panel.ts`'s `update-status` listener, which had the same shape.
 */
let _inFlight: Promise<void> | null = null;

/**
 * Seed the OS preference and keep it current for the life of this window.
 *
 * ORDER MATTERS, and it is listener-then-seed. Registering first means a flip
 * that happens between the two calls arrives as an event rather than being
 * lost in the gap; the seed can only ever be as stale as the moment it was
 * asked, and any change after that is covered.
 *
 * SILENT ON FAILURE, in both halves and for two different reasons. An older
 * backend has no `get_os_prefers_dark` command, and `preview.ts`'s harness has
 * no backend at all — in both cases the value simply stays at
 * `theme-resolve.ts`'s daylight default, which is the documented fallback for
 * "the question could not be answered" and matches what
 * `config/mod.rs::resolve_theme` does with `None`. A toast here would fire on
 * every launch of a build that is working exactly as designed.
 *
 * Awaited by both callers BEFORE their first paint, so a dark machine with
 * "Auto" selected comes up in Starry night rather than flashing Earthy and
 * cross-fading a beat later.
 */
export function initOsTheme(): Promise<void> {
  if (_wired) return Promise.resolve();
  if (_inFlight) return _inFlight;
  _inFlight = run().finally(() => { _inFlight = null; });
  return _inFlight;
}

async function run(): Promise<void> {
  try {
    await listen<{ dark?: boolean }>("os-theme-changed", (e) => {
      // `=== true` and not a truthiness test: the payload crosses a JSON
      // boundary, and an absent field must read as "not dark" rather than as
      // `undefined` sneaking through a `!!`.
      setOsPrefersDark(e.payload?.dark === true);
    });
    _wired = true;
  } catch {
    /* no backend (harness) or a build without the watcher — the seed below
       still runs, and a fixed theme never needed either. */
  }
  try {
    setOsPrefersDark(await invoke<boolean>("get_os_prefers_dark"));
  } catch {
    /* older backend / harness: daylight, the documented fallback. */
  }
}
