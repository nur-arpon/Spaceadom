/// config/mod.rs — Config load, parse, and save logic.

pub mod defaults;
pub mod schema;

pub use schema::*;

use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

/// Shared, thread-safe config handle used throughout the application.
pub type SharedConfig = Arc<RwLock<AppConfig>>;

/// Returns the path to config.json inside the SpaceToggle data directory.
pub fn config_path() -> PathBuf {
    crate::startup::data_dir().join("config.json")
}

// ---------------------------------------------------------------------------
// THEME "auto" — PROBLEM 255, the Rust half
// ---------------------------------------------------------------------------
//
// The theme pill stores the LITERAL string `"auto"`. Nothing resolves it away
// before it reaches `config.json`, on purpose: "match Windows" is the user's
// choice and it has to survive a restart, an OS theme flip and a config
// round-trip intact. What that means is that every consumer of "which of the
// three real palettes" has to resolve it, and there are now three of them —
// the dashboard (`main.ts::resolveTheme`), the overlay
// (`src/theme-resolve.ts`, shared by `overlay.ts`/`toast.ts`) and THIS ONE.
//
// Rust's copy exists for exactly one field: `dark_mode`. That bool is what
// `save_config` emits as `theme-changed` and what `AppConfig` carries around,
// and `config/mod.rs` recomputes it from `theme` on EVERY load. Before this,
// the recompute was `cfg.theme != "earthy"`, which for a config whose theme is
// `"auto"` — the default for every new install since PROBLEM 255 — set
// `dark_mode = true` unconditionally, in daylight, on the second launch
// onward. That is PROBLEM 255's own "found but could not fix" item 1.
//
// The three resolvers MUST agree. They are separated by process boundaries
// (Rust, two webviews) so they cannot literally be one function; what they
// can be is one RULE, written the same way three times, with this comment and
// `main.ts::resolveTheme`'s doc pointing at each other.

/// The literal the pill stores when the user picks "Auto".
pub const THEME_AUTO: &str = "auto";

/// **The pure rule.** Raw config value + "does the OS say dark?" → one of the
/// three real palettes.
///
/// `os_dark == None` means the question could not be answered (the registry
/// value is absent, unreadable, or this is not Windows) and resolves to
/// Earthy. That is deliberately the same answer the frontend gives when
/// `matchMedia` reports no dark preference: a missing signal has never meant
/// anything but daylight in this app, and two halves that disagree about the
/// fallback would put the dashboard and the overlay in different palettes on
/// exactly the machines least able to explain why.
///
/// `"auto"` never resolves to Warcry, matching `main.ts::resolveTheme`:
/// Windows has a light/dark preference, not an iron-and-war-banners one.
pub(crate) fn resolve_theme(raw: &str, os_dark: Option<bool>) -> &'static str {
    match raw {
        "warcry" => "warcry",
        "starry" => "starry",
        "earthy" => "earthy",
        // "auto", and anything unrecognised, including the empty string a
        // pre-PROBLEM-144 config carries before the migration above runs.
        _ => {
            if os_dark.unwrap_or(false) {
                "starry"
            } else {
                "earthy"
            }
        }
    }
}

/// `dark_mode` for a raw theme value. One line, but named, because the
/// "everything that is not Earthy is dark" rule is stated in four files and
/// this is the one place Rust states it.
pub(crate) fn dark_mode_for(raw: &str, os_dark: Option<bool>) -> bool {
    resolve_theme(raw, os_dark) != "earthy"
}

