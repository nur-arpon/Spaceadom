/// config/schema.rs — Canonical data structures for config.json

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Every key→binding map in `config.json`, in ONE named type.
///
/// **It is a `BTreeMap`, and the ordering is the whole point (PROBLEM 250
/// follow-up — LIVE TEST 2026-09-05).** It was a `HashMap`, whose iteration
/// order is randomised per process by design, so `serde_json` wrote the keys
/// in a different order on every save. Measured on 2026-09-05, comparing the
/// config before the MSIX test with the config after it:
///
/// ```text
///   77,912 bytes  sha256 9ADE0FDE…   (before)
///   77,912 bytes  sha256 4CE86B44…   (after)
///   full JSON diff: NO semantic difference — every key and value identical
///   first differing byte: 491 — "z" first vs "h" first inside a bindings map
/// ```
///
/// Two things that costs, and the second is the expensive one:
///
///  1. **A config hash is not a change detector.** Anything comparing two
///     saves — a backup deduplicator, a "did the user change anything?" check,
///     a diff in a bug report — sees every save as a change. During the MSIX
///     test this cost a real diagnostic step: proving the packaged copy had
///     changed nothing required a full semantic JSON diff, because the hashes
///     said otherwise.
///  2. **Every save rewrites the whole file's byte layout.** The rolling
///     backups (PROBLEM 94) therefore differ from each other for no reason,
///     and a user's `config.json` never settles.
///
/// `BTreeMap` fixes it by construction rather than by remembering to sort at
/// each serialisation site: the keys are single lowercase characters and
/// special-key names, so lexicographic order is also the order a human would
/// expect to read them in. `serde_json`'s `preserve_order` (IndexMap) was the
/// alternative and is worse here — it is a Cargo feature that would change how
/// EVERY map in the dependency tree serialises to preserve *insertion* order,
/// which for a map rebuilt from disk on every load is not a stable order at
/// all, only a different unstable one. And IndexMap is not already a
/// dependency of this crate.
///
/// The API is a strict superset of what this codebase used (`new`, `insert`,
/// `get`, `remove`, `clear`, `len`, `is_empty`, `values`, `iter`, indexing) —
/// checked by grep across `src-tauri/src` before the switch, not assumed.
/// `String` is `Ord`, so nothing else was needed.
///
/// Generalise: **if two runs that did the same thing must produce the same
/// bytes, that has to be a property of the type, not a habit of the caller.**
pub type BindingMap = BTreeMap<String, KeyBinding>;

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
    ///
    /// SUPERSEDED by `tray_promoted_for` (PROBLEM 142) and kept only so old
    /// configs still deserialise. Do not read it: as a bare bool it could not
    /// express WHICH icon was promoted, which is the whole bug below.
    #[serde(default)]
    pub tray_promoted: bool,

    /// PROBLEM 142 — the exe path the tray icon was last promoted FOR.
    ///
    /// Windows 11 keys icon visibility to the EXECUTABLE PATH. The bare
    /// `tray_promoted` bool above latched true on 2026-08-12 for
    /// `{6D809377-…ProgramFiles…}\Spaceadom\spaceadom.exe`, and then PROBLEM
    /// 129 moved the install to `%LOCALAPPDATA%\Spaceadom` in 1.0.41. To
    /// Windows that is a DIFFERENT icon, freshly hidden — but the latch said
    /// "already done", so it was never promoted again and the owner had to
    /// click the chevron every time.
    ///
    /// Storing the PATH keeps both properties: promotion happens once per
    /// install location, and a user who later drags the icon back into the
    /// overflow is never overridden, because the path has not changed.
    #[serde(default)]
    pub tray_promoted_for: String,

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

    /// Apps Spaceadom stands down inside — the owner's "exception list".
    ///
    /// Stored as LOWERCASE EXE STEMS ("photoshop", never
    /// "C:\...\Photoshop.exe"), the same normalisation the conflicts and
    /// fullscreen-allowlist code use. While one of these is the foreground
    /// window the hook passes EVERYTHING through, so hold-Space canvas panning
    /// in Photoshop/Figma/Blender works exactly as it does with Spaceadom
    /// closed.
    ///
    /// `#[serde(default)]` is load-bearing: every config written before
    /// 1.0.79 lacks the field, and without it they all fail to deserialise.
    ///
    /// PROBLEM 267 — entries carry a SCOPE now (`AppException`): off
    /// entirely, Space only, or middle button only. A 1.0.79–1.0.109 file
    /// stores plain strings; `AppException`'s own `Deserialize` reads them as
    /// `OffEntirely`, which is the meaning they always had, so no migration
    /// pass and no `Vec<String>` twin field are needed.
    #[serde(default)]
    pub excluded_apps: Vec<AppException>,

    /// All user-defined shortcut profiles.
    pub profiles: Vec<Profile>,

    /// Special-key bindings active when Space is held.
    /// Keys: "esc", "enter", "tab", "f1"–"f12", "up", "down", "left", "right".
    /// Each maps to a KeyBinding action identical to alpha bindings.
    /// Empty by default (user opts in via Settings).
    #[serde(default)]
    pub special_keys: BindingMap,

    /// PHASE A — Advanced mode: shows "Run command" and the full catalogue in
    /// the key editor. UI only; the engine ignores it (a `Command` binding
    /// that is already in the file runs whether or not this is on).
    #[serde(default)]
    pub advanced_mode: bool,

    /// 1.0.119 (brief 4 §4) — has the key editor's "Key combo" hint been
    /// shown once? Same shape as `tour_done`: written `true` the first time
    /// the Key combo page opens; afterwards the hint collapses to a "How
    /// does this work?" link. Absent (every earlier config) = not yet seen.
    #[serde(default)]
    pub key_combo_hint_seen: bool,

    /// Whether Nocturne (dark) mode is enabled. Drives body.nocturne on both
    /// the dashboard and overlay windows. Defaults to false (Earthy/light).
    #[serde(default)]
    pub dark_mode: bool,

    /// PROBLEM 144 — which of the four looks the app wears:
    /// `"auto"` (follows Windows' own light/dark setting — feature 2, added
    /// 2026-09-05), `"earthy"` (daylight), `"warcry"` (iron and war-banners)
    /// or `"starry"` (night sky). Replaces the old two-state dark toggle.
    ///
    /// `"auto"` IS STORED LITERALLY, ON PURPOSE. Rust never resolves it —
    /// there is no reliable, dependency-free way for THIS process to read
    /// Windows' AppsUseLightTheme signal that is worth adding for one string,
    /// and the frontend already has to own the resolution for a LIVE
    /// `matchMedia` listener regardless (an OS theme flip while the app is
    /// running has no Rust-side event to hang a resave on). So this field can
    /// contain a value none of `dark_mode`, the overlay, or any Rust code
    /// understands as a real palette; `resolveTheme()` in `main.ts` is the
    /// one place "auto" becomes "earthy" or "starry", and it is a TypeScript
    /// file specifically because only the frontend has a theme media query to
    /// ask. See that function's doc comment for the corollary this leaves
    /// open for the OVERLAY window specifically.
    ///
    /// `dark_mode` above is KEPT and kept in sync, because it is what drives
    /// `body.nocturne` on BOTH windows and the overlay has no idea themes
    /// exist. Earthy is light; warcry and starry are both nocturne underneath,
    /// so every rule that already works in the dark keeps working and each
    /// theme only re-tints on top. Migrated from `dark_mode` on first load, so
    /// an existing config keeps the look it had.
    /// Serde default is the EMPTY string, deliberately, not "earthy" (and
    /// UNCHANGED by feature 2 — this is the per-field default `serde` applies
    /// when an OLD config on disk lacks the key entirely, never what a NEW
    /// config is created with; see `impl Default for AppConfig` below for
    /// that): an absent key must be distinguishable from a deliberate choice,
    /// or the migration in `config/mod.rs` cannot tell an upgrading
    /// dark-mode user from a new install and would silently flip them into
    /// daylight.
    #[serde(default)]
    pub theme: String,

    /// PROBLEM 144 — the personality layer: character toggles, convoy
    /// staggers, flames, themed sounds and (later) the live sky.
    ///
    /// The owner's rule for the night theme, in his words: with fun ON the
    /// Starry night is the new sky; with it OFF, Starry night is "the previous
    /// dark mode we have been using all along" — i.e. plain nocturne, no star
    /// field. So this gates the starry PALETTE, not the theme choice itself.
    /// OFF at first install since 2026-08-20 (the owner's decision): a new
    /// user meets plain, quiet controls first and opts INTO the personality.
    /// Frontend readers must therefore treat a missing value as false
    /// (`=== true`, never `!== false`).
    #[serde(default)]
    pub fun_mode: bool,

    /// PROBLEM 144 — hide the keyboard board and the whole dashboard chrome,
    /// leaving only the sky. Esc or the corner control brings it back.
    #[serde(default)]
    pub hide_keyboard: bool,

    /// PROBLEM 144 — "Show me around": when true every setting's description
    /// is open.
    /// OFF at first install since 2026-08-20, same decision as fun_mode —
    /// the descriptions are one label-press away rather than pre-opened.
    #[serde(default)]
    pub show_me_around: bool,

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

    /// PROBLEM 237 — "Warm up the app picker at startup": warm up the app
    /// picker after startup so the first open is instant — costs one
    /// background scan per boot.
    ///
    /// When true, a few seconds after the windows exist the picker worker
    /// validates the on-disk app-list cache and refreshes it in the
    /// background (PowerShell + icons, 6-17 s measured, on a below-normal-
    /// priority STA thread — never the main thread, never before the hook,
    /// the engine or the tray). When false, nothing runs until the first
    /// picker open, which is served from the disk cache when it is fresh and
    /// scans off the main thread when it is not — the window stays alive
    /// either way; only the first open of a session may show "Scanning…".
    ///
    /// `default = "default_true"`, NOT a bare `#[serde(default)]`: every
    /// config on disk predates this key, and the owner asked for the picker
    /// to "start working as fast as Raycast" — ON is the behaviour that
    /// delivers that, so the upgrade path must land there. `Default` agrees;
    /// `first_install_tests` holds both paths to it.
    #[serde(default = "default_true")]
    pub warm_picker_at_startup: bool,

    /// Visual-effects level: "auto" (follow the OS reduced-motion signal),
    /// "full" (all effects even if the OS asks for less), or "reduced".
    /// "auto" honours accessibility by default while giving testers on
    /// effects-off machines a way to see the app as designed (PROBLEM 47).
    #[serde(default = "default_motion")]
    pub motion: String,

    /// PROBLEM 174 — "Guide-to-toast motion". OFF by default.
    ///
    /// When a shortcut fires while the Space ring is up, the message pill can
    /// either be slung out of the ring on a 940ms arc (on) or simply appear
    /// bottom-centre while the ring collapses on its own (off). Off is exactly
    /// 1.0.27's behaviour.
    ///
    /// WHY IT IS A SETTING, AND WHY OFF. The owner, 2026-08-24, relaying his
    /// testers: *"some people gave me feedback that they found it disturbing,
    /// too much time consuming and doesn't add much to the functionality… just
    /// have an on off switch for this specific space hud to toast, off by
    /// default."* This is also the third time the flight has been switched off
    /// and back on (off for 1.0.33, back for 1.0.51), so making it the user's
    /// choice retires the argument.
    ///
    /// It is not only taste. The flight brought a state machine with it —
    /// `_stageMode`, `_slingStaged`, `_slingHeld`, `_hudBusy` — and the owner's
    /// 2026-08-24 log caught `hudBusy=true` latched across three minutes and
    /// many keypresses, which blocks every `overlay_fit` and therefore every
    /// toast and every HUD placement. The latch is fixed separately (PROBLEM
    /// 175); with this off, none of that machinery runs at all.
    ///
    /// `#[serde(default)]` = false, so an existing config upgrades into the
    /// quiet behaviour without a migration. Deliberate: the default the owner
    /// asked for is the one everybody should land on, whichever build they
    /// came from.
    #[serde(default)]
    pub hud_toast_flight: bool,

    /// PROBLEM 206 — pointer activation on the Guide HUD. **ON by default
    /// since PROBLEM 209.**
    ///
    /// While Space is held and the ring is up, POINTING the cursor in a
    /// binding's direction ARMS it (the chip lights up); releasing Space — or
    /// left-clicking — launches that binding. Both gestures live in the
    /// mouse hook plus the `st-hud-pointer` poller; this flag is mirrored
    /// into `hook::POINTER_HUD_ACTIVATION` by
    /// `publish_pointer_hud_activation`, which MUST be called from BOTH the
    /// startup load (lib.rs) and `config::save` (PROBLEM 180's rule).
    ///
    /// **THE DEFAULT WAS FLIPPED ON 2026-08-27 BY THE OWNER'S EXPLICIT
    /// DECISION, KNOWINGLY OVERRIDING THIS CODEBASE'S OWN CONVENTION.** The
    /// convention — spelled out on `hud_toast_flight` directly above — is
    /// that brand-new behaviour ships OFF so nobody is surprised by it. He
    /// asked for this one ON anyway, in the same conversation that replaced
    /// containment with directional sectors: he wants the feature met, not
    /// found. It is his app and his call. **Do not "restore the convention"
    /// here** — that is not a bug being fixed, it is a decision being
    /// reversed, and it needs him, not a tidy-up.
    ///
    /// So: `default = "default_true"`, NOT a bare `#[serde(default)]` — a
    /// bool's `Default` is `false`, so the bare attribute would read OFF for
    /// every config already on disk and the flip would reach nobody who
    /// already runs the app. Frontend readers use `!== false`, NEVER
    /// `=== true`; same shape as `send_logs` below, and the OPPOSITE of
    /// `hud_toast_flight` above. **Both conventions live in this one struct
    /// on purpose** — see `hud_show_specials`, which is the other one — so
    /// check what a field's default IS before copying a neighbour's read.
    /// `first_install_tests` checks BOTH paths, because `Default` governs a
    /// fresh install and the serde attr governs an old file, and nothing
    /// forces them to agree except that test.
    #[serde(default = "default_true")]
    pub pointer_hud_activation: bool,

    /// PROBLEM 263 — HOLD THE MIDDLE MOUSE BUTTON TO RAISE THE RING. **ON by
    /// default, by the owner's decision on 2026-09-08.**
    ///
    /// The ring now has three triggers: the keyboard hook's Space, the
    /// own-window fallback's Space (PROBLEM 259) and this. Holding the middle
    /// button raises the same ring in the same place; releasing over a chip
    /// launches it, a letter tapped during the hold launches that binding, and
    /// a QUICK middle click is replayed through `SendInput` so browsers still
    /// open links in a new tab and close tabs. The arbitration that stops two
    /// triggers ever serving one gesture is stated once, in `hook/mod.rs` beside
    /// `MIDDLE_TAP_MS`.
    ///
    /// Mirrored into `hook::MIDDLE_BUTTON_RING` by
    /// `publish_middle_button_ring`, which MUST be called from BOTH the startup
    /// load (lib.rs) and `config::save` — PROBLEM 180's rule, sixth instance.
    ///
    /// **THIS IS NOT THE ONLY GATE, AND THE OTHER ONE IS NOT A SETTING.**
    /// `hook/orbit_apps.rs` holds a BUILT-IN list of 3D, CAD and design
    /// programs — SolidWorks, Fusion 360, Blender, AutoCAD, Photoshop, Figma
    /// and the rest — where middle-drag already orbits a model or pans a
    /// canvas. Inside those the middle button is handed straight back to
    /// Windows however this flag reads. That list is SEPARATE from
    /// `excluded_apps` and applies to the middle button ONLY: a user's Space
    /// shortcuts inside SolidWorks are untouched by it.
    ///
    /// `default = "default_true"`, NOT a bare `#[serde(default)]`, and the
    /// frontend reads `!== false` — same shape and same reason as
    /// `pointer_hud_activation` above: the owner asked for this ON, and a
    /// bool's `Default` is `false`, so the bare attribute would deliver the
    /// feature to nobody who already runs the app. It is the second field in
    /// this struct to ship a NEW behaviour ON, knowingly overriding the
    /// new-behaviour-defaults-off convention `hud_toast_flight` follows. Do not
    /// "restore the convention" — that is a decision being reversed, and it
    /// needs the owner. `first_install_tests` checks BOTH paths.
    #[serde(default = "default_true")]
    pub middle_button_ring: bool,

    /// PROBLEM 267 — WHAT the middle button raises: the new cursor-anchored
    /// icon ring (`IconRing`, default) or phase 1's centred Guide HUD
    /// (`GuideHud`, byte-for-byte 1.0.109). The owner's decision, 2026-09-13:
    /// a choice, not a replacement. See `MiddleRingStyle`. Only meaningful
    /// while `middle_button_ring` is on; the Settings pill is inert otherwise.
    ///
    /// `#[serde(default)]` = `IconRing` (the enum's `Default`), which is the
    /// upgrade path every existing config travels; `Default` below agrees and
    /// `first_install_tests` holds both to it.
    #[serde(default)]
    pub middle_ring_style: MiddleRingStyle,

    /// PROBLEM 267 — how much the icon ring shows: the eight favourites
    /// (`MyEight`, default) or everything bound plus the specials on a second
    /// ring (`All`). See `MiddleRingScope`. Both paths default to `MyEight`.
    #[serde(default)]
    pub middle_ring_scope: MiddleRingScope,

    /// 2026-09-15 — HOW the `All` scope is arranged: the concentric
    /// Fibonacci `Rings` (default, the behaviour every existing install
    /// already has) or the phyllotaxis `Spiral`. Owner's addition, and a
    /// TOGGLE rather than a replacement. Meaningless for `MyEight`, and the
    /// Settings pill only appears under `All` for that reason.
    ///
    /// `#[serde(default)]` = `Rings`, so nothing changes for a config that
    /// predates the field — which is every config on disk.
    #[serde(default)]
    pub all_ring_layout: AllRingLayout,

    /// PROBLEM 267 — the user's chosen favourites for the icon ring, as bound
    /// LETTERS (`"m"`, `"b"`, …), in ring order, at most
    /// `middle_ring::FAVOURITES_MAX` (15 since round 3; older eight-long lists
    /// carry over). EMPTY means "not chosen": `middle_ring::favourites_for`
    /// then uses the first six bound letters of the active profile, computed at ring time
    /// and never written back — so a user who never opens the picker is never
    /// pinned to a snapshot of a profile they keep editing. Letters that are no
    /// longer bound are skipped at ring time, not deleted here.
    #[serde(default)]
    pub middle_ring_favourites: Vec<String>,

    /// Show the SPECIAL keys on the Space HUD's inner ring? ON by default.
    ///
    /// Display only. The eight specials (Esc, the backtick PiP, the Boss Key
    /// and friends) keep working exactly as they always have whether this is
    /// on or off — the gate is in `engine/mod.rs`, where an empty `specials`
    /// vec is sent in the `GuideHudPayload` instead of the eight entries, and
    /// the overlay page simply renders what it is given. Nothing in the hook
    /// or the engine's key HANDLING consults this.
    ///
    /// **NO HOOK ATOMIC, DELIBERATELY.** Every other setting the HUD path
    /// touches has one because the hook callback needs it and may not read
    /// config (PROBLEM 58/134/184). This one is read on the Space-HOLD path,
    /// inside a config read that already happens, on the engine thread — an
    /// atomic would be a second source of truth for no gain and one more
    /// thing to forget to publish (PROBLEM 180's failure mode). Do not add
    /// one.
    ///
    /// `default = "default_true"`, and the frontend reads `!== false`: this
    /// is EXISTING behaviour becoming optional, not new behaviour arriving,
    /// so a config that predates the field must keep showing the specials it
    /// has always shown. **That is the opposite convention from
    /// `hud_toast_flight` above** — and `pointer_hud_activation` directly
    /// above is a third case again, a new behaviour the owner chose to ship
    /// ON. Three fields, two conventions, one struct; the deciding question
    /// is always "what did this config do YESTERDAY?", never "what does the
    /// field next to it do?".
    #[serde(default = "default_true")]
    pub hud_show_specials: bool,

    /// How many RINGS of app shortcuts the Space HUD lays out:
    /// `"auto"` (default), `"one"` or `"two"`.
    ///
    /// **A STRING ENUM, not a bool, and modelled on `motion` above** — same
    /// three-valued shape (`"auto" | "full" | "reduced"`), same
    /// `#[serde(default = ...)]` returning a `String`, same "an unknown value
    /// means auto" tolerance on the reading side. It is deliberately NOT
    /// `theme`'s shape: `theme`'s serde default is the empty string so a
    /// migration can tell "never set" from "set to earthy", and there is no
    /// migration here — an absent key simply means auto.
    ///
    /// **THIS FIELD AND `hud_show_specials` ARE ONE SYSTEM.** The specials
    /// occupy the HUD's INNER band, so they can only exist when the apps need
    /// just the outer one:
    ///
    /// ```text
    ///   rows   specials   result
    ///   one    on         specials inner ring + apps outer ring
    ///   one    off        one app band, no inner ring
    ///   two    on/off     two app bands, specials NOT rendered
    ///   auto   on         specials shown IF the apps fit one ring
    ///   auto   off        band count by arithmetic, no inner ring
    /// ```
    ///
    /// Rust decides the DETERMINISTIC half of that table (see
    /// `engine::specials_for_hud`): with `"two"` the specials vec is sent
    /// empty, exactly as it is when `hud_show_specials` is off. `"auto"` is
    /// the page's call and only the page's — the band count depends on
    /// MEASURED label widths, which exist nowhere but in the overlay
    /// document — so Rust keeps sending the specials and the page drops them
    /// if it ends up needing two bands.
    ///
    /// **NO HOOK ATOMIC, DELIBERATELY — and nothing on the hook path may grow
    /// one.** Same argument as `hud_show_specials` above but stronger: the
    /// hook never reads this at all. It is read once on the Space-HOLD path
    /// inside a config borrow that already happens (engine thread), and
    /// otherwise it only travels to the overlay page as an event. An atomic
    /// here would be a second source of truth with no reader and one more
    /// thing to forget to publish (PROBLEM 180's failure mode). Do not add
    /// one; `pointer.rs`'s snapshot is unaffected by this setting because it
    /// only ever holds the APPS ring.
    ///
    /// Default `"auto"`: this is a brand-new choice, and auto reproduces
    /// exactly what every existing build already does — pick whatever fits.
    /// Both paths matter and `first_install_tests` holds both to it: `Default`
    /// governs a fresh install, the serde attribute governs the config already
    /// on every existing user's disk, and nothing forces them to agree except
    /// that test (see `pointer_hud_activation`, where the field-removal path
    /// was the only one that reached anybody).
    #[serde(default = "default_band_count")]
    pub hud_band_count: String,

    /// Use the NEW Magnetic Sector ring layout for the Space HUD, or the
    /// CLASSIC ring that shipped in 1.0.88? **ON (= the new layout) by
    /// default, by the owner's explicit decision on 2026-08-27.**
    ///
    /// `true` = the new layout, `false` = the classic one. The toggle is the
    /// ESCAPE HATCH, not the invitation: his words were *"give an option to
    /// use this new HUD layout or old layout — in settings, toggle"*, and the
    /// new ring is what he wants to open the app and see.
    ///
    /// **THIS DELIBERATELY OVERRIDES THIS STRUCT'S OWN CONVENTION** that
    /// brand-new behaviour ships OFF — the convention `hud_toast_flight`
    /// above follows and documents. The precedent for overriding it is
    /// `pointer_hud_activation` above, flipped by the same owner on the same
    /// day for the same reason. **Do not "harmonise" these three fields.**
    /// They disagree on purpose: `hud_toast_flight` is a bare
    /// `#[serde(default)]` (new behaviour, OFF), `pointer_hud_activation` and
    /// this one are `default = "default_true"` (new behaviour the owner chose
    /// to ship ON), and `hud_show_specials` is `default_true` for a third
    /// reason again (existing behaviour becoming optional). The deciding
    /// question is never "what does the field next to it do?".
    ///
    /// So: `default = "default_true"`, NOT a bare `#[serde(default)]` — a
    /// bool's `Default` is `false`, which would hand the classic layout to
    /// every config already on disk and deliver the new one to nobody.
    /// Frontend readers use `!== false`, NEVER `=== true`.
    ///
    /// **A DEFAULT ONLY EVER REACHES USERS WHOSE FILE PREDATES THE FIELD**,
    /// so `first_install_tests` asserts BOTH paths — the fresh `Default` AND
    /// the field-removed-from-an-old-config path. That second one is where
    /// this flip actually lands, and it is the one that nearly went
    /// unwritten: `pointer_hud_activation` had already been written to the
    /// owner's own config on 2026-08-27, so its flip reached him through the
    /// WRITTEN value, not through the default at all.
    ///
    /// **NO HOOK ATOMIC, DELIBERATELY.** Nothing on the hook path reads this
    /// — the layout is chosen inside the overlay page, from measured label
    /// widths, and this field travels there as the `hud-layout-changed` event
    /// plus the `get_config` seed. Same argument as `hud_band_count` above,
    /// and stronger: not even the engine consults it. An atomic here would be
    /// a second source of truth with no reader and one more thing to forget
    /// to publish (PROBLEM 180's failure mode). Do not add one.
    #[serde(default = "default_true")]
    pub hud_magnetic_layout: bool,

    /// PROBLEM 195 — whether crash and error reports may be sent to Sentry.
    ///
    /// **TRUE MEANS SENDING IS HAPPENING.** This field is the positive
    /// statement; the Settings switch is its NEGATION ("Don't send logs"), so
    /// the switch reads `!send_logs` and writes the opposite of its own checked
    /// state. Getting that backwards is the easiest possible mistake here and
    /// the only one a user could not detect, so it is spelled out in three
    /// places: here, in `settings-panel.ts`, and in `set_send_logs`.
    ///
    /// `default = "default_true"`, NOT a bare `#[serde(default)]`: a bool's
    /// `Default` is `false`, so the bare attribute would silently invert the
    /// intended default for every config written before 1.0.82 — i.e. every
    /// existing user would be opted out while the UI showed them opted in.
    ///
    /// Read at runtime through `telemetry::SENDING_ENABLED`, not from here:
    /// this struct is a snapshot behind an RwLock and the panic hook cannot
    /// take a lock. `telemetry::publish` copies it into the atomic from both
    /// the startup load and `config::save`.
    #[serde(default = "default_true")]
    pub send_logs: bool,

    /// PROBLEM 242 — has the first-run "Guided first bind" tour been seen?
    ///
    /// Bare `#[serde(default)]`, NOT `default_true`, and that is the whole
    /// point: `false` is what every config written before 1.0.97 must read
    /// as, because those users have never been offered the walkthrough and
    /// the tour is meant to reach them. This is the opposite case from
    /// `send_logs` directly above, where absent had to mean ON.
    ///
    /// Written `true` exactly once — by the frontend, through `save_config`,
    /// on the run the user finishes OR skips the tour. Rust never reads it;
    /// it exists only so the dashboard can ask "has this happened before?"
    /// across restarts. Skipping counts as done: "Skip" means never nag
    /// again, and Settings' "Show me the walkthrough" is the way back in,
    /// which ignores this field entirely.
    #[serde(default)]
    pub tour_done: bool,

    /// PROBLEM 245 — the in-app updater's ONLY switch, and it is not in the
    /// UI. Owner's decision: updates install themselves, silently, for
    /// everyone, with no Settings row. This field is the config-file escape
    /// hatch for the one person who needs to pin a version: set it to
    /// `false` in `%APPDATA%\Spaceadom\config.json` and `updater::run_check`
    /// logs that it skipped. Nothing in the app ever writes it.
    ///
    /// `default_true`, same reasoning as `send_logs`: a bare
    /// `#[serde(default)]` would read as `false` for every config written
    /// before this field existed, i.e. every existing user would silently
    /// stop receiving updates. `first_install_tests` holds both paths to it.
    #[serde(default = "default_true")]
    pub auto_update: bool,

    /// TOUCHPAD T2 (1.0.120, docs/TOUCHPAD-BRIEF-T2.md) — the four edge bands
    /// of a Precision Touchpad. Everything OFF by default; a config that
    /// predates the field reads as "nothing set up" (bare `#[serde(default)]`),
    /// which is exactly what a fresh install shows too. `first_install_tests`
    /// holds both paths to it.
    #[serde(default)]
    pub touchpad: Touchpad,
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
/// See `hud_band_count`. `"auto"` — the behaviour every build before 1.0.89
/// already had, so an upgrading config changes nothing by acquiring the key.
fn default_band_count() -> String { "auto".into() }
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
            tray_promoted_for: String::new(),
            guide_hud_delay_ms: 300,
            overlay_compositing: default_overlay_compositing(),
            opacity_floor_pct: 25,
            browser_path: None,
            fullscreen_allowlist: vec![
                "vlc.exe".into(),
                "mpv.exe".into(),
            ],
            // Empty by default. Nobody gets an app excluded without asking.
            excluded_apps: Vec::new(),
            profiles: Vec::new(),
            special_keys: BindingMap::new(),
            advanced_mode: false,
            key_combo_hint_seen: false,
            dark_mode: false,
            // FEATURE 2 (2026-09-05) — NEW installs default to "auto", not
            // "earthy". This is `AppConfig::default()`, used ONLY when there
            // is genuinely no config on disk to read (`generate_defaults()`
            // in config/mod.rs) — an EXISTING config missing the key still
            // gets the untouched `#[serde(default)]` empty string above,
            // which `config/mod.rs`'s migration turns into "earthy"/"starry"
            // from `dark_mode`, exactly as before. Those are two different
            // code paths on purpose: an upgrading user's prior choice must
            // never be silently replaced with "auto", but a first-time user
            // has no prior choice for "auto" to override.
            theme: "auto".to_string(),
            fun_mode: false,
            hide_keyboard: false,
            show_me_around: false,
            sound_enabled: false,
            run_at_startup: true,
            // PROBLEM 237 — ON. Must agree with the `default = "default_true"`
            // on the field; first_install_tests holds both to it.
            warm_picker_at_startup: true,
            motion: default_motion(),
            hud_toast_flight: false,
            // PROBLEM 209 — ON, by the owner's explicit decision on
            // 2026-08-27, knowingly overriding the new-behaviour-defaults-off
            // convention that the line above follows. Must agree with the
            // `default = "default_true"` on the field; first_install_tests
            // holds both to it.
            pointer_hud_activation: true,
            // PROBLEM 263 — ON, by the owner's decision on 2026-09-08; the
            // second field here to ship a NEW behaviour on. Must agree with the
            // `default = "default_true"` on the field; first_install_tests
            // holds both to it. The 3D/CAD safety net is NOT this flag — it is
            // the built-in list in hook/orbit_apps.rs, which is not a setting.
            middle_button_ring: true,
            // PROBLEM 267 — the NEW ring by default, the owner's decision on
            // 2026-09-13; `GuideHud` is the way back to phase 1. Must agree
            // with the `#[serde(default)]` (= the enum's `#[default]`) on the
            // field; first_install_tests holds both to it. Scope: the eight.
            // Favourites: empty = "the first eight bound letters, lazily".
            middle_ring_style: MiddleRingStyle::IconRing,
            middle_ring_scope: MiddleRingScope::MyEight,
            all_ring_layout: AllRingLayout::Rings,
            middle_ring_favourites: Vec::new(),
            // PROBLEM 209 — ON. The specials ring has always been drawn; this
            // setting only lets someone turn it off. An existing config must
            // keep what it had.
            hud_show_specials: true,
            // "auto" — let the arithmetic pick the band count, which is what
            // every build before this one did unconditionally. Must agree with
            // `default = "default_band_count"` on the field; first_install_tests
            // holds both to it.
            hud_band_count: default_band_count(),
            // ON — the new Magnetic Sector ring, by the owner's explicit
            // decision on 2026-08-27, knowingly overriding the
            // new-behaviour-defaults-off convention that `hud_toast_flight`
            // above follows. Must agree with the `default = "default_true"`
            // on the field; first_install_tests holds both to it.
            hud_magnetic_layout: true,
            // PROBLEM 195 — ON. Crash reporting is only useful if it is on by
            // default; the opt-out is one switch at the bottom of Settings and
            // PRIVACY.md says exactly what it sends.
            send_logs: true,
            // PROBLEM 242 — FALSE on a fresh install, which is what makes the
            // first-run tour appear at all. Must agree with the bare
            // `#[serde(default)]` on the field; first_install_tests holds
            // both to it.
            tour_done: false,
            // PROBLEM 245 — ON. Must agree with `default = "default_true"` on
            // the field; first_install_tests holds both to it.
            auto_update: true,
            // TOUCHPAD T2 — every band off, the page in its Chocolate look,
            // the one-finger demo not yet seen. Must agree with the bare
            // `#[serde(default)]` on the field; `touchpad_tests` holds both.
            touchpad: Touchpad::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// TOUCHPAD T2 (1.0.120) — the edge-band settings
