//! PROBLEM 249 — "What's new in this version".
//!
//! The app already tells you it updated: PROBLEM 245's one-time
//! `Updated to 1.0.X` toast, held in memory until a VISIBLE dashboard asks for
//! it. That toast is a version number and nothing else — it says something
//! happened without saying what, which is the shape of notification a user
//! learns to dismiss without reading. This module gets the actual release notes
//! and hands the UI something worth opening.
//!
//! **Where the text comes from.** The GitHub release for the running version:
//!
//! ```text
//! GET https://api.github.com/repos/nur-arpon/Spaceadom/releases/tags/v1.0.101
//! ```
//!
//! Unauthenticated. GitHub allows 60 requests an hour per IP without a token,
//! and this app makes at most ONE per version change, so the limit is not a
//! constraint and a token — which would have to be a committed secret — is not
//! worth having. The response body's `body` field is the release notes as
//! Markdown, written by release.yml or by hand on the releases page.
//!
//! **Nothing here is a secret and nothing is scrubbed.** The endpoint is public
//! text about a public release; there is no user data on this path at all. The
//! only sanitising done is on the VERSION STRING, and that is a path-safety
//! measure, not a privacy one — see `sanitize_version`.
//!
//! **It never blocks startup.** The fetch runs on its own `st-whats-new`
//! thread, after a settle longer than the window creation and the updater's own
//! first check, and every failure ends in one log line. Offline, rate-limited,
//! a release with an empty body, a version that has no release yet: all of them
//! land on "use the cache if there is one, otherwise say nothing".
//!
//! **The cache** is `%APPDATA%\Spaceadom\release-notes-<version>.md`, one file
//! per version, written once. It is what makes the What's New panel work on a
//! laptop that updated last night and opened its lid on a train this morning.
//!
//! **Two ways to reach the UI, because one of them can be missed.** This is the
//! same lesson PROBLEM 245 paid for: an `--autostart` relaunch builds the
//! dashboard webview HIDDEN, so an event emitted on a background thread can
//! arrive before any listener exists and simply vanish. So the payload is BOTH
//! emitted as `whats-new-available` AND parked in a static that
//! `get_whats_new()` returns on demand — the page can ask on every
//! visibility change, exactly as it already does for the update notice.

use std::sync::Mutex;
use std::time::Duration;

/// The GitHub releases-by-tag endpoint. The tag is `v` + the version, which is
/// what release.yml pushes.
pub const RELEASES_TAG_API: &str = "https://api.github.com/repos/nur-arpon/Spaceadom/releases/tags/";

/// GitHub's API **rejects a request with no User-Agent** (403, and the body
/// says so). Ours names the app and the version, which is what GitHub asks for.
const UA: &str = concat!("Spaceadom/", env!("CARGO_PKG_VERSION"), " (+https://github.com/nur-arpon/Spaceadom)");

/// One request, one timeout. Longer than the API needs and short enough that a
/// captive-portal hang does not keep the thread alive into the next hour.
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// Past window creation (10 s), past the updater's first check (15 s). The
/// notes are the least urgent thing this app does; they wait for everything.
const SETTLE_DELAY: Duration = Duration::from_secs(25);

/// A release body larger than this is a mistake or an attack on the panel's
/// layout, not release notes. Truncated with a marker rather than refused, so
/// something useful still shows.
const MAX_NOTES_BYTES: usize = 64 * 1024;

/// The event the UI listens for. Payload: [`WhatsNew`].
pub const EVENT_WHATS_NEW: &str = "whats-new-available";

/// What the UI is told when a launch follows a version change.
///
/// `has_notes` is the field that decides the presentation, and it is present
/// even when it is `false` on purpose: emitting only on success would leave the
/// UI racing a network request it cannot see, with no way to know whether to
/// keep waiting or fall back to the plain toast. With this field the answer is
/// always definite — `true` → show the panel, `false` → keep PROBLEM 245's
/// `Updated to 1.0.X` toast exactly as it is today.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WhatsNew {
    pub version: String,
    pub has_notes: bool,
    /// The version went BACKWARDS — this launch follows a rollback, not an
    /// update, so "Updated to" would be the wrong word.
    pub rolled_back: bool,
}

/// Parked for a UI that was not listening yet. See the module doc.
static WHATS_NEW: Mutex<Option<WhatsNew>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// Pure
// ---------------------------------------------------------------------------

