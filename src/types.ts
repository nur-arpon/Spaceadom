/**
 * types.ts — Canonical TypeScript type definitions for SpaceToggle OS.
 *
 * These mirror the Rust structs in src-tauri/src/config/schema.rs exactly.
 * All Tauri IPC commands serialize/deserialize through these shapes.
 */

// ---------------------------------------------------------------------------
// Core config types (mirrors AppConfig, Profile, KeyBinding in schema.rs)
// ---------------------------------------------------------------------------

/**
 * PHASE A (2026-09-18) — what a key does when Space is held, beyond "open
 * this app or link". Mirrors Rust's `Action` (schema.rs): `kind` is the
 * serde tag, snake_case.
 *
 *   uri        ms-settings:… / shell:… / any URI Windows opens
 *   chord      virtual-key codes in press order ([0x5B,0x10,0x53] = Win+Shift+S)
 *   command    a PowerShell line (no window, 60 s cap) — Advanced mode only
 *              (UI rule); `elevated` asks Windows for its own UAC prompt
 *              every time (step 3; absent = false in 1.0.116/117 files)
 *   brightness ±delta on the internal panel (WMI)
 *   toggle     flip a Windows setting (Bluetooth, Wi‑Fi, dark mode, night light,
 *              taskbar auto-hide, screen off, sleep, lock, show desktop) — step 2
 *   special    one of `SPECIAL_IDS` — the fourteen built-in specials
 */
export type Action =
  | { kind: "uri"; target: string }
  | { kind: "chord"; keys: number[] }
  | { kind: "command"; line: string; elevated?: boolean }
  | { kind: "brightness"; delta: number }
  | { kind: "toggle"; what: string }
  | { kind: "special"; id: string };

/**
 * The seventeen built-in specials, in Rust's `SPECIAL_IDS` order. Every one
 * is seeded on a key (1.0.126: `next_speaker` on `\`, `app_volume_down` /
 * `app_volume_up` on `-` / `=`).
 */
export const SPECIAL_IDS = [
  "boss_key", "pip", "pip_fullscreen", "force_close", "cycle_profile", "search",
  "pause", "voice_typing", "screenshot", "osk", "scroll_top", "scroll_bottom",
  "move_window_left", "move_window_right", "next_speaker",
  "app_volume_down", "app_volume_up",
] as const;
export type SpecialId = (typeof SPECIAL_IDS)[number];

/** True when a binding points at anything at all — Rust's `is_mapped`. */
export function isMapped(b: KeyBinding | undefined | null): boolean {
  return !!(b && (b.action || b.app || b.web_url));
}

export interface KeyBinding {
  /** Executable file name or absolute path. null if not mapped to an app. */
  app: string | null;
  /** URL to open in preferred browser. null if not a web target. */
  web_url: string | null;
  /** Human-readable display label shown in the key matrix. */
  label: string | null;
  /** Base64-encoded PNG icon override. null = auto-extract from app. */
  icon_override?: string | null;
  /**
   * Absolute path to a SPECIFIC browser exe to open `web_url` in, overriding
   * the OS default-browser lookup. null/absent = unchanged behaviour.
   *
   * The owner's hard requirement, stated twice: *"make sure the default
   * browser launches from URL if not explicitly set to specific."* This field
   * is the ONLY thing that diverts a URL, and Rust re-checks that on its side
   * (`browser_profiles::should_use_specific_browser`). Never write `""` here —
   * write null. Rust treats blank as unset, but only as a backstop.
   */
  browser_exe?: string | null;
  /** Chromium internal profile folder ("Profile 1"), for --profile-directory=. */
  browser_profile_dir?: string | null;
  /**
   * The profile's ACCOUNT LABEL as it read when picked — the signed-in email's
   * local part ("nur.arpon"), or the browser's own display name ("ARPON'S
   * STUDIES") when the profile is not signed in. Stored so the Guide HUD can
   * show "Chrome — nur.arpon" without re-reading the browser's Local State on
   * the latency-sensitive Space-hold path.
   *
   * NEVER the full address: this field is written to `config.json` and read
   * back into toasts and the HUD, and an address belongs in neither.
   * Pins written before 1.0.95 hold the display name and are NOT migrated —
   * re-picking the profile is one press and rewrites it.
   */
  browser_profile_name?: string | null;
  /**
   * PROBLEM 267 — a LINK's favicon as a complete `data:` URL, fetched ONCE
   * at bind time (`fetch_site_icon`, from the key editor's URL commit) and
   * stored here so the middle-button icon ring never fetches anything at ring
   * time. null/absent = no icon (the ring draws a letter disc) — and the next
   * edit of the key tries once more.
   */
  site_icon?: string | null;
  /**
   * PHASE A — the key's action when it is not an app or a link. Absent/null
   * on every binding written before 2026-09-18, and absent/null MEANS the
   * legacy `app` / `web_url` fields are the action. When set, `app` and
   * `web_url` are null (the editor writes one or the other, never both).
   */
  action?: Action | null;
}