// ---------------------------------------------------------------------------

/// Which edge of the touchpad a band lives on. Stored lowercase
/// (`"left"` …). The gesture engine's `Edge` is this same type; `raw.rs`'s
/// probe-only `Edge` predates it and stays where the example expects it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchEdge {
    Left,
    Right,
    Top,
    Bottom,
}

impl TouchEdge {
    pub const ALL: [TouchEdge; 4] = [TouchEdge::Left, TouchEdge::Right, TouchEdge::Top, TouchEdge::Bottom];

    /// Left/right run along Y; top/bottom along X.
    pub fn is_vertical(self) -> bool {
        matches!(self, TouchEdge::Left | TouchEdge::Right)
    }
}

/// What a band does while a finger slides along it. `None` is the page's
/// "Nothing" — the band stays drawn, arms, freezes the pointer, and changes
/// nothing (so a user can park an edge without losing its geometry).
///
/// `Chords` (1.0.122, the page's "Any shortcut") is step-based: the signed
/// travel is quantised into steps (`touchpad::actions::chord_steps`) and each
/// new step sends `forward` or `backward` ONCE through the one-batch
/// `send_keys_checked` path (PROBLEM 227 cookie). Sliding back sends the other
/// chord, so it is reversible by the opposite slide. Serialised externally
/// tagged, so the unit variants stay the plain strings 1.0.120 wrote
/// (`"scrub"`) and this one is `{"chords":{"forward":[…],"backward":[…]}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BandAction {
    Brightness,
    Volume,
    Scrub,
    #[default]
    None,
    Chords {
        #[serde(default)]
        forward: Vec<u16>,
        #[serde(default)]
        backward: Vec<u16>,
    },
}

