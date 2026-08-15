/**
 * types.ts — Canonical TypeScript type definitions for SpaceToggle OS.
 *
 * These mirror the Rust structs in src-tauri/src/config/schema.rs exactly.
 * All Tauri IPC commands serialize/deserialize through these shapes.
 */

// ---------------------------------------------------------------------------
// Core config types (mirrors AppConfig, Profile, KeyBinding in schema.rs)
// ---------------------------------------------------------------------------

export interface KeyBinding {
  /** Executable file name or absolute path. null if not mapped to an app. */
  app: string | null;
  /** URL to open in preferred browser. null if not a web target. */
  web_url: string | null;
  /** Human-readable display label shown in the key matrix. */
  label: string | null;
  /** Base64-encoded PNG icon override. null = auto-extract from app. */
  icon_override?: string | null;
}

export interface Profile {
  /** Unique alphanumeric profile identifier (1–24 chars). */
  name: string;
  /** Map of lowercase key character → binding. Keys: a–z. */
  bindings: Record<string, KeyBinding>;
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
  /** All user-defined shortcut profiles. */
  profiles: Profile[];
  /** Nocturne (dark) mode. ONE setting drives dashboard AND overlay. */
  dark_mode?: boolean;
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

// ---------------------------------------------------------------------------
// App-level UI state (frontend-only, not persisted)
// ---------------------------------------------------------------------------

export interface AppState {
  config: AppConfig;
  selectedKey: string | null;
  isDetailPanelOpen: boolean;
}
