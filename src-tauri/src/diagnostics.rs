//! diagnostics.rs — PROBLEM 253. "Report a problem": one zip the user can
//! attach to a GitHub issue, and NOTHING is ever uploaded from here.
//!
//! # Why a file and not an upload
//!
//! The app already reports crashes to Sentry (PROBLEM 195), and that path is
//! deliberately narrow: one scrubbed message, no log, no config, and a switch
//! that turns it off. It answers "did it crash". It cannot answer "why does
//! Space+D open the wrong window on YOUR machine", because the things that
//! settle that — the last few thousand lines of `debug.log`, which keys are
//! bound to what, which monitors are attached, whether a rival copy is
//! installed — are far too large and far too personal to send automatically.
//!
//! So this builds the evidence into a file, opens Explorer with it selected,
//! and stops. **The user decides whether it leaves the machine.** That is the
//! whole reason it is a zip on the desktop side of the wire rather than an
//! HTTP POST, and nothing in this module may ever grow a network call.
//!
//! # What is in the bundle, and what is deliberately not
//!
//! | Entry | Why |
//! | --- | --- |
//! | `description.txt` | what the user typed, verbatim |
//! | `debug.log.tail.txt` | the last [`LOG_TAIL_LINES`] lines, SCRUBBED line by line |
//! | `debug.log.0.tail.txt` | the last [`ROLLED_TAIL_LINES`] of the rolled log, if it exists, SCRUBBED |
//! | `config.json` | SCRUBBED — see below |
//! | `system.txt` | build, install kind, Windows version, monitors, hook health, safe mode, rival verdict — every path and log line SCRUBBED |
//! | `listing-data-dir.txt` | the data dir's file NAMES and SIZES only, header path and names SCRUBBED |
//! | `listing-backups.txt` | the same for `%LOCALAPPDATA%\SpaceadomBackups` |
//!
//! **REVIEW FIXES 2026-09-05 (H3) — the word SCRUBBED on the log rows is new,
//! and until it was true this module's central promise was false.** The
//! config was scrubbed from the first version; the LOGS were shipped verbatim,
//! and this app logs every foreground window's TITLE on every Smart Search
//! press, along with the exe path, the data dir, the rival-install path and
//! every URL it fetches. A user is told the report contains nothing personal
//! and then attaches it to a public issue tracker. [`scrub_for_report`] is
//! what closes it; the four ways it is stricter than
//! [`crate::telemetry::scrub`] are on that function.
//!
//! **The listings are names and sizes, never contents.** `picker-cache.json`
//! alone is ~800 KB of every application installed on the machine, and the
//! backups folder is a stack of complete configs. Neither belongs in a report
//! that is going to a public issue tracker; that they EXIST, and how big they
//! are, is the diagnostic — a zero-byte or missing picker cache explains an
//! empty app picker on its own.
//!
//! # The config is scrubbed, and one field is redacted outright
//!
//! Every string in `config.json` goes through [`crate::telemetry::scrub`] — the
//! same function the Sentry paths use, so there is one definition of "safe to
//! send" in this codebase and not two. That turns `C:\Users\<name>\…` into
//! `<path>`, any non-local URL into `<url>`, and anything address-shaped into
//! `<email>`.
//!
//! `scrub` is not enough on its own, and the reason is specific:
//! **`browser_profile_name` holds an email LOCAL PART.** The browser-profile
//! feature reads the signed-in account out of each Chromium profile's
//! `Local State` and the UI shows only the part before the `@` — so a real
//! config on this machine contains `"browser_profile_name": "nur.arpon"`, which
//! has no `@`, is not address-shaped, and sails straight through `scrub`. It is
//! therefore redacted by NAME, before scrubbing, along with the other
//! account-label spellings in [`REDACTED_FIELDS`]. Extending that list is the
//! right way to handle a new identity-bearing field; widening `scrub` itself is
//! not, because `scrub` is shared with the crash reporter and a rule that makes
//! sense for a config key makes none for a stack trace.
//!
//! **Accepted cost, stated so nobody "fixes" it later:** an app path in a
//! binding becomes `<path>`, so the bundle cannot say WHICH exe a key launches.
//! The binding's `label` survives (it is a plain word like "Brave"), and the key
//! and profile structure survive, which is what the diagnostic actually needs.
//! Keeping the basename was considered and rejected: a file name is exactly
//! where a user's own name turns up.
//!
//! # Rules
//!
//! 1. **No network, ever.** Not a check, not a ping, not an "anonymous
//!    statistic". If a future version uploads, it does it somewhere else, with
//!    its own consent step.
//! 2. **Never fail the whole bundle because one input is missing.** A missing
//!    `debug.log.0`, an unreadable config, a data dir that will not enumerate —
//!    each becomes a one-line note INSIDE the zip. A user reporting a problem
//!    is already having a bad day; "could not build the report" is the worst
//!    possible answer, and the entry that failed is usually the evidence.
//! 3. **Every entry is written from a `&str` this module composed.** No file is
//!    ever streamed in whole, so there is no path by which an unscrubbed file
//!    ends up in the archive by accident.

use std::io::Write;

/// Lines of `debug.log` to carry. At this project's log volume that is roughly
/// the last 20-40 minutes of a busy session, which is where the fault the user
/// is reporting will be.
pub const LOG_TAIL_LINES: usize = 5_000;

/// Lines of the ROLLED log (`debug.log.0`) to carry. Fewer, because it is only
/// there to cover a fault that happened just before a 5 MB rotation.
pub const ROLLED_TAIL_LINES: usize = 2_000;

/// Longest description accepted. A text box has no limit; a zip entry should.
pub const MAX_DESCRIPTION: usize = 20_000;

/// The issue tracker the report is meant for.
pub const ISSUES_URL: &str = "https://github.com/nur-arpon/Spaceadom/issues";

/// Config keys whose VALUE is redacted outright, before scrubbing, because the
/// value is an account identity that is not address-shaped and would survive
/// [`crate::telemetry::scrub`] untouched. See the module header.
pub const REDACTED_FIELDS: &[&str] = &[
    "browser_profile_name",
    "account_label",
    "account_name",
    "account",
    "email",
    "user_email",
    "signed_in_as",
];

/// What replaces a redacted value.
pub const REDACTION: &str = "<redacted-account-label>";