/** PROBLEM 267 — one App-exceptions row's scope. Mirrors `ExceptionScope`. */
export type ExceptionScope = "off_entirely" | "space_only" | "middle_only";

/**
 * PROBLEM 267 — one App-exceptions row: the exe STEM and what stands down
 * inside it. Rust's `AppException`. A config written before 1.0.110 stores
 * plain strings; Rust reads those as `off_entirely` and REWRITES them as
 * objects on the next save, so this type is what `get_config` returns — but
 * `excludedList()` in settings-panel.ts still tolerates a bare string.
 */
export interface AppException {
  exe: string;
  scope: ExceptionScope;
}

/** PROBLEM 267 — what the middle button raises. Mirrors `MiddleRingStyle`. */
export type MiddleRingStyle = "icon_ring" | "guide_hud";
/** PROBLEM 267 — how much the icon ring shows. Mirrors `MiddleRingScope`. */
export type MiddleRingScope = "my_eight" | "all";
/** 2026-09-15 — how the "All" scope is laid out. Mirrors `AllRingLayout`. */
export type AllRingLayout = "rings" | "spiral";

export interface Profile {
  /** Unique profile name (1–24 chars, any printable text — PROBLEM 197). */
  name: string;
  /** Map of lowercase key character → binding. Keys: a–z. */
  bindings: Record<string, KeyBinding>;
  /**
   * One emoji standing in for this profile, or null/absent for none.
   *
   * **NULL IS THE NORMAL STATE AND EVERY SURFACE MUST KEEP ITS OLD LOOK FOR
   * IT.** The popover row falls back to the name's initial, the top-right pill
   * keeps its letter disc, and the Guide HUD's SPACE pill renders no extra
   * element at all. Three readers, one rule.
   *
   * Optional in the TYPE because every `config.json` on disk predates the
   * field — read it as `p.emoji ?? null`, never as `p.emoji!`.
   *
   * A single GRAPHEME CLUSTER, which is not a single JS character: "👨‍👩‍👧"
   * has `.length === 8` and five code points. Never index into it, never
   * `slice` it, never `maxLength`-cap an input to 1 — use `Array.from(s)` if
   * you have to count anything. Rust's `schema::emoji_is_valid` is the check
   * that is actually enforced (`set_profile_emoji`); anything here is a
   * fail-fast before the round trip.
   */
  emoji?: string | null;
  /**
   * PHASE A — has Rust seeded the twelve default specials into `bindings`?
   * Rust sets it on load; the UI never writes it. Absent = not yet (a config
   * that has not been through a Phase A load).
   */
  specials_seeded?: boolean;
}