impl BandAction {
    /// The page's "Any shortcut" with nothing recorded yet.
    pub fn empty_chords() -> BandAction {
        BandAction::Chords { forward: Vec::new(), backward: Vec::new() }
    }

    /// The wire name (`touchpad-live.action`, the same lowercase serde tags).
    pub fn wire(&self) -> &'static str {
        match self {
            BandAction::Brightness => "brightness",
            BandAction::Volume => "volume",
            BandAction::Scrub => "scrub",
            BandAction::None => "none",
            BandAction::Chords { .. } => "chords",
        }
    }
}

/// One edge band. `width` is a fraction of the pad's SHORT side, `length` a
/// fraction of the edge (centred). Both are clamped on load by
/// `Band::clamped` — the page shows width as px of its drawn 780×520 pad and
/// length as a percentage, as the design does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Band {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub action: BandAction,
    #[serde(default = "Band::default_width")]
    pub width: f32,
    #[serde(default = "Band::default_length_side")]
    pub length: f32,
    #[serde(default = "Band::default_sensitivity")]
    pub sensitivity: u8,
    #[serde(default)]
    pub invert: bool,
}

impl Band {
    pub const WIDTH_MIN: f32 = 0.04;
    pub const WIDTH_MAX: f32 = 0.25;
    pub const LENGTH_MIN: f32 = 0.30;
    pub const LENGTH_MAX: f32 = 1.0;
    pub const SENS_MIN: u8 = 1;
    pub const SENS_MAX: u8 = 10;

    /// 0.12 of the pad's short side (1.0.122; was 0.07). The T1b probe proved
    /// its bands at 12 %, and the owner's 1.0.121 run at 7 % (5 mm on this
    /// pad) never armed.
    pub fn default_width() -> f32 {
        0.12
    }
    /// The most keys one recorded chord may hold — `engine::actions::chord`'s
    /// batch limit, so a saved chord is always sendable.
    pub const CHORD_MAX_KEYS: usize = 8;
    pub fn default_length_side() -> f32 {
        0.70
    }
    pub fn default_length_top() -> f32 {
        0.80
    }
    pub fn default_sensitivity() -> u8 {
        6
    }

    /// The owner's defaults per edge: left = brightness, right = volume,
    /// top = scrub, bottom = nothing (a normal edge since 1.0.122; the user
    /// picks its action). All OFF.
    pub fn for_edge(edge: TouchEdge) -> Band {
        let (action, length) = match edge {
            TouchEdge::Left => (BandAction::Brightness, Self::default_length_side()),
            TouchEdge::Right => (BandAction::Volume, Self::default_length_side()),
            TouchEdge::Top => (BandAction::Scrub, Self::default_length_top()),
            TouchEdge::Bottom => (BandAction::None, Self::default_length_top()),
        };
        Band {
            enabled: false,
            action,
            width: Self::default_width(),
            length,
            sensitivity: Self::default_sensitivity(),
            invert: false,
        }
    }

    /// Every number inside its range; NaN reads as the default. Pure.
    pub fn clamped(mut self) -> Band {
        if !self.width.is_finite() {
            self.width = Self::default_width();
        }
        if !self.length.is_finite() {
            self.length = Self::default_length_side();
        }
        self.width = self.width.clamp(Self::WIDTH_MIN, Self::WIDTH_MAX);
        self.length = self.length.clamp(Self::LENGTH_MIN, Self::LENGTH_MAX);
        self.sensitivity = self.sensitivity.clamp(Self::SENS_MIN, Self::SENS_MAX);
        if let BandAction::Chords { forward, backward } = &mut self.action {
            forward.truncate(Self::CHORD_MAX_KEYS);
            backward.truncate(Self::CHORD_MAX_KEYS);
        }
        self
    }
}

impl Default for Band {
    fn default() -> Self {
        Band::for_edge(TouchEdge::Left)
    }
}

/// What happens when a finger LANDS in the square two enabled bands share.
/// `Ask` = no default: the page asks the moment the second overlapping band
/// is switched on, and until the user answers a landing in that square
/// belongs to NO band (normal pointing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CornerRule {
    #[default]
    Ask,
    AlwaysHorizontal,
    AlwaysVertical,
}

/// Per-corner owner when the user set one by hand. `None` = follow
/// `corner_rule`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Corners {
    #[serde(default)]
    pub tl: Option<TouchEdge>,
    #[serde(default)]
    pub tr: Option<TouchEdge>,
    #[serde(default)]
    pub bl: Option<TouchEdge>,
    #[serde(default)]
    pub br: Option<TouchEdge>,
}

impl Corners {
    /// The owner recorded for the corner where `vertical` meets `horizontal`.
    pub fn get(&self, vertical: TouchEdge, horizontal: TouchEdge) -> Option<TouchEdge> {
        match (vertical, horizontal) {
            (TouchEdge::Left, TouchEdge::Top) => self.tl,
            (TouchEdge::Right, TouchEdge::Top) => self.tr,
            (TouchEdge::Left, TouchEdge::Bottom) => self.bl,
            (TouchEdge::Right, TouchEdge::Bottom) => self.br,
            _ => None,
        }
    }

    pub fn set(&mut self, vertical: TouchEdge, horizontal: TouchEdge, owner: Option<TouchEdge>) {
        match (vertical, horizontal) {
            (TouchEdge::Left, TouchEdge::Top) => self.tl = owner,
            (TouchEdge::Right, TouchEdge::Top) => self.tr = owner,
            (TouchEdge::Left, TouchEdge::Bottom) => self.bl = owner,
            (TouchEdge::Right, TouchEdge::Bottom) => self.br = owner,
            _ => {}
        }
    }

    /// How many corners were set by hand (the page's "1 corner set by hand").
    pub fn set_by_hand(&self) -> usize {
        [self.tl, self.tr, self.bl, self.br].iter().filter(|c| c.is_some()).count()
    }
}

/// The page's look: the design's Chocolate tokens (default) or the app's
/// own theme variables. The home thumbnail always matches the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TouchpadLook {
    #[default]
    Chocolate,
    App,
}

/// The whole touchpad section. All four edges are ordinary bands (the bottom
/// stopped being "reserved" in 1.0.122, owner decision 2026-09-19 17:30).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Touchpad {
    #[serde(default = "Touchpad::default_left")]
    pub left: Band,
    #[serde(default = "Touchpad::default_right")]
    pub right: Band,
    #[serde(default = "Touchpad::default_top")]
    pub top: Band,
    #[serde(default = "Touchpad::default_bottom")]
    pub bottom: Band,
    #[serde(default)]
    pub corner_rule: CornerRule,
    #[serde(default)]
    pub corners: Corners,
    #[serde(default)]
    pub page_look: TouchpadLook,
    #[serde(default)]
    pub demo_seen: bool,
    /// "Show the touchpad under the keyboard" (owner, 2026-09-19) — the home
    /// thumbnail. Default ON, and only shown in Settings when a Precision
    /// Touchpad is present; OFF hides the thumbnail and the keyboard centres
    /// as board-only. `default_true` so an existing config keeps the
    /// thumbnail it has always seen once a pad is detected.
    #[serde(default = "default_true")]
    pub show_thumbnail: bool,
    /// "Show a toast while sliding" (owner, 2026-09-19, 1.0.123) — while an
    /// edge slide changes volume or brightness (or scrubs / sends chords) the
    /// overlay shows ONE toast that updates in place ("🔊 Volume 62%") and
    /// fades ~600 ms after the finger lifts. Read by the reader through the
    /// existing `snapshot()` / `CONFIG_GEN` path, never on the raw-input
    /// callback directly. `default_true` so an existing config gets it.
    #[serde(default = "default_true")]
    pub slide_toast: bool,
}

impl Touchpad {
    fn default_left() -> Band {
        Band::for_edge(TouchEdge::Left)
    }
    fn default_right() -> Band {
        Band::for_edge(TouchEdge::Right)
    }
    fn default_top() -> Band {
        Band::for_edge(TouchEdge::Top)
    }
    fn default_bottom() -> Band {
        Band::for_edge(TouchEdge::Bottom)
    }

    pub fn band(&self, edge: TouchEdge) -> &Band {
        match edge {
            TouchEdge::Left => &self.left,
            TouchEdge::Right => &self.right,
            TouchEdge::Top => &self.top,
            TouchEdge::Bottom => &self.bottom,
        }
    }

    pub fn band_mut(&mut self, edge: TouchEdge) -> &mut Band {
        match edge {
            TouchEdge::Left => &mut self.left,
            TouchEdge::Right => &mut self.right,
            TouchEdge::Top => &mut self.top,
            TouchEdge::Bottom => &mut self.bottom,
        }
    }

    /// Ranges enforced on every band. Pure; applied on load and on save so
    /// the engine never sees a value the page cannot show.
    pub fn normalised(mut self) -> Touchpad {
        for e in TouchEdge::ALL {
            let mut b = self.band(e).clone();
            // 1.0.120/121 saved the old 7% default — a 5 mm band nobody could
            // land in (owner's left edge never armed, 2026-09-19). A band still
            // at exactly that old default takes the proven 12%; anything the
            // user set by hand is left alone.
            if (b.width - 0.07).abs() < 1e-6 {
                b.width = Band::default_width();
            }
            *self.band_mut(e) = b.clamped();
        }
        self
    }

    /// Edges with an enabled band, in the ALL order.
    pub fn enabled_edges(&self) -> Vec<TouchEdge> {
        TouchEdge::ALL.iter().copied().filter(|e| self.band(*e).enabled).collect()
    }

    pub fn any_enabled(&self) -> bool {
        !self.enabled_edges().is_empty()
    }
}

impl Default for Touchpad {
    fn default() -> Self {
        Touchpad {
            left: Self::default_left(),
            right: Self::default_right(),
            top: Self::default_top(),
            bottom: Self::default_bottom(),
            corner_rule: CornerRule::Ask,
            corners: Corners::default(),
            page_look: TouchpadLook::Chocolate,
            demo_seen: false,
            show_thumbnail: true,
            slide_toast: true,
        }
    }
}

#[cfg(test)]
mod touchpad_tests {
    use super::*;

    #[test]
    fn a_config_predating_the_field_reads_as_nothing_set_up() {
        // Same fixture rule as `first_install_tests`: delete the field from a
        // current config rather than hand-write an old one.
        let mut v = serde_json::to_value(AppConfig::default()).expect("serialise");
        v.as_object_mut().expect("object").remove("touchpad");
        let c: AppConfig = serde_json::from_value(v).expect("a config predating `touchpad` must load");
        assert_eq!(c.touchpad, Touchpad::default());
        assert!(!c.touchpad.any_enabled());
        assert_eq!(c.touchpad.page_look, TouchpadLook::Chocolate);
        assert!(!c.touchpad.demo_seen);
        assert_eq!(c.touchpad.corner_rule, CornerRule::Ask);
    }

    /// 1.0.123 — a config written before `slide_toast` existed must read it
    /// as ON (the owner wants the live toast by default), and an explicit
    /// `false` must survive the round trip.
    #[test]
    fn slide_toast_defaults_on_when_the_key_is_absent() {
        let mut v = serde_json::to_value(AppConfig::default()).expect("serialise");
        let tp = v["touchpad"].as_object_mut().expect("touchpad object");
        assert!(tp.remove("slide_toast").is_some(), "the default config writes the key");
        let c: AppConfig = serde_json::from_value(v).expect("a 1.0.122 config must load");
        assert!(c.touchpad.slide_toast, "absent must mean ON");

        let mut v = serde_json::to_value(AppConfig::default()).expect("serialise");
        v["touchpad"]["slide_toast"] = serde_json::Value::Bool(false);
        let c: AppConfig = serde_json::from_value(v).expect("explicit false loads");
        assert!(!c.touchpad.slide_toast);
        assert!(!c.touchpad.normalised().slide_toast, "normalised() keeps the switch");
    }

    #[test]
    fn the_owner_defaults_per_edge() {
        let t = Touchpad::default();
        assert_eq!(t.left.action, BandAction::Brightness);
        assert_eq!(t.right.action, BandAction::Volume);
        assert_eq!(t.top.action, BandAction::Scrub);
        assert_eq!(t.bottom.action, BandAction::None);
        assert!((t.left.length - 0.70).abs() < 1e-6);
        assert!((t.top.length - 0.80).abs() < 1e-6);
        assert!((t.left.width - 0.12).abs() < 1e-6, "12 % of the short side (1.0.122)");
        assert_eq!(t.left.sensitivity, 6);
        for e in TouchEdge::ALL {
            assert!(!t.band(e).enabled, "{e:?} must start off");
        }
    }

    #[test]
    fn ranges_are_clamped_and_the_bottom_band_is_an_ordinary_band() {
        let mut t = Touchpad::default();
        t.left.width = 0.9;
        t.left.length = 0.1;
        t.left.sensitivity = 40;
        t.right.width = f32::NAN;
        t.bottom.enabled = true;
        t.bottom.action = BandAction::Volume;
        let n = t.normalised();
        assert!((n.left.width - Band::WIDTH_MAX).abs() < 1e-6);
        assert!((n.left.length - Band::LENGTH_MIN).abs() < 1e-6);
        assert_eq!(n.left.sensitivity, Band::SENS_MAX);
        assert!((n.right.width - Band::default_width()).abs() < 1e-6);
        // 1.0.122: no reserved edge — the bottom keeps what the user set.
        assert!(n.bottom.enabled);
        assert_eq!(n.bottom.action, BandAction::Volume);
        assert_eq!(n.enabled_edges(), vec![TouchEdge::Bottom]);
    }

