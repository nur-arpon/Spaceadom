/**
 * theme-resolve.ts — PROBLEM 255. The ONE rule that turns the config's raw
 * theme value into one of the three real palettes, shared by BOTH webviews.
 *
 * WHY THIS FILE EXISTS. The theme pill can store the literal string `"auto"`
 * ("match Windows"), and nothing resolves it away before it reaches
 * `config.json` — that is the user's choice and it has to survive a restart
 * and an OS theme flip intact. The cost is that every consumer of "which of
 * the three palettes am I wearing" has to resolve it, and there are three
 * consumers in two processes:
 *
 *   1. the dashboard  — `main.ts::resolveTheme` / `applyLook()`
 *   2. the overlay    — `overlay.ts` + `components/toast.ts::applyThemeName`
 *   3. Rust           — `config/mod.rs::resolve_theme` (for `dark_mode`)
 *
 * PROBLEM 255 shipped with the rule in `main.ts` only, and `toast.ts`'s
 * `applyThemeName` treated anything that was not `"warcry"`/`"starry"` as
 * Earthy — so an overlay handed the literal `"auto"` rendered in daylight
 * while the dashboard beside it was in Starry night. Two windows of one app
 * in two palettes at once is precisely what CLAUDE.md's "ONE setting drives
 * everything" rule exists to prevent.
 *
 * ---------------------------------------------------------------------------
 * REVIEW FIXES 2026-09-05 (H4) — THE SPLIT PALETTE, AND WHY `matchMedia` HAD
 * TO GO
 * ---------------------------------------------------------------------------
 * Sharing the RULE was not enough, because the two webviews were not being
 * asked the same QUESTION. `tauri.conf.json` pinned the `settings` window to
 * `"theme": "Light"`, which Tauri forwards to WebView2 as
 * `SetPreferredColorScheme(Light)` — so `matchMedia("(prefers-color-scheme:
 * dark)")` answered **false in the dashboard on a dark machine**, permanently
 * and by configuration, while the overlay (no pin) answered true. Same rule,
 * two answers:
 *
 *   - `"auto"` rendered Earthy in the dashboard and Starry night in the
 *     overlay, on the same machine, at the same moment;
 *   - worse, `settings-panel.ts`'s theme-pill handler resolved `"auto"`
 *     through this file and then PERSISTED `dark_mode: false` from that
 *     answer — so the dashboard's wrong reading was written into
 *     `config.json` and handed to Rust and the overlay as fact.
 *
 * `prefers-color-scheme` is therefore not a usable source here: it reports
 * *what this webview was told to render as*, which is an app decision, not
 * *what Windows wants*, which is the user's. The one process that can read
 * the user's answer without a webview standing in the way is Rust —
 * `config/mod.rs::os_prefers_dark()` reads
 * `HKCU\…\Themes\Personalize\AppsUseLightTheme` directly.
 *
 * So the OS preference now ARRIVES here rather than being read here:
 * `src/os-theme.ts` seeds it from the `get_os_prefers_dark` command and keeps
 * it current from the `os-theme-changed` event that `src-tauri/src/theme_watch.rs`
 * emits globally. One value, one process reading the registry, both webviews
 * consuming the identical bool — which is the same shape as every other
 * cross-window setting in this app (CLAUDE.md: global `emit`, seed on load,
 * because an event that only fires on CHANGE leaves a freshly-opened window
 * wrong).
 *
 * A LEAF MODULE (CLAUDE.md, PROBLEM 148), and that is UNCHANGED and still
 * load-bearing. It imports NOTHING — not `invoke`, not `listen`, not
 * `main.ts`, not a component — so both bundles and `preview.ts`'s harness can
 * import it without dragging anything behind it. The Tauri wiring lives one
 * file away in `os-theme.ts` precisely so this file can stay importable from
 * anywhere, and so the harness can drive it by calling
 * [`setOsPrefersDark`] directly with no backend at all.
 *
 * Rust's copy of the RULE cannot be shared away — it is another language in
 * another process — so it is held in step by name and by comment instead:
 * `config/mod.rs::resolve_theme` carries the identical `match`, the identical
 * "never Warcry" note, and the identical fallback. Change one, change both.
 */

/** The three real palettes. `"auto"` is never one of them — it is the
 *  instruction to pick one, and this module is what carries it out. */
export type ResolvedTheme = "earthy" | "warcry" | "starry";

/** The literal the theme pill stores when the user picks "Auto". */
export const THEME_AUTO = "auto";

/**
 * The last answer Rust gave to "does Windows want dark app surfaces?".
 *
 * `false` until told otherwise, and that default is deliberate rather than a
 * placeholder: it is the SAME fallback `config/mod.rs::resolve_theme` applies
 * to an unreadable registry value (`os_dark == None` → Earthy). A missing
 * signal has never meant anything but daylight in this app, and the two
 * halves must not disagree about the fallback — a dashboard and an overlay
 * that resolved an unanswerable question differently would show two palettes
 * at once on exactly the machines least able to say why.
 */