export interface AppConfig {
  /** Schema version. Current = 1. */
  version: number;
  /** Name of the currently active profile. */
  active_profile: string;
  /** Adaptive rollover window in milliseconds (default: 50). */
  rollover_ms: number;
  /** Typing speed in WPM; drives rollover_ms (PROBLEM 69). Optional on
   *  configs written by =<1.0.5, so always read it as `?? 65`. */
  typing_wpm?: number;
  /** Milliseconds Space must be held before Guide HUD appears (default: 300). */
  guide_hud_delay_ms: number;
  /** Minimum window opacity enforced by scroll-wheel modifier (0–100 %). */
  opacity_floor_pct: number;
  /** Absolute path to preferred browser executable. null on first run. */
  browser_path: string | null;
  /** Process names never treated as exclusive-fullscreen for hook suppression. */
  fullscreen_allowlist: string[];
  /** Apps Spaceadom stands down inside — the "App exceptions" list.
   *  LOWERCASE EXE STEMS ("photoshop"), matching hook/exclusions.rs.
   *  Optional: every config written before 1.0.79 lacks it. */
  excluded_apps?: Array<AppException | string>;
  /** All user-defined shortcut profiles. */
  profiles: Profile[];
  /** Nocturne (dark) mode. ONE setting drives dashboard AND overlay. */
  dark_mode?: boolean;
  /** PROBLEM 144 — "earthy" | "warcry" | "starry". Migrated from dark_mode. */
  theme?: string;
  /** PROBLEM 144 — the personality layer. Also decides whether Starry night
   *  is the new sky or the plain nocturne this app has always had. */
  fun_mode?: boolean;
  /** PROBLEM 144 — clear the dashboard away and leave only the sky. */
  hide_keyboard?: boolean;
  /** PROBLEM 144 — every setting's description open at once. */
  show_me_around?: boolean;
  /** Optional WebAudio sine ticks. Off by default. */
  sound_enabled?: boolean;
  /**
   * Visual-effects level: "auto" follows the OS reduced-motion signal,
   * "full" forces all effects on, "reduced" forces them off.
   * "auto" is the default — but a tester whose Windows had animation effects
   * disabled saw a completely motionless app and reported it as broken, so
   * the override matters (PROBLEM 47).
   */
  motion?: "auto" | "full" | "reduced";
  /**
   * PROBLEM 174 — "Guide-to-toast motion". OFF by default.
   *
   * On: a shortcut's message pill is slung out of the Space ring on a 940ms
   * arc. Off: the ring collapses on its own and the message appears
   * bottom-centre — exactly 1.0.27's behaviour.
   *
   * Read it as `=== true`, NEVER `!== false`: it is absent from every config
   * written before 1.0.73, and those users are precisely the ones who asked
   * for the motion to stop.
   */
  hud_toast_flight?: boolean;
  /**
   * PROBLEM 206 — pointer activation on the Guide HUD. **ON by default since
   * PROBLEM 209.**
   *
   * On: while Space is held and the ring is up, pointing the cursor in a
   * chip's DIRECTION arms it (the chip lights up), and releasing Space — or
   * clicking — launches that binding. The cursor never has to reach the chip.
   *
   * Read it as `!== false`, NEVER `=== true` — the OPPOSITE of
   * `hud_toast_flight` directly above, and the same as `send_logs` below. The
   * key is absent from every config written before 1.0.88, and absent must
   * now mean ON: the owner flipped this default on 2026-08-27, knowingly
   * overriding his own new-behaviour-defaults-off convention, and `=== true`
   * would quietly deliver that flip to nobody who already runs the app.
   */
  pointer_hud_activation?: boolean;
  /**
   * PROBLEM 263 — HOLD THE MIDDLE MOUSE BUTTON TO RAISE THE RING. **ON by
   * default, by the owner's decision on 2026-09-08.**
   *
   * The ring has three triggers now: the keyboard hook's Space, the own-window
   * fallback's Space (PROBLEM 259) and this. Holding the middle button raises
   * the same ring in the same centred place; a QUICK middle click is replayed
   * through `SendInput` so browsers still open links in a new tab.
   *
   * Read it as `!== false`, NEVER `=== true` — the same rule, for the same
   * reason, as `pointer_hud_activation` directly above. Rust declares it
   * `#[serde(default = "default_true")]`, the key is absent from every config
   * written before this feature existed, and absent must read ON. `=== true`
   * here would show the switch OFF for every existing user while Rust ran the
   * feature — the switch and the app disagreeing, which is the one class of bug
   * in this panel nobody can see from the outside.
   *
   * **THIS IS NOT THE ONLY GATE AND THE OTHER ONE IS NOT A SETTING.**
   * `src-tauri/src/hook/orbit_apps.rs` holds a built-in list of 3D, CAD and
   * design programs — SolidWorks, Fusion 360, Blender, AutoCAD, Photoshop,
   * Figma and the rest — where middle-drag already orbits a model or pans a
   * canvas, and inside those the middle button is handed straight back to
   * Windows however this flag reads. That list is separate from the user's App
   * exceptions and applies to the middle button ONLY: Space shortcuts inside
   * SolidWorks are untouched by it.
   */
  middle_button_ring?: boolean;
  /**
   * PROBLEM 267 — WHAT the middle button raises: `"icon_ring"` (default;
   * the cursor-anchored ring of real icons) or `"guide_hud"` (phase 1: the
   * same centred Guide HUD as Space, exactly as 1.0.109). Absent = icon ring.
   * Only meaningful while `middle_button_ring` is on.
   */
  middle_ring_style?: MiddleRingStyle;
  /**
   * PROBLEM 267 — how much the icon ring shows: `"my_eight"` (default) or
   * `"all"` (the favourites, then every other bound letter and the
   * specials, on as many rings as needed). Absent = favourites (the wire
   * name `my_eight` is kept for compatibility; the UI says "Favourites").
   */
  middle_ring_scope?: MiddleRingScope;
  /**
   * 2026-09-15 — HOW the `"all"` scope is arranged: `"rings"` (default,
   * the concentric Fibonacci rings) or `"spiral"` (one phyllotaxis spiral,
   * the packing a sunflower's seed head uses). Absent = rings, which is
   * what every existing install already draws. Ignored for Favourites.
   */
  all_ring_layout?: AllRingLayout;
  /**
   * PROBLEM 267 — the user's chosen favourites, as bound letters in ring
   * order (at most fifteen since round 3). EMPTY = not chosen: Rust uses the
   * first six bound letters of the active profile at ring time and writes
   * nothing back.
   */
  middle_ring_favourites?: string[];
  /**
   * PROBLEM 209 — show the SPECIAL keys on the Space HUD's inner ring?
   * ON by default.
   *
   * Display only. Esc, the backtick PiP, the Boss Key and the rest keep
   * working exactly as before when this is off; only the ring stops drawing
   * them. Rust does the whole job by sending an empty `specials` list.
   *
   * Read it as `!== false`, NEVER `=== true`: the ring has been drawn since
   * the HUD existed, so a config that predates the setting must keep drawing
   * it. Existing behaviour becoming optional, not new behaviour arriving —
   * which is a different question from the one `hud_toast_flight` answers,
   * even though the two rows sit side by side in Settings.
   */
  hud_show_specials?: boolean;
  /**
   * PHASE A — Advanced mode. Step 4 (2026-09-19): OFF, the key editor is the
   * App or link picker alone — no kind row; ON, the row unlocks Spaceadom
   * special, Key combo, Controls and Run command. Absent = off. UI only;
   * Rust ignores it.
   */
  advanced_mode?: boolean;
  /** 1.0.119 (brief 4 §4) — the key editor's "Key combo" hint has been shown
   *  once; afterwards it collapses to a "How does this work?" link. Same
   *  shape as `tour_done`: absent = not yet seen, read as `=== true`. */
  key_combo_hint_seen?: boolean;
  /**
   * How many RINGS of app shortcuts the Space HUD lays out. Default "auto".
   *
   * A STRING ENUM, like `motion` above and unlike every switch around it —
   * three states cannot be a bool, and pretending otherwise is how a setting
   * ends up with a fourth state nobody named. Read it with a fallback, never
   * with a comparison chain: anything that is not exactly "one" or "two" means
   * "auto", including the `undefined` that every config written before 1.0.89
   * supplies and the `""` a bare serde default would have produced.
   *
   * **IT IS ONE SYSTEM WITH `hud_show_specials` ABOVE.** The specials occupy
   * the HUD's inner band, so they can only be drawn when the apps need just
   * the outer one:
   *
   *   one  + specials on  -> specials inner ring, apps outer ring
   *   one  + specials off -> a single app band, no inner ring
   *   two  + either       -> two app bands, specials not rendered
   *   auto + specials on  -> specials IF the apps fit one ring, else dropped
   *   auto + specials off -> band count by arithmetic
   *
   * Rust resolves the deterministic rows of that table by sending an empty
   * specials list (engine/mod.rs `specials_for_hud`). "auto" is the overlay
   * page's call and only the page's, because the band count depends on
   * MEASURED label widths. So Settings must never present the two controls as
   * independent — see the inert treatment of the specials switch when this is
   * "two".
   */
  hud_band_count?: "auto" | "one" | "two";
  /**
   * Use the NEW Magnetic Sector ring for the Space HUD, or the CLASSIC ring
   * that shipped in 1.0.88? **ON (= the new ring) by default.**
   *
   * `true` = the new layout, `false` = the classic one. The toggle in
   * Settings ("New ring layout") is the ESCAPE HATCH, not the invitation —
   * the owner asked for the new ring to be what the app opens with, and for
   * the switch to be the way back.
   *
   * Read it as `!== false`, NEVER `=== true` — the same as
   * `pointer_hud_activation` above and the OPPOSITE of `hud_toast_flight`.
   * The key is absent from every config written before 1.0.89, and absent
   * must mean ON: a default only ever reaches users whose file predates the
   * field, so `=== true` would quietly deliver the new ring to nobody who
   * already runs the app.
   *
   * **DO NOT COMPARE THIS BOOL ANYWHERE DOWNSTREAM.** The overlay reads it
   * through `components/hud-layout.ts`, which normalises it to a NAME
   * (`"magnetic"` | `"classic"`) exactly once. Settings is the only other
   * reader, and only to draw its own switch.
   *
   * **IT GATES `hud_band_count` ABOVE.** The rows pill only means anything
   * for the new ring; the classic ring has its own fixed shape. Settings
   * therefore greys the pill out while this is off — presentation only, the
   * stored row choice is never written, so turning the layout back on
   * restores it.
   */
  hud_magnetic_layout?: boolean;
  /** Spaceadom logon task enabled (run at startup). ON by default. */
  run_at_startup?: boolean;
  /**
   * Overlay rendering mode — a MEASUREMENT of this machine, not a preference.
   * "auto" = let WebView2 use the GPU; "software" = launch it with
   * --disable-gpu. On machines whose driver cannot composite the transparent
   * overlay, the HUD and toasts paint ZERO pixels while everything else looks
   * healthy — Rust reports visible=true, the JS runs, the sound plays and apps
   * still launch. The pixel self-test writes "software" and never reverts on
   * its own, so the gear panel's toggle is the only way back (PROBLEM 92).
   */
  overlay_compositing?: "auto" | "software";
  /**
   * PROBLEM 195 — may crash and error reports be sent to Sentry?
   *
   * **TRUE MEANS SENDING IS HAPPENING**, and true is the default. The switch
   * in Settings is called "Don't send logs", which is the NEGATION of this
   * field: it renders as `checked = !send_logs` and writes `!checked`.
   *
   * Read it as `!== false`, like `run_at_startup` and NOT like
   * `hud_toast_flight`: it is absent from every config written before 1.0.82,
   * and absent must mean ON, which is what Rust's serde default also says.
   */
  send_logs?: boolean;
  /**
   * PROBLEM 242 — has the first-run "Guided first bind" tour been seen?
   *
   * **FALSE IS THE INTERESTING VALUE.** It is written `true` exactly once, on
   * the run where the user finishes the walkthrough or skips it, and never
   * read again after that. Absent from every config written before 1.0.97,
   * and absent must mean "not yet seen" — so read it as `=== true`, like
   * `hud_toast_flight` and NOT like `send_logs`: a config that predates the
   * field belongs to someone who has never been offered the tour.
   *
   * Skipping writes it too. "Skip" means never nag again, not "ask me next
   * time" — the Settings header's "Show me the walkthrough" is the way back,
   * and it ignores this field entirely.
   */
  tour_done?: boolean;