/// The config field whose value is a base64 image, replaced by a note of its
/// size rather than shipped. REVIEW FIXES 2026-09-05 (H3).
pub const ICON_FIELD: &str = "icon_override";

// ---------------------------------------------------------------------------
// REVIEW FIXES 2026-09-05 (H3) — scrubbing the LOG, not just the config.
//
// **What was wrong.** The module header promised "one scrubbed message, no
// log, no config", and the config half was true. The log half was not: the
// bundle shipped `debug.log`'s last 5,000 lines and `debug.log.0`'s tail
// VERBATIM, plus a directory listing whose header is a full path. This app
// logs, in the ordinary course of working:
//
//   · every foreground window's TITLE, on every Smart Search press
//     (`engine/actions/focus_engine.rs:64` — `smart_search: proc='…'
//     title='…' -> …`). A window title is a document name, an email subject
//     line, a browser tab, a chat contact.
//   · the exe path, the install path, the data dir, the rival-install path,
//     the picker's file paths — every one of them under `C:\Users\<name>`.
//   · URLs, including the `web_url` bindings a user launches, query strings
//     and all.
//
// The user is told the report contains nothing personal, and then attaches it
// to a PUBLIC GitHub issue. That is the whole of the problem: this is the one
// place in the app where a privacy claim is made about a file that leaves the
// machine, and the claim was true of one entry out of six.
//
// `scrub_for_report` is deliberately STRICTER than `telemetry::scrub`, which
// it wraps rather than replaces — see the four differences on the function.
// ---------------------------------------------------------------------------

/// What replaces a window title.
pub const TITLE_REDACTION: &str = "<window title omitted>";

/// The one log format that carries a window title verbatim. Keyed on the exact
/// text `focus_engine.rs` writes, so a change there fails the test here rather
/// than quietly widening what a report contains.
const TITLE_MARKER: &str = "title='";

/// **Every log line and every path that goes into a report bundle passes
/// through here.** (REVIEW FIXES 2026-09-05, H3.)
///
/// Four things it does that [`crate::telemetry::scrub`] does not, each for a
/// reason that only applies to a file a user attaches to a public issue:
///
/// 1. **Window titles are masked**, before anything else runs, so a title that
///    happens to contain a path or an address is gone as a whole rather than
///    picked at. `telemetry::scrub` never had to care: a crash report carries a
///    stack, not a foreground window.
/// 2. **There is no localhost carve-out.** `telemetry::scrub` keeps
///    `tauri.localhost` and `127.0.0.1` URLs verbatim because they are the
///    app's own bundle and are what makes a minified JS stack readable. In a
///    report they are neither: a `http://localhost:5173/?token=…` from a dev
///    session, or any loopback URL with a query string, is exactly the shape
///    this is meant to remove.
/// 3. **The remainder after a space in a path is redacted too.**
///    `telemetry::scrub` consumes a path up to the first whitespace, so
///    `C:\Users\me\My Documents\tax.pdf` came out as `<path> Documents\tax.pdf`.
///    That was acceptable for a crash report (the user name, the drive and the
///    profile are what matter) and is not acceptable here.
/// 4. **The real profile name is removed wherever it appears**, path or not —
///    a registry key, a window title's remains, a service account, a log line
///    that names the user without a drive letter in front of it.
///
/// It is a whole-line function so it can be mapped over a log; it is safe to
/// apply to a single path too.
pub fn scrub_for_report(line: &str) -> String {
    scrub_for_report_with(line, &current_user_names())
}

/// The pure half: `user_names` is injected so the tests do not depend on who is
/// logged in on the machine running them.
pub fn scrub_for_report_with(line: &str, user_names: &[String]) -> String {
    let masked = mask_window_titles(line);
    let scrubbed = crate::telemetry::scrub(&masked);
    let no_urls = redact_all_urls(&scrubbed);
    let no_tails = redact_path_remainders(&no_urls);
    redact_user_names(&no_tails, user_names)
}

/// The names that identify THIS machine's owner: `%USERNAME%`, and the last
/// component of `%USERPROFILE%` in case they differ (a renamed account keeps
/// its original profile folder — a real and common case, and the folder name
/// is the one that appears in paths).
fn current_user_names() -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut push = |s: String| {
        // Two characters is the floor: a one-character user name would turn
        // every occurrence of that letter in the log into `<user>`, which
        // destroys the report to protect a name nobody could identify from.
        if s.len() >= 2 && !names.iter().any(|n: &String| n.eq_ignore_ascii_case(&s)) {
            names.push(s);
        }
    };
    if let Ok(u) = std::env::var("USERNAME") {
        push(u.trim().to_string());
    }
    if let Ok(p) = std::env::var("USERPROFILE") {
        if let Some(last) = p.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next() {
            push(last.to_string());
        }
    }
    names
}

/// Replace the text between `title='` and the closing `'` with
/// [`TITLE_REDACTION`], keeping the surrounding line intact so the DECISION the
/// line records is still readable — which is the only reason the line is in the
/// bundle at all.
fn mask_window_titles(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find(TITLE_MARKER) {
        let (before, after_marker) = rest.split_at(at + TITLE_MARKER.len());
        out.push_str(before);
        // The format is `title='{}' -> {}`. Prefer the `' -> ` terminator so a
        // title containing an apostrophe is still masked whole; fall back to
        // the next quote, and to the end of the line if there is neither.
        let end = after_marker
            .find("' -> ")
            .or_else(|| after_marker.find('\''))
            .unwrap_or(after_marker.len());
        out.push_str(TITLE_REDACTION);
        rest = &after_marker[end..];
    }
    out.push_str(rest);
    out
}

/// Replace every remaining `http://` / `https://` run with `<url>`.
///
/// Runs AFTER `telemetry::scrub`, so the only URLs still standing are the ones
/// that function deliberately kept: `tauri.localhost`, `localhost`, `127.0.0.1`
/// and `*.localhost`. In a diagnostics bundle they are removed too — see the
/// function doc for why the carve-out does not apply here.
fn redact_all_urls(text: &str) -> String {
    let ends =
        |ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | ')' | ']' | '>' | ',' | ';');
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let http = rest.find("http://");
        let https = rest.find("https://");
        let first = match (http, https) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let Some(at) = first else { break };
        out.push_str(&rest[..at]);
        out.push_str("<url>");
        let tail = &rest[at..];
        let stop = tail.find(ends).unwrap_or(tail.len());
        rest = &tail[stop..];
    }
    out.push_str(rest);
    out
}

