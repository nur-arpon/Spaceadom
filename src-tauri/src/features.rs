//! PHASE A step 3 (2026-09-19, 1.0.118) — the two compile-time switches that
//! HIDE, rather than delete, the parts of Phase A the owner judged "a
//! shortcut app, not a utility" after an hour on 1.0.117.
//!
//! Everything behind them stays compiled, tested and reachable by flipping
//! the constant. The frontend has the same two switches in
//! `src/config/features.ts` (`FEATURES.windowsCatalogue`,
//! `FEATURES.hazardousToggles`) and `the_ts_switches_match_the_rust_ones`
//! reads that file with `include_str!` and refuses to let the two drift.
//!
//! * `WINDOWS_CATALOGUE` — the "open a settings page" catalogue
//!   (`ms-settings:` / `shell:` rows, the search box). Off: the editor's
//!   Windows tab becomes a short "Controls" list. The ENGINE still executes
//!   an `Action::Uri` from an existing config either way — a binding made in
//!   1.0.116/117 keeps working; only the UI hides the rows.
//! * `HAZARDOUS_TOGGLES` — `screen_off`, `sleep`, `bluetooth`, `wifi`,
//!   `dark_mode`, `night_light`. Off: none of them appears in the editor, and
//!   `screen_off` / `sleep` are NEUTRALISED at run time as well
//!   (`engine::actions::toggle::neutralised`): the owner restarted his laptop
//!   to escape Screen off on 1.0.117 (every Space+U woke the panel and turned
//!   it off again, six times in 37 s). The other four still execute if
//!   already bound — slow, not dangerous.

/// The "open a settings page" catalogue in the key editor.
pub const WINDOWS_CATALOGUE: bool = false;

/// Screen off, sleep, Bluetooth, Wi‑Fi, dark mode, night light in the editor
/// — and the first two at run time.
pub const HAZARDOUS_TOGGLES: bool = false;

/// The toggles `HAZARDOUS_TOGGLES` governs, by `Action::Toggle::what`.
pub const HAZARDOUS_TOGGLE_IDS: &[&str] =
    &["screen_off", "sleep", "bluetooth", "wifi", "dark_mode", "night_light"];

/// The two that are neutralised at run time when the switch is off — the
/// ones that leave the machine in a state the user has to recover from.
pub const NEUTRALISED_TOGGLE_IDS: &[&str] = &["screen_off", "sleep"];

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontend's `src/config/features.ts` must say what this file says
    /// — the literal `windowsCatalogue: <bool>` / `hazardousToggles: <bool>`
    /// is read from the TS source, so flipping one side without the other
    /// fails the build's tests rather than shipping a UI that hides what the
    /// engine shows or vice versa.
    #[test]
    fn the_ts_switches_match_the_rust_ones() {
        let ts = include_str!("../../src/config/features.ts");
        let flag = |name: &str| -> bool {
            let needle = format!("{name}:");
            let at = ts.find(&needle).unwrap_or_else(|| panic!("features.ts has no `{name}:`"));
            let rest = ts[at + needle.len()..].trim_start();
            if rest.starts_with("true") {
                true
            } else if rest.starts_with("false") {
                false
            } else {
                panic!("features.ts: `{name}` is not a bare true/false literal")
            }
        };
        assert_eq!(flag("windowsCatalogue"), WINDOWS_CATALOGUE, "windowsCatalogue drifted");
        assert_eq!(flag("hazardousToggles"), HAZARDOUS_TOGGLES, "hazardousToggles drifted");
        assert!(ts.contains("as const"), "the TS object must be `as const` so the literals are readable");
    }

    #[test]
    fn the_neutralised_set_is_inside_the_hazardous_set() {
        for id in NEUTRALISED_TOGGLE_IDS {
            assert!(HAZARDOUS_TOGGLE_IDS.contains(id), "{id}");
            assert!(crate::engine::actions::toggle::TOGGLE_IDS.contains(id), "{id} is a real toggle");
        }
        for id in HAZARDOUS_TOGGLE_IDS {
            assert!(crate::engine::actions::toggle::TOGGLE_IDS.contains(id), "{id} is a real toggle");
        }
    }
}
