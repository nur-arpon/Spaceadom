/// config/schema.rs — Canonical data structures for config.json

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Root configuration file schema (version 1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Schema version for future migration support.
    pub version: u32,

    /// Name of the currently active profile.
    pub active_profile: String,

    /// Adaptive rollover window in milliseconds (default: 0 = disabled).
    /// If > 0: an alpha key hit within this window after Space↓ is treated as normal typing.
    /// 0 = always-modifier (V11 behavior): Space is always a modifier, never types a space directly.
    ///
    /// PROBLEM 69: users do not think in milliseconds. This is now DERIVED from
    /// `typing_wpm` by the Settings UI — it stays the value the hook reads, but
    /// the human-facing control is typing speed. Editing it directly still works.
    pub rollover_ms: u64,

    /// The user's typing speed in words per minute, which drives `rollover_ms`
    /// (PROBLEM 69).
    ///
    /// THIS COMMENT USED TO SAY "a FASTER typist needs a WIDER window". That is
    /// backwards and contradicted the mapping directly below it. The hook
    /// measures the delay from Space-down to the next key going down, which IS
    /// the typist's inter-key interval (`12000 / wpm`) and gets SHORTER as
    /// speed rises — so a faster typist needs a NARROWER window, and a slower
    /// one needs a wider window to keep their leisurely spacing on the "typing"
    /// side of the threshold. See `rollover_ms_for_wpm`.
    ///
    /// Defaults to `DEFAULT_TYPING_WPM`. `#[serde(default)]` so a config
    /// written by =<1.0.5 still loads; that config's existing `rollover_ms` is
    /// left alone unless it is below `MIN_ROLLOVER_MS`, which is unsafe at any
    /// speed and gets recomputed on load (PROBLEM 95).
    #[serde(default = "default_typing_wpm")]
    pub typing_wpm: u32,

    /// PROBLEM 76 — true once the tray icon has been promoted out of the
    /// Windows 11 overflow flyout. Promotion happens ONCE; after that the
    /// user's own arrangement of their taskbar corner is never overridden.
    /// Not shown in Settings.
    #[serde(default)]
    pub tray_promoted: bool,

    /// Milliseconds Space must be held before the Guide HUD appears (default: 300).
    pub guide_hud_delay_ms: u64,

    /// PROBLEM 80 — how the overlay webview is composited.
    /// "auto"     (default): GPU compositing, plus a runtime self-test that
    ///            detects the driver pathology where the transparent overlay
    ///            composes ZERO pixels (Rust reports visible=true, JS runs,
    ///            sound plays, screen shows nothing — observed live on the
    ///            owner's laptop 2026-08-12 while the same build painted fine
    ///            on a friend's).
    /// "software": WebView2 is launched with --disable-gpu. Set automatically
    ///            by the self-test after 3 consecutive dead-pixel verdicts;
    ///            never switched back automatically.
    #[serde(default = "default_overlay_compositing")]
    pub overlay_compositing: String,

    /// Minimum window opacity enforced by the scroll-wheel opacity modifier (0–100 %).
    pub opacity_floor_pct: u8,

    /// Absolute path to the preferred browser executable. `null` on first run.
    pub browser_path: Option<String>,

    /// Process names that are never treated as exclusive-fullscreen for hook suppression.
    pub fullscreen_allowlist: Vec<String>,

    /// All user-defined shortcut profiles.
    pub profiles: Vec<Profile>,

    /// Special-key bindings active when Space is held.
    /// Keys: "esc", "enter", "tab", "f1"–"f12", "up", "down", "left", "right".
    /// Each maps to a KeyBinding action identical to alpha bindings.
    /// Empty by default (user opts in via Settings).
    #[serde(default)]
    pub special_keys: HashMap<String, KeyBinding>,

    /// Whether Nocturne (dark) mode is enabled. Drives body.nocturne on both
    /// the dashboard and overlay windows. Defaults to false (Earthy/light).
    #[serde(default)]
    pub dark_mode: bool,

    /// Whether the optional WebAudio sine-tick sound effects are enabled.
    /// Sent to the overlay via the "sound-changed" event. Defaults to false.
    #[serde(default)]
    pub sound_enabled: bool,

    /// Whether the Spaceadom logon task is enabled (run at startup).
    /// ON by default — this is a keyboard utility; starting with Windows is
    /// its expected behaviour, and the Settings toggle is the opt-out.
    /// Config is the source of truth; startup::apply_task_enabled() applies
    /// it to the Scheduled Task (schtasks' status text is localized, so it is
    /// never parsed back).
    #[serde(default = "default_true")]
    pub run_at_startup: bool,

    /// Visual-effects level: "auto" (follow the OS reduced-motion signal),
    /// "full" (all effects even if the OS asks for less), or "reduced".
    /// "auto" honours accessibility by default while giving testers on
    /// effects-off machines a way to see the app as designed (PROBLEM 47).
    #[serde(default = "default_motion")]
    pub motion: String,
}