/// `<path> Documents\tax.pdf` → `<path>`.
///
/// `telemetry::scrub` stops a path at the first whitespace, because a Windows
/// path legitimately contains spaces and there is no way to know where it ends.
/// The rule here is the conservative one that costs nothing: after a `<path>`
/// marker, keep swallowing single-space-separated tokens for exactly as long as
/// each one still contains a separator. A token with no `\` or `/` in it is a
/// word in a sentence, and the walk stops there.
fn redact_path_remainders(text: &str) -> String {
    const MARK: &str = "<path>";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(MARK) {
        out.push_str(&rest[..at + MARK.len()]);
        rest = &rest[at + MARK.len()..];
        loop {
            let Some(after_space) = rest.strip_prefix(' ') else { break };
            let end = after_space
                .find(|ch: char| ch.is_whitespace())
                .unwrap_or(after_space.len());
            let token = &after_space[..end];
            if token.is_empty() || !token.contains(['\\', '/']) {
                break;
            }
            rest = &after_space[end..];
        }
    }
    out.push_str(rest);
    out
}

/// Replace every case-insensitive occurrence of each name with `<user>`.
fn redact_user_names(text: &str, names: &[String]) -> String {
    let mut out = text.to_string();
    for name in names {
        if name.len() < 2 {
            continue;
        }
        let lower_name = name.to_ascii_lowercase();
        let mut result = String::with_capacity(out.len());
        let mut rest: &str = &out;
        loop {
            let hay = rest.to_ascii_lowercase();
            let Some(at) = hay.find(&lower_name) else { break };
            result.push_str(&rest[..at]);
            result.push_str("<user>");
            rest = &rest[at + lower_name.len()..];
        }
        result.push_str(rest);
        out = result;
    }
    out
}

// ---------------------------------------------------------------------------
// Pure helpers — every one of these is a plain string function so the tests do
// not need a machine that has ever run Spaceadom.
// ---------------------------------------------------------------------------

/// The last `n` lines of `text`, oldest first, with a header saying how much
/// was dropped.
///
/// The header is not decoration: a tail with no note reads as "this is the
/// whole log", and someone reading the bundle then concludes the app produced
/// 5,000 lines and stopped.
///
/// **REVIEW FIXES 2026-09-05 (H3): every line goes through
/// [`scrub_for_report`].** This function is the ONLY way a log reaches the
/// archive (`build_bundle` calls it for `debug.log` and `debug.log.0` and
/// copies nothing else), so scrubbing here is what makes the promise
/// structural rather than a habit at the call sites.
pub fn tail_lines(text: &str, n: usize, what: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();
    let start = total.saturating_sub(n);
    let head = if start == 0 {
        format!("=== {what} — all {total} line(s) ===\n")
    } else {
        format!(
            "=== {what} — the last {} of {total} line(s); {start} earlier line(s) not included ===\n",
            total - start
        )
    };
    let mut out = String::with_capacity(head.len() + text.len().min(n * 120));
    out.push_str(&head);
    for line in &lines[start..] {
        out.push_str(&scrub_for_report(line));
        out.push('\n');
    }
    out
}

/// Scrub a `config.json` for the bundle.
///
/// Walks the JSON rather than the raw text so that a redaction is keyed on the
/// FIELD NAME, which is the only reliable way to catch a value like
/// `"nur.arpon"` that carries an identity but no recognisable shape. Every
/// surviving string then goes through [`crate::telemetry::scrub`].
///
/// A config that will not parse is not passed through raw — that would be the
/// one path by which an unscrubbed personal file reaches the archive. It is
/// replaced by a note, and `scrub` is applied to the parse error itself (serde
/// error messages quote the input).
pub fn scrub_config_json(raw: &str) -> String {
    let parsed: serde_json::Value =
        match serde_json::from_str(raw.trim_start_matches('\u{feff}')) {
            Ok(v) => v,
            Err(e) => {
                return format!(
                    "config.json could not be parsed as JSON, so it is NOT included: {}\n\
                     (Including it raw would mean shipping an unscrubbed personal file, which \
                     this bundle never does.)\n",
                    crate::telemetry::scrub(&e.to_string())
                )
            }
        };
    let cleaned = scrub_value(&parsed, None);
    serde_json::to_string_pretty(&cleaned)
        .unwrap_or_else(|e| format!("config.json could not be re-serialised: {e}\n"))
}

/// Recursive half of [`scrub_config_json`]. `key` is the name of the field this
/// value was found under, or `None` at the root and inside arrays.
fn scrub_value(v: &serde_json::Value, key: Option<&str>) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::String(s) => {
            if key.is_some_and(is_redacted_field) {
                Value::String(REDACTION.to_string())
            } else if key.is_some_and(|k| k.eq_ignore_ascii_case(ICON_FIELD)) {
                // REVIEW FIXES 2026-09-05 (H3) — `icon_override` holds a
                // base64 PNG a user pasted or picked. It is not identifying
                // and it is not readable, but a keyboard with twenty custom
                // icons turns a 40 KB config into several megabytes of base64
                // that nobody will ever look at, in an archive whose whole
                // purpose is to be small enough to attach to an issue. The
                // SIZE is kept, because "this binding has a custom icon and it
                // is 62 KB" is the only fact anyone diagnosing would want.
                Value::String(format!("<icon data omitted, {} bytes>", s.len()))
            } else {
                Value::String(crate::telemetry::scrub(s))
            }
        }
        Value::Array(items) => {
            // An array inherits its parent's key: `"emails": ["a", "b"]` must
            // redact both entries, not neither.
            Value::Array(items.iter().map(|i| scrub_value(i, key)).collect())
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, val)| (k.clone(), scrub_value(val, Some(k))))
                .collect(),
        ),
        // Numbers, bools and null carry nothing. Cloned rather than matched
        // individually so a future JSON type cannot silently fall through.
        other => other.clone(),
    }
}

/// Case-insensitive match against [`REDACTED_FIELDS`]. Case-insensitive because
/// a field renamed `accountLabel` in some future serde rename would otherwise
/// walk straight past the list.
pub fn is_redacted_field(key: &str) -> bool {
    REDACTED_FIELDS.iter().any(|f| key.eq_ignore_ascii_case(f))
}