let _osDark = false;

/** Everyone who wants to hear about a change. See [`onSystemThemeChange`]. */
const _subscribers = new Set<() => void>();

/**
 * Record what Windows currently wants, and tell every subscriber IF IT MOVED.
 *
 * The ONLY writer in the shipping app is `os-theme.ts` (the `get_os_prefers_dark`
 * seed and the `os-theme-changed` event). `preview.ts` calls it directly,
 * which is the whole point of keeping this a leaf: the harness can flip the
 * OS preference with no backend, no registry and no `matchMedia` override,
 * and watch both consumers move together.
 *
 * **Only on a real change.** `theme_watch.rs` already de-duplicates on the
 * Rust side, but the seed and the first event can carry the same value
 * milliseconds apart, and a subscriber list that fires on every restatement
 * would run `applyLook()`'s cross-fade for a change that did not happen.
 *
 * Each callback is isolated in its own `try`: two windows' worth of listeners
 * go through here, and one throwing must not silently deprive the rest — a
 * half-themed app is harder to diagnose than a wholly wrong one.
 */
export function setOsPrefersDark(dark: boolean): void {
  if (dark === _osDark) return;
  _osDark = dark;
  // Copied before iterating: a callback is allowed to unsubscribe itself,
  // and mutating a Set mid-iteration is how one listener silently skips the
  // next one.
  for (const cb of [..._subscribers]) {
    try {
      cb();
    } catch {
      /* one window's listener must not take the others down */
    }
  }
}

/**
 * Does the OS want dark app surfaces right now?
 *
 * A plain read of the value Rust last reported. It used to call
 * `window.matchMedia("(prefers-color-scheme: dark)")`, which was wrong for
 * the reason spelled out at the top of this file: inside a Tauri window that
 * query answers "what colour scheme was this webview told to prefer", and the
 * dashboard was told "Light" by `tauri.conf.json`.
 *
 * Still a function rather than an exported `let`, so every call site keeps
 * reading the CURRENT value instead of capturing a copy at import time.
 */
export function systemPrefersDark(): boolean {
  return _osDark;
}

/**
 * **The rule.** Raw config value → one of the three real palettes.
 *
 * `"auto"` only ever resolves to Earthy or Starry night — never Warcry.
 * Warcry is a deliberate choice with no system equivalent (Windows has a
 * light/dark preference, not an iron-and-war-banners one), so there is
 * nothing for it to mean here.
 *
 * Anything unrecognised — `undefined`, `""` (a pre-PROBLEM-144 config before
 * Rust's migration runs), a value from a newer build — falls through the same
 * branch as `"auto"`. A missing or garbage value has never meant anything but
 * daylight in this app, and matching the OS is at least as good a fallback as
 * that.
 *
 * PURE, apart from the module value it reads at call time: it is called from
 * the dashboard's `applyLook()`, from the settings panel's click handler (to
 * decide `dark_mode` and which theme chord to play for the SAME choice), and
 * from the overlay's `applyThemeName`. Three callers that must never compute
 * this differently — and since H4 they can no longer be handed different
 * inputs either.
 */
export function resolveTheme(rawTheme: string | undefined): ResolvedTheme {
  if (rawTheme === "warcry" || rawTheme === "starry") return rawTheme;
  if (rawTheme === "earthy") return "earthy";
  return systemPrefersDark() ? "starry" : "earthy";
}

/**
 * Run `cb` whenever Windows' own light/dark setting changes.
 *
 * Every window that resolves `"auto"` needs this — a setting that follows the
 * OS but only notices at launch is a setting that is wrong for most of the
 * day. Both webviews register their own, and both are fed by the SAME value
 * now: `theme_watch.rs` emits `os-theme-changed` globally (CLAUDE.md's rule —
 * global `emit`, never `emit_to`, the only arrangement that has ever
 * delivered here) and `os-theme.ts` funnels it into [`setOsPrefersDark`] in
 * each window.
 *
 * That replaces two independent `matchMedia` listeners. They were not merely
 * redundant: each answered its own window's *preferred* colour scheme, so the
 * dashboard's never fired at all under the old `"theme": "Light"` pin.
 *
 * Returns a teardown function. Nothing in this app currently calls it (both
 * listeners live for the life of their window), but a listener with no way
 * off is the kind of thing a future harness discovers the hard way — and
 * `preview.ts` now genuinely uses it.
 */
export function onSystemThemeChange(cb: () => void): () => void {
  _subscribers.add(cb);
  return () => {
    _subscribers.delete(cb);
  };
}
