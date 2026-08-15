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
                    let mut dirty = false;
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
                            "config: JSON parse error ({e}) — original preserved at {}, regenerating defaults",
                            backup.display()
                        ),
                        Err(be) => log::error!(
                            "config: JSON parse error ({e}) AND backup failed ({be}) — regenerating defaults"
                        ),
                    }
                    generate_defaults()
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
pub fn backup_dir() -> PathBuf {
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
                });
            }
        }
    }

    result
}

/// Parse `"key", ["app", "url"], "key2", ["app2", "url2"], ...`
fn parse_map_body(body: &str) -> std::collections::HashMap<String, schema::KeyBinding> {
    let mut map = std::collections::HashMap::new();
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
                schema::KeyBinding { app, web_url: web, label, icon_override: None },
            );
            i += 6; // advance past this entry
        } else {
            i += 1;
        }
    }
    map
}
