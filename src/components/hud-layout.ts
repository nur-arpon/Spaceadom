/**
 * hud-layout.ts — the overlay's copy of the user's `hud_magnetic_layout`
 * setting, and the one place the config's BOOL becomes a readable STRING.
 *
 * WHY A MODULE OF ITS OWN, AND WHY A LEAF. Exactly the reason
 * `hud-band-count.ts` beside it is one: `applyTheme`, `applySound` and
 * `applyFlight` all live in `toast.ts` and are called from `overlay.ts` on
 * load, and that is the pattern this follows — but `toast.ts` is being
 * rewritten in parallel with this change (the Magnetic Sector ring itself
 * lives there), and a setter that lives in the file being rewritten cannot be
 * added by the file that needs to call it. Nothing here imports from the app
 * (no `main.ts`, no `toast.ts`, no Tauri), so `preview.ts` can pull it in
 * without dragging the bootstrap along — the PROBLEM 148 rule.
 *
 * THE CONTRACT, both halves of it:
 *
 *   1. `overlay.ts` SEEDS this from `get_config` on load, and
 *   2. `overlay.ts` UPDATES it from the global `hud-layout-changed` event
 *      that `save_config` emits.
 *
 * Both are required and always have been for every setting the overlay reads:
 * an event that only fires on CHANGE leaves a freshly created overlay (first
 * launch, or a display-change rebuild) carrying this module's own default
 * until the user next touches the setting. That is the rule CLAUDE.md records
 * for the theme, and the seed is the half that gets skipped.
 *
 * READERS: call `getHudLayout()` at the moment you lay the ring out, or read
 * the `window.__stHudLayout` mirror if you would rather not import anything.
 * The two are always the same value — the mirror is written by the same setter.
 */

/**
 * The two layouts. `"magnetic"` is the new Magnetic Sector ring (1.0.89);
 * `"classic"` is the ring that shipped in 1.0.88 and every build before it.
 *
 * A STRING, even though the config field is a BOOL. That is the whole point
 * of this module: `hud_magnetic_layout === true` reads as "magnetic layout?
 * yes" at the call site and as nothing at all six months from now, and the
 * moment a third layout exists a bool has to be replaced everywhere it was
 * compared. Nobody downstream should ever compare the raw bool — call
 * `getHudLayout()` and switch on the name.
 */
export type HudLayout = "magnetic" | "classic";

declare global {
  interface Window {
    /**
     * A read-only mirror of `getHudLayout()`, for a reader that would rather
     * not take an import. Written by `applyHudLayout` and nowhere else —
     * assigning to it does NOT change what the module returns.
     */
    __stHudLayout?: HudLayout;
  }
}

/**
 * "magnetic" until seeded, and that is deliberate — it is the OPPOSITE of the
 * reasoning in `hud-band-count.ts`, which starts on the pre-1.0.89 behaviour.
 *
 * The shipped default here is ON (see `hud_magnetic_layout` in `schema.rs`:
 * the owner's explicit decision on 2026-08-27, knowingly overriding the
 * new-behaviour-defaults-off convention). An overlay whose seed fails — an
 * offline `get_config`, a rejected invoke — must therefore draw the layout
 * the config would have given it, not the one it is replacing. Starting on
 * "classic" would mean a failed seed silently hides the feature that is
 * supposed to be the default.
 */
let _layout: HudLayout = "magnetic";

const _subs = new Set<(v: HudLayout) => void>();

/**
 * Set the layout from an untrusted value — the config field, an event
 * payload, a URL parameter — and return what it was normalised to.
 *
 * NORMALISATION IS THE POINT, and it is one-sided on purpose: ONLY the exact
 * boolean `false` means `"classic"`. Everything else — `true`, `undefined`
 * (every config written before 1.0.89), `null`, `0`, `""`, `"classic"` as a
 * string, an object — lands on `"magnetic"`.
 *
 * That is the TypeScript spelling of the schema's `#[serde(default =
 * "default_true")]` and of the `!== false` rule `types.ts` states for the
 * field: absent must mean ON, because a default only ever reaches users whose
 * file predates the field. A `=== true` here would deliver the owner's flip
 * to nobody who already runs the app — the exact failure
 * `pointer_hud_activation` documents.
 *
 * Note `0` and `""` land on "magnetic" too, which is NOT what a truthiness
 * test would do. `!== false` is the rule, not `Boolean(v)`; a falsy-but-not-
 * false value means "nothing was said", and nothing-said means the default.
 */
export function applyHudLayout(v: unknown): HudLayout {
  const next: HudLayout = v === false ? "classic" : "magnetic";
  const changed = next !== _layout;
  _layout = next;
  try {
    window.__stHudLayout = next;
  } catch {
    /* no window (a test harness): the module value is still authoritative */
  }
  if (changed) _subs.forEach((fn) => { try { fn(next); } catch { /* a bad subscriber must not break the setter */ } });
  return next;
}

/** What the user chose. Read it when you lay the ring out, not at import. */
export function getHudLayout(): HudLayout {
  return _layout;
}

/**
 * Be told when it changes, for a surface that is already on screen when the
 * user flips the setting. Returns its own unsubscribe.
 *
 * Not needed for the Space ring itself — the ring only exists while Space is
 * held, and the setting can only be changed from the dashboard — but a HUD
 * that is rebuilt on resize may as well be rebuilt on this too.
 */
export function onHudLayoutChange(fn: (v: HudLayout) => void): () => void {
  _subs.add(fn);
  return () => { _subs.delete(fn); };
}