/// PROBLEM 105 — the profile every OTHER profile silently falls back to.
///
/// `handle_alpha` looks a key up in the active profile and, when it is
/// unassigned there, uses this profile's binding instead. That makes it
/// structurally different from the other stock profiles, and nothing in the UI
/// said so: the user deleted it to see what would happen and every unassigned
/// key in every other profile stopped working, with no warning at the moment
/// of deletion.
///
/// Named here so the engine and the delete path agree on one spelling.
pub const FALLBACK_PROFILE: &str = "Founders";

fn default_true() -> bool { true }
fn default_motion() -> String { "full".into() }
/// PROBLEM 95 — the default is now chosen for SAFETY ACROSS UNKNOWN TYPISTS,
/// not to reproduce the pre-slider build.
///
/// The old default (70 wpm → 120 ms) was picked to match the window this app
/// shipped with before the slider existed. Measured 2026-08-13, that window is
/// narrower than a 70 wpm typist's own key spacing (171 ms), so a 180 ms
/// spacebar hold turned 18 of 18 words into commands. It is fine for a light
/// thumb and catastrophic for a heavy one, and a fresh install cannot know
/// which it has.
///
/// 60 wpm → 280 ms is deliberately conservative: it clears the inter-key
/// interval of anyone typing faster than ~43 wpm, and slower typists release
/// Space long before the next key so they never reach the comparison at all.
pub const DEFAULT_TYPING_WPM: u32 = 60;
/// The window a fresh install gets. Must equal rollover_ms_for_wpm(DEFAULT).
pub const DEFAULT_ROLLOVER_MS: u64 = 280;
/// No setting may ever produce a window below this. See PROBLEM 72 and 95.
/// 200 ms clears the inter-key interval of a 60 wpm typist (200 ms) — the
/// slowest speed someone who has selected "Very fast" might plausibly type at.
pub const MIN_ROLLOVER_MS: u64 = 200;
/// Capped at the DEFAULT Guide-HUD delay (300ms) on purpose: if the window
/// exceeded it, the HUD would appear announcing command mode while the next
/// key was still being typed. At 30 wpm the inter-key gap (400ms) is wider
/// than this cap, but a typist that slow releases Space long before the next
/// key, so the comparison is never reached - measured 0/18 at every hold.
pub const MAX_ROLLOVER_MS: u64 = 300;

fn default_typing_wpm() -> u32 { DEFAULT_TYPING_WPM }
fn default_overlay_compositing() -> String { "auto".into() }