/// **Does Windows currently want dark app surfaces?**
///
/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize` →
/// `AppsUseLightTheme` (REG_DWORD, `1` = light, `0` = dark). Read-only, HKCU,
/// no elevation — the same hive and the same "never write a Control Panel key
/// on our own initiative" rule `set_hook_timeout` follows.
///
/// **`AppsUseLightTheme`, not `SystemUsesLightTheme`.** Windows keeps two
/// independent switches ("Default app mode" and "Default Windows mode") and
/// only the first governs how an application's own surfaces should look; the
/// second is the taskbar and Start. A user who runs a light taskbar with dark
/// apps is a common configuration, and reading the wrong value would fight
/// them.
///
/// `None`, never a guessed default, when the value is absent or unreadable —
/// so [`resolve_theme`] can apply ONE fallback rule instead of this function
/// inventing a second one. The value genuinely is absent on some machines
/// (a fresh install that has never opened the Personalisation page), and
/// "absent" is not "light" as a matter of fact; it only happens to produce
/// the same answer here.
///
/// **A note for anyone verifying this from the agent shell**: that shell runs
/// inside an MSIX container which virtualises HKCU (CLAUDE.md, PROBLEM 143),
/// so a value read or written from there may be the container's private copy
/// rather than the machine's. That affects the SHELL, not the app — the
/// installed `spaceadom.exe` has no package identity and no redirection view,
/// and reads the real hive. Do not "fix" a discrepancy observed only from the
/// agent shell.
#[cfg(windows)]
pub(crate) fn os_prefers_dark() -> Option<bool> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .ok()?;
    let light: u32 = key.get_value("AppsUseLightTheme").ok()?;
    Some(light == 0)
}

#[cfg(not(windows))]
pub(crate) fn os_prefers_dark() -> Option<bool> {
    None
}

/// Load config from disk. On first run (file missing), attempt to parse V11 script,
/// fall back to hardcoded defaults, and write the initial config.json.
pub fn load_or_init() -> SharedConfig {
    let path = config_path();

    let config = if path.exists() {
        match std::fs::read_to_string(&path) {
            // Tolerate a UTF-8 BOM: serde_json rejects it, and external tools
            // (PowerShell 5.1 `-Encoding UTF8`) add one — that once made the
            // app discard the user's entire config as unparseable (2026-08-10).
            Ok(raw) => match serde_json::from_str::<AppConfig>(raw.trim_start_matches('\u{feff}')) {
                Ok(mut cfg) => {
                    log::info!("config: loaded from {}", path.display());
                    // PROBLEM 94 — back up on LOAD, not only on save. Backups
                    // were originally written from save_to_disk, which meant a
                    // user who set their bindings up once and never changed
                    // anything again had NO backup at all — exactly the user
                    // most hurt by losing it. A config that just parsed
                    // cleanly is by definition a known-good one worth keeping.
                    write_backup(raw.trim_start_matches('\u{feff}'));

                    // PROBLEM 144 — migrate the old two-state dark toggle into the 3-way
                    // theme, ONCE. An existing config has no `theme` key, so serde gives it
                    // the "earthy" default — which would silently flip a dark-mode user into
                    // daylight on upgrade. Derive it from what they actually had instead.
                    let migrated_theme = cfg.theme.is_empty();
                    if migrated_theme {
                        cfg.theme = if cfg.dark_mode { "starry" } else { "earthy" }.to_string();
                        log::info!(
                            "config: migrated dark_mode={} -> theme=\"{}\" (PROBLEM 144)",
                            cfg.dark_mode, cfg.theme
                        );
                    }
                    // The two must never disagree: `dark_mode` is what drives body.nocturne on
                    // the dashboard AND the overlay, and the overlay knows nothing about themes.
                    //
                    // PROBLEM 255 — this used to read `cfg.theme != "earthy"`,
                    // which is TRUE for `"auto"` and therefore turned every new
                    // install (whose default theme is now "auto") dark on its
                    // second launch, in daylight, whatever Windows said. It
                    // goes through `dark_mode_for` now, which asks the OS.
                    // `cfg.theme` itself is left as the literal `"auto"` — the
                    // user's CHOICE is "match Windows" and resolving it away
                    // here would quietly convert that into a fixed palette the
                    // next time the config was written back.
                    cfg.dark_mode = dark_mode_for(&cfg.theme, os_prefers_dark());
                    // Write the migrated fields straight back, rather than waiting
                    // for the user's next settings change. A config whose file does
                    // not match the config the app is running is exactly the kind of
                    // state this project has been bitten by before.
                    let mut dirty = migrated_theme;
                    // Auto-upgrade legacy 0ms rollover to 120ms to fix typing bugs
                    if cfg.rollover_ms == 0 {
                        cfg.rollover_ms = 120;
                        dirty = true;
                    }
                    // PROBLEM 69 — a config written before typing_wpm existed
                    // gets the serde default, which would make the Settings
                    // slider DISPLAY a speed that does not match the window
                    // actually in force. Adopt the proven default instead of
                    // inventing one, keeping the window the user already had.
                    if !raw.contains("\"typing_wpm\"") {
                        log::info!(
                            "config: no typing_wpm (pre-slider config) — adopting the default \
                             {} wpm and keeping the existing {}ms window",
                            schema::DEFAULT_TYPING_WPM,
                            cfg.rollover_ms
                        );
                        cfg.typing_wpm = schema::DEFAULT_TYPING_WPM;
                        dirty = true;
                    }

                    // PROBLEM 72 + 95 — RAISE a window that is too narrow to be
                    // safe. Two different generations of config land here:
                    //   * 1.0.6/1.0.7 wrote windows as low as 62ms from a
                    //     mapping that ran BACKWARDS (window grew with speed).
                    //   * 1.0.8-1.0.14 wrote 110-199ms from `8400 / wpm`, which
                    //     looked reasonable but is 0.7x the typist's own
                    //     inter-key interval, so the window sat UNDER ordinary
                    //     typing at every setting. Measured 2026-08-13: at
                    //     70 wpm / 120ms a 180ms spacebar hold turned 18 of 18
                    //     words into commands.
                    // Neither is a value a user meaningfully chose, and the
                    // failure is all-or-nothing, so repair rather than warn.
                    if cfg.rollover_ms < schema::MIN_ROLLOVER_MS {
                        // Keep the user's chosen SPEED; only recompute the
                        // window from it. Someone who set "Fast" still gets
                        // fast — just a window that is actually safe.
                        let repaired = schema::rollover_ms_for_wpm(cfg.typing_wpm);
                        log::warn!(
                            "config: rollover_ms {}ms is below the safe minimum ({}ms) — that \
                             window is narrower than the gap between your own keystrokes, so a \
                             long spacebar press could fire a shortcut mid-sentence (PROBLEM 95). \
                             Recomputing from your {} wpm setting: {}ms. Adjust under \
                             Settings > Typing speed.",
                            cfg.rollover_ms,
                            schema::MIN_ROLLOVER_MS,
                            cfg.typing_wpm,
                            repaired
                        );
                        cfg.rollover_ms = repaired;
                        dirty = true;
                    }
                    if dirty {
                        let _ = save_to_disk(&cfg, &path);
                    }
                    cfg
                }
                Err(e) => {
                    // NEVER silently destroy a config that fails to parse: it
                    // is the user's data and the error may be one stray byte.
                    // Preserve it next to the original before regenerating.
                    let backup = path.with_extension("json.corrupt");
                    match std::fs::copy(&path, &backup) {
                        Ok(_) => log::error!(
                            "config: JSON parse error ({e}) — original preserved at {}",
                            backup.display()
                        ),
                        Err(be) => log::error!(
                            "config: JSON parse error ({e}) AND backup failed ({be})"
                        ),
                    }
                    // PROBLEM 159 — this used to go straight to defaults, which
                    // is a FACTORY RESET while a perfectly good copy of the
                    // user's profiles sits in the backup folder that PROBLEM
                    // 102 exists to maintain. Preserving the broken file and
                    // then ignoring the working one is the worst of both.
                    //
                    // Try the newest backup that parses. Only if none does —
                    // or there are none — do we regenerate.
                    match newest_valid_backup() {
                        Some((cfg, from)) => {
                            log::warn!(
                                "config: recovered from backup {} — the unreadable file is at {}",
                                from.display(), backup.display()
                            );
                            // Write it back immediately: if this launch crashes
                            // before the first save, the next one must not have
                            // to make this decision again.
                            let _ = save_to_disk(&cfg, &path);
                            cfg
                        }
                        None => {
                            log::error!("config: no usable backup either — regenerating defaults");
                            generate_defaults()
                        }
                    }
                }
            },
            Err(e) => {
                log::error!("config: read error ({e}), regenerating defaults");
                generate_defaults()
            }
        }
    } else {
        // One-time migration from the previous product identity: a user who
        // already ran "SpaceToggle V14" on this machine keeps every binding
        // and setting when Spaceadom first starts, instead of being reseeded.
        let legacy = crate::startup::legacy_data_dir().join("config.json");
        if legacy.exists() {
            match std::fs::read_to_string(&legacy)
                .map_err(|e| e.to_string())
                .and_then(|s| serde_json::from_str::<AppConfig>(&s).map_err(|e| e.to_string()))
            {
                Ok(cfg) => {
                    log::info!("config: migrated from legacy V14 config at {legacy:?}");
                    if let Err(e) = save_to_disk(&cfg, &path) {
                        log::error!("config: failed to write migrated config: {e}");
                    }
                    return Arc::new(RwLock::new(cfg));
                }
                Err(e) => {
                    log::warn!("config: legacy V14 config found but unreadable ({e}) — seeding defaults");
                }
            }
        }
        // PROBLEM 94 — a MISSING config with a backup available is not a first
        // run, it is a loss: an uninstall that removed the data folder, a
        // profile reset, a sync tool, a disk error. Restoring is unambiguously
        // right here — there is nothing to overwrite. This is the case that
        // cost this user 104 bindings and 5 custom icons on 2026-08-13, which
        // were only partially recovered from an accidental Windows shadow copy.
        if let Some((backup, len)) = newest_richer_backup(0) {
            match std::fs::read_to_string(&backup)
                .map_err(|e| e.to_string())
                .and_then(|s| {
                    serde_json::from_str::<AppConfig>(s.trim_start_matches('\u{feff}'))
                        .map_err(|e| e.to_string())
                }) {
                Ok(cfg) => {
                    log::warn!(
                        "config: config.json is MISSING but a {len}-byte backup exists at {} — \
                         restoring it. Your profiles and bindings were NOT lost.",
                        backup.display()
                    );
                    if let Err(e) = save_to_disk(&cfg, &path) {
                        log::error!("config: failed to write the restored config: {e}");
                    }
                    return Arc::new(RwLock::new(cfg));
                }
                Err(e) => log::warn!("config: backup at {} unreadable ({e})", backup.display()),
            }
        }

        log::info!("config: no config.json found — first run, seeding defaults");
        let cfg = try_parse_v11().unwrap_or_else(generate_defaults);
        // Write initial config to disk
        if let Err(e) = save_to_disk(&cfg, &path) {
            log::error!("config: failed to write initial config.json: {e}");
        }
        cfg
    };

    // PROBLEM 94 — the config EXISTS but a much richer backup does too. That
    // is the signature of a reset or a partial wipe. Do NOT auto-restore: a
    // user who deliberately reset their profile would find it undone, which
    // is its own kind of data loss. Say so loudly instead, with the path.
    {
        let current = std::fs::metadata(&path).map(|m| m.len() as usize).unwrap_or(0);
        if let Some((backup, len)) = newest_richer_backup(current.saturating_add(current / 2)) {
            log::warn!(
                "config: the current config is {current} bytes but a {len}-byte backup exists at \
                 {}. If your profiles or bindings vanished, that backup has them — copy it over \
                 config.json to restore.",
                backup.display()
            );
        }
    }

    Arc::new(RwLock::new(config))
}

/// Persist the config to disk atomically (write-then-rename).
pub fn save(config: &AppConfig) -> Result<(), String> {
    let path = config_path();
    // PROBLEM 180 — republish which optional special keys are bound. This is
    // the single funnel every mutation site goes through, which is why the
    // publish belongs here and not at each caller. Paired with the call in
    // lib.rs's startup load: the atomic starts at 0, so without that one every
    // bit is clear from launch until the first save.
    crate::hook::publish_bound_specials(config);
    // PROBLEM 180 again, for the App-exceptions list — published from BOTH
    // here and the startup load in lib.rs. Published from save alone, the
    // feature would be dead from launch until the user happened to save.
    crate::hook::exclusions::publish_excluded_apps(config);
    // PROBLEM 206 — the same PROBLEM 180 rule, fourth instance: the
    // pointer-HUD toggle is read on the hook path as an atomic. Published
    // HERE, in the one funnel every mutation goes through — reset_config and
    // friends included; publishing at the save_config COMMAND would miss
    // them, which is PROBLEM 180's exact bug.
    crate::hook::publish_pointer_hud_activation(config);
    // PROBLEM 263, the same PROBLEM 180 rule a sixth time — the middle-button
    // ring trigger is read on the MOUSE callback as an atomic, so it has to be
    // republished here, in the one funnel every mutation goes through, and
    // seeded at the startup load in lib.rs.
    crate::hook::publish_middle_button_ring(config);
    // PROBLEM 195, same rule a third time — the crash-reporting kill switch is
    // a runtime-checked atomic (the panic hook cannot take this struct's lock),
    // so it has to be republished on every save AND seeded at the startup load
    // in lib.rs. This is the line that makes the "Don't send logs" switch take
    // effect immediately instead of at the next launch.
    crate::telemetry::publish(config);
    // `hud_band_count` and `hud_show_specials` are deliberately ABSENT from
    // this list. Nothing on the hook path reads either of them: both are read
    // on the engine thread inside a config borrow that already happens, and
    // both reach the overlay page as a Tauri event from `save_config`. An
    // atomic here would be a second source of truth with no reader — see
    // `hud_band_count`'s comment in schema.rs. Do not add one.
    save_to_disk(config, &path).map_err(|e| e.to_string())
}

fn save_to_disk(config: &AppConfig, path: &PathBuf) -> std::io::Result<()> {
    // Ensure directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // Atomic write: write to .tmp then rename
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, path)?;

    // Info, not debug: disk writes are rare and this line is the primary
    // evidence for save-frequency bugs (the double-save was found with it).
    log::info!("config: saved {} bytes to {}", json.len(), path.display());
    write_backup(&json);
    Ok(())
}

/// PROBLEM 94 — where rolling backups live.
///
/// Deliberately NOT under the app's data dir and NOT under a folder named
/// after the product or bundle id: an uninstaller that removes
/// `%APPDATA%\Spaceadom` or `%LOCALAPPDATA%\com.spaceadom.app` would take the
/// backups with it, which is precisely the case they exist for.
///
/// PROBLEM 254 — that reasoning does not apply to a portable copy. There is
/// no separate uninstall step to survive: the whole point of "portable" is
/// one self-contained folder, so backups belong INSIDE `portable::data_root`
/// with everything else, not off in `%LOCALAPPDATA%` where deleting the
/// portable folder would leave them orphaned on the machine — the opposite
/// of what a portable user expects when they delete the folder.
pub fn backup_dir() -> PathBuf {
    if crate::portable::is_portable() {
        return crate::portable::data_root().join("backups");
    }
    let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::startup::data_dir());
    base.join("SpaceadomBackups")
}

/// PROBLEM 94 — keep the last N configs so a wipe is recoverable.
///
/// On 2026-08-13 this user's config went from 67222 bytes to 12155 bytes of
/// factory defaults: profile "hi", 104 bindings and 5 custom base64 icons,
/// gone. It was only PARTIALLY recovered, from a Windows Volume Shadow Copy
/// that happened to exist — an accident, not a feature. The app writes its
/// own backups now.
///
/// Best-effort throughout: a backup failure must never break a config save.
/// Only writes when the content actually differs from the newest backup, so
/// an idle app does not churn the disk.
/// The newest backup that actually parses, with the path it came from.
///
/// PROBLEM 159. `write_backup` has kept timestamped copies since PROBLEM 102,
/// and until now nothing ever read them back automatically — the log told the
/// user a backup existed and left them to copy it by hand. A friend who has
/// never opened that folder will not do that; they will see an app that forgot
/// their bindings.
///
/// Newest first, and a backup that does not parse is SKIPPED rather than
/// aborting the search: corruption tends to hit the most recent write, which
/// is exactly the one a naive "restore the latest" would pick.
fn newest_valid_backup() -> Option<(AppConfig, std::path::PathBuf)> {
    newest_valid_backup_in(&backup_dir())
}

/// The testable half. Split out because this is a RECOVERY branch — a user
/// only reaches it after something has already gone wrong, which is precisely
/// the kind of code that ships unexercised and fails when it finally runs
/// (PROBLEM 118's lesson, and CLAUDE.md's rule for when to add a test).
fn newest_valid_backup_in(dir: &std::path::Path) -> Option<(AppConfig, std::path::PathBuf)> {
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    // By MODIFIED TIME, not by filename: the name carries a timestamp today,
    // and sorting by a naming convention breaks silently the day it changes.
    files.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    for p in files.into_iter().rev() {
        let Ok(raw) = std::fs::read_to_string(&p) else { continue };
        if let Ok(cfg) = serde_json::from_str::<AppConfig>(raw.trim_start_matches('\u{feff}')) {
            return Some((cfg, p));
        }
    }
    None
}

fn write_backup(json: &str) {
    // Retention is handled by prune_backups (PROBLEM 102) — every save from
    // the last hour, one per hour for a day, one per day for a week.
    let dir = backup_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }

    let mut existing: Vec<_> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("config-")
            })
            .collect(),
        Err(_) => return,
    };
    existing.sort_by_key(|e| e.file_name());

    // Unchanged since the last backup? Nothing to do.
    if let Some(newest) = existing.last() {
        if let Ok(prev) = std::fs::read_to_string(newest.path()) {
            if prev == json {
                return;
            }
        }
    }

    // Timestamped from the system clock via a monotonic-ish counter: no chrono
    // dependency here, and the ordering is what matters, not the wall time.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let target = dir.join(format!("config-{stamp}.json"));
    if let Err(e) = std::fs::write(&target, json) {
        log::debug!("config: backup write failed ({e}) — the save itself succeeded");
        return;
    }

    prune_backups(&dir, stamp);
    log::debug!("config: backup written to {}", target.display());
}

// ---------------------------------------------------------------------------
// One profile as a file — export, import, and the pre-delete backup
// ---------------------------------------------------------------------------

/// Serialise ONE profile in the `ProfileExport` shape. Pretty-printed: this is
/// a file a person may open, and a one-line 26-binding blob is not readable.
pub fn profile_export_json(p: &Profile) -> Result<String, String> {
    serde_json::to_string_pretty(&ProfileExport::of(p))
        .map_err(|e| format!("Could not serialise the profile: {e}"))
}

/// Read a `.json` back into a `Profile`, or say why it is not one.
///
/// **THE ERROR STRINGS ARE THE FEATURE.** Import is the one path here where
/// the input comes from outside the app entirely, so "that file is not a
/// Spaceadom profile" has to be distinguishable from "that file is damaged" —
/// a bare `serde_json` message names a byte offset, which tells a user
/// nothing. Every branch below says what was expected in plain words.
///
/// The name is NOT validated here and NOT deduped here: the caller owns the
/// live profile list and is the only thing that can know what "already taken"
/// means. `parse` parses.
pub fn parse_profile_export(raw: &str) -> Result<Profile, String> {
    // The same BOM tolerance the main config load has (2026-08-10): PowerShell
    // 5.1's `-Encoding UTF8` adds one and serde_json rejects it outright.
    let raw = raw.trim_start_matches('\u{feff}');

    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| format!("That file is not valid JSON ({e})."))?;
    let Some(obj) = value.as_object() else {
        return Err("That file is not a Spaceadom profile — it is JSON, but not an object.".into());
    };
    let Some(marker) = obj.get("spaceadom_profile").and_then(|v| v.as_u64()) else {
        // The single most likely wrong file is the user's whole config.json,
        // which lives in the same folder as the backups — say so by name.
        let hint = if obj.contains_key("profiles") {
            " That looks like a whole config.json — Import takes ONE profile, exported \
             from this popover."
        } else {
            ""
        };
        return Err(format!(
            "That file is not a Spaceadom profile export.{hint}"
        ));
    };
    if marker as u32 > ProfileExport::VERSION {
        return Err(format!(
            "That profile was exported by a NEWER version of Spaceadom (format {marker}; \
             this build understands {}). Update Spaceadom and try again.",
            ProfileExport::VERSION
        ));
    }

    let export: ProfileExport = serde_json::from_value(value)
        .map_err(|e| format!("That profile file is damaged and could not be read ({e})."))?;

    if export.name.trim().is_empty() {
        return Err("That profile file has no name in it.".into());
    }
    // An emoji that fails validation is DROPPED, not an error: the bindings are
    // what the user came for, and refusing an otherwise-good import over a
    // decoration would be the wrong trade. Logged so it is not silent.
    let emoji = match export.emoji {
        Some(e) if emoji_is_valid(&e) => Some(e),
        Some(e) => {
            log::warn!(
                "import_profile: dropped an invalid emoji ({} char(s)) from '{}' — the \
                 bindings were imported unchanged",
                e.chars().count(),
                export.name
            );
            None
        }
        None => None,
    };

    Ok(Profile {
        name: export.name.trim().to_string(),
        bindings: export.bindings,
        emoji,
    })
}

/// Write a timestamped copy of ONE profile beside the rolling config backups,
/// and return where it went.
///
/// Called by `delete_profile` BEFORE the profile is removed. The 10-second
/// Undo in the popover is the fast way back and covers the mis-click; this
/// covers the other case — the user who notices next week, by which time the
/// undo stack is long gone and `config-*.json` may have been pruned past it.
///
/// Best-effort by design: a backup failure must never stop a delete the user
/// asked for. It logs at INFO because the path is the whole point — a backup
/// nobody can find is not a backup (`log::debug` would not reach debug.log's
/// default level).
///
/// The `profile-` prefix keeps these clear of `prune_backups`, which only ever
/// matches `config-<stamp>.json`. These files are NOT pruned: one profile is a
/// few KB and deletes are rare, so there is nothing to ration.
pub fn write_profile_backup(p: &Profile) -> Option<PathBuf> {
    let json = profile_export_json(p)
        .map_err(|e| log::warn!("profile backup: {e}"))
        .ok()?;
    let dir = backup_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("profile backup: could not create {} ({e})", dir.display());
        return None;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let target = dir.join(format!("profile-{}-{stamp}.json", sanitise_for_filename(&p.name)));
    match std::fs::write(&target, &json) {
        Ok(()) => {
            log::info!(
                "profile backup: wrote '{}' ({} bytes) to {} before deleting it",
                p.name,
                json.len(),
                target.display()
            );
            Some(target)
        }
        Err(e) => {
            log::warn!("profile backup: write to {} failed ({e})", target.display());
            None
        }
    }
}

/// Make a profile name safe to put in a FILENAME.
///
/// PROBLEM 197 loosened profile names to "any 1-24 characters without control
/// codes", and the reasoning it recorded was explicit: a name is never a
/// filename. **This function is the one place that stopped being true**, so
/// the constraint lives here and nowhere else — the name in `config.json` is
/// still whatever the user typed.
///
/// Anything outside `[A-Za-z0-9._-]` becomes `_`. **The SEPARATORS are the
/// part that matters**: `\`, `/` and `:` are what a traversal needs, and once
/// they are substituted a surviving `..` is two literal characters in the
/// middle of a filename, not a parent-directory hop. `<>"|?*` go the same way
/// because Windows reserves them.
///
/// A stem left as nothing but padding — "🚀" (every character substituted) or
/// ".." (nothing but dots) — becomes `profile`, so the result can never name a
/// directory and never reads like a bug (`profile--1234.json`).
fn sanitise_for_filename(name: &str) -> String {
    let out: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' })
        .take(24)
        .collect();
    if out.chars().all(|c| matches!(c, '_' | '.' | '-')) {
        "profile".to_string()
    } else {
        out
    }
}

