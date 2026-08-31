/**
 * hud-band-count.ts — the overlay's copy of the user's `hud_band_count`
 * setting, and the one place it is normalised.
 *
 * WHY A MODULE OF ITS OWN, AND WHY A LEAF. `applyTheme`, `applySound` and
 * `applyFlight` all live in `toast.ts` and are called from `overlay.ts` on
 * load; that is the pattern this follows. It is split out only because
 * `toast.ts` is being rewritten in parallel with this change, and a setter
 * that lives in the file being rewritten cannot be added by the file that
 * needs to call it. Nothing here imports from the app (no `main.ts`, no
 * `toast.ts`), so `preview.ts` can pull it in without dragging the bootstrap
 * along — the PROBLEM 148 rule.
 *
 * THE CONTRACT, both halves of it:
 *
 *   1. `overlay.ts` SEEDS this from `get_config` on load, and
 *   2. `overlay.ts` UPDATES it from the global `hud-band-count-changed` event
 *      that `save_config` emits.
 *
 * Both are required and always have been for every setting the overlay reads:
 * an event that only fires on CHANGE leaves a freshly created overlay (first
 * launch, or a display-change rebuild) carrying this module's own default
 * until the user next touches the setting. That is the rule CLAUDE.md records
 * for the theme, and the seed is the half that gets skipped.
 *
 * READERS: call `getBandCount()` at the moment you lay the ring out, or read
 * the `window.__stBandCount` mirror if you would rather not import anything.
 * The two are always the same value — the mirror is written by the same setter.
 */

/** The three states. Anything else means "auto"; see `applyBandCount`. */
export type HudBandCount = "auto" | "one" | "two";

declare global {
  interface Window {
    /**
     * A read-only mirror of `getBandCount()`, for a reader that would rather
     * not take an import. Written by `applyBandCount` and nowhere else —
     * assigning to it does NOT change what the module returns.
     */
    __stBandCount?: HudBandCount;
  }
}

/**
 * "auto" until seeded. It is the correct thing to start on: auto is the
 * shipped default AND the behaviour every build before 1.0.89 had, so an
 * overlay whose seed fails (an offline `get_config`, a rejected invoke) draws
 * what it has always drawn instead of imposing a layout nobody chose.
 */
let _bandCount: HudBandCount = "auto";

const _subs = new Set<(v: HudBandCount) => void>();

/**
 * Set the band count from an untrusted value — a config field, an event
 * payload, a URL parameter — and return what it was normalised to.
 *
 * NORMALISATION IS THE POINT. Only the exact strings "one" and "two" mean
 * anything; `undefined` (every config written before 1.0.89), `""` (what a
 * bare `#[serde(default)]` on a String would have produced), a stray "1", a
 * number, `null` — all of it lands on "auto". The failure this prevents is
 * silent and one-directional: a value that fell through to "two" would hide
 * the specials ring for a user who never opened Settings, with nothing on
 * screen to explain it.
 */
export function applyBandCount(v: unknown): HudBandCount {
  const next: HudBandCount = v === "one" ? "one" : v === "two" ? "two" : "auto";
  const changed = next !== _bandCount;
  _bandCount = next;
  try {
    window.__stBandCount = next;
  } catch {
    /* no window (a test harness): the module value is still authoritative */
  }
  if (changed) _subs.forEach((fn) => { try { fn(next); } catch { /* a bad subscriber must not break the setter */ } });
  return next;
}

/** What the user chose. Read it when you lay the ring out, not at import. */
export function getBandCount(): HudBandCount {
  return _bandCount;
}

/**
 * Be told when it changes, for a surface that is already on screen when the
 * user flips the setting. Returns its own unsubscribe.
 *
 * Not needed for the Space ring itself — the ring only exists while Space is
 * held, and the setting can only be changed from the dashboard — but a HUD
 * that is rebuilt on resize may as well be rebuilt on this too.
 */
export function onBandCountChange(fn: (v: HudBandCount) => void): () => void {
  _subs.add(fn);
  return () => { _subs.delete(fn); };
}