/// PROBLEM 72 — words-per-minute → rollover window, in milliseconds.
///
/// THE FIRST VERSION OF THIS MAPPING WAS BACKWARDS AND SHIPPED. It read
/// `wpm * 1.4 + 20`, i.e. the window GREW with speed, so selecting "Slow"
/// produced a 62 ms window and the app fired commands during ordinary typing.
/// A tester hit exactly that and reported *more* accidental launches, not
/// fewer. The reasoning error: I modelled "fast typists overlap keys more" and
/// ignored the only quantity the hook actually measures.
///
/// What the hook measures is the delay from Space-DOWN to the next letter
/// going down. Under the window → typing; over it → a deliberate Space+key
/// command. That delay tracks the typist's inter-key interval, which is
/// `12000 / wpm` ms and therefore gets SHORTER as speed rises:
///
/// ```text
///   40 wpm -> ~300 ms between keys   needs a WIDE window
///   70 wpm -> ~170 ms
///  120 wpm -> ~100 ms                a narrow window is safe
/// ```
///
/// PROBLEM 95 — the SECOND version of this mapping was also wrong, in a
/// subtler way, and this is the fix. It read `8400 / wpm`, anchored so 70 wpm
/// reproduced the pre-slider 120 ms window. But the comment above already
/// derives the quantity that matters — the inter-key interval is `12000 / wpm`
/// — and 8400/wpm is only **0.7x** of it. The window therefore sat BELOW the
/// typist's own spacing at every single setting, so the "under the window →
/// typing" branch could not fire from the interval alone. The only thing
/// preventing false launches was the user releasing Space before the next key.
///
/// MEASURED 2026-08-13 by injecting real prose (both harness controls passing):
/// at 70 wpm / 120 ms, holding Space 180 ms turned **18 of 18** words into
/// commands. Not an occasional misfire — every word, because the condition is
/// structural, not probabilistic.
///
/// ```text
///   wpm   interval   OLD 8400/wpm      NEW 16800/wpm
///    40     300 ms     210  (< gap!)     300  (> gap)
///    70     171 ms     120  (< gap!)     240  (> gap)
///   120     100 ms     110  (~= gap)     200  (> gap)
/// ```
///
/// The window must EXCEED the interval, with margin, or it is not a window at
/// all. 1.4x is that margin: comfortably clear of ordinary typing while still
/// well under the 300 ms Guide-HUD delay, so a deliberate hold feels the same.
///
/// Clamped to 200..=320 ms. The FLOOR is the safety-critical half — it is what
/// keeps a user who selects "Very fast" but actually types at 60 wpm from
/// having every word fire a command.
///
/// Kept in Rust so the mapping has ONE definition; `settings-panel.ts` mirrors
/// it and the two must be changed together.
pub fn rollover_ms_for_wpm(wpm: u32) -> u64 {
    let wpm = wpm.max(1) as f64;
    let raw = 16800.0 / wpm;
    raw.round().clamp(MIN_ROLLOVER_MS as f64, MAX_ROLLOVER_MS as f64) as u64
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            version: 1,
            active_profile: "Founders".into(),
            // The pre-slider default, exactly: 70 wpm → 120 ms.
            rollover_ms: DEFAULT_ROLLOVER_MS,
            typing_wpm: DEFAULT_TYPING_WPM,
            tray_promoted: false,
            guide_hud_delay_ms: 300,
            overlay_compositing: default_overlay_compositing(),
            opacity_floor_pct: 25,
            browser_path: None,
            fullscreen_allowlist: vec![
                "vlc.exe".into(),
                "mpv.exe".into(),
            ],
            profiles: Vec::new(),
            special_keys: HashMap::new(),
            dark_mode: false,
            sound_enabled: false,
            run_at_startup: true,
            motion: default_motion(),
        }
    }
}

/// A named shortcut profile containing per-key bindings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Unique alphanumeric profile identifier (1–24 chars, `[a-zA-Z0-9_]`).
    pub name: String,

    /// Map of lowercase key character → binding.
    /// Keys present in V11: a–z.
    pub bindings: HashMap<String, KeyBinding>,
}

/// A single key's action binding.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KeyBinding {
    /// Executable file name or absolute path (e.g. `"brave.exe"` or full path).
    /// `null` if not mapped to an app.
    pub app: Option<String>,

    /// URL to open in the preferred browser. `null` if not a web target.
    pub web_url: Option<String>,

    /// Human-readable display label shown in the key matrix.
    pub label: Option<String>,

    /// Absolute path to a custom icon override (base64 PNG).
    /// `null` = extract from `app` automatically.
    pub icon_override: Option<String>,
}

impl KeyBinding {
    /// Returns true if this binding has any action defined.
    pub fn is_mapped(&self) -> bool {
        self.app.is_some() || self.web_url.is_some()
    }
}

/// Hook engine status snapshot (sent to frontend on request).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookStatus {
    pub installed: bool,
    pub bypass_active: bool,
    pub fullscreen_suppressed: bool,
    pub active_profile: String,
}

/// Result of a Windows OS hotkey conflict check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictResult {
    pub has_conflict: bool,
    pub conflicting_combo: Option<String>,
    pub description: Option<String>,
}