  /** TOUCHPAD T2 (1.0.120) — the four edge bands. Optional: a config written
   *  before 1.0.120 has no `touchpad` and reads as `Touchpad`'s defaults. */
  touchpad?: Touchpad;
}

// ---------------------------------------------------------------------------
// TOUCHPAD T2 (1.0.120) — mirrors `config/schema.rs`'s Touchpad types.
// ---------------------------------------------------------------------------

export type TouchEdge = "left" | "right" | "top" | "bottom";
/** Externally tagged like serde: the unit variants are plain strings, and
 *  "Any shortcut" (1.0.122) is `{ chords: { forward, backward } }` — two
 *  chords in press order, sent once per quantised step of the slide. */
export type BandChords = { chords: { forward: number[]; backward: number[] } };
export type BandAction = "brightness" | "volume" | "scrub" | "none" | BandChords;
/** The action's family — what the page's "Does what" rows pick between. */
export type BandActionKind = "brightness" | "volume" | "scrub" | "none" | "chords";
export type CornerRule = "ask" | "always_horizontal" | "always_vertical";
export type TouchpadLook = "chocolate" | "app";
/** Emitted by `touchpad::mod`; `"none"` covers "no pad" and "not Precision". */
export type TouchpadPresence = "precision" | "none";

export interface Band {
  enabled: boolean;
  action: BandAction;
  /** Fraction of the pad's short side (0.04..=0.25). */
  width: number;
  /** Fraction of the edge, centred (0.30..=1.0). */
  length: number;
  /** 1..=10. */
  sensitivity: number;
  invert: boolean;
}