/// What the Export save dialog should suggest as a filename STEM (no `.json`).
///
/// Same sanitiser as the backup writer, deliberately: the two files are the
/// same format, and a user who finds one in the backups folder and one in
/// Documents should not have to work out that they are the same thing.
pub fn suggested_export_filename(profile_name: &str) -> String {
    sanitise_for_filename(profile_name)
}

/// PROBLEM 102 — keep backups SPREAD ACROSS TIME, not just the newest N.
///
/// The first version was a plain 10-deep ring. It failed the first time it
/// mattered: while the user was actively binding keys, ten saves happened
/// within minutes, so all ten copies were from the last few minutes and the
/// 84 KB config from 23:16 had already been pushed out by the time anyone
/// looked for it. A count-based ring has no time depth exactly when the user
/// is most active — which is exactly when mistakes get made.
///
/// Raising the count would only buy a bigger constant. This keeps:
///   * EVERY save from the last hour   — undo a mistake you notice at once
///   * ONE per hour for 24 hours       — undo one you notice after lunch
///   * ONE per day for 7 days          — undo one you notice next week
///
/// Roughly 30-40 files at steady state, a couple of MB. The newest file in
/// each bucket wins, so what survives is always the most complete version of
/// that period.
fn prune_backups(dir: &PathBuf, now: u64) {
    const HOUR: u64 = 3_600;
    const DAY: u64 = 86_400;

    let mut stamped: Vec<(u64, PathBuf)> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let ts = name
                    .strip_prefix("config-")?
                    .strip_suffix(".json")?
                    .parse::<u64>()
                    .ok()?;
                Some((ts, e.path()))
            })
            .collect(),
        Err(_) => return,
    };
    // Newest first, so the first file seen in any bucket is the one to keep.
    stamped.sort_by(|a, b| b.0.cmp(&a.0));

    let mut seen_hour: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut seen_day: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut removed = 0usize;

    for (ts, path) in stamped {
        let age = now.saturating_sub(ts);
        let keep = if age <= HOUR {
            true // the last hour is kept in full
        } else if age <= DAY {
            seen_hour.insert(ts / HOUR) // one per hour
        } else if age <= 7 * DAY {
            seen_day.insert(ts / DAY) // one per day
        } else {
            false // older than a week
        };
        if !keep && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        log::debug!("config: pruned {removed} backup(s) outside the keep windows");
    }
}

