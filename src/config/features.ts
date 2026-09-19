/**
 * PHASE A step 3 (2026-09-19, 1.0.118) — the two switches that HIDE, rather
 * than delete, the parts of Phase A the owner judged "a shortcut app, not a
 * utility" after an hour on 1.0.117. Mirrored in Rust by
 * `src-tauri/src/features.rs`; a Rust test reads THIS file with
 * `include_str!` and asserts the two literals agree, so keep each value a
 * bare `true` / `false` on the same line as its name.
 *
 *  windowsCatalogue  the "open a settings page" catalogue in the key editor
 *                    (ms-settings: / shell: rows, the search box). Off: the
 *                    Windows tab is a short "Controls" list, Advanced only.
 *                    The engine still runs a `uri` binding from an existing
 *                    config either way.
 *  hazardousToggles  screen off, sleep, Bluetooth, Wi‑Fi, dark mode, night
 *                    light in the editor. Off: never listed; screen off and
 *                    sleep are also neutralised by the engine.
 */
export const FEATURES = {
  windowsCatalogue: false,
  hazardousToggles: false,
} as const;

/** The toggles `hazardousToggles` governs, by `Action.what`. */
export const HAZARDOUS_TOGGLE_IDS: ReadonlySet<string> = new Set([
  "screen_off", "sleep", "bluetooth", "wifi", "dark_mode", "night_light",
]);