export interface Corners {
  tl: TouchEdge | null;
  tr: TouchEdge | null;
  bl: TouchEdge | null;
  br: TouchEdge | null;
}

export interface Touchpad {
  left: Band;
  right: Band;
  top: Band;
  bottom: Band;
  corner_rule: CornerRule;
  corners: Corners;
  page_look: TouchpadLook;
  demo_seen: boolean;
  show_thumbnail: boolean;
  /** 1.0.123 — "Show a toast while sliding"; absent reads as true in Rust. */
  slide_toast: boolean;
}

/** `touchpad-caps` event payload. */
export interface TouchpadCaps {
  presence: TouchpadPresence;
  pad_mm: [number, number] | null;
}

/** `touchpad-live` event payload; every field is null on gesture end. */
export interface TouchpadLive {
  edge: TouchEdge | null;
  action: BandActionKind | null;
  value_pct: number | null;
  travel: number | null;
  /** Chords only: the forward chord's name ("Ctrl+Tab"). */
  chord?: string | null;
  /** Chords only: the signed step count sent so far this gesture. */
  steps?: number | null;
}

// ---------------------------------------------------------------------------
// IPC response types (mirrors Rust structs in schema.rs)
// ---------------------------------------------------------------------------

export interface HookStatus {
  installed: boolean;
  bypass_active: boolean;
  fullscreen_suppressed: boolean;
  active_profile: string;
}