/// PROBLEM 94 — the newest backup that actually contains user data, if any.
///
/// "Contains user data" is judged by byte size: a factory-defaults config is
/// ~12 KB, and every real one observed on this machine was 21-67 KB. A
/// backup no bigger than the current file is not worth offering.
pub fn newest_richer_backup(current_len: usize) -> Option<(PathBuf, usize)> {
    let dir = backup_dir();
    let mut best: Option<(PathBuf, usize, std::ffi::OsString)> = None;
    for e in std::fs::read_dir(&dir).ok()?.flatten() {
        let name = e.file_name();
        if !name.to_string_lossy().starts_with("config-") {
            continue;
        }
        let len = e.metadata().ok()?.len() as usize;
        if len <= current_len {
            continue;
        }
        // Must actually parse, or it is not a restore candidate.
        let Ok(raw) = std::fs::read_to_string(e.path()) else { continue };
        if serde_json::from_str::<AppConfig>(raw.trim_start_matches('\u{feff}')).is_err() {
            continue;
        }
        if best.as_ref().map_or(true, |(_, _, n)| name > *n) {
            best = Some((e.path(), len, name));
        }
    }
    best.map(|(p, l, _)| (p, l))
}

fn generate_defaults() -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.profiles = defaults::generate();
    cfg
}