/// `YYYYMMDD-HHMMSS` in UTC, from a Unix timestamp.
///
/// UTC, and the doc says so, because this ends up in a FILE NAME: a local-time
/// stamp with no zone is ambiguous the moment the report crosses a time zone to
/// reach whoever is reading it, and the report's own `system.txt` carries the
/// timestamp too. Hand-rolled from the civil-calendar algorithm rather than
/// adding `chrono`: this project has one date to format and `chrono` would be a
/// new dependency tree for it.
pub fn format_stamp(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let secs_of_day = unix_secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// Howard Hinnant's `civil_from_days`, days since 1970-01-01 → (y, m, d).
/// Transcribed rather than derived; the shift-by-719468 constant is the number
/// of days between 0000-03-01 and 1970-01-01 in the proleptic Gregorian
/// calendar.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The report's file name for a given timestamp.
pub fn report_name(unix_secs: u64) -> String {
    format!("spaceadom-report-{}.zip", format_stamp(unix_secs))
}

/// Names and sizes of everything one directory deep. Never contents.
///
/// Sub-directories are listed as `<dir>` with no recursion — the two folders
/// this is pointed at are flat, and a recursive walk of a data dir is how a
/// listing accidentally becomes an inventory of somebody's machine.
///
/// **REVIEW FIXES 2026-09-05 (H3): the header path and every file name are
/// scrubbed.** The header was a full `C:\Users\<name>\AppData\…` path, and the
/// names in `%APPDATA%\Spaceadom` include `picker-cache.json` — but also
/// whatever a future feature drops there, which is why the name is scrubbed
/// rather than assumed harmless.
pub fn folder_listing(dir: &std::path::Path) -> String {
    let mut out = format!(
        "=== {} (names and sizes only — no contents) ===\n",
        scrub_for_report(&dir.display().to_string())
    );
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            out.push_str(&format!("could not be listed: {}\n", scrub_for_report(&e.to_string())));
            return out;
        }
    };
    let mut rows: Vec<(String, String)> = Vec::new();
    for entry in rd.flatten() {
        let name = scrub_for_report(&entry.file_name().to_string_lossy());
        let size = match entry.metadata() {
            Ok(m) if m.is_dir() => "<dir>".to_string(),
            Ok(m) => format!("{} bytes", m.len()),
            Err(e) => format!("<unreadable: {e}>"),
        };
        rows.push((name, size));
    }
    rows.sort();
    if rows.is_empty() {
        out.push_str("(empty)\n");
    }
    for (name, size) in rows {
        out.push_str(&format!("{name}\t{size}\n"));
    }
    out
}