export interface ConflictResult {
  has_conflict: boolean;
  conflicting_combo: string | null;
  description: string | null;
}

export interface ProfileSummary {
  name: string;
  binding_count: number;
}

/** One entry from `list_start_menu_apps` — a real app detected on this PC. */
export interface AppInfo {
  name: string;
  path: string;
  /** Base64 PNG from IShellItemImageFactory. Render it; never a letter disc. */
  icon_base64: string | null;
}

/** One profile inside one detected browser (mirrors Rust `BrowserProfile`). */
export interface BrowserProfile {
  /** Chromium's internal folder name — what --profile-directory= wants. */
  directory: string;
  /** The human name from the browser's own Local State. */
  display_name: string;
  /**
   * The signed-in account email, from Local State's `info_cache[dir].user_name`.
   * `null` when the profile is not signed in — MEASURED 2026-08-31 (see the
   * Rust doc comment on `BrowserProfile::email`): that covers both a present-
   * but-empty `user_name` (Edge, Samsung) and the key being absent entirely
   * (Brave), which Rust already collapses to one value here. Never render an
   * empty line for it — check for `null`/falsy, not for `""`.
   *
   * RENDER THIS IN A TOOLTIP ONLY. The visible label is `account_label`.
   */
  email: string | null;
  /**
   * **The label to render.** The local part of `email` (everything before the
   * `@`), or `display_name` when the profile is not signed in. Computed in
   * Rust (`browser_profiles::account_label`) so the picker, the HUD chip, the
   * key-editor chip and the toasts cannot drift apart.
   *
   * Optional in the TYPE, not in the data: `readLastKnown()` deserialises a
   * list that a build of 1.0.94 or earlier may have written to localStorage,
   * and that shape has no such field. Read it through `labelOf()`, never
   * directly, so a cached tile falls back to the display name instead of
   * painting `undefined`.
   */
  account_label?: string;
}

/**
 * One Chromium browser found on this PC (mirrors Rust `DetectedBrowser`).
 *
 * Detection is structural, not a vendor list: anything with a Chromium-shaped
 * `Local State` AND a resolvable launcher exe qualifies, so forks are picked up
 * without a code change. Measured on the owner's machine 2026-08-26: Brave,
 * Chrome, Edge, Samsung Browser and (MSIX-packaged) Arc.
 */
/**
 * The OS default browser (mirrors Rust `DefaultBrowserInfo`, added with
 * `get_default_browser` 2026-08-26).
 *
 * Resolved from the SAME place `run_browser` resolves it — the http/https
 * UserChoice handler — deliberately, so the key editor's leading disc can never
 * show a different browser than the key actually opens. `null` from the command
 * means no handler is registered (or its command line will not parse), which is
 * the same condition that makes launching a URL fail.
 */
export interface DefaultBrowserInfo {
  /** Absolute path to the browser executable. */
  exe: string;
  /** Human name, e.g. "Edge" — the same naming the fallback toasts use. */
  name: string;
  /** Base64 PNG, 48px, from the same extractor and cache as AppInfo's. */
  icon_base64: string | null;
}

export interface DetectedBrowser {
  browser_name: string;
  browser_exe: string;
  user_data_dir: string;
  /** Base64 PNG. Same extractor and cache as AppInfo's. */
  icon_base64: string | null;
  profiles: BrowserProfile[];
}

// ---------------------------------------------------------------------------
// App-level UI state (frontend-only, not persisted)
// ---------------------------------------------------------------------------

export interface AppState {
  config: AppConfig;
  selectedKey: string | null;
  isDetailPanelOpen: boolean;
}