/// Attempt to parse V11 AHK bindings from install-v11.ps1 in the workspace.
/// Returns `None` if the file is missing or the format is unrecognised.
fn try_parse_v11() -> Option<AppConfig> {
    // Look for install-v11.ps1 relative to current exe or CWD
    let candidates = [
        PathBuf::from("install-v11.ps1"),
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("install-v11.ps1"),
    ];

    let raw = candidates.iter().find_map(|p| std::fs::read_to_string(p).ok())?;
    log::info!("config: found install-v11.ps1 — attempting V11 parse");

    // The AHK script embeds profile maps as:
    //   Static Founders := Map("a", ["app.exe",""], "b", ["","https://url"], ...)
    // We use a simple regex-style approach with string scanning.
    let profiles = parse_ahk_profiles(&raw);

    if profiles.is_empty() {
        log::warn!("config: V11 parse yielded no profiles — falling back to hardcoded defaults");
        return None;
    }

    let mut cfg = AppConfig::default();
    cfg.profiles = profiles;
    Some(cfg)
}

/// Very lightweight parser for the AHK Map() literal format.
/// Extracts only the three named profiles by scanning for their Static Map blocks.
fn parse_ahk_profiles(src: &str) -> Vec<schema::Profile> {
    let profile_names = ["Founders", "Gamers", "Professionals"];
    let mut result = Vec::new();

    for name in profile_names {
        let marker = format!("Static {name} := Map(");
        if let Some(start) = src.find(&marker) {
            let slice = &src[start + marker.len()..];
            // Find the closing ')' of this Map call (count parens)
            let mut depth = 1usize;
            let mut end = 0;
            for (i, ch) in slice.char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let map_body = &slice[..end];
            let bindings = parse_map_body(map_body);
            if !bindings.is_empty() {
                result.push(schema::Profile {
                    name: name.to_string(),
                    bindings,
                    // The v11 AutoHotkey script has no emoji concept, so an
                    // imported profile starts without one — which is exactly
                    // the state `Profile::emoji` documents as normal.
                    emoji: None,
                });
            }
        }
    }

    result
}