/// Make a version safe to put in a filename and a URL. Pure.
///
/// `get_release_notes` takes its version FROM THE FRONTEND, and that value ends
/// up in a path join. `data_dir().join("release-notes-../../evil.md")` escapes
/// the data directory, so the version is not trusted: only ASCII
/// alphanumerics, `.`, `-`, `_` and `+` survive, a `..` anywhere is refused
/// outright, and anything that is not left holding a digit is refused too.
///
/// Generalise: **a value that reaches a path join is untrusted until a pure
/// function says otherwise**, and that function belongs next to its tests.
pub(crate) fn sanitize_version(v: &str) -> Option<String> {
    let v = v.trim().trim_start_matches('v').trim();
    if v.is_empty() || v.len() > 64 || v.contains("..") {
        return None;
    }
    if !v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+')) {
        return None;
    }
    if !v.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(v.to_string())
}

/// The API URL for a version. Pure. `None` for a version that failed
/// sanitising, so a bad value can never be turned into a request at all.
pub(crate) fn tag_url(version: &str) -> Option<String> {
    Some(format!("{RELEASES_TAG_API}v{}", sanitize_version(version)?))
}

/// The cache filename for a version. Pure.
pub(crate) fn cache_file_name(version: &str) -> Option<String> {
    Some(format!("release-notes-{}.md", sanitize_version(version)?))
}

/// Pull the release notes out of a GitHub releases API response. Pure.
///
/// `None` for every shape that is not a release with text in it: the 404
/// `{"message":"Not Found"}` GitHub returns before a tag exists, a rate-limit
/// body, a release whose notes were left blank, and anything that is not JSON.
/// Over-long bodies are truncated rather than dropped.
pub(crate) fn parse_release_body(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let body = value.get("body")?.as_str()?.trim();
    if body.is_empty() {
        return None;
    }
    if body.len() <= MAX_NOTES_BYTES {
        return Some(body.to_string());
    }
    // Cut on a char boundary, never mid-codepoint: release notes contain emoji.
    let mut cut = MAX_NOTES_BYTES;
    while cut > 0 && !body.is_char_boundary(cut) {
        cut -= 1;
    }
    Some(format!("{}\n\n…", &body[..cut]))
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

fn cache_path(version: &str) -> Option<std::path::PathBuf> {
    Some(crate::startup::data_dir().join(cache_file_name(version)?))
}

fn read_cache(version: &str) -> Option<String> {
    let path = cache_path(version)?;
    let text = std::fs::read_to_string(&path).ok()?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return None;
    }
    log::info!("release notes: served {} from the cache ({} bytes)", path.display(), text.len());
    Some(text)
}

fn write_cache(version: &str, notes: &str) {
    let Some(path) = cache_path(version) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match std::fs::write(&path, notes) {
        Ok(()) => log::info!("release notes: cached {} bytes to {}", notes.len(), path.display()),
        Err(e) => log::warn!("release notes: could not cache to {} ({e})", path.display()),
    }
}

// ---------------------------------------------------------------------------
// Fetch
// ---------------------------------------------------------------------------

/// One GET, one parse. Every failure is a `String` that goes to the log and
/// never to the user — an absent What's New panel is the whole of the
/// user-facing failure mode.
async fn fetch_notes(version: &str) -> Result<String, String> {
    let url = tag_url(version).ok_or_else(|| format!("version {version:?} is not usable in a URL"))?;
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(UA)
        .build()
        .map_err(|e| format!("could not build the HTTP client: {e}"))?;
    log::info!("release notes: fetching {url}");
    let response = client
        .get(&url)
        // Asking for the versioned media type is what GitHub's own docs say to
        // do; without it the shape of `body` is only conventionally stable.
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|e| format!("request failed (offline, DNS, TLS or timeout): {e}"))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| format!("could not read the response body: {e}"))?;
    if !status.is_success() {
        // 404 before the release exists, 403 when the 60/hour limit is spent.
        return Err(format!("GitHub answered {status} — no notes this time ({} bytes)", text.len()));
    }
    parse_release_body(&text).ok_or_else(|| {
        format!("the release for {version} has no notes in its body ({} bytes of JSON)", text.len())
    })
}