/// Pull the lines of a log that match any of `needles`, keeping the last
/// `limit`. Used for the monitor/DPI and hook-health sections of `system.txt`:
/// the facts are already in the log, and re-measuring them here would mean two
/// sources that can disagree.
/// **REVIEW FIXES 2026-09-05 (H3): every line is scrubbed on the way out.**
/// The needles are matched against the RAW line — a filter that ran on
/// already-scrubbed text would stop finding lines whose only match was inside a
/// path — and only the survivors are scrubbed.
pub fn log_lines_matching(log: &str, needles: &[&str], limit: usize) -> String {
    let hits: Vec<&str> = log
        .lines()
        .filter(|l| needles.iter().any(|n| l.contains(n)))
        .collect();
    if hits.is_empty() {
        return "(nothing in the log matched)\n".to_string();
    }
    let start = hits.len().saturating_sub(limit);
    let mut out = String::new();
    for line in &hits[start..] {
        out.push_str(&scrub_for_report(line));
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// The archive
// ---------------------------------------------------------------------------

/// Everything that goes into one bundle, already composed as text.
///
/// A struct of finished strings rather than a list of paths, deliberately: it
/// is what lets the test build a bundle from a fixture in a temp dir and assert
/// on the result, and it is rule 3 in the module header made structural — there
/// is no code path that copies a file into the archive without this module
/// having read and scrubbed it first.
pub struct Bundle {
    /// `(entry name inside the zip, contents)`, written in order.
    pub entries: Vec<(String, String)>,
    /// Where the `.zip` goes.
    pub out_path: std::path::PathBuf,
}

/// Write the archive. Returns the path it wrote.
pub fn write_bundle(bundle: &Bundle) -> Result<std::path::PathBuf, String> {
    if let Some(dir) = bundle.out_path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    let file = std::fs::File::create(&bundle.out_path)
        .map_err(|e| format!("could not create {}: {e}", bundle.out_path.display()))?;
    let mut zw = zip::ZipWriter::new(file);
    // Deflated, not Stored: a 5,000-line log tail is a megabyte of highly
    // repetitive text and compresses roughly tenfold, which is the difference
    // between a report a user will attach to an issue and one they will not.
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for (name, contents) in &bundle.entries {
        zw.start_file(name.as_str(), opts)
            .map_err(|e| format!("could not start zip entry '{name}': {e}"))?;
        zw.write_all(contents.as_bytes())
            .map_err(|e| format!("could not write zip entry '{name}': {e}"))?;
    }
    zw.finish()
        .map_err(|e| format!("could not finish {}: {e}", bundle.out_path.display()))?;
    Ok(bundle.out_path.clone())
}

/// Where reports go: `%APPDATA%\Spaceadom\reports`.
pub fn reports_dir() -> std::path::PathBuf {
    crate::startup::data_dir().join("reports")
}

/// Read a file, or return a one-line note explaining why not. Rule 2.
///
/// REVIEW FIXES 2026-09-05 (H3): the failure note names a path, and that note
/// goes into the archive as the entry's whole contents. Scrubbed. (The success
/// branch is raw on purpose — its caller scrubs, through `tail_lines` for the
/// logs and `scrub_config_json` for the config.)
fn read_or_note(path: &std::path::Path, what: &str) -> String {
    match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => scrub_for_report(&format!(
            "{what} could not be read from {}: {e}\n",
            path.display()
        )),
    }
}

/// Compose `system.txt`.
///
/// Everything here is either a compile-time constant, a runtime probe the app
/// already runs for its own reasons, or a line lifted out of `debug.log`. It
/// re-measures nothing: two sources for one fact is how a diagnostic starts
/// disagreeing with the log it ships beside.
fn system_txt(log: &str, stamp_unix: u64) -> String {
    let mut s = String::new();
    s.push_str("=== Spaceadom diagnostics ===\n");
    s.push_str(&format!(
        "report written (UTC): {}\n",
        format_stamp(stamp_unix)
    ));
    s.push_str(&format!("version: {}\n", env!("CARGO_PKG_VERSION")));
    // REVIEW FIXES 2026-09-05 (H3): `scrub_for_report`, not `telemetry::scrub`
    // — the difference is the remainder after a space, and
    // `C:\Program Files\Spaceadom\spaceadom.exe` is exactly that shape.
    s.push_str(&format!(
        "exe: {}\n",
        std::env::current_exe()
            .map(|p| scrub_for_report(&p.display().to_string()))
            .unwrap_or_else(|e| scrub_for_report(&format!("<unknown: {e}>")))
    ));
    s.push_str(&format!("install kind: {:?}\n", crate::updater::detect_install_kind()));
    s.push_str(&format!("packaged (MSIX/Store): {}\n", crate::packaged::is_packaged()));
    s.push_str(&format!("windows: {}\n", windows_build()));
    s.push_str(&format!("safe mode: {}\n", crate::safe_mode::describe()));

    let (rival_found, rival_path, rival_version) = crate::rival_install::status();
    s.push_str(&format!(
        "rival install: found={rival_found} kind={:?} version={rival_version:?} path={:?}\n",
        crate::rival_install::status_kind(),
        scrub_for_report(&rival_path)
    ));

    let health = crate::commands::get_hook_health();
    s.push_str(&format!(
        "hook health: timeout_ms={:?} raised={} evictions={} rivals={:?}\n",
        health.timeout_ms, health.raised, health.evictions, health.rivals
    ));
    s.push_str(&format!(
        "hook installed right now: {}\n",
        crate::hook::HOOK_INSTALLED.load(std::sync::atomic::Ordering::Relaxed)
    ));

    s.push_str("\n=== monitors / DPI, as the log recorded them ===\n");
    s.push_str(&log_lines_matching(
        log,
        &["monitor", "scale", "work area", "display:"],
        40,
    ));

    s.push_str("\n=== hook health, last lines from the log ===\n");
    s.push_str(&log_lines_matching(
        log,
        &["hook:", "WATCHDOG", "genuinely fired", "DEAF"],
        60,
    ));
    s
}

/// The Windows build string, from the registry. `winreg` is already a
/// dependency; `RtlGetVersion` would need another `windows` feature for one
/// line of a text file.
#[cfg(windows)]
fn windows_build() -> String {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
    else {
        return "<could not read HKLM CurrentVersion>".into();
    };
    let get = |name: &str| key.get_value::<String, _>(name).unwrap_or_default();
    let ubr = key.get_value::<u32, _>("UBR").unwrap_or(0);
    format!(
        "{} (DisplayVersion {}, build {}.{}) ",
        get("ProductName"),
        get("DisplayVersion"),
        get("CurrentBuild"),
        ubr
    )
}

#[cfg(not(windows))]
fn windows_build() -> String {
    "<not Windows>".into()
}

/// Build the whole bundle and reveal it in Explorer. The command's body.
///
/// **Blocking.** Reads two logs, a config and two directory listings, then
/// deflates a few megabytes. The Tauri command that calls it is `async` and
/// hands this to a blocking thread — PROBLEM 237 is what a non-`async` command
/// doing this much work on the main thread looks like ("Not responding" for
/// six to seventeen seconds).
pub fn build_bundle(description: &str) -> Result<std::path::PathBuf, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // `startup::data_dir()` and NOT `logger::log_dir()`, deliberately.
    //
    // `run()` initialises the logger with `logger::init(&startup::data_dir())`,
    // so that is where `debug.log` actually IS. `logger::log_dir()` recomputes
    // `%APPDATA%\Spaceadom` from scratch, and since PROBLEM 254 made
    // `startup::data_dir()` portable-aware (a `portable.txt` beside the exe
    // moves everything into `<exe dir>\data\`) the two disagreed in a portable
    // copy. Reading the log from anywhere but where the logger was pointed is
    // how a bundle ships an empty or months-old log while looking perfectly
    // healthy.
    //
    // RECONCILED 2026-09-05: `logger::log_dir()` is now itself a wrapper
    // around `startup::data_dir()`, so the two agree again and this line
    // could be written either way. It stays as it is, deliberately — this
    // module must read the log from the place the logger was POINTED AT, and
    // saying so directly does not depend on a second function continuing to
    // agree. That is the property that was missing when they diverged.
    let log_dir = crate::startup::data_dir();
    let log_path = log_dir.join("debug.log");
    let rolled_path = log_dir.join("debug.log.0");
    let raw_log = read_or_note(&log_path, "debug.log");

    let mut entries: Vec<(String, String)> = Vec::new();

    // The user's own words first — it is the one entry a human reads before
    // anything else, and zip readers show entries in write order.
    let described = if description.len() > MAX_DESCRIPTION {
        let mut end = MAX_DESCRIPTION;
        while end > 0 && !description.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}\n[truncated at {MAX_DESCRIPTION} bytes]\n", &description[..end])
    } else {
        description.to_string()
    };
    entries.push((
        "description.txt".into(),
        format!(
            "What the user described:\n\n{described}\n\n\
             (Written by Spaceadom {} — nothing in this archive was uploaded anywhere.)\n",
            env!("CARGO_PKG_VERSION")
        ),
    ));

    entries.push((
        "system.txt".into(),
        system_txt(&raw_log, stamp),
    ));

    entries.push((
        "debug.log.tail.txt".into(),
        tail_lines(&raw_log, LOG_TAIL_LINES, "debug.log"),
    ));

    if rolled_path.exists() {
        let rolled = read_or_note(&rolled_path, "debug.log.0");
        entries.push((
            "debug.log.0.tail.txt".into(),
            tail_lines(&rolled, ROLLED_TAIL_LINES, "debug.log.0 (rolled)"),
        ));
    }

    let config_path = crate::config::config_path();
    let config_raw = read_or_note(&config_path, "config.json");
    entries.push(("config.json".into(), scrub_config_json(&config_raw)));

    entries.push((
        "listing-data-dir.txt".into(),
        folder_listing(&crate::startup::data_dir()),
    ));
    entries.push((
        "listing-backups.txt".into(),
        folder_listing(&crate::config::backup_dir()),
    ));

    let out_path = reports_dir().join(report_name(stamp));
    let written = write_bundle(&Bundle { entries, out_path })?;

    log::info!(
        "diagnostics: report bundle written to {} ({} bytes). Nothing was uploaded — the user \
         decides whether it leaves this machine.",
        written.display(),
        std::fs::metadata(&written).map(|m| m.len()).unwrap_or(0)
    );
    reveal_in_explorer(&written);
    Ok(written)
}