/// Parse `"key", ["app", "url"], "key2", ["app2", "url2"], ...`
fn parse_map_body(body: &str) -> schema::BindingMap {
    let mut map = schema::BindingMap::new();
    // Tokenise on `"` delimiters
    let tokens: Vec<&str> = body.split('"').collect();
    // Structure: idx 0=whitespace, 1=key, 2=, [", 3=app, 4=", ",", 5=web, 6=...
    let mut i = 1;
    while i + 4 < tokens.len() {
        let key = tokens[i].trim();
        if key.len() == 1 && key.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false) {
            let app_raw = tokens.get(i + 2).unwrap_or(&"").trim();
            let web_raw = tokens.get(i + 4).unwrap_or(&"").trim();

            let app = if app_raw.is_empty() { None } else { Some(app_raw.to_string()) };
            let web = if web_raw.is_empty() { None } else { Some(web_raw.to_string()) };
            let label = app.as_deref()
                .map(|s| s.trim_end_matches(".exe").to_string())
                .or_else(|| web.as_deref().map(|s| {
                    s.trim_start_matches("https://")
                        .split('/')
                        .next()
                        .unwrap_or(s)
                        .to_string()
                }));

            map.insert(
                key.to_string(),
                // The v11 AutoHotkey script this parses has no concept of a
                // browser profile, so these are always absent on this route.
                schema::KeyBinding {
                    app,
                    web_url: web,
                    label,
                    icon_override: None,
                    browser_exe: None,
                    browser_profile_dir: None,
                    browser_profile_name: None,
                    // PROBLEM 267 — no favicon on this route either; the
                    // editor fetches one when the key is next edited.
                    site_icon: None,
                },
            );
            i += 6; // advance past this entry
        } else {
            i += 1;
        }
    }
    map
}