/// Cache first, network second. The order matters: it is what makes a laptop
/// that updated last night and has no signal this morning still show the panel.
async fn notes_for(version: &str) -> Option<String> {
    if let Some(cached) = read_cache(version) {
        return Some(cached);
    }
    // REVIEW FIXES 2026-09-05 (MEDIUM) — A PACKAGED COPY MAKES NO NETWORK CALL
    // FROM HERE.
    //
    // The cache is still read above, deliberately: a Store install that has
    // notes on disk should show them. What it must not do is GO AND GET them.
    //
    // Three reasons, and the first is the one that matters. **A Store
    // submission has to declare what the app talks to**, and the Store's own
    // "what's new" text is the channel the platform provides for exactly this
    // — an app that additionally polls api.github.com to render its own
    // release notes is describing a second, undeclared update channel next to
    // the one the Store owns. Second, the version a packaged copy is running
    // is the PACKAGE version, which is a four-part `1.0.100.0` minted by
    // `AppxManifest.xml`, not the `v1.0.100` tag this API is keyed on: the
    // request is a 404 by construction on every packaged launch. Third,
    // CLAUDE.md's rule for a packaged build is that the in-app updater is
    // INERT, and these notes are that updater's user-facing half — leaving the
    // fetch on made the app half-inert in a way nothing in the log said.
    //
    // Same shape as `updater::rollback_available`'s packaged/portable gates:
    // the return value would often be `None` anyway, and the LOG LINE is the
    // point. "No notes because we did not ask" and "no notes because GitHub
    // said 404" are different facts, and a silent shared `None` makes them
    // indistinguishable in a bug report.
    if crate::packaged::is_packaged() {
        log::info!(
            "release notes: PACKAGED (MSIX/Store) — no network request was made for {version}. \
             The Store owns update messaging for a packaged copy, the package version is not a \
             git tag this API knows, and the in-app updater is inert here by design \
             (PROBLEM 250). Anything already cached on disk is still shown."
        );
        return None;
    }
    match fetch_notes(version).await {
        Ok(notes) => {
            write_cache(version, &notes);
            Some(notes)
        }
        Err(why) => {
            log::info!("release notes: none for {version} — {why}");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// The release notes for a version, or for the running one when `version` is
/// omitted. Cache, then network, then `None`.
///
/// ASYNC, and that is load-bearing: a non-`async` `#[tauri::command]` runs on
/// the MAIN thread, where a 20-second network timeout would freeze both
/// webviews (PROBLEM 205/237 are what that costs).
#[tauri::command]
pub async fn get_release_notes(app: tauri::AppHandle, version: Option<String>) -> Option<String> {
    let version = version
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| app.package_info().version.to_string());
    let Some(version) = sanitize_version(&version) else {
        log::warn!("release notes: refused the version {version:?} — it is not a version string");
        return None;
    };
    notes_for(&version).await
}

/// The parked What's New payload, for a page that was not listening when the
/// event fired. Cheap, synchronous, and safe on the main thread: one mutex.
#[tauri::command]
pub fn get_whats_new() -> Option<WhatsNew> {
    WHATS_NEW.lock().ok().and_then(|w| w.clone())
}

// ---------------------------------------------------------------------------
// The background pass
// ---------------------------------------------------------------------------

/// Spawn `st-whats-new`: after the settle, if this launch follows a version
/// change, get the notes and tell the UI. Never blocks the caller; a thread
/// that cannot be spawned costs the session its What's New panel and nothing
/// else.
pub fn schedule(app: tauri::AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("st-whats-new".into())
        .spawn(move || {
            std::thread::sleep(SETTLE_DELAY);
            run(&app);
        });
    if spawned.is_err() {
        log::error!("release notes: could not spawn st-whats-new — no What's New this session");
    }
}

fn run(app: &tauri::AppHandle) {
    use tauri::Emitter;

    // PEEK, never take: the one-shot notice belongs to the dashboard's toast
    // and must still be there when a VISIBLE window asks for it.
    let Some(version) = crate::updater::peek_update_notice() else {
        log::info!("release notes: this launch is not the first of a new version — nothing to show");
        return;
    };
    let rolled_back = crate::updater::update_was_a_rollback();
    let notes = tauri::async_runtime::block_on(notes_for(&version));
    let payload = WhatsNew {
        version: version.clone(),
        has_notes: notes.is_some(),
        rolled_back,
    };
    if let Ok(mut w) = WHATS_NEW.lock() {
        *w = Some(payload.clone());
    }
    log::info!(
        "release notes: {EVENT_WHATS_NEW} for {version} — notes {}, rolled_back {rolled_back}. \
         Parked for get_whats_new() too, because an --autostart relaunch builds the dashboard \
         hidden and an event with no listener is gone (PROBLEM 245's lesson).",
        if payload.has_notes { "available" } else { "unavailable, the plain toast stands" }
    );
    if let Err(e) = app.emit(EVENT_WHATS_NEW, payload) {
        log::warn!("release notes: could not emit {EVENT_WHATS_NEW} ({e})");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real GitHub releases-by-tag response, trimmed to the fields this
    /// module reads plus enough neighbours to prove it picks the right one.
    /// `name` and `body` differ deliberately: reading `name` would look correct
    /// on every release whose title happens to repeat the first line.
    const SAMPLE: &str = r###"{
      "url": "https://api.github.com/repos/nur-arpon/Spaceadom/releases/191919191",
      "html_url": "https://github.com/nur-arpon/Spaceadom/releases/tag/v1.0.101",
      "id": 191919191,
      "tag_name": "v1.0.101",
      "target_commitish": "main",
      "name": "Spaceadom 1.0.101",
      "draft": false,
      "prerelease": false,
      "created_at": "2026-09-05T09:00:00Z",
      "published_at": "2026-09-05T09:12:00Z",
      "assets": [
        { "name": "Spaceadom_1.0.101_x64-setup.exe", "size": 5872341 },
        { "name": "latest.json", "size": 431 }
      ],
      "body": "## What's new\r\n\r\n- **Check for updates** now lives in the tray menu.\r\n- You can go back to the previous version if an update misbehaves.\r\n- Fixed: the ring could clip its outer chips on a 16-app profile.\r\n"
    }"###;

    const NOT_FOUND: &str = r#"{"message":"Not Found","documentation_url":"https://docs.github.com/rest","status":"404"}"#;
    const RATE_LIMITED: &str = r#"{"message":"API rate limit exceeded for 203.0.113.7.","documentation_url":"https://docs.github.com/rest/overview/rate-limits-for-the-rest-api"}"#;

    #[test]
    fn a_real_release_response_yields_its_body_and_not_its_title() {
        let notes = parse_release_body(SAMPLE).expect("the sample has a body");
        assert!(notes.starts_with("## What's new"), "{notes}");
        assert!(notes.contains("Check for updates"));
        assert!(notes.contains("go back to the previous version"));
        // The title is NOT the notes.
        assert!(!notes.contains("Spaceadom 1.0.101"));
        // Trailing whitespace is trimmed, so the panel never opens on a blank line.
        assert_eq!(notes, notes.trim());
    }

    #[test]
    fn every_shape_that_is_not_release_notes_is_none() {
        assert_eq!(parse_release_body(NOT_FOUND), None, "404 before the tag exists");
        assert_eq!(parse_release_body(RATE_LIMITED), None, "the 60/hour limit");
        assert_eq!(parse_release_body(r#"{"body":null}"#), None);
        assert_eq!(parse_release_body(r#"{"body":""}"#), None);
        assert_eq!(parse_release_body(r#"{"body":"   \r\n  "}"#), None, "whitespace only");
        assert_eq!(parse_release_body(r#"{"body":123}"#), None, "wrong type");
        assert_eq!(parse_release_body("<html>502 Bad Gateway</html>"), None);
        assert_eq!(parse_release_body(""), None);
        assert_eq!(parse_release_body("[]"), None);
    }

    /// A body far past the cap is truncated, not dropped — something useful
    /// still shows — and never cut through the middle of a character.
    #[test]
    fn an_enormous_body_is_truncated_on_a_char_boundary() {
        let huge: String = "🌙".repeat(MAX_NOTES_BYTES); // 4 bytes each
        let json = serde_json::json!({ "body": huge }).to_string();
        let notes = parse_release_body(&json).expect("still has notes");
        assert!(notes.len() < huge.len());
        assert!(notes.ends_with('…'));
        // The load-bearing assertion: it is still valid UTF-8 with no
        // replacement characters, i.e. the cut landed on a boundary.
        assert!(!notes.contains('\u{FFFD}'));
        assert!(notes.starts_with("🌙🌙"));

        // Exactly at the cap is NOT truncated.
        let exact = "a".repeat(MAX_NOTES_BYTES);
        let json = serde_json::json!({ "body": exact }).to_string();
        assert_eq!(parse_release_body(&json), Some(exact));
    }

    /// The version reaches a `Path::join`. This is the test that keeps it safe.
    #[test]
    fn a_version_that_could_escape_the_data_directory_is_refused() {
        for bad in [
            "../../evil",
            "..",
            "1.0.101/../../evil",
            r"..\..\windows\system32",
            "1.0.101\0",
            "1.0.101 && calc",
            "1.0.101/latest",
            r"C:\Windows",
            "",
            "   ",
            "v",
            "release",                 // no digit
            "1.0.101?token=x",
            "1.0.101#frag",
            "%2e%2e%2f",
            &"9".repeat(65),           // over the length cap
        ] {
            assert_eq!(sanitize_version(bad), None, "must be refused: {bad:?}");
            assert_eq!(tag_url(bad), None, "must never become a URL: {bad:?}");
            assert_eq!(cache_file_name(bad), None, "must never become a path: {bad:?}");
        }
    }

    #[test]
    fn an_ordinary_version_survives_sanitising_in_every_form_it_arrives_in() {
        assert_eq!(sanitize_version("1.0.101").as_deref(), Some("1.0.101"));
        assert_eq!(sanitize_version(" v1.0.101 ").as_deref(), Some("1.0.101"));
        assert_eq!(sanitize_version("1.0.101-rc.1").as_deref(), Some("1.0.101-rc.1"));
        assert_eq!(sanitize_version("1.0.101+build.7").as_deref(), Some("1.0.101+build.7"));
    }

    /// The URL is the tag GitHub actually holds — `v` + version — and it is
    /// built from the SANITISED value, not the raw one.
    #[test]
    fn the_api_url_is_the_v_prefixed_tag_on_the_right_repository() {
        assert_eq!(
            tag_url("1.0.101").as_deref(),
            Some("https://api.github.com/repos/nur-arpon/Spaceadom/releases/tags/v1.0.101")
        );
        // A version that already carries the v does not get two of them.
        assert_eq!(tag_url("v1.0.101"), tag_url("1.0.101"));
        assert!(RELEASES_TAG_API.starts_with("https://api.github.com/repos/nur-arpon/Spaceadom/"));
        assert!(RELEASES_TAG_API.ends_with("/releases/tags/"));
    }

    #[test]
    fn the_cache_is_one_markdown_file_per_version_in_the_data_directory() {
        assert_eq!(cache_file_name("1.0.101").as_deref(), Some("release-notes-1.0.101.md"));
        assert_ne!(cache_file_name("1.0.101"), cache_file_name("1.0.102"));
    }

    /// GitHub answers 403 to a request with no User-Agent, and the app name is
    /// what its documentation asks callers to send.
    #[test]
    fn the_user_agent_names_the_app_and_its_version() {
        assert!(UA.starts_with("Spaceadom/"), "{UA}");
        assert!(UA.len() > "Spaceadom/".len(), "the version must not be empty: {UA}");
        assert!(UA.contains("github.com/nur-arpon/Spaceadom"), "{UA}");
    }

    /// The payload the UI switches on. `has_notes` is present in BOTH cases on
    /// purpose — that is what removes the race between the panel and the toast.
    #[test]
    fn the_whats_new_payload_always_states_whether_there_are_notes() {
        let with = WhatsNew { version: "1.0.101".into(), has_notes: true, rolled_back: false };
        let j = serde_json::to_value(&with).unwrap();
        assert_eq!(j["version"], "1.0.101");
        assert_eq!(j["has_notes"], true);
        assert_eq!(j["rolled_back"], false);

        let without = WhatsNew { version: "1.0.101".into(), has_notes: false, rolled_back: true };
        let j = serde_json::to_value(&without).unwrap();
        assert_eq!(j["has_notes"], false, "the plain toast stands, and the UI is TOLD so");
        assert_eq!(j["rolled_back"], true);
    }

    /// The whole point of the settle: this runs behind window creation (10 s)
    /// and behind the updater's first check (15 s).
    #[test]
    fn the_whats_new_fetch_waits_for_everything_else_to_finish_starting() {
        assert!(SETTLE_DELAY > crate::AUTOSTART_SETTLE, "must not compete with window creation");
        assert!(SETTLE_DELAY.as_secs() >= 25, "must sit behind the updater's first check");
        assert!(FETCH_TIMEOUT.as_secs() <= 30, "a captive portal must not pin the thread");
    }
}