/// Open Explorer with the report selected.
///
/// `explorer /select,<path>` and NOT `ShellExecute` on the folder: the point is
/// that the user can see WHICH file to attach, and a folder of a dozen reports
/// with no selection is the same problem the banner was trying to solve.
///
/// Best-effort. The path is returned to the UI either way, so a failure here
/// costs a convenience, not the report.
pub fn reveal_in_explorer(path: &std::path::Path) {
    #[cfg(windows)]
    {
        // ONE argument, comma-separated with no space: `explorer /select, X`
        // (with a space) opens the user's Documents folder instead, silently.
        //
        // REVIEW FIXES 2026-09-05 (MEDIUM), two changes on one line:
        //
        //   · `explorer.exe`, not `explorer`. A bare name sends the process
        //     creation through the PATH search, which on Windows tries the
        //     CURRENT DIRECTORY first. The current directory of this process
        //     is whatever it was started in — for a shortcut-launched app,
        //     the shortcut's "Start in"; for an installer relaunch, the
        //     installer's temp folder. An `explorer` (no extension) or
        //     `explorer.exe` sitting there would be run instead of Windows'.
        //     Naming the extension does not fix the directory search by
        //     itself, but it removes the extension-less variant and makes the
        //     intent explicit.
        //   · the path is QUOTED inside the argument. A report path always
        //     comes from `reports_dir()` and cannot contain a comma today, but
        //     `/select,C:\a,b\x.zip` would silently truncate at the comma and
        //     open the wrong folder, and the value is a path built from the
        //     data dir, which a portable install (PROBLEM 254) takes from the
        //     exe's own location.
        let arg = format!("/select,\"{}\"", path.display());
        match std::process::Command::new("explorer.exe").arg(&arg).spawn() {
            Ok(_) => log::info!("diagnostics: revealed {} in Explorer", path.display()),
            Err(e) => log::warn!(
                "diagnostics: could not open Explorer for {} ({e}) — the path is still returned \
                 to the dashboard, which shows it",
                path.display()
            ),
        }
    }
    #[cfg(not(windows))]
    let _ = path;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- REVIEW FIXES 2026-09-05 (H3) — nothing personal survives a report ---

    /// The user name the fixtures below belong to. Injected rather than read
    /// from the environment so the test asserts the same thing on CI, on the
    /// owner's machine, and inside the agent container.
    fn owner() -> Vec<String> {
        vec!["beamu".to_string()]
    }

    /// A log tail shaped exactly like a real one: the three things this app
    /// genuinely writes into `debug.log` in the ordinary course of working.
    const SAMPLE_LOG: &str = concat!(
        "2026-09-05 10:00:01 INFO  smart_search: proc='chrome.exe' \
         title='re: invoice — nur.arpon@example.com - gmail' -> prompt site: UIA -> bottom input\n",
        "2026-09-05 10:00:02 INFO  updater: checking \
         https://github.com/nur-arpon/Spaceadom/releases/latest/download/latest.json?token=abc123 \
         for a release newer than 1.0.100\n",
        "2026-09-05 10:00:03 INFO  startup: data dir C:\\Users\\beamu\\AppData\\Roaming\\Spaceadom\n",
        "2026-09-05 10:00:04 INFO  picker: launching C:\\Users\\beamu\\My Documents\\tax return.pdf\n",
        "2026-09-05 10:00:05 INFO  overlay_fit: dev server http://localhost:5173/?token=deadbeef\n",
        "2026-09-05 10:00:06 INFO  profile: signed in as beamu on this PC\n",
    );

    #[test]
    fn a_scrubbed_log_keeps_no_title_no_address_no_url_and_no_user() {
        let out: String = SAMPLE_LOG
            .lines()
            .map(|l| scrub_for_report_with(l, &owner()))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            !out.contains("invoice"),
            "a WINDOW TITLE survived. Titles are document names, email subjects and chat \
             contacts, and this bundle goes on a public issue tracker:\n{out}"
        );
        assert!(!out.contains('@'), "an email address survived:\n{out}");
        assert!(!out.contains("example.com"), "an email domain survived:\n{out}");
        assert!(
            !out.contains("token=abc123") && !out.contains("token=deadbeef"),
            "a URL QUERY STRING survived:\n{out}"
        );
        assert!(!out.contains("http://"), "an http URL survived:\n{out}");
        assert!(!out.contains("https://"), "an https URL survived:\n{out}");
        assert!(
            !out.contains("beamu"),
            "the USER NAME survived. telemetry::scrub only removes it when it follows a drive \
             letter; a report has to remove it everywhere:\n{out}"
        );
        assert!(
            !out.contains("tax return.pdf") && !out.contains("Documents"),
            "the remainder of a path AFTER A SPACE survived — this is exactly what \
             telemetry::scrub leaves behind and what scrub_for_report exists to finish:\n{out}"
        );

        // And what SURVIVES is the whole point: the report must still be
        // readable as a diagnostic.
        assert!(out.contains("smart_search:"), "the decision line is gone too:\n{out}");
        assert!(out.contains("prompt site: UIA"), "the DECISION must survive:\n{out}");
        assert!(out.contains("updater: checking"), "the updater line is gone:\n{out}");
        assert!(out.contains("1.0.100"), "the version is gone:\n{out}");
    }

    #[test]
    fn there_is_no_localhost_carve_out_in_a_report() {
        // telemetry::scrub KEEPS these on purpose — they are the app's own
        // bundle origin and they make a minified JS stack readable in a crash
        // report. A diagnostics bundle is a different document with a
        // different promise, and a loopback URL there is a dev session's
        // token, not a stack frame.
        for url in [
            "http://tauri.localhost/assets/main-DXnrU6sV.js",
            "http://localhost:5173/?token=deadbeef",
            "http://127.0.0.1:1420/index.html",
        ] {
            let kept = crate::telemetry::scrub(url);
            assert!(kept.contains("http"), "precondition: telemetry::scrub keeps {url}");
            let out = scrub_for_report_with(url, &owner());
            assert_eq!(out, "<url>", "a report must not keep {url}");
        }
    }

    #[test]
    fn a_title_containing_an_apostrophe_is_still_masked_whole() {
        let line = "INFO smart_search: proc='word.exe' title='beamu's résumé — draft' -> word: ctrl+f";
        let out = scrub_for_report_with(line, &owner());
        assert!(!out.contains("résumé"), "the title leaked past its apostrophe:\n{out}");
        assert!(!out.contains("beamu"), "the user name leaked:\n{out}");
        assert!(out.contains("word: ctrl+f"), "the decision must survive:\n{out}");
    }

    #[test]
    fn scrubbing_is_idempotent_and_leaves_an_ordinary_line_alone() {
        let plain = "2026-09-05 10:00:00 INFO  hook: rollover window 220 ms, 3 keys held";
        assert_eq!(scrub_for_report_with(plain, &owner()), plain);
        let once = scrub_for_report_with(SAMPLE_LOG, &owner());
        assert_eq!(
            scrub_for_report_with(&once, &owner()),
            once,
            "running the scrubber twice must not eat its own markers"
        );
    }

    #[test]
    fn a_one_character_user_name_is_ignored_rather_than_destroying_the_log() {
        // The guard that stops a user called "a" turning every 'a' in the log
        // into <user>. A report nobody can read protects nothing.
        let line = "hook: rollover window 220 ms";
        assert_eq!(scrub_for_report_with(line, &["a".to_string()]), line);
    }

    #[test]
    fn the_log_tail_and_the_matched_lines_are_both_scrubbed() {
        // The two functions that are the ONLY routes a log takes into the
        // archive. Asserted separately from the scrubber itself because
        // "the scrubber works" and "the scrubber is called" are different
        // claims, and it was the second one that was false.
        let tail = tail_lines(SAMPLE_LOG, 100, "debug.log");
        assert!(!tail.contains('@'), "tail_lines shipped an address:\n{tail}");
        assert!(!tail.contains("invoice"), "tail_lines shipped a window title:\n{tail}");

        let matched = log_lines_matching(SAMPLE_LOG, &["smart_search:"], 10);
        assert!(!matched.contains("invoice"), "log_lines_matching shipped a title:\n{matched}");
        assert!(matched.contains("smart_search:"), "the filter still has to match:\n{matched}");
    }

    #[test]
    fn a_base64_icon_is_replaced_by_its_size() {
        let raw = r#"{"bindings":{"c":{"label":"Chrome","icon_override":"AAAABBBBCCCC"}}}"#;
        let out = scrub_config_json(raw);
        assert!(
            !out.contains("AAAABBBBCCCC"),
            "the base64 icon was shipped whole — a keyboard of custom icons is megabytes of \
             base64 in an archive meant to be attachable:\n{out}"
        );
        assert!(
            out.contains("icon data omitted, 12 bytes"),
            "the SIZE has to survive, or nobody can tell an absent icon from an omitted \
             one:\n{out}"
        );
        assert!(out.contains("Chrome"), "the rest of the binding must survive:\n{out}");
    }

    /// A throwaway directory under the OS temp dir. No `tempfile` dependency:
    /// this project pins its crates deliberately and one test helper is not a
    /// reason to add a tree.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let p = std::env::temp_dir().join(format!("spaceadom-diag-test-{tag}-{nanos}"));
            std::fs::create_dir_all(&p).expect("temp dir");
            Self(p)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A config shaped like a real one: a Windows path, a URL, a full address,
    /// and — the case that matters — an account label that is an email LOCAL
    /// PART with no `@` in it at all.
    const SAMPLE_CONFIG: &str = r#"{
      "active_profile": "Founders",
      "profiles": [{
        "name": "Founders",
        "bindings": {
          "b": {
            "app": "C:\\Users\\beamu\\AppData\\Local\\BraveSoftware\\brave.exe",
            "label": "Brave",
            "browser_profile_dir": "Profile 1",
            "browser_profile_name": "nur.arpon"
          },
          "m": {
            "web_url": "https://mail.google.com/mail/u/mcdengineeringdesign@gmail.com/",
            "label": "Mail"
          }
        }
      }],
      "support_contact": "mcdengineeringdesign@gmail.com",
      "rollover_ms": 120,
      "run_at_startup": true
    }"#;

    #[test]
    fn no_email_or_account_label_survives_a_scrubbed_config() {
        let out = scrub_config_json(SAMPLE_CONFIG);
        // The whole promise, asserted on the OUTPUT TEXT rather than field by
        // field: a future field added to the config cannot quietly opt out of
        // this test the way a per-field assertion would let it.
        assert!(!out.contains('@'), "an '@' survived scrubbing:\n{out}");
        assert!(
            !out.contains("mcdengineeringdesign"),
            "an address local part survived:\n{out}"
        );
        assert!(
            !out.contains("nur.arpon"),
            "browser_profile_name is an email local part and MUST be redacted by name — \
             scrub() cannot see it, there is no '@' to anchor on:\n{out}"
        );
        assert!(out.contains(REDACTION), "the redaction marker is missing:\n{out}");
        assert!(!out.contains("beamu"), "a user name survived in a path:\n{out}");
        assert!(!out.contains("mail.google.com"), "a URL host survived:\n{out}");

        // …and the diagnostic value is still there.
        assert!(out.contains("Founders"), "profile names must survive:\n{out}");
        assert!(out.contains("Brave"), "binding labels must survive:\n{out}");
        assert!(out.contains("rollover_ms"), "settings must survive:\n{out}");
    }

    #[test]
    fn an_unparseable_config_is_replaced_not_passed_through() {
        // The one path by which an unscrubbed personal file could reach the
        // archive. It must not exist.
        let broken = "{ this is not json, and it mentions beamu@example.com";
        let out = scrub_config_json(broken);
        assert!(out.contains("could not be parsed"), "{out}");
        assert!(!out.contains("@example.com"), "the raw input leaked:\n{out}");
    }

    #[test]
    fn redacted_field_matching_ignores_case() {
        assert!(is_redacted_field("browser_profile_name"));
        assert!(is_redacted_field("Browser_Profile_Name"));
        assert!(is_redacted_field("EMAIL"));
        assert!(!is_redacted_field("label"));
        assert!(!is_redacted_field("name"));
    }

    #[test]
    fn an_array_under_a_redacted_key_is_redacted_element_by_element() {
        // An array inherits its parent's key, so BOTH entries go — including
        // the bare local part, which `scrub` alone cannot see.
        let out = scrub_config_json(r#"{"account_label": ["a@b.com", "plainlocalpart"]}"#);
        assert!(!out.contains("plainlocalpart"), "{out}");
        assert!(!out.contains('@'), "{out}");
        assert_eq!(out.matches(REDACTION).count(), 2, "both elements:\n{out}");
    }

    #[test]
    fn a_key_that_is_not_on_the_list_still_gets_the_ordinary_scrub() {
        // The other half of the rule, and the one that would silently rot: a
        // field nobody thought about is not left alone, it just does not get
        // the name-based redaction on top.
        let out = scrub_config_json(r#"{"some_new_field": "write to a@b.com or C:\\Users\\x"}"#);
        assert!(!out.contains('@'), "{out}");
        assert!(!out.contains("Users"), "{out}");
        assert!(out.contains("<email>") && out.contains("<path>"), "{out}");
        assert!(!out.contains(REDACTION), "it is scrubbed, not redacted:\n{out}");
    }

    #[test]
    fn tail_keeps_the_end_and_says_what_it_dropped() {
        let log: String = (1..=100).map(|i| format!("line {i}\n")).collect();
        let out = tail_lines(&log, 10, "debug.log");
        assert!(out.contains("line 100"), "the newest line must be kept");
        assert!(!out.contains("line 90\n"), "line 90 is outside the last 10");
        assert!(out.contains("line 91"));
        assert!(
            out.contains("90 earlier line(s) not included"),
            "a tail with no note reads as the whole log:\n{out}"
        );
    }

    #[test]
    fn tail_of_a_short_log_says_it_is_complete() {
        let out = tail_lines("a\nb\nc\n", 5_000, "debug.log");
        assert!(out.contains("all 3 line(s)"), "{out}");
    }

    #[test]
    fn a_bundle_contains_exactly_the_entries_it_was_given() {
        let tmp = TempDir::new("bundle");
        let out_path = tmp.path().join("reports").join("spaceadom-report-test.zip");
        let bundle = Bundle {
            entries: vec![
                ("description.txt".into(), "the HUD never appears".into()),
                ("system.txt".into(), "version: test".into()),
                ("config.json".into(), scrub_config_json(SAMPLE_CONFIG)),
                ("listing-data-dir.txt".into(), "picker-cache.json\t827272 bytes\n".into()),
            ],
            out_path: out_path.clone(),
        };
        let written = write_bundle(&bundle).expect("write the bundle");
        assert!(written.exists(), "the zip was not created");

        let file = std::fs::File::open(&written).expect("open the zip");
        let mut zip = zip::ZipArchive::new(file).expect("the file must be a readable zip");
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).expect("entry").name().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "description.txt",
                "system.txt",
                "config.json",
                "listing-data-dir.txt"
            ],
            "entries must be present, in write order"
        );

        // And the contents must survive the round trip — a deflate that wrote
        // an empty entry would still produce the names above.
        use std::io::Read;
        let mut body = String::new();
        zip.by_name("description.txt")
            .expect("description.txt")
            .read_to_string(&mut body)
            .expect("read it back");
        assert_eq!(body, "the HUD never appears");

        let mut cfg = String::new();
        zip.by_name("config.json")
            .expect("config.json")
            .read_to_string(&mut cfg)
            .expect("read it back");
        assert!(!cfg.contains("nur.arpon"), "the archived config is not scrubbed:\n{cfg}");
        assert!(!cfg.contains('@'), "the archived config still has an address:\n{cfg}");
    }

    #[test]
    fn a_listing_reports_names_and_sizes_and_never_contents() {
        let tmp = TempDir::new("listing");
        std::fs::write(tmp.path().join("picker-cache.json"), "SECRET-CONTENTS-XYZ")
            .expect("fixture");
        std::fs::create_dir_all(tmp.path().join("reports")).expect("fixture dir");
        let out = folder_listing(tmp.path());
        assert!(out.contains("picker-cache.json"), "{out}");
        assert!(out.contains("19 bytes"), "the size must be reported:\n{out}");
        assert!(!out.contains("SECRET-CONTENTS-XYZ"), "contents leaked:\n{out}");
        assert!(out.contains("reports\t<dir>"), "a sub-directory must be named:\n{out}");
    }

    #[test]
    fn a_missing_folder_is_a_note_not_a_failure() {
        let out = folder_listing(std::path::Path::new(
            r"Z:\definitely-not-a-real-folder-spaceadom",
        ));
        assert!(out.contains("could not be listed"), "{out}");
    }

    #[test]
    fn the_report_name_is_a_sortable_utc_stamp() {
        // 2026-09-05 12:34:56 UTC = 1788611696.
        assert_eq!(format_stamp(1_788_611_696), "20260905-123456");
        assert_eq!(report_name(1_788_611_696), "spaceadom-report-20260905-123456.zip");
        // The epoch and a leap day, because the civil-calendar transcription is
        // the one part of this file that is easy to get subtly wrong.
        assert_eq!(format_stamp(0), "19700101-000000");
        assert_eq!(format_stamp(1_709_164_800), "20240229-000000");
    }

    #[test]
    fn log_extraction_takes_the_last_matches_only() {
        let log = "a monitor line 1\nnoise\na monitor line 2\nnoise\na monitor line 3\n";
        let out = log_lines_matching(log, &["monitor"], 2);
        assert!(out.contains("line 2") && out.contains("line 3"), "{out}");
        assert!(!out.contains("line 1"), "{out}");
        assert!(!out.contains("noise"), "{out}");
        assert_eq!(
            log_lines_matching(log, &["nothing-here"], 5),
            "(nothing in the log matched)\n"
        );
    }
}