/// PROBLEM 255 — the `"auto"` theme resolver, the Rust third of a rule that
/// is also written in `main.ts::resolveTheme` and `src/theme-resolve.ts`.
/// Pure: no registry, no filesystem, no `AppConfig`.
#[cfg(test)]
mod auto_theme_tests {
    use super::*;

    #[test]
    fn a_named_theme_is_returned_untouched_whatever_the_os_says() {
        for os in [None, Some(true), Some(false)] {
            assert_eq!(resolve_theme("earthy", os), "earthy");
            assert_eq!(resolve_theme("warcry", os), "warcry");
            assert_eq!(resolve_theme("starry", os), "starry");
        }
    }

    #[test]
    fn auto_follows_the_os_and_never_becomes_warcry() {
        assert_eq!(resolve_theme(THEME_AUTO, Some(true)), "starry");
        assert_eq!(resolve_theme(THEME_AUTO, Some(false)), "earthy");
    }

    /// The fallback the three copies of this rule have to share. An
    /// unanswerable OS question is daylight, NOT dark — a dashboard and an
    /// overlay that disagreed about this would show two palettes at once on
    /// exactly the machines least able to say why.
    #[test]
    fn an_unanswerable_os_question_is_daylight() {
        assert_eq!(resolve_theme(THEME_AUTO, None), "earthy");
        assert!(!dark_mode_for(THEME_AUTO, None));
    }

    /// The empty string is what a pre-PROBLEM-144 config carries before the
    /// migration in `load_or_init` runs. It must not be able to crash or to
    /// mean "dark" on its own.
    #[test]
    fn an_empty_or_unknown_theme_resolves_like_auto() {
        assert_eq!(resolve_theme("", Some(true)), "starry");
        assert_eq!(resolve_theme("", Some(false)), "earthy");
        assert_eq!(resolve_theme("nocturne-2", Some(true)), "starry");
    }

    /// THE REGRESSION THIS FIX EXISTS FOR. `cfg.theme != "earthy"` — the old
    /// one-liner — is `true` for `"auto"`, so every new install went dark on
    /// its second launch regardless of the OS setting (PROBLEM 255's own
    /// "found but could not fix" item 1).
    #[test]
    fn auto_in_daylight_is_not_dark_mode() {
        assert!(
            !dark_mode_for(THEME_AUTO, Some(false)),
            "\"auto\" with a light OS must be dark_mode=false — the old \
             `theme != \"earthy\"` recompute got this wrong for every new install"
        );
        assert!(dark_mode_for(THEME_AUTO, Some(true)));
        // And the named themes still map the way the overlay has always
        // expected: everything that is not Earthy sits on the nocturne base.
        assert!(!dark_mode_for("earthy", Some(true)));
        assert!(dark_mode_for("warcry", Some(false)));
        assert!(dark_mode_for("starry", Some(false)));
    }

    /// The OS probe itself is not a pure function and cannot assert a value —
    /// this machine's setting is whatever the owner chose. What it CAN assert
    /// is that it answers without panicking and that its answer is usable, so
    /// a broken registry path shows up as a test failure rather than as a
    /// silent permanent `None` that reads exactly like "the user picked
    /// light".
    #[test]
    fn the_os_probe_answers_without_panicking() {
        let answer = os_prefers_dark();
        // Whatever it says, feeding it back through the pure rule must land
        // on one of the two palettes "auto" is allowed to produce.
        let resolved = resolve_theme(THEME_AUTO, answer);
        assert!(
            resolved == "earthy" || resolved == "starry",
            "\"auto\" resolved to {resolved}, which is not one of the two palettes it may \
             ever produce"
        );
    }
}

#[cfg(test)]
mod backup_recovery_tests {
    use super::*;
    use std::io::Write;

    /// A unique scratch dir per test — these run on parallel threads
    /// (PROBLEM 130: four tests sharing one static was a flaky-test bug here
    /// before), so nothing may share a path.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("spaceadom-bk-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        let p = dir.join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        f.sync_all().unwrap();
        // Windows timestamps are coarse; without this the "newest" ordering is
        // a coin flip and the test passes or fails at random.
        std::thread::sleep(std::time::Duration::from_millis(20));
        p
    }

    fn valid_json(profile: &str) -> String {
        let mut c = AppConfig::default();
        c.active_profile = profile.to_string();
        serde_json::to_string(&c).unwrap()
    }

    #[test]
    fn picks_the_newest_backup_that_parses() {
        let d = scratch("newest");
        write(&d, "a.json", &valid_json("Older"));
        write(&d, "b.json", &valid_json("Newer"));
        let (cfg, from) = newest_valid_backup_in(&d).expect("a backup must be found");
        assert_eq!(cfg.active_profile, "Newer", "the NEWEST valid backup wins");
        assert!(from.ends_with("b.json"));
    }

    /// The case the whole feature exists for: corruption hits the most recent
    /// write, so a naive "restore the latest" restores the broken one.
    #[test]
    fn skips_a_corrupt_newest_and_falls_back() {
        let d = scratch("skip");
        write(&d, "a.json", &valid_json("Good"));
        write(&d, "b.json", "{ this is not json");
        let (cfg, from) = newest_valid_backup_in(&d).expect("must fall back past the corrupt one");
        assert_eq!(cfg.active_profile, "Good");
        assert!(from.ends_with("a.json"));
    }

    #[test]
    fn no_backups_at_all_is_none_not_a_panic() {
        assert!(newest_valid_backup_in(&scratch("empty")).is_none());
        // A directory that does not exist must also be None, not a panic —
        // a first-run machine has no backup folder yet.
        assert!(newest_valid_backup_in(std::path::Path::new(r"Z:\no\such\dir")).is_none());
    }

    #[test]
    fn every_backup_corrupt_is_none() {
        let d = scratch("allbad");
        write(&d, "a.json", "nope");
        write(&d, "b.json", r#"{"version":"#);
        assert!(newest_valid_backup_in(&d).is_none(), "must regenerate, not half-restore");
    }
}