    #[test]
    fn chords_round_trip_and_a_bare_chords_object_reads_empty() {
        let mut t = Touchpad::default();
        t.bottom.enabled = true;
        t.bottom.action = BandAction::Chords { forward: vec![0x11, 0x54], backward: vec![0x11, 0x57] };
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains(r#""action":{"chords":{"forward":[17,84],"backward":[17,87]}}"#), "{json}");
        assert!(json.contains(r#""action":"scrub""#), "the unit variants are still plain strings: {json}");
        let back: Touchpad = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
        // A chords object with nothing recorded yet.
        let t2: Touchpad = serde_json::from_str(r#"{"top":{"action":{"chords":{}}}}"#).unwrap();
        assert_eq!(t2.top.action, BandAction::empty_chords());
        assert_eq!(t2.top.action.wire(), "chords");
        // A 1.0.120 config (plain strings) still loads.
        let t3: Touchpad = serde_json::from_str(r#"{"left":{"enabled":true,"action":"brightness"}}"#).unwrap();
        assert_eq!(t3.left.action, BandAction::Brightness);
    }

    #[test]
    fn an_over_long_chord_is_cut_to_the_sendable_length() {
        let mut b = Band::for_edge(TouchEdge::Top);
        b.action = BandAction::Chords { forward: (1..=12).collect(), backward: Vec::new() };
        let c = b.clamped();
        match c.action {
            BandAction::Chords { forward, backward } => {
                assert_eq!(forward.len(), Band::CHORD_MAX_KEYS);
                assert!(backward.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_partial_band_object_fills_in_its_defaults() {
        let t: Touchpad = serde_json::from_str(r#"{"top":{"enabled":true}}"#).unwrap();
        assert!(t.top.enabled);
        // A bare object has no edge context, so `Band`'s own serde defaults
        // apply: the side length, `None` action. The page sets the action.
        assert_eq!(t.top.action, BandAction::None);
        assert!((t.top.width - 0.12).abs() < 1e-6);
        assert_eq!(t.top.sensitivity, 6);
        assert_eq!(t.right, Band::for_edge(TouchEdge::Right));
    }

    #[test]
    fn corners_and_looks_serialise_lowercase() {
        let mut t = Touchpad::default();
        t.corners.set(TouchEdge::Right, TouchEdge::Top, Some(TouchEdge::Right));
        t.page_look = TouchpadLook::App;
        t.corner_rule = CornerRule::AlwaysVertical;
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains(r#""tr":"right""#), "{json}");
        assert!(json.contains(r#""page_look":"app""#), "{json}");
        assert!(json.contains(r#""corner_rule":"always_vertical""#), "{json}");
        assert!(json.contains(r#""action":"scrub""#), "{json}");
        assert_eq!(t.corners.get(TouchEdge::Right, TouchEdge::Top), Some(TouchEdge::Right));
        assert_eq!(t.corners.get(TouchEdge::Left, TouchEdge::Top), None);
        assert_eq!(t.corners.set_by_hand(), 1);
        let back: Touchpad = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }
}

/// A named shortcut profile containing per-key bindings.
///
/// **THE VEC ORDER IS FUNCTIONAL, NOT COSMETIC.** `AppConfig::profiles` is the
/// order RAlt cycles in, so `reorder_profiles` (commands.rs) is a behaviour
/// change every time it is called, not a display preference. Nothing may sort
/// this vec for presentation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Unique profile name (1–24 chars, any printable text — see
    /// `commands::regex_lite`, PROBLEM 197).
    pub name: String,

    /// Map of lowercase key character → binding.
    /// Keys present in V11: a–z.
    pub bindings: BindingMap,

    /// One emoji standing in for this profile — on the popover row's disc, on
    /// the dashboard's top-right pill, and on the Guide HUD's SPACE pill.
    ///
    /// `None` is the normal state, and every surface must render its EXISTING
    /// look when it is None (the pill keeps its initial letter). Optional in
    /// serde too: every config on disk predates this field, and a bare
    /// `#[serde(default)]` on an `Option` is exactly right here — the absent
    /// value and the "no emoji" value are the same thing, unlike the bools in
    /// `AppConfig` where absence had to be told apart from a chosen `false`.
    ///
    /// Validated by `emoji_is_valid` below — ONE grapheme cluster, which is
    /// not the same as one `char` and not the same as a byte budget.
    #[serde(default)]
    pub emoji: Option<String>,

    /// PHASE A — has `seed_specials` run on this profile? `false` on every
    /// config written before 2026-09-18 (serde default), so the seed runs
    /// exactly once per profile on the next load and then never again — which
    /// is what lets a user DELETE a special (remove its key) and have it stay
    /// deleted.
    #[serde(default)]
    pub specials_seeded: bool,
}

/// The longest a valid single-cluster emoji may be, counted in `char`s.
///
/// A cap is still needed even though the cluster count below is the real rule:
/// a pathological string could be one cluster and thousands of code points
/// (combining marks stack without limit), and that string would be written to
/// `config.json` and rendered into a 30px disc. 16 clears every real sequence
/// — the longest emoji in Unicode 15 is the seven-code-point tag flag
/// (🏴󠁧󠁢󠁥󠁮󠁧󠁿), and a four-person family with skin tones reaches 15.
pub const EMOJI_MAX_CHARS: usize = 16;

/// How many grapheme CLUSTERS a string contains, near enough for this field.
///
/// **DO NOT REPLACE THIS WITH `chars().count() == 1`.** That was the obvious
/// first version and it rejects almost every emoji a person would pick:
/// 👨‍👩‍👧 is five code points, 👍🏽 is two, ❤️ is two, 🇧🇩 is two. A byte
/// budget is worse still — 👨‍👩‍👧 is 18 bytes.
///
/// This is a deliberate approximation of UAX #29, not an implementation of it:
/// no `unicode-segmentation` dependency is pulled in for one config field. It
/// counts a new cluster for every code point EXCEPT the ones that by
/// definition attach to the one before:
///
/// * ZWJ (U+200D) and whatever follows it — emoji ZWJ sequences;
/// * variation selectors (U+FE00–FE0F) — the ️ in ❤️;
/// * skin-tone modifiers (U+1F3FB–1F3FF);
/// * tag characters (U+E0020–E007F) — the subdivision flags;
/// * the keycap mark (U+20E3);
/// * the common combining-mark blocks;
/// * the SECOND regional indicator of a pair — a country flag is two.
///
/// Where it differs from the real algorithm (an unpaired regional indicator, a
/// lone combining mark) it errs toward ACCEPTING, which is the right direction
/// for a field whose only consequence is a glyph in a disc.
pub fn cluster_count(s: &str) -> usize {
    let mut clusters = 0usize;
    let mut after_zwj = false;
    let mut regional_open = false;

    for c in s.chars() {
        let cp = c as u32;
        let joins_previous = matches!(cp,
            0xFE00..=0xFE0F        // variation selectors
            | 0x1F3FB..=0x1F3FF    // emoji skin-tone modifiers
            | 0xE0020..=0xE007F    // tag characters (subdivision flags)
            | 0x20E3               // combining enclosing keycap
            | 0x0300..=0x036F      // combining diacritical marks
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x20D0..=0x20FF
            | 0xFE20..=0xFE2F
        );
        let is_regional = (0x1F1E6..=0x1F1FF).contains(&cp);

        if cp == 0x200D {
            after_zwj = true;
            regional_open = false;
            continue;
        }
        if joins_previous {
            after_zwj = false;
            continue;
        }
        if is_regional && regional_open {
            // Second half of a flag pair — closes it, so a third indicator
            // starts a new cluster rather than extending this one forever.
            regional_open = false;
            after_zwj = false;
            continue;
        }
        if after_zwj {
            after_zwj = false;
            regional_open = is_regional;
            continue;
        }
        clusters += 1;
        regional_open = is_regional;
    }
    clusters
}

/// Is this an acceptable value for `Profile::emoji`?
///
/// One cluster, at most `EMOJI_MAX_CHARS` code points, no control characters
/// and no internal whitespace. Deliberately NOT "is this in an emoji block":
/// the owner asked for a slot the user fills from Windows' own emoji panel,
/// and that panel serves kaomoji and symbols alongside emoji. Anything that
/// renders as one glyph is a legitimate answer.
pub fn emoji_is_valid(s: &str) -> bool {
    !s.is_empty()
        && s.chars().count() <= EMOJI_MAX_CHARS
        && !s.chars().any(|c| c.is_control() || c.is_whitespace())
        && cluster_count(s) == 1
}

/// ONE profile on its own, as a `.json` file.
///
/// Three things write and read this shape and they must not drift apart:
/// Export (the save dialog), Import (the open dialog), and the silent backup
/// `delete_profile` drops into `%LOCALAPPDATA%\SpaceadomBackups` before it
/// destroys anything. `config::profile_export_json` and
/// `config::parse_profile_export` are the only two functions that touch it.
///
/// **`spaceadom_profile` IS LOAD-BEARING, NOT DECORATION.** The backups folder
/// already holds whole-config files, and `config::newest_valid_backup_in`
/// walks every `*.json` in there trying to parse each as an `AppConfig`. The
/// marker (plus a required `bindings` map) is what makes "is this a profile or
/// a config?" a decision rather than a guess — and what stops Import from
/// cheerfully accepting an arbitrary JSON file whose keys happen to overlap.
/// `profile_export_is_not_a_config` asserts the other half of that.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileExport {
    /// Format version of THIS file, not of `config.json`. 1 today.
    pub spaceadom_profile: u32,
    pub name: String,
    #[serde(default)]
    pub emoji: Option<String>,
    pub bindings: BindingMap,
    /// PHASE A — carried so an export made after a special was deliberately
    /// removed does not get it re-seeded on import. Absent in every export
    /// written before 2026-09-18, which then seeds on import — correct, since
    /// those exports predate the specials being keys at all.
    #[serde(default)]
    pub specials_seeded: bool,
}

/// PHASE A step 3 (§5) — every `Command` binding in a profile, as
/// `(key id, line, elevated)` in sorted key order: what the import dialog
/// lists before the user confirms (a profile a friend sent must never run
/// anything on import, and the user must see every line first).
pub fn command_lines(p: &Profile) -> Vec<(String, String, bool)> {
    let mut out: Vec<(String, String, bool)> = p
        .bindings
        .iter()
        .filter_map(|(k, b)| match &b.action {
            Some(Action::Command { line, elevated }) => Some((k.clone(), line.clone(), *elevated)),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

impl ProfileExport {
    /// The current format version. Bump only if the shape changes
    /// incompatibly; `parse_profile_export` accepts anything <= this.
    pub const VERSION: u32 = 1;

    pub fn of(p: &Profile) -> Self {
        Self {
            spaceadom_profile: Self::VERSION,
            name: p.name.clone(),
            emoji: p.emoji.clone(),
            bindings: p.bindings.clone(),
            specials_seeded: p.specials_seeded,
        }
    }
}

/// PHASE A (2026-09-18) — WHAT a key does when Space is held, beyond "open
/// this app or link". Tagged `{"kind": "...", ...}` on disk.
///
/// `KeyBinding::action` is `None` for every binding written before this
/// enum existed, and `None` means the legacy `app` / `web_url` fields ARE the
/// action — nothing on disk changes shape, and `is_mapped` treats the two
/// forms alike. The 12 built-in specials that used to be fixed `KeyCombo`
/// variants in the hook are `Special { id }` bindings now, seeded once per
/// profile by `seed_specials` (see `DEFAULT_SPECIALS`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// `ms-settings:display`, `shell:Downloads`, `control.exe /name
    /// Microsoft.PowerOptions`, or any URI Windows knows how to open.
    Uri { target: String },
    /// Virtual-key codes in PRESS order, e.g. `[0x5B, 0x10, 0x53]` =
    /// Win+Shift+S. Sent as ONE `send_keys_checked` batch (PROBLEM 227).
    Chord { keys: Vec<u16> },
    /// A PowerShell line (`powershell.exe -NoProfile -ExecutionPolicy Bypass
    /// -Command <line>`, no window, killed after 60 s — PHASE A step 3; it
    /// was `cmd.exe /C` in 1.0.116/117). Advanced only — that is a UI
    /// concern; the engine just runs it. `elevated` asks Windows for its own
    /// UAC prompt every time (`ShellExecuteW` verb `runas`); the app's
    /// process never elevates. `#[serde(default)]`: every 1.0.116/117
    /// binding has no such key and reads as `false`.
    Command {
        line: String,
        #[serde(default)]
        elevated: bool,
    },
    /// +10 / -10 on the internal panel through WMI.
    Brightness { delta: i32 },
    /// One of `SPECIAL_IDS`.
    Special { id: String },
    /// PHASE A step 2 (2026-09-19) — FLIP a Windows setting and toast the
    /// new state: one of `engine::actions::toggle::TOGGLE_IDS` (`bluetooth`,
    /// `wifi`, `dark_mode`, `night_light`, `taskbar_autohide`, `screen_off`,
    /// `sleep`, `lock`, `show_desktop`). An unknown id logs a warning and
    /// toasts "Unknown toggle" — a config from a newer build.
    Toggle { what: String },
}

/// The built-in specials, by id. The order is the order the HUD's inner
/// ring and the icon ring list them when they are on their default keys.
pub const SPECIAL_IDS: &[&str] = &[
    "boss_key",
    "pip",
    "pip_fullscreen",
    "force_close",
    "cycle_profile",
    "search",
    "pause",
    "voice_typing",
    "screenshot",
    "osk",
    "scroll_top",
    "scroll_bottom",
    // PHASE A step 3 (2026-09-19) — Space+← / Space+→ send Win+Shift+←/→,
    // Windows' own "move this window to the other monitor".
    "move_window_left",
    "move_window_right",
];

/// Is `id` one of the fourteen? Pure; the UI and the seed both ask.
pub fn is_special_id(id: &str) -> bool {
    SPECIAL_IDS.contains(&id)
}

/// The default key for each special — EXACTLY the table the hook hard-coded
/// before Phase A (Esc = Boss Key, ` = PiP, Tab = fullscreen PiP, ⌫ = force
/// close, RAlt = cycle profile, `,` = search, `.` = pause, `;` = voice
/// typing, `/` = screenshot, `'` = on-screen keyboard, ↑↑ / ↓↓ = scroll).
/// Keys are the dashboard's key ids (`keyboard-matrix.ts`), the same ids
/// `hook::vk_for_key_id` maps. Space + scroll (opacity) is a gesture, not a
/// key, and is not here.
pub const DEFAULT_SPECIALS: &[(&str, &str)] = &[
    ("esc", "boss_key"),
    ("backtick", "pip"),
    ("tab", "pip_fullscreen"),
    ("backspace", "force_close"),
    ("ralt", "cycle_profile"),
    ("comma", "search"),
    ("period", "pause"),
    ("semicolon", "voice_typing"),
    ("slash", "screenshot"),
    ("quote", "osk"),
    ("up", "scroll_top"),
    ("down", "scroll_bottom"),
    ("left", "move_window_left"),
    ("right", "move_window_right"),
];

/// PHASE A step 3 — the pair the SECOND seeding pass fills into a profile
/// that was already seeded by 1.0.116/117 (`seed_specials`, pass 2).
pub const LATE_SPECIALS: &[(&str, &str)] = &[("left", "move_window_left"), ("right", "move_window_right")];

/// Seed `DEFAULT_SPECIALS` into every profile that has not been seeded yet.
/// Returns `true` when anything changed (the caller writes the file back).
///
/// IDEMPOTENT BY CONSTRUCTION: a profile is seeded ONCE, and the flag is what
/// remembers it — a key the user later REMOVES is simply absent, and the seed
/// never re-adds it because `specials_seeded` is already true. A key that is
/// already present when the seed runs (an old config where the user had bound
/// `esc` through a hand edit, or a Phase A config being re-read) keeps its own
/// binding untouched.
///
/// PASS 2 (PHASE A step 3, 2026-09-19): a profile that 1.0.116/117 already
/// seeded (`specials_seeded == true`) predates `move_window_left` /
/// `move_window_right`. When BOTH `left` and `right` are absent from its
/// `bindings` the pair is added (`LATE_SPECIALS`) and logged once; a profile
/// where the user has bound EITHER key to anything is left alone — that is
/// the only signal there is that the user has been there, and the cost of
/// guessing wrong is overwriting a binding he made. Idempotent: once the
/// pair is in, the "both absent" test is false. Known edge: a user who later
/// removes BOTH arrows gets them back on the next load; removing one keeps
/// both away.
pub fn seed_specials(cfg: &mut AppConfig) -> bool {
    let mut changed = false;
    let mut late: Vec<String> = Vec::new();
    for p in cfg.profiles.iter_mut() {
        if p.specials_seeded {
            if LATE_SPECIALS.iter().all(|(k, _)| !p.bindings.contains_key(*k)) {
                for (key, id) in LATE_SPECIALS {
                    p.bindings.insert(
                        (*key).to_string(),
                        KeyBinding { action: Some(Action::Special { id: (*id).to_string() }), ..Default::default() },
                    );
                }
                late.push(p.name.clone());
                changed = true;
            }
            continue;
        }
        for (key, id) in DEFAULT_SPECIALS {
            if p.bindings.contains_key(*key) {
                continue;
            }
            p.bindings.insert(
                (*key).to_string(),
                KeyBinding {
                    action: Some(Action::Special { id: (*id).to_string() }),
                    ..Default::default()
                },
            );
        }
        p.specials_seeded = true;
        changed = true;
    }
    if !late.is_empty() {
        log::info!(
            "config: seeded Space+Left / Space+Right (move the window to the other screen, Phase A step 3,              1.0.118) into {} already-seeded profile(s) that had neither arrow bound: {}",
            late.len(),
            late.join(", ")
        );
    }
    changed
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

    /// Absolute path to a SPECIFIC browser exe to launch a `web_url` into,
    /// overriding the OS default-browser lookup. `None` = existing behaviour,
    /// unchanged — `run_browser()` asks Windows who owns http/https.
    ///
    /// THE OWNER STATED THIS TWICE: *"make sure the default browser launches
    /// from URL if not explicitly set to specific."* So this field is the ONLY
    /// thing that can divert a URL away from the default browser, and
    /// `smart_cascade` consults `browser_profiles::should_use_specific_browser`
    /// before it chooses a path. See `hard_requirement_tests` in
    /// `browser_profiles.rs` — the guard has its own test because a regression
    /// here is invisible until someone's links start opening in the wrong
    /// browser.
    ///
    /// `#[serde(default)]` is LOAD-BEARING and is written out explicitly rather
    /// than inherited from anything: a plain `Option<T>` with no attribute is a
    /// REQUIRED key to serde, so every config.json on disk today — none of
    /// which has this key — would fail to deserialise and the user would lose
    /// every binding they have. That is not hypothetical; PROBLEM 159 is this
    /// project's own config-corruption incident from exactly this class of
    /// mistake.
    #[serde(default)]
    pub browser_exe: Option<String>,

    /// The Chromium INTERNAL profile folder name (e.g. `"Profile 1"`), passed
    /// as `--profile-directory=`. `None` = launch that browser normally,
    /// whatever its own default/last-used profile is.
    ///
    /// Two different meanings depending on the sibling fields, both deliberate:
    ///   · with `web_url` + `browser_exe` — open that URL in that profile;
    ///   · with `app` (which IS the browser exe) and NO url — "just open
    ///     Brave's Studies profile". That binding still flows through the
    ///     normal focus → minimize → launch cascade; the parameter only
    ///     affects the LAUNCH leg (owner's decision 2026-08-26).
    ///
    /// Explicit `#[serde(default)]` for the same reason as above.
    #[serde(default)]
    pub browser_profile_dir: Option<String>,

    /// The HUMAN-READABLE profile name (e.g. `"ARPON'S STUDIES"`) that
    /// `browser_profile_dir` pointed at when the user picked it.
    ///
    /// Stored rather than looked up. The Guide HUD shows "Brave — Studies"
    /// (owner's decision 2026-08-26) and the HUD is built on a latency-
    /// sensitive path — it must appear inside the user's configured delay, so
    /// re-reading and JSON-parsing the browser's ~96 KB `Local State` on every
    /// Space-hold to translate `"Profile 1"` into a name is the wrong trade.
    /// Storing it also survives the browser being uninstalled, so the key still
    /// reads as something meaningful instead of degrading to a folder name.
    ///
    /// The cost is staleness: renaming the profile inside the browser will not
    /// update this until the user re-picks it. That is the accepted trade.
    ///
    /// Explicit `#[serde(default)]` for the same reason as above.
    #[serde(default)]
    pub browser_profile_name: Option<String>,

    /// PROBLEM 267 — a LINK's icon: the site's favicon as a complete `data:`
    /// URL (`data:image/x-icon;base64,…` or `data:image/png;base64,…`),
    /// **fetched ONCE, at bind time**, by the key editor through
    /// `site_icon::fetch_site_icon` the moment a URL is bound. The
    /// middle-button ring shows it as the tile; the "Choose your eight" picker
    /// shows it beside the row; nothing ever fetches at ring time — a ring that
    /// waited on the network would arrive after the hand had let go.
    ///
    /// `None` = the fetch failed or never ran (a config from before this
    /// field): the ring draws a letter disc, and the NEXT edit of that key
    /// tries once more (the editor re-fetches whenever it commits a URL whose
    /// binding has no icon). Explicit `#[serde(default)]` for the reason
    /// `browser_exe` spells out — a plain `Option` is REQUIRED to serde.
    #[serde(default)]
    pub site_icon: Option<String>,

    /// PHASE A — the key's action when it is not an app or a link. `None` =
    /// the legacy fields above ARE the action; every config written before
    /// 2026-09-18 reads as `None`. Explicit `#[serde(default)]` for the reason
    /// `browser_exe` spells out — a plain `Option` is REQUIRED to serde, and
    /// PROBLEM 159 is what a required key costs.
    #[serde(default)]
    pub action: Option<Action>,
}

impl KeyBinding {
    /// Returns true if this binding has any action defined.
    pub fn is_mapped(&self) -> bool {
        self.action.is_some() || self.app.is_some() || self.web_url.is_some()
    }
}

/// PROBLEM 267 — WHAT a middle-button hold raises. The owner's decision on
/// 2026-09-13: the new cursor-anchored icon ring is a CHOICE, not a
/// replacement, and phase 1 must survive byte-for-byte behind the other value.
///
/// `IconRing` (default) — phase 2: the ring of real icons blooming out of the
/// cursor (`middle_ring.rs`, `guide_hud::show_middle_ring`).
/// `GuideHud` — phase 1 exactly as 1.0.109 ships it: `MiddleButtonDown` is
/// normalised to `SpaceDown` at the top of `engine::dispatch` and the centred
/// Guide HUD comes up. That normalisation line is untouched by this feature.
///
/// Serde default = `IconRing`, so a config predating the field gets the new
/// ring; `first_install_tests` holds BOTH paths to it. Stored as
/// `"icon_ring"` / `"guide_hud"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MiddleRingStyle {
    #[default]
    IconRing,
    GuideHud,
}

/// PROBLEM 267 — HOW MUCH the icon ring shows. `MyEight` (default) is the
/// FAVOURITES scope since round 3: any number the user ticks, 1 to
/// `middle_ring::FAVOURITES_MAX` (`middle_ring_favourites`, or the first
/// six bound letters until they choose) — the variant keeps its name only
/// so the serialised `"my_eight"` is unchanged; every user-facing string
/// says "Favourites". `All`: the favourites first, then every other bound
/// letter and the actionable specials, on as many rings as the layout law
/// needs (`middle_ring::layout_arcs`). Stored as `"my_eight"` / `"all"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MiddleRingScope {
    #[default]
    MyEight,
    All,
}

/// 2026-09-15 — HOW the `All` scope is laid out.
///
/// * `Rings` (default) — the concentric Fibonacci rings this app has always
///   drawn: 5 / 8 / 13 / 21 tiles at 108 / 175 / 283 / 458, each ring
///   staggered from the one inside it by the golden angle.
/// * `Spiral` — one phyllotaxis spiral (`middle_ring::spiral_slots`): tile
///   *i* at the golden angle × *i*, at the radius that keeps the area per
///   tile constant. The same packing a sunflower's seed head uses, which is
///   why it has no rings to align and no gaps to leave.
///
/// Both obey the same containment law (`All` clamps the centre on-screen and
/// warps the cursor), and both are picked by the same nearest-tile test.
/// Stored as `"rings"` / `"spiral"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AllRingLayout {
    #[default]
    Rings,
    Spiral,
}

/// PROBLEM 267 round 3 — `middle_ring_motion` (`bloom` | `ripple`, 1.0.110's
/// follow-up) is GONE: ripple is the only motion. The key is ignored on read
/// (no `deny_unknown_fields`), so a config that still carries it loads
/// unchanged; a save simply stops writing it.

/// PROBLEM 267 — the per-app exception SCOPE. Until 1.0.109 an entry in
/// `excluded_apps` meant one thing: Spaceadom off in that app. Three meanings
/// now, and the old one is the first:
///
/// * `OffEntirely` — Space AND the middle button stand down (the 1.0.79
///   behaviour; every plain-string entry migrates to this).
/// * `SpaceOnly` — Space keeps working; the middle button is handed back to
///   the app. This is what `hook/orbit_apps.rs`'s built-in 3D/CAD/design list
///   means, so a built-in row shows at `SpaceOnly` by default.
/// * `MiddleOnly` — the middle-button ring keeps working; Space passes
///   through untouched (a game that uses Space, say, where the ring is still
///   wanted).
///
/// Consumed in ONE place, `hook::exclusions::resolve_scope`, which turns the
/// user's list plus the built-in list into the two atomics the mouse and
/// keyboard callbacks read. Stored as `"off_entirely"` / `"space_only"` /
/// `"middle_only"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExceptionScope {
    #[default]
    OffEntirely,
    SpaceOnly,
    MiddleOnly,
}

/// PROBLEM 267 — one row of `excluded_apps`: an exe STEM (lowercase, no
/// `.exe`, the same normalisation `exclusions::normalize_stem` applies) and
/// its scope.
///
/// **BACK-COMPAT IS IN `Deserialize`, NOT IN A MIGRATION PASS.** Every config
/// written from 1.0.79 to 1.0.109 stores this field as a plain array of
/// strings (`["photoshop", "figma"]`). `deserialize_app_exception` accepts
/// EITHER a bare string (→ `OffEntirely`, the meaning it always had) OR the
/// object form, so an old file parses without anyone having to run anything,
/// and the first save writes the object form. Serialisation is always the
/// object. `excluded_apps_migration_tests` holds both shapes to it.
///
/// A built-in row the user has NOT changed is never stored here — the UI
/// derives it from `orbit_apps`; only a CHANGED built-in row (e.g. SolidWorks
/// moved to `MiddleOnly`) lands in this list, and moving it back to
/// `SpaceOnly` removes the entry again, so a built-in is never duplicated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppException {
    pub exe: String,
    #[serde(default)]
    pub scope: ExceptionScope,
}

impl AppException {
    pub fn new(exe: impl Into<String>, scope: ExceptionScope) -> Self {
        AppException { exe: exe.into(), scope }
    }
}

impl<'de> Deserialize<'de> for AppException {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Untagged: a JSON string is the 1.0.79–1.0.109 shape, an object is
        // the 1.0.110+ shape. Anything else is an error, as it always was.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Legacy(String),
            Full {
                exe: String,
                #[serde(default)]
                scope: ExceptionScope,
            },
        }
        Ok(match Raw::deserialize(d)? {
            Raw::Legacy(exe) => AppException { exe, scope: ExceptionScope::OffEntirely },
            Raw::Full { exe, scope } => AppException { exe, scope },
        })
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

/// The upgrade path for the browser-profile fields (2026-08-26).
///
/// THIS IS THE ACTUAL SAFETY NET, not a formality. Every `config.json` on disk
/// today was written before `browser_exe` / `browser_profile_dir` /
/// `browser_profile_name` existed. A plain `Option<T>` field is REQUIRED to
/// serde — absence is an error, not `None` — so shipping these three without an
/// explicit `#[serde(default)]` on EACH one would make every existing config
/// fail to load, and this project has already lived through that exact class of
/// incident once (PROBLEM 159).
///
/// The fixture is written out as a LITERAL old-shaped JSON string rather than
/// built by serialising a current `KeyBinding` and deleting keys. Both styles
/// appear in this file and each is right for its job: the `AppConfig` tests
/// above must round-trip a struct with 30-odd fields, half of which have no
/// serde default, so building-then-deleting is the only maintainable option
/// there. `KeyBinding` has four old fields, and here the literal is the POINT —
/// it is a byte-accurate sample of what is actually sitting in the owner's
/// `%APPDATA%\Spaceadom\config.json` right now, and it cannot silently start
/// including the new keys the way a serialise-then-delete fixture could if
/// someone later removes a `remove()` line.
#[cfg(test)]
mod key_binding_upgrade_tests {
    use super::*;

    /// A binding exactly as versions up to 1.0.84 wrote it: four keys, and the
    /// three new ones ENTIRELY ABSENT from the JSON.
    #[test]
    fn a_binding_written_before_the_browser_fields_still_parses() {
        let old = r#"{
            "app": "C:\\Program Files\\BraveSoftware\\Brave-Browser\\Application\\brave.exe",
            "web_url": null,
            "label": "Brave",
            "icon_override": null
        }"#;

        let b: KeyBinding = serde_json::from_str(old).expect(
            "a KeyBinding written before the browser-profile fields MUST still \
             deserialise — without an explicit #[serde(default)] on each new \
             Option field this fails and every existing user loses every binding",
        );

        // The old fields survive untouched...
        assert_eq!(b.label.as_deref(), Some("Brave"));
        assert!(b.app.is_some(), "the old app path must round-trip");
        assert!(b.web_url.is_none());
        assert!(b.icon_override.is_none());

        // ...and the new ones read as "never set", which is what routes this
        // binding down the UNCHANGED default-browser path.
        assert!(
            b.browser_exe.is_none(),
            "an absent browser_exe must read as None, never as an error"
        );
        assert!(
            b.browser_profile_dir.is_none(),
            "an absent browser_profile_dir must read as None"
        );
        assert!(
            b.browser_profile_name.is_none(),
            "an absent browser_profile_name must read as None"
        );
    }

    /// The same proof one level up: a whole PROFILE full of old bindings, which
    /// is the shape the loader actually meets. A field attribute can be right
    /// on the struct and still be defeated by a container that fails first, so
    /// the nesting is exercised rather than assumed.
    #[test]
    fn a_whole_profile_of_old_bindings_still_parses() {
        let old = r#"{
            "name": "Founders",
            "bindings": {
                "b": { "app": "brave.exe", "web_url": null, "label": "Brave", "icon_override": null },
                "g": { "app": null, "web_url": "https://github.com", "label": "GitHub", "icon_override": null }
            }
        }"#;

        let p: Profile = serde_json::from_str(old)
            .expect("a profile written before the browser-profile fields must still load");
        assert_eq!(p.bindings.len(), 2);
        for (key, b) in &p.bindings {
            assert!(b.browser_exe.is_none(), "key {key}: browser_exe must default to None");
            assert!(b.browser_profile_dir.is_none(), "key {key}: browser_profile_dir must default to None");
            assert!(b.browser_profile_name.is_none(), "key {key}: browser_profile_name must default to None");
        }
    }

    /// The final link: run THIS MACHINE'S REAL `config.json` through the real
    /// parser. The literal fixture above is a model of that file; this checks
    /// the model against the thing it models, which is the only way to know the
    /// fixture has not quietly gone stale.
    ///
    /// Ignored by default — it depends on a file outside the repo, so it is a
    /// diagnostic rather than a gate (same reasoning as `icon_smoke`).
    /// Run: `cargo test --lib -- --ignored --nocapture the_real_config`
    #[test]
    #[ignore]
    fn the_real_config_on_this_machine_still_loads() {
        let Some(appdata) = std::env::var_os("APPDATA") else {
            println!("APPDATA unset — skipping");
            return;
        };
        let path = std::path::PathBuf::from(appdata).join(r"Spaceadom\config.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            println!("no config at {} — skipping", path.display());
            return;
        };

        let cfg: AppConfig = serde_json::from_str(&text).unwrap_or_else(|e| {
            panic!(
                "THE OWNER'S REAL CONFIG NO LONGER PARSES: {e}\n  {}\n\
                 This is the PROBLEM 159 failure mode. Do not ship.",
                path.display()
            )
        });

        let bindings: usize = cfg.profiles.iter().map(|p| p.bindings.len()).sum();
        println!(
            "parsed {} ({} profiles, {} bindings, active '{}')",
            path.display(),
            cfg.profiles.len(),
            bindings,
            cfg.active_profile
        );
        assert!(bindings > 0, "the real config should contain bindings");
        for p in &cfg.profiles {
            for (k, b) in &p.bindings {
                assert!(
                    b.browser_exe.is_none(),
                    "{}/{k}: a config written before this feature must read browser_exe as None",
                    p.name
                );
            }
        }
    }

    /// A binding that DOES carry the new fields must round-trip them, or the
    /// user's choice would be silently dropped on the next save — a failure
    /// with no symptom until the key opens the wrong profile.
    #[test]
    fn the_new_fields_round_trip_when_they_are_set() {
        let b = KeyBinding {
            app: None,
            web_url: Some("https://example.com".into()),
            label: Some("Example".into()),
            icon_override: None,
            browser_exe: Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe".into()),
            browser_profile_dir: Some("Profile 1".into()),
            browser_profile_name: Some("Work".into()),
            site_icon: None,
            action: None,
        };
        let json = serde_json::to_string(&b).expect("serialise");
        let back: KeyBinding = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back.browser_exe, b.browser_exe);
        assert_eq!(back.browser_profile_dir.as_deref(), Some("Profile 1"));
        assert_eq!(back.browser_profile_name.as_deref(), Some("Work"));
    }

    /// Rename-profile feature: bindings live INSIDE the `Profile` object
    /// (`commands::apply_profile_rename` only ever touches the `name` field),
    /// so the thing that could actually go wrong is the serde round-trip a
    /// save/load cycle puts every profile through. This proves that changing
    /// `name` — exactly what a rename does to the in-memory struct before
    /// `config::save` writes it out — carries every binding across two full
    /// JSON round-trips unchanged.
    #[test]
    fn a_profile_round_trip_carries_every_binding_across_a_rename() {
        let mut bindings = BindingMap::new();
        bindings.insert(
            "a".to_string(),
            KeyBinding { app: Some("brave.exe".into()), ..Default::default() },
        );
        bindings.insert(
            "g".to_string(),
            KeyBinding { web_url: Some("https://github.com".into()), ..Default::default() },
        );
        let profile = Profile { name: "Old Name".into(), bindings, emoji: None, specials_seeded: false };

        // Round-trip once, as an ordinary save/load would.
        let json = serde_json::to_string(&profile).expect("serialise");
        let mut reloaded: Profile = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(reloaded.bindings.len(), 2, "first round-trip must keep both bindings");

        // Now rename in place — the exact mutation `apply_profile_rename`
        // performs — and round-trip again, as the SAVE the rename triggers
        // would.
        reloaded.name = "New Name".into();
        let json2 = serde_json::to_string(&reloaded).expect("serialise after rename");
        let renamed: Profile = serde_json::from_str(&json2).expect("deserialise after rename");

        assert_eq!(renamed.name, "New Name");
        assert_eq!(renamed.bindings.len(), 2, "a rename must not lose a binding");
        assert_eq!(
            renamed.bindings.get("a").and_then(|b| b.app.as_deref()),
            Some("brave.exe")
        );
        assert_eq!(
            renamed.bindings.get("g").and_then(|b| b.web_url.as_deref()),
            Some("https://github.com")
        );
    }
}

/// `Profile::emoji` — the validator and the two paths a config travels.
///
/// This is here rather than in `commands.rs` because the rule is a SCHEMA
/// rule: what may be stored, not what a particular command does with it.
#[cfg(test)]
mod profile_emoji_tests {
    use super::*;

    /// The whole reason `cluster_count` exists. Every one of these is a single
    /// glyph on screen and NONE of them is a single `char` — a naive
    /// `chars().count() == 1` (or any byte budget) rejects the lot, which is
    /// how an emoji picker ends up accepting only the ASCII-adjacent ones.
    #[test]
    fn multi_codepoint_emoji_are_one_cluster() {
        // The case named in the brief: a ZWJ family. Five code points,
        // 18 bytes, one glyph.
        assert_eq!("👨‍👩‍👧".chars().count(), 5, "fixture check: this IS multi-codepoint");
        assert_eq!("👨‍👩‍👧".len(), 18, "fixture check: bytes are not the unit either");
        assert!(emoji_is_valid("👨‍👩‍👧"), "a ZWJ family must be accepted");

        assert!(emoji_is_valid("👍🏽"), "an emoji with a skin-tone modifier is one cluster");
        assert!(emoji_is_valid("❤️"), "a variation selector does not start a cluster");
        assert!(emoji_is_valid("🇧🇩"), "a flag is a REGIONAL INDICATOR PAIR, still one");
        assert!(emoji_is_valid("🏴󠁧󠁢󠁥󠁮󠁧󠁿"), "a tag-sequence flag is one cluster");
        assert!(emoji_is_valid("1️⃣"), "a keycap is one cluster");
        assert!(emoji_is_valid("🚀"), "and the simple case still works");
        assert!(emoji_is_valid("é"), "a letter is a legitimate answer too");
    }

    #[test]
    fn two_glyphs_a_blank_or_a_control_character_are_refused() {
        assert!(!emoji_is_valid("AB"), "two clusters is not one");
        assert!(!emoji_is_valid("🚀🚀"), "nor is two emoji");
        assert!(
            !emoji_is_valid("🇧🇩🇧🇩"),
            "four regional indicators are TWO flags — the pair must close"
        );
        assert!(!emoji_is_valid(""), "empty means 'no emoji', which is None, not \"\"");
        assert!(!emoji_is_valid("🚀 "), "a trailing space would break the row's layout");
        assert!(!emoji_is_valid("a\nb"), "a control character must never reach the disc");
        // The cap is a backstop against a pathological single cluster, not the
        // main rule: 17 combining marks on one base is still one cluster.
        let stacked: String = std::iter::once('a')
            .chain(std::iter::repeat('\u{0301}').take(EMOJI_MAX_CHARS))
            .collect();
        assert_eq!(cluster_count(&stacked), 1, "it really is one cluster…");
        assert!(!emoji_is_valid(&stacked), "…and the length cap is what refuses it");
    }

    /// The round trip, both directions, because this field is written to
    /// `config.json` and read back on every launch.
    #[test]
    fn an_emoji_survives_a_save_and_load_and_absence_reads_as_none() {
        let p = Profile {
            name: "Founders".into(),
            bindings: BindingMap::new(),
            emoji: Some("👨‍👩‍👧".into()),
            specials_seeded: false,
        };
        let json = serde_json::to_string(&p).expect("serialise");
        let back: Profile = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(
            back.emoji.as_deref(),
            Some("👨‍👩‍👧"),
            "a ZWJ sequence must come back byte-identical, not re-encoded"
        );

        // THE PATH EVERY EXISTING USER TRAVELS: their config.json has no
        // `emoji` key at all. It must parse, and it must read as None — the
        // value that makes every surface keep its current look.
        let mut v = serde_json::to_value(&p).expect("serialise");
        v.as_object_mut().expect("object").remove("emoji");
        let old: Profile = serde_json::from_value(v)
            .expect("a profile written before this field existed must still parse");
        assert!(old.emoji.is_none(), "an absent emoji must read as None");
    }
}

#[cfg(test)]
mod first_install_tests {
    use super::*;

    /// PROBLEM 157 — the owner has now asked for these three defaults twice
    /// ("when someone installs it, it should install with fun mode off and
    /// show me around off… and it should open up the earthy theme first"), and
    /// they are the first thing a stranger sees. They are also easy to flip by
    /// accident: three bools/strings among thirty fields, changed by anyone
    /// adding a feature that "should obviously be on".
    ///
    /// **UPDATED 2026-09-05 for feature 2 ("Follow system theme"):** a new
    /// install's `theme` is now `"auto"`, not `"earthy"` — the owner's
    /// decision for THIS pass, superseding the PROBLEM 157 sentence quoted
    /// above without erasing why that sentence existed (it is still true that
    /// a stranger's very first look must not be a random coin flip of a
    /// palette; "auto" answers that by following the one preference — light
    /// or dark — the stranger has ALREADY told Windows). `resolveTheme()` in
    /// `main.ts` is what turns "auto" into "earthy" for a light-mode user,
    /// which this Rust-only test cannot exercise — it can only prove the
    /// field Rust hands the frontend is what feature 2 decided it should be.
    ///
    /// Both paths are checked, because they can drift APART: `Default` is what
    /// a fresh install writes, and the serde defaults are what an OLD config
    /// missing the field falls back to (still `""`, migrated by `config/mod.rs`
    /// — untouched by this pass; see that field's doc comment). A mismatch
    /// between the two would mean the same user gets a different app
    /// depending on when they installed.
    #[test]
    fn first_install_is_quiet_and_earthy() {
        let d = AppConfig::default();
        assert!(!d.fun_mode, "fun_mode must be OFF at first install");
        assert!(!d.show_me_around, "show_me_around must be OFF at first install");
        assert!(!d.hide_keyboard, "the keyboard must be visible at first install");
        assert_eq!(d.theme, "auto", "first install follows the system theme (feature 2)");
        assert!(
            !d.dark_mode,
            "dark_mode itself still starts false — it is resolved live from \"auto\" by the \
             frontend's matchMedia check, which this Rust-only default cannot perform",
        );
        // PROBLEM 174 — the owner's testers found the ring→toast flight
        // "disturbing, too much time consuming". Off is where everyone lands,
        // and both paths must agree on that.
        assert!(
            !d.hud_toast_flight,
            "the guide-to-toast flight must be OFF at first install"
        );
        // PROBLEM 209 — the owner reversed PROBLEM 206's default on
        // 2026-08-27, knowingly overriding the convention the assertion
        // directly above enforces for `hud_toast_flight`. Point-to-launch is
        // ON at first install: he wants it met, not found. If this assertion
        // ever fails because someone "restored the convention", that is a
        // decision being reversed and it needs the owner, not a patch.
        assert!(
            d.pointer_hud_activation,
            "pointer HUD activation must be ON at first install (owner's decision, 2026-08-27)"
        );
        // PROBLEM 263 — the middle-button ring trigger, ON at first install by
        // the owner's decision on 2026-09-08. The THIRD assertion here to
        // knowingly override the new-behaviour-defaults-off convention the
        // `hud_toast_flight` assertion above enforces, and it is deliberate for
        // the same reason as `pointer_hud_activation`: he wants it met, not
        // found. If this ever fails because someone "restored the convention",
        // that is a decision being reversed and it needs the owner.
        //
        // NOTE WHAT THIS DOES *NOT* SAY. It is not an assertion that the middle
        // button is swallowed everywhere on a fresh install — `orbit_apps.rs`'s
        // built-in list stands the trigger down inside every 3D, CAD and design
        // program, and `middle_button_down_accepted` refuses outright until the
        // exclusion watcher has completed one probe. This flag is the SWITCH,
        // not the behaviour.
        assert!(
            d.middle_button_ring,
            "holding the middle mouse button must raise the ring at first install \
             (owner's decision, 2026-09-08 — PROBLEM 263)"
        );
        // PROBLEM 267 — and WHAT it raises is the new icon ring, by the
        // owner's decision on 2026-09-13; phase 1 is the other value, not the
        // default. The scope starts at the eight and the favourites start
        // EMPTY (= "the first eight bound letters", decided at ring time).
        assert_eq!(
            d.middle_ring_style,
            MiddleRingStyle::IconRing,
            "a fresh install's middle button raises the cursor-anchored icon ring (PROBLEM 267)"
        );
        assert_eq!(d.middle_ring_scope, MiddleRingScope::MyEight, "the eight, not all of them");
        // 2026-09-15 — Spiral is a CHOICE. A fresh install draws the rings
        // it always has, so nothing an existing user sees can change until
        // they flip the pill themselves.
        assert_eq!(d.all_ring_layout, AllRingLayout::Rings, "Rings, never Spiral, by default");
        assert!(d.middle_ring_favourites.is_empty(), "favourites are computed lazily until chosen");
        // PROBLEM 209 — the specials ring has been drawn since the HUD
        // existed; making it optional must not change what a first install
        // looks like.
        assert!(
            d.hud_show_specials,
            "the HUD's specials ring must be SHOWN at first install"
        );
        // The band count and the specials ring are ONE system (see the field's
        // comment). "auto" is the only value that reproduces every previous
        // build's behaviour, so a first install must land there — and it must
        // not land on "one" or "two", either of which would silently impose a
        // layout on a user who never asked for one.
        assert_eq!(
            d.hud_band_count, "auto",
            "the HUD band count must be AUTO at first install"
        );
        // 2026-08-27 - the owner asked for the new Magnetic Sector ring to be
        // what a fresh install SEES, with the toggle as the way back to the
        // 1.0.88 ring. That knowingly overrides the convention the
        // `hud_toast_flight` assertion above enforces, exactly as
        // `pointer_hud_activation` does. If this assertion ever fails because
        // someone "restored the convention", that is a decision being
        // reversed and it needs the owner, not a patch.
        assert!(
            d.hud_magnetic_layout,
            "the new ring layout must be ON at first install (owner's decision, 2026-08-27)"
        );
        // 1.0.96 — `Profile::emoji`. A shipped profile with an emoji nobody
        // chose is the same failure as fun_mode being on: a stranger's first
        // screen, decided by whoever added a feature rather than by the owner.
        // The rule the three readers depend on ("None keeps the look this app
        // has always had") only has a subject if a fresh install produces None.
        assert!(
            d.profiles.iter().all(|p| p.emoji.is_none()),
            "no profile may ship WITH an emoji — the disc must open on its initial letter"
        );
        // PROBLEM 237 — the picker pre-warm is ON at first install: the owner
        // asked for the picker to open as fast as Raycast, and a fresh install
        // is exactly the machine with no disk cache yet.
        assert!(
            d.warm_picker_at_startup,
            "warm_picker_at_startup must be ON at first install (PROBLEM 237)"
        );
        // PROBLEM 242 — the whole first-run tour hangs off this one bool being
        // FALSE on a fresh install. If a later change ever gives it a
        // `default_true` (by copying the field above it, which is exactly how
        // this class of mistake happens), the walkthrough silently stops
        // existing for every new user and nothing else breaks — no error, no
        // log line, no failing test but this one.
        assert!(
            !d.tour_done,
            "tour_done must be FALSE at first install — it is what makes the \
             guided first-bind tour appear at all (PROBLEM 242)"
        );
    }

    #[test]
    fn a_config_missing_the_new_fields_also_lands_quiet_and_earthy() {
        // Build the fixture by DELETING the three fields from a current config
        // rather than hand-writing a 1.0.40 one: a literal would need every
        // field that has no serde default (rollover_ms and friends), and would
        // rot the moment someone adds another. This stays honest for free.
        let mut v = serde_json::to_value(AppConfig::default()).expect("serialise");
        let obj = v.as_object_mut().expect("object");
        obj.remove("fun_mode");
        obj.remove("show_me_around");
        obj.remove("theme");
        obj.remove("hud_toast_flight");
        obj.remove("pointer_hud_activation");
        obj.remove("middle_button_ring");
        obj.remove("middle_ring_style");
        obj.remove("middle_ring_scope");
        obj.remove("all_ring_layout");
        obj.remove("middle_ring_favourites");
        obj.remove("hud_show_specials");
        obj.remove("hud_band_count");
        obj.remove("hud_magnetic_layout");
        obj.remove("warm_picker_at_startup");
        obj.remove("tour_done");
        // 1.0.96 — `emoji` is on PROFILE, not on AppConfig, so removing it here
        // means walking into the profiles array. That is the whole point: this
        // is the only test that exercises a NESTED absent field, and a nested
        // `Option` without `#[serde(default)]` fails the WHOLE parse rather
        // than defaulting — the config would not load at all, for everybody,
        // on the first run of 1.0.96.
        for p in obj
            .get_mut("profiles")
            .and_then(|p| p.as_array_mut())
            .expect("a config always has profiles")
        {
            p.as_object_mut().expect("a profile is an object").remove("emoji");
        }
        let c: AppConfig = serde_json::from_value(v).expect("a config without the new fields must still parse");
        assert!(
            c.profiles.iter().all(|p| p.emoji.is_none()),
            "a profile written before the emoji field must read as None, not fail to parse"
        );
        assert!(!c.fun_mode, "a missing fun_mode must read as OFF");
        assert!(!c.show_me_around, "a missing show_me_around must read as OFF");
        // theme's serde default is deliberately EMPTY so migration can tell
        // "never set" from "set to earthy" — see config/mod.rs.
        assert_eq!(c.theme, "", "theme's absence must stay distinguishable");
        // The UPGRADE path matters more than the fresh-install one here: every
        // existing user has a config without this key, and they are exactly the
        // people who asked for the motion to stop.
        assert!(
            !c.hud_toast_flight,
            "a config predating this field must read as OFF, not ON"
        );
        // PROBLEM 209 — THE FIELD-REMOVAL PATH IS WHERE THE DEFAULT FLIP
        // ACTUALLY LANDS. Every config on disk today either predates
        // pointer_hud_activation or was written with it false-by-default;
        // `Default` alone would only reach a brand-new install, i.e. nobody.
        // The serde attribute is what carries the owner's decision to the
        // machines that already run the app, so an ABSENT field must read ON.
        // (A config that says `false` explicitly still reads OFF — that is a
        // user's own choice and serde never overrides it.)
        assert!(
            c.pointer_hud_activation,
            "a config predating pointer_hud_activation must now read as ON \
             (owner's decision, 2026-08-27) — this is the path the flip travels"
        );
        // PROBLEM 263 — AND THIS IS THE PATH THE MIDDLE-BUTTON DEFAULT ACTUALLY
        // TRAVELS, for the reason the `pointer_hud_activation` block directly
        // above spells out at length: EVERY config on disk today predates this
        // key, so `Default` alone would deliver the feature to nobody. A bool's
        // `Default` is `false`, so a bare `#[serde(default)]` here would ship a
        // feature that is on in the code, off for every existing user, and
        // shown as ON by a Settings row reading `!== false` — wrong in three
        // places at once, and silent in all three.
        assert!(
            c.middle_button_ring,
            "a config predating middle_button_ring must read as ON — this is the path the \
             owner's 2026-09-08 decision actually travels (PROBLEM 263)"
        );
        // PROBLEM 267 — every config on disk predates `middle_ring_style`, so
        // this is the path the owner's 2026-09-13 default travels: an ABSENT
        // field is the NEW ring. (A file that says "guide_hud" explicitly is a
        // user's own choice and reads as such — see the migration tests.)
        assert_eq!(
            c.middle_ring_style,
            MiddleRingStyle::IconRing,
            "a config predating middle_ring_style must read as the icon ring (PROBLEM 267)"
        );
        assert_eq!(c.middle_ring_scope, MiddleRingScope::MyEight);
        assert_eq!(
            c.all_ring_layout,
            AllRingLayout::Rings,
            "a config predating all_ring_layout must read as the rings (2026-09-15)"
        );
        assert!(c.middle_ring_favourites.is_empty());
        // PROBLEM 209 — and the opposite direction of the same rule: the
        // specials ring has always been drawn, so a config that never heard
        // of the setting must keep drawing it.
        assert!(
            c.hud_show_specials,
            "a config predating hud_show_specials must read as ON — the ring \
             has always been there and this only makes it optional"
        );
        // THE FIELD-REMOVAL PATH IS THE ONLY ONE ANY EXISTING USER TRAVELS —
        // the same lesson `pointer_hud_activation` records directly above.
        // Every config on disk today predates this key, and a `String`'s
        // `Default` is the EMPTY string, so a bare `#[serde(default)]` would
        // hand every one of them `""`. `""` is not one of the three values, so
        // the reading side would have to guess, and a reader that guessed
        // wrong would re-lay-out the HUD for people who never touched a
        // setting. The named default is what stops that.
        assert_eq!(
            c.hud_band_count, "auto",
            "a config predating hud_band_count must read as \"auto\", not \"\" \
             — a bare #[serde(default)] gives a String the empty string"
        );
        // AND THIS IS THE PATH THE LAYOUT DEFAULT ACTUALLY TRAVELS. A default
        // only ever reaches users whose file PREDATES the field: every config
        // on disk today was written before 1.0.89, so `Default` alone would
        // hand the new ring to nobody. A bool's `Default` is `false`, so a
        // bare `#[serde(default)]` here would silently give every existing
        // user the classic layout while the switch in Settings showed it on.
        // (A config that says `false` explicitly still reads OFF - that is a
        // user's own choice and serde never overrides it.)
        assert!(
            c.hud_magnetic_layout,
            "a config predating hud_magnetic_layout must read as ON - this is the path the owner's 2026-08-27 default actually travels"
        );
        // PROBLEM 237 — same path, same reason: every config on disk predates
        // this key, and the pre-warm is what makes the first picker open of a
        // session instant, so an ABSENT field must read ON.
        assert!(
            c.warm_picker_at_startup,
            "a config predating warm_picker_at_startup must read as ON (PROBLEM 237)"
        );
        // PROBLEM 242 — and the OPPOSITE direction of the same rule, which is
        // why it sits directly under the three assertions that go the other
        // way. Everyone whose config predates `tour_done` is by definition
        // someone the tour has never been offered to, so an ABSENT field must
        // read FALSE. `default_true` here would suppress the walkthrough for
        // exactly the people it was built for.
        assert!(
            !c.tour_done,
            "a config predating tour_done must read as FALSE — an absent field \
             means the tour has never been seen, so it must still be offered"
        );
    }

    /// PROBLEM 245 — `auto_update` is the third default-TRUE bool, and the one
    /// whose silent inversion would be the least visible of all: an app that
    /// simply never updates again looks exactly like an app with no updates
    /// to offer. Both paths, and an explicit `false` must be honoured — it
    /// is the only way a user can pin a version.
    #[test]
    fn auto_update_defaults_to_true_on_both_paths_and_honours_an_explicit_false() {
        assert!(AppConfig::default().auto_update, "a fresh install must self-update");
        let mut v = serde_json::to_value(AppConfig::default()).expect("serialise");
        v.as_object_mut().expect("object").remove("auto_update");
        let c: AppConfig = serde_json::from_value(v.clone())
            .expect("a config without auto_update must parse");
        assert!(c.auto_update, "a config predating auto_update must read as ON");
        v.as_object_mut().expect("object").insert("auto_update".into(), false.into());
        let c: AppConfig = serde_json::from_value(v).expect("parse");
        assert!(!c.auto_update, "an explicit false is the escape hatch and must survive");
    }

    /// PROBLEM 195 — `send_logs` is the one bool in this struct whose default
    /// is TRUE, and a bare `#[serde(default)]` would make it false without
    /// changing a single visible thing: the app would simply stop reporting
    /// crashes, for everybody, silently, forever. That is a failure with no
    /// symptom, which is exactly the kind this project writes tests for.
    ///
    /// Both paths again, and here the UPGRADE path is the one that matters:
    /// every config on disk today was written before this field existed.
    /// 2026-08-27 — the new Magnetic Sector ring layout. Its own test, beside
    /// `send_logs`'s, because it is the second field in this struct whose
    /// default is TRUE for a reason that is NOT this codebase's convention,
    /// and because the two assertions inside the shared first-install tests
    /// above are easy to lose in a merge.
    ///
    /// **A DEFAULT ONLY EVER REACHES USERS WHOSE FILE PREDATES THE FIELD.**
    /// That is why the second half of this test is the important one: every
    /// config on disk today was written before 1.0.89, so `Default` alone
    /// governs nobody but a brand-new install. A bool's `Default` is `false`,
    /// so a bare `#[serde(default)]` would hand the CLASSIC ring to every
    /// existing user while the switch in Settings, which reads `!== false`,
    /// showed the new one selected — the app and its own UI disagreeing, with
    /// no symptom the user could report.
    #[test]
    fn hud_magnetic_layout_defaults_to_true_on_both_paths() {
        assert!(
            AppConfig::default().hud_magnetic_layout,
            "a fresh install must open on the NEW ring layout (owner's decision, 2026-08-27)"
        );

        let mut v = serde_json::to_value(AppConfig::default()).expect("serialise");
        v.as_object_mut().expect("object").remove("hud_magnetic_layout");
        let c: AppConfig = serde_json::from_value(v)
            .expect("a config without hud_magnetic_layout must parse");
        assert!(
            c.hud_magnetic_layout,
            "a config predating hud_magnetic_layout must read as ON - a bare              #[serde(default)] gives false for a bool and would deliver the              new layout to nobody who already runs the app"
        );

        // And the other direction: an EXPLICIT false is the user's own choice,
        // and serde must never override it with the default.
        let mut v = serde_json::to_value(AppConfig::default()).expect("serialise");
        v.as_object_mut().expect("object")
            .insert("hud_magnetic_layout".into(), serde_json::Value::Bool(false));
        let c: AppConfig = serde_json::from_value(v).expect("parse");
        assert!(
            !c.hud_magnetic_layout,
            "an explicit false must stay false - that is the escape hatch working"
        );
    }

    #[test]
    fn send_logs_defaults_to_true_on_both_paths() {
        assert!(
            AppConfig::default().send_logs,
            "a fresh install must have crash reporting ON — the opt-out is the switch"
        );

        let mut v = serde_json::to_value(AppConfig::default()).expect("serialise");
        v.as_object_mut().expect("object").remove("send_logs");
        let c: AppConfig = serde_json::from_value(v).expect("a config without send_logs must parse");
        assert!(
            c.send_logs,
            "a config predating send_logs must read as TRUE — a bare #[serde(default)] \
             gives false for a bool and would silently opt every existing user out"
        );
    }

    // ── PROBLEM 250 follow-up (LIVE TEST 2026-09-05) — determinism ────────────
    //
    // The defect these pin: `profiles[].bindings` was a `HashMap`, so
    // `serde_json` wrote its keys in a different order on every save and two
    // saves of IDENTICAL content never produced identical bytes. Measured that
    // day on the owner's real config — 77,912 bytes both times, sha256
    // 9ADE0FDE… before and 4CE86B44… after, no semantic difference anywhere,
    // first differing byte at 491 ("z" first vs "h" first inside a bindings
    // map). See `BindingMap`.

    /// A fresh install serialises to the SAME BYTES every time, in the same
    /// process and across processes.
    ///
    /// The in-process half would pass for a `HashMap` too — one `RandomState`
    /// per map instance means one order per map — so the assertion that
    /// actually catches the bug is the second one: two INDEPENDENTLY BUILT
    /// maps holding the same pairs. That is what two launches of the app do,
    /// and it is what was failing.
    #[test]
    fn a_fresh_config_serialises_to_identical_bytes_every_time() {
        let a = serde_json::to_string_pretty(&AppConfig::default()).expect("serialise");
        let b = serde_json::to_string_pretty(&AppConfig::default()).expect("serialise");
        assert_eq!(a, b, "two default configs must serialise byte-identically");

        // Independently built maps, inserted in DELIBERATELY OPPOSITE orders —
        // the shape a HashMap cannot survive.
        let mk = |reverse: bool| {
            let mut keys: Vec<&str> = "abcdefghijklmnopqrstuvwxyz".split("").filter(|s| !s.is_empty()).collect();
            if reverse {
                keys.reverse();
            }
            let mut m = BindingMap::new();
            for k in keys {
                m.insert(
                    k.to_string(),
                    KeyBinding { label: Some(k.to_uppercase()), ..Default::default() },
                );
            }
            Profile { name: "Founders".into(), bindings: m, emoji: None, specials_seeded: false }
        };
        assert_eq!(
            serde_json::to_string(&mk(false)).expect("serialise"),
            serde_json::to_string(&mk(true)).expect("serialise"),
            "insertion order must not reach the file — this is the 2026-09-05 defect"
        );

        // PHASE A — and a SEEDED fresh install is just as stable: two
        // independently built default configs, seeded separately, are the
        // same bytes, and the seeded bytes carry every special exactly as
        // `DEFAULT_SPECIALS` lists it.
        let seeded = || {
            let mut c = AppConfig::default();
            c.profiles = crate::config::defaults::generate();
            assert!(seed_specials(&mut c));
            serde_json::to_string_pretty(&c).expect("serialise")
        };
        let (x, y) = (seeded(), seeded());
        assert_eq!(x, y, "a seeded fresh config must serialise byte-identically");
        assert!(x.contains(r#""kind": "special""#), "the seed is on disk as tagged actions");
        assert!(x.contains(r#""specials_seeded": true"#));
    }

    /// The round trip a running app performs on every save: load what is on
    /// disk, hand it back, write it out. The bytes must not move.
    ///
    /// Runs over the REAL seed profiles (26 bindings each) plus a populated
    /// `special_keys` map, because `special_keys` had the identical defect and
    /// is empty in `AppConfig::default()` — a test built only from defaults
    /// would have passed while it was still a `HashMap`.
    #[test]
    fn deserialising_and_reserialising_a_config_is_byte_identical() {
        let mut cfg = AppConfig::default();
        cfg.profiles = vec![
            crate::config::defaults::founders_profile(),
            crate::config::defaults::gamers_profile(),
            crate::config::defaults::professionals_profile(),
        ];
        for (k, url) in [
            ("f1", "https://one.example"),
            ("enter", "https://two.example"),
            ("up", "https://three.example"),
            ("esc", "https://four.example"),
            ("tab", "https://five.example"),
        ] {
            cfg.special_keys.insert(
                k.to_string(),
                KeyBinding { web_url: Some(url.into()), ..Default::default() },
            );
        }

        let first = serde_json::to_string_pretty(&cfg).expect("serialise");
        let reloaded: AppConfig = serde_json::from_str(&first).expect("parse what we just wrote");
        let second = serde_json::to_string_pretty(&reloaded).expect("re-serialise");
        assert_eq!(
            first, second,
            "a load→save round trip must not change one byte; a config hash that moves \
             on its own is not a change detector"
        );

        // Third pass, from a fresh parse of the SECOND string: proves the
        // fixed point is the file, not one lucky pair of runs.
        let again: AppConfig = serde_json::from_str(&second).expect("parse");
        assert_eq!(second, serde_json::to_string_pretty(&again).expect("serialise"));

        // And the order really is sorted, not merely stable — that is what
        // makes a hand-read diff of two configs legible.
        let founders = &cfg.profiles[0];
        let keys: Vec<&str> = founders.bindings.keys().map(String::as_str).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted, "bindings must serialise in sorted key order");
    }

    /// A `config.json` written before this change — keys in whatever order the
    /// old `HashMap` happened to produce — must still parse, and must come out
    /// sorted afterwards. Every existing user's file is exactly this.
    #[test]
    fn a_config_written_in_the_old_random_order_still_parses_and_is_normalised() {
        let json = r#"{
            "spaceadom_profile": 1,
            "name": "Founders",
            "bindings": {
                "z": { "app": "Zoom.exe" },
                "a": { "web_url": "https://gemini.google.com" },
                "m": { "web_url": "https://cinemaos.live/" }
            }
        }"#;
        let p: ProfileExport = serde_json::from_str(json).expect("an old-order file must parse");
        assert_eq!(p.bindings.len(), 3);
        assert_eq!(
            p.bindings.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["a", "m", "z"],
            "whatever order it arrives in, it leaves sorted"
        );
    }

}

/// PROBLEM 267 — the App-exceptions SCOPE migration and the two new enums'
/// wire forms. The class of bug these guard is the one `key_binding_upgrade_tests`
/// already records: a field whose shape changes must read every shape that is
/// already on disk, or the user loses the whole config (PROBLEM 159).
#[cfg(test)]
mod excluded_apps_migration_tests {
    use super::*;

    /// A 1.0.79–1.0.109 file: `excluded_apps` is a plain array of strings.
    /// Each must read as `OffEntirely` — the ONE meaning the field had.
    #[test]
    fn a_plain_string_list_reads_as_off_entirely() {
        let mut v = serde_json::to_value(AppConfig::default()).expect("serialise");
        v.as_object_mut()
            .unwrap()
            .insert("excluded_apps".into(), serde_json::json!(["photoshop", "Figma.exe"]));
        let c: AppConfig = serde_json::from_value(v).expect("an old-shaped list must parse");
        assert_eq!(
            c.excluded_apps,
            vec![
                AppException::new("photoshop", ExceptionScope::OffEntirely),
                // NOT normalised here — `publish_excluded_apps` normalises on
                // the way to the hook, as it always has; the parse is faithful.
                AppException::new("Figma.exe", ExceptionScope::OffEntirely),
            ]
        );
    }

    /// The new shape, including a row with the scope omitted (→ OffEntirely)
    /// and the two mixed in one array — a hand-edited file may well do that.
    #[test]
    fn the_object_form_and_a_mixed_list_both_parse() {
        let mut v = serde_json::to_value(AppConfig::default()).expect("serialise");
        v.as_object_mut().unwrap().insert(
            "excluded_apps".into(),
            serde_json::json!([
                { "exe": "sldworks", "scope": "middle_only" },
                { "exe": "game" },
                "blender",
                { "exe": "kicad", "scope": "space_only" }
            ]),
        );
        let c: AppConfig = serde_json::from_value(v).expect("mixed shapes must parse");
        assert_eq!(c.excluded_apps[0], AppException::new("sldworks", ExceptionScope::MiddleOnly));
        assert_eq!(c.excluded_apps[1].scope, ExceptionScope::OffEntirely, "a missing scope is OFF");
        assert_eq!(c.excluded_apps[2], AppException::new("blender", ExceptionScope::OffEntirely));
        assert_eq!(c.excluded_apps[3].scope, ExceptionScope::SpaceOnly);
    }

    /// What a save writes: always the object form, with the scope spelled
    /// out — so the NEXT read never has to guess, and an old build reading a
    /// new file fails loudly (an object where it wants a string) rather than
    /// silently treating "space only" as "off entirely".
    #[test]
    fn a_save_writes_the_object_form_and_round_trips() {
        let mut c = AppConfig::default();
        c.excluded_apps = vec![
            AppException::new("photoshop", ExceptionScope::OffEntirely),
            AppException::new("sldworks", ExceptionScope::MiddleOnly),
        ];
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains(r#"{"exe":"photoshop","scope":"off_entirely"}"#), "{json}");
        assert!(json.contains(r#"{"exe":"sldworks","scope":"middle_only"}"#));
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.excluded_apps, c.excluded_apps);
    }

    /// The two ring enums' wire forms, and that an EXPLICIT `guide_hud` is
    /// honoured — the default only fills an absence, never overrides a choice.
    #[test]
    fn the_ring_enums_have_stable_wire_forms_and_honour_an_explicit_choice() {
        let mut c = AppConfig::default();
        c.middle_ring_style = MiddleRingStyle::GuideHud;
        c.middle_ring_scope = MiddleRingScope::All;
        c.all_ring_layout = AllRingLayout::Spiral;
        c.middle_ring_favourites = vec!["m".into(), "b".into()];
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains(r#""middle_ring_style":"guide_hud""#), "{json}");
        assert!(json.contains(r#""middle_ring_scope":"all""#));
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.middle_ring_style, MiddleRingStyle::GuideHud, "an explicit choice survives");
        assert_eq!(back.middle_ring_scope, MiddleRingScope::All);
        // PROBLEM 267 round 3 — a 1.0.110/1.0.111 config still carrying
        // `middle_ring_motion` loads, the key is ignored, and a save no
        // longer writes it (ripple is the only motion).
        let with_motion = json.replace(r#""middle_ring_scope":"all""#, r#""middle_ring_scope":"all","middle_ring_motion":"bloom""#);
        let back_m: AppConfig = serde_json::from_str(&with_motion).expect("an old motion key is ignored");
        assert_eq!(back_m.middle_ring_scope, MiddleRingScope::All);
        assert!(!serde_json::to_string(&back_m).unwrap().contains("middle_ring_motion"));
        assert_eq!(back.middle_ring_favourites, vec!["m".to_string(), "b".to_string()]);
        // An unknown value is an ERROR, not a silent default: it can only
        // come from a hand edit or a newer build, and either deserves a loud
        // failure over a quietly different ring.
        let bad = json.replace(r#""guide_hud""#, r#""triangle""#);
        assert!(serde_json::from_str::<AppConfig>(&bad).is_err());
    }

    /// `site_icon` on a binding: absent parses as None (the pre-1.0.110
    /// shape), present round-trips untouched.
    #[test]
    fn a_bindings_site_icon_is_optional_and_round_trips() {
        let old = r#"{ "app": null, "web_url": "https://github.com", "label": "GitHub", "icon_override": null }"#;
        let b: KeyBinding = serde_json::from_str(old).expect("a pre-site_icon binding parses");
        assert!(b.site_icon.is_none());
        let with = KeyBinding { site_icon: Some("data:image/png;base64,AAAA".into()), ..b };
        let json = serde_json::to_string(&with).unwrap();
        let back: KeyBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(back.site_icon.as_deref(), Some("data:image/png;base64,AAAA"));
    }
}

/// PHASE A (2026-09-18) — the `Action` enum's wire form, the seed, and the
/// `is_mapped` truth table. The class of bug guarded is PROBLEM 159's: a
/// config written before a field existed must read unchanged, and a
/// migration must run once and only once.
#[cfg(test)]
mod phase_a_action_tests {
    use super::*;

    /// Every variant survives a JSON round trip, and the tag is the
    /// snake_case `kind` the TypeScript union (`src/types.ts`) matches on.
    #[test]
    fn every_action_variant_round_trips_through_serde() {
        let all = [
            Action::Uri { target: "ms-settings:display".into() },
            Action::Chord { keys: vec![0x5B, 0x10, 0x53] },
            Action::Command { line: "control.exe /name Microsoft.PowerOptions".into(), elevated: false },
            Action::Brightness { delta: -10 },
            Action::Special { id: "boss_key".into() },
            Action::Toggle { what: "bluetooth".into() },
        ];
        for a in all {
            let json = serde_json::to_string(&a).expect("serialise");
            let back: Action = serde_json::from_str(&json).expect("deserialise");
            assert_eq!(back, a, "{json}");
        }
        assert_eq!(
            serde_json::to_string(&Action::Special { id: "pip".into() }).unwrap(),
            r#"{"kind":"special","id":"pip"}"#
        );
        assert_eq!(
            serde_json::to_string(&Action::Brightness { delta: 10 }).unwrap(),
            r#"{"kind":"brightness","delta":10}"#
        );
        assert_eq!(
            serde_json::to_string(&Action::Toggle { what: "bluetooth".into() }).unwrap(),
            r#"{"kind":"toggle","what":"bluetooth"}"#
        );
    }

    /// `is_mapped`: an action counts, an app counts, a link counts, and an
    /// empty binding does not.
    #[test]
    fn is_mapped_truth_table() {
        let mk = |app: Option<&str>, url: Option<&str>, action: Option<Action>| KeyBinding {
            app: app.map(str::to_string),
            web_url: url.map(str::to_string),
            action,
            ..Default::default()
        };
        assert!(!mk(None, None, None).is_mapped());
        assert!(mk(Some("x.exe"), None, None).is_mapped());
        assert!(mk(None, Some("https://a"), None).is_mapped());
        assert!(mk(None, None, Some(Action::Special { id: "pip".into() })).is_mapped());
        assert!(mk(None, None, Some(Action::Uri { target: "shell:Downloads".into() })).is_mapped());
        // A binding with ONLY a label is not mapped — the label describes a
        // target, and there is none.
        assert!(!KeyBinding { label: Some("Ghost".into()), ..Default::default() }.is_mapped());
    }

    /// An OLD config — no `action`, no `specials_seeded`, no `advanced_mode`
    /// — loads, is seeded exactly once, and a profile that already had `esc`
    /// bound keeps its own binding.
    #[test]
    fn an_old_config_loads_gets_seeded_once_and_keeps_an_existing_esc() {
        let json = r#"{
            "version": 1,
            "active_profile": "Founders",
            "rollover_ms": 120,
            "guide_hud_delay_ms": 300,
            "opacity_floor_pct": 25,
            "browser_path": null,
            "fullscreen_allowlist": [],
            "profiles": [
                { "name": "Founders", "bindings": {
                    "a": { "app": "a.exe", "web_url": null, "label": "A", "icon_override": null },
                    "esc": { "app": "esc.exe", "web_url": null, "label": "Hand-bound", "icon_override": null }
                } },
                { "name": "Gamers", "bindings": {} }
            ]
        }"#;
        let mut cfg: AppConfig = serde_json::from_str(json).expect("an old config must parse");
        assert!(!cfg.advanced_mode);
        assert!(cfg.profiles.iter().all(|p| !p.specials_seeded));
        assert!(cfg.profiles[0].bindings["a"].action.is_none(), "old bindings read as None");

        assert!(seed_specials(&mut cfg), "the first seed changes something");
        for p in &cfg.profiles {
            assert!(p.specials_seeded, "{}: flagged", p.name);
            for (key, id) in DEFAULT_SPECIALS {
                if p.name == "Founders" && *key == "esc" {
                    continue;
                }
                assert_eq!(
                    p.bindings[*key].action,
                    Some(Action::Special { id: (*id).to_string() }),
                    "{}: {key} → {id}",
                    p.name
                );
            }
        }
        // The hand-bound Esc is untouched.
        let esc = &cfg.profiles[0].bindings["esc"];
        assert_eq!(esc.app.as_deref(), Some("esc.exe"));
        assert!(esc.action.is_none());
        // The letter is untouched too.
        assert_eq!(cfg.profiles[0].bindings["a"].label.as_deref(), Some("A"));
        assert_eq!(cfg.profiles[0].bindings.len(), 2 + DEFAULT_SPECIALS.len() - 1);
        assert_eq!(cfg.profiles[1].bindings.len(), DEFAULT_SPECIALS.len());

        // Second seed: nothing changes — and a REMOVED special stays removed.
        cfg.profiles[1].bindings.remove("period");
        assert!(!seed_specials(&mut cfg), "a seeded profile is never touched again");
        assert!(!cfg.profiles[1].bindings.contains_key("period"), "removing a special sticks");

        // And it survives a save→load round trip byte-for-byte.
        let first = serde_json::to_string_pretty(&cfg).unwrap();
        let back: AppConfig = serde_json::from_str(&first).unwrap();
        assert_eq!(first, serde_json::to_string_pretty(&back).unwrap());
        assert!(back.profiles.iter().all(|p| p.specials_seeded));
    }

    /// The twelve ids in the seed are all real special ids, each special is
    /// seeded exactly once, and the seed covers every id.
    #[test]
    fn the_seed_table_covers_every_special_exactly_once() {
        assert_eq!(DEFAULT_SPECIALS.len(), SPECIAL_IDS.len());
        for id in SPECIAL_IDS {
            assert_eq!(
                DEFAULT_SPECIALS.iter().filter(|(_, s)| s == id).count(),
                1,
                "{id} must be seeded on exactly one key"
            );
            assert!(is_special_id(id));
        }
        assert!(!is_special_id("nope"));
        let mut keys: Vec<&str> = DEFAULT_SPECIALS.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), DEFAULT_SPECIALS.len(), "one key, one special");
    }

    /// PHASE A step 3 (§5) — the import listing: every command line, sorted
    /// by key, with its elevation; a profile with none lists nothing.
    #[test]
    fn command_lines_lists_every_command_binding_sorted_by_key() {
        let mut b = BindingMap::new();
        b.insert("z".into(), KeyBinding { action: Some(Action::Command { line: "Get-Date".into(), elevated: false }), ..Default::default() });
        b.insert("a".into(), KeyBinding { action: Some(Action::Command { line: "Restart-Service Spooler".into(), elevated: true }), ..Default::default() });
        b.insert("f1".into(), KeyBinding { action: Some(Action::Uri { target: "ms-settings:".into() }), ..Default::default() });
        b.insert("b".into(), KeyBinding { app: Some("b.exe".into()), ..Default::default() });
        let p = Profile { name: "P".into(), bindings: b, emoji: None, specials_seeded: true };
        assert_eq!(
            command_lines(&p),
            vec![
                ("a".to_string(), "Restart-Service Spooler".to_string(), true),
                ("z".to_string(), "Get-Date".to_string(), false),
            ]
        );
        let none = Profile { name: "N".into(), bindings: BindingMap::new(), emoji: None, specials_seeded: true };
        assert!(command_lines(&none).is_empty());
    }

    /// PHASE A step 3 — `Command.elevated` defaults to false, so every
    /// 1.0.116/117 binding (`{"kind":"command","line":"…"}`) still reads.
    #[test]
    fn a_command_without_the_elevated_key_reads_as_not_elevated() {
        let a: Action = serde_json::from_str(r#"{"kind":"command","line":"Get-Date"}"#).unwrap();
        assert_eq!(a, Action::Command { line: "Get-Date".into(), elevated: false });
        let b: Action = serde_json::from_str(r#"{"kind":"command","line":"x","elevated":true}"#).unwrap();
        assert_eq!(b, Action::Command { line: "x".into(), elevated: true });
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains(r#""elevated":true"#), "{json}");
    }

    /// PHASE A step 3 — the second seeding pass. A fresh profile gets all
    /// fourteen; a 1.0.117 profile (seeded, no arrows) gets the two added
    /// and nothing else touched; a profile where `left` is already an app is
    /// left alone entirely; the pass is idempotent.
    #[test]
    fn the_second_pass_adds_the_arrow_specials_to_an_already_seeded_profile() {
        let fresh = || Profile { name: "F".into(), bindings: BindingMap::new(), emoji: None, specials_seeded: false };
        // A 1.0.117 profile: seeded with the old twelve, flag set.
        let old_twelve = || {
            let mut b = BindingMap::new();
            for (k, id) in DEFAULT_SPECIALS.iter().filter(|(k, _)| *k != "left" && *k != "right") {
                b.insert((*k).to_string(), KeyBinding { action: Some(Action::Special { id: (*id).to_string() }), ..Default::default() });
            }
            b.insert("a".into(), KeyBinding { app: Some("a.exe".into()), ..Default::default() });
            Profile { name: "Old".into(), bindings: b, emoji: None, specials_seeded: true }
        };
        // One where the user already put an app on `left`.
        let left_taken = || {
            let mut p = old_twelve();
            p.name = "LeftTaken".into();
            p.bindings.insert("left".into(), KeyBinding { app: Some("left.exe".into()), ..Default::default() });
            p
        };
        let mut cfg = AppConfig::default();
        cfg.profiles = vec![fresh(), old_twelve(), left_taken()];
        assert!(seed_specials(&mut cfg));

        let f = &cfg.profiles[0];
        assert_eq!(f.bindings.len(), 14, "fresh: all fourteen");
        assert_eq!(f.bindings["left"].action, Some(Action::Special { id: "move_window_left".into() }));
        assert_eq!(f.bindings["right"].action, Some(Action::Special { id: "move_window_right".into() }));

        let o = &cfg.profiles[1];
        assert_eq!(o.bindings.len(), 12 + 1 + 2, "1.0.117 profile: the two added, nothing else");
        assert_eq!(o.bindings["left"].action, Some(Action::Special { id: "move_window_left".into() }));
        assert_eq!(o.bindings["right"].action, Some(Action::Special { id: "move_window_right".into() }));
        assert_eq!(o.bindings["a"].app.as_deref(), Some("a.exe"));
        assert!(o.specials_seeded);

        let l = &cfg.profiles[2];
        assert_eq!(l.bindings["left"].app.as_deref(), Some("left.exe"), "the user's app stays");
        assert!(!l.bindings.contains_key("right"), "and the pair is not half-added");

        // Idempotent.
        assert!(!seed_specials(&mut cfg), "nothing left to seed");
        assert_eq!(cfg.profiles[1].bindings.len(), 15);
    }

    /// A profile export carries the flag, so an export made after the user
    /// removed a special does not get it back on import.
    #[test]
    fn a_profile_export_carries_specials_seeded() {
        let p = Profile {
            name: "X".into(),
            bindings: BindingMap::new(),
            emoji: None,
            specials_seeded: true,
        };
        let e = ProfileExport::of(&p);
        assert!(e.specials_seeded);
        let json = r#"{"spaceadom_profile":1,"name":"Old","bindings":{}}"#;
        let old: ProfileExport = serde_json::from_str(json).unwrap();
        assert!(!old.specials_seeded, "an export from before Phase A seeds on import");
    }
}
