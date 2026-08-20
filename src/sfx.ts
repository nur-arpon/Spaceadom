/**
 * sfx.ts — the dashboard's one Sfx instance (PROBLEM 147).
 *
 * WHY THIS IS ITS OWN MODULE AND NOT A CONST IN main.ts: three different
 * modules make sounds (main, settings-panel, starry-sky), and main.ts already
 * imports two of them. Hanging the instance off main would have starry-sky
 * import main while main imports starry-sky — a cycle whose failure mode is a
 * silently `undefined` binding at first use, not a build error. A leaf module
 * that imports nothing from the app cannot be part of one.
 *
 * The gates are CLOSURES over a getter, not values, so flipping "Sound ticks"
 * or "Fun mode" takes effect on the very next sound with nothing to
 * re-register — and the config can be swapped wholesale (reload, reset to
 * defaults) without leaving a stale object captured here.
 */
import type { AppConfig } from "./types";
import { Sfx } from "./sounds.js";

let _get: () => AppConfig | null = () => null;

/** Called once from bootstrap, before the first sound can possibly fire. */
export function bindSfxConfig(get: () => AppConfig | null): void {
  _get = get;
}

export const sfx = new Sfx({
  enabled: () => _get()?.sound_enabled === true,
  fun: () => _get()?.fun_mode === true,   // off-by-default since 2026-08-20
  volume: () => 40,
});

/**
 * WebAudio refuses to start a context before a real user gesture, and a
 * context created too early stays "suspended" forever on some builds — so the
 * unlock rides the first pointer or key event and then removes itself.
 * Capture phase, so a handler that stops propagation cannot mute the app.
 */
export function wireSfxUnlock(): void {
  const go = (): void => {
    sfx.unlock();
    window.removeEventListener("pointerdown", go, true);
    window.removeEventListener("keydown", go, true);
  };
  window.addEventListener("pointerdown", go, true);
  window.addEventListener("keydown", go, true);
}