/// Export / Import / the pre-delete backup — the ONE profile file format.
#[cfg(test)]
mod profile_file_tests {
    use super::*;

    fn sample() -> Profile {
        let mut b = crate::config::BindingMap::new();
        b.insert("a".to_string(), KeyBinding { app: Some("brave.exe".into()), ..Default::default() });
        b.insert(
            "g".to_string(),
            KeyBinding {
                web_url: Some("https://github.com".into()),
                browser_profile_dir: Some("Profile 1".into()),
                browser_profile_name: Some("nur.arpon".into()),
                ..Default::default()
            },
        );
        Profile { name: "Founders".into(), bindings: b, emoji: Some("👨‍👩‍👧".into()) }
    }

    /// Export → Import must be lossless, INCLUDING the browser-profile pin
    /// fields. Those three are optional on `KeyBinding` and were once deleted
    /// by omission on a different round trip (see the `#[serde(default)]`
    /// history on `KeyBinding::browser_exe`) — an export that quietly dropped
    /// them would look perfect and lose the pin.
    #[test]
    fn a_profile_survives_export_and_import_intact() {
        let p = sample();
        let json = profile_export_json(&p).expect("export");
        let back = parse_profile_export(&json).expect("import");

        assert_eq!(back.name, "Founders");
        assert_eq!(back.emoji.as_deref(), Some("👨‍👩‍👧"), "the ZWJ emoji must survive the file");
        assert_eq!(back.bindings.len(), 2);
        assert_eq!(back.bindings["a"].app.as_deref(), Some("brave.exe"));
        assert_eq!(back.bindings["g"].browser_profile_dir.as_deref(), Some("Profile 1"));
        assert_eq!(back.bindings["g"].browser_profile_name.as_deref(), Some("nur.arpon"));
    }

    /// THE REASON `spaceadom_profile` EXISTS. These files land in the same
    /// folder as the rolling whole-config backups, and `newest_valid_backup_in`
    /// tries to parse every `*.json` in there as an `AppConfig`. If a profile
    /// export could parse as a config, a recovery would silently restore a
    /// factory-default app with one profile in it.
    #[test]
    fn profile_export_is_not_a_config() {
        let json = profile_export_json(&sample()).expect("export");
        assert!(
            serde_json::from_str::<AppConfig>(&json).is_err(),
            "a profile export must NOT parse as a whole config — recovery reads this folder"
        );
        // And the other direction: a whole config must not import as a profile.
        let cfg = serde_json::to_string(&AppConfig::default()).unwrap();
        let err = parse_profile_export(&cfg).expect_err("a config is not a profile");
        assert!(
            err.contains("config.json"),
            "the error must NAME the likely mistake, not just refuse: {err}"
        );
    }

    #[test]
    fn junk_is_refused_in_words_a_person_can_act_on() {
        assert!(parse_profile_export("not json at all").unwrap_err().contains("valid JSON"));
        assert!(parse_profile_export("[1,2,3]").unwrap_err().contains("not an object"));
        assert!(parse_profile_export("{}").unwrap_err().contains("profile export"));
        // A file from a future format version must say so rather than
        // half-importing whatever fields happen to still line up.
        let future = r#"{"spaceadom_profile":99,"name":"X","bindings":{}}"#;
        let err = parse_profile_export(future).unwrap_err();
        assert!(err.contains("NEWER version"), "{err}");
    }

    /// A bad emoji must not cost the user their bindings.
    #[test]
    fn an_invalid_emoji_is_dropped_but_the_bindings_import() {
        let raw = r#"{"spaceadom_profile":1,"name":"X","emoji":"AB",
                      "bindings":{"a":{"app":"x.exe","web_url":null,"label":null}}}"#;
        let p = parse_profile_export(raw).expect("must still import");
        assert!(p.emoji.is_none(), "two clusters is not an emoji");
        assert_eq!(p.bindings.len(), 1, "the bindings are what the user came for");
    }

    /// PROBLEM 197 said a profile name is never a filename. `write_profile_backup`
    /// is the one place that became untrue, so the sanitiser is what keeps it so.
    #[test]
    fn a_profile_name_can_never_escape_the_backup_folder() {
        // The `..` survive — and that is FINE, which is the point worth
        // recording. A traversal needs a SEPARATOR, and every `\` and `/` has
        // become `_`, so what is left is two dots inside one filename.
        assert_eq!(sanitise_for_filename(r"..\..\windows\system32"), ".._.._windows_system32");
        assert_eq!(sanitise_for_filename("My Profile"), "My_Profile");
        assert_eq!(sanitise_for_filename("a/b:c*d?"), "a_b_c_d_");
        // A stem of nothing but padding must not name a directory.
        assert_eq!(sanitise_for_filename(".."), "profile");
        assert_eq!(sanitise_for_filename("."), "profile");
        assert_eq!(sanitise_for_filename("🚀"), "profile", "an all-emoji name still needs a stem");
        // Whatever comes out, joining it to the backup dir must stay INSIDE it.
        for name in ["..", r"..\..\etc", "a/b", "🚀", "normal"] {
            let joined = std::path::Path::new(r"C:\bk").join(sanitise_for_filename(name));
            assert_eq!(
                joined.parent(),
                Some(std::path::Path::new(r"C:\bk")),
                "'{name}' escaped the folder as {}",
                joined.display()
            );
        }
        assert!(sanitise_for_filename(&"x".repeat(80)).chars().count() <= 24);
    }
}
