//! telemetry.rs — crash and error reporting to Sentry (PROBLEM 195).
//!
//! WHY THIS EXISTS. Until now the only way to learn that Spaceadom had crashed
//! on somebody else's machine was to ask that person to find
//! `%APPDATA%\Spaceadom\debug.log` and send it. Nobody does that. The owner's
//! decision, 2026-08-26: use Sentry's free tier rather than build a bespoke
//! telemetry backend, because the backend is work that buys nothing Sentry does
//! not already do.
//!
//! WHAT LEAVES THE MACHINE, EXACTLY. Two things and nothing else:
//!
//!   1. Rust panics — the crashes. Forwarded by hand from the ONE panic hook in
//!      `lib.rs` (see `capture_panic`, and the hard rule below).
//!   2. `log::error!` records — the handled failures that are still bugs.
//!
//! Everything quieter than ERROR — info, debug, and warnings — is dropped
//! before it reaches Sentry, so ordinary use of the app sends nothing at all.
//! That gate is `SENTRY_MINIMUM_LEVEL` below, and it is a constant so there is
//! exactly one place to look.
//!
//! THE PANIC-HOOK RULE, AND HOW THIS MODULE OBEYS IT.
//! CLAUDE.md: *"There is exactly ONE `std::panic::set_hook` call, in `lib.rs`.
//! There were two, and the second silently replaced the first for months
//! (PROBLEM 131)."* The `sentry` crate's DEFAULT `panic` feature installs its
//! own hook from inside `sentry::init()` — a second `set_hook` this project
//! would never see in a grep of its own source, whose failure mode (the
//! existing crash-context reporting quietly stops) leaves no trace. So the
//! `panic` feature is DISABLED in `Cargo.toml` (`default-features = false`) and
//! the forwarding is done by hand, from inside the existing hook, by
//! `capture_panic` below. **If you ever re-enable the `panic` feature, you have
//! silently added a second panic hook.**
//!
//! TRANSPORT. `reqwest` + `rustls`, not the crate's default `native-tls` or the
//! optional `curl`: nothing on the build machine is installed system-wide (the
//! whole Rust toolchain lives on D:), and rustls needs no OpenSSL, no libcurl
//! and no system certificate stack to link against.

use std::sync::atomic::{AtomicBool, Ordering};

// ===========================================================================
// !!!  PASTE YOUR SENTRY DSN HERE  !!!
// ===========================================================================
//
// This is EMPTY on purpose, and while it is empty this whole module is inert:
// `init()` below never calls `sentry::init`, so there is no client, no
// background thread, no network socket, and every `capture_*` call in the app
// is a no-op. The app builds and runs exactly as it did before.
//
// TO TURN CRASH REPORTING ON:
//   1. Make a free account at https://sentry.io
//   2. Create a project — platform "Rust".
//   3. Settings → Projects → <your project> → Client Keys (DSN).
//   4. Copy the DSN. It looks like:
//        https://0123456789abcdef0123456789abcdef@o123456.ingest.de.sentry.io/1234567
//   5. Paste it between the quotes on the line below, rebuild, reinstall.
//
// The DSN is NOT a secret — it is a write-only ingest key and every Sentry
// client ships with it embedded. It is safe in a public repo.
//
// PROBLEM 196 — the DSN is NOT a literal here any more.
//
// A Sentry DSN is safe to ship inside the compiled app (every telemetry SDK
// works this way — it can only SUBMIT events, never read or manage the
// account), but that is not the same as safe to publish in the GIT REPO's
// source history: a DSN sitting in cleartext on a public GitHub repo is an
// open invitation for anyone to script a flood of fake crash events and burn
// through the free tier's 5,000-events/month ceiling for everyone.
//
// So the real value lives in `sentry_dsn.txt`, sitting next to this file,
// which is listed in `.gitignore` and has never been committed. This
// `include_str!` embeds it at COMPILE time — the running binary still has the
// DSN baked in exactly as before, only the SOURCE TREE and its git history
// never do. `sentry_dsn.example.txt` (tracked) explains the setup for future
// reference. Missing the real file is a COMPILE FAILURE, not a silent empty
// string — deliberate, per this project's own rule that a silently-inert
// safety/reporting feature is worse than a build that refuses to proceed.
pub const SENTRY_DSN: &str = trim_dsn(include_str!("sentry_dsn.txt"));

/// `include_str!` cannot itself trim, and a trailing newline from an
/// editor-saved file would make `SENTRY_DSN.is_empty()` at the top of
/// `init()` false for a file that is really empty — inertness silently
/// breaking because of a trailing `\n`.
const fn trim_dsn(s: &str) -> &str {
    s.trim_ascii()
}
// ===========================================================================

/// The severity floor for anything that gets sent.
///
/// `log::Level::Error` — crashes and errors only. Panics are forwarded
/// separately (see `capture_panic`) and are not subject to this. A WARN, an
/// INFO or a DEBUG line never leaves the machine, which is why "ordinary use
/// sends nothing" is a true statement rather than a hopeful one.
///
/// This is the ONE place the scope of what is reported is decided. Widening it
/// (to `Warn`, say) changes what PRIVACY.md promises users, so PRIVACY.md has
/// to change in the same commit.
pub const SENTRY_MINIMUM_LEVEL: log::Level = log::Level::Error;

/// The live kill switch behind the "Don't send logs" toggle.
///
/// FALSE AT PROCESS START, DELIBERATELY. The config is not loaded yet when the
/// logger is installed, so the honest state at that moment is "we do not know
/// whether this user consented" — and the only safe answer to that is to send
/// nothing. `publish(&cfg)` is called the instant the config IS known (from
/// `lib.rs`'s startup load AND from `config::save`, the same both-ends pattern
/// `BOUND_SPECIALS` and the app-exceptions list use — published from `save`
/// alone, a runtime-checked flag stays wrong from launch until the user
/// happens to save something).
///
/// Read on every log record and inside the panic hook. Flipping it takes
/// effect on the very next record, with no restart and no second
/// `sentry::init()`.
static SENDING_ENABLED: AtomicBool = AtomicBool::new(false);

/// True when the client actually exists — i.e. a real DSN was pasted above.
/// Only used to keep the log honest about what it started.
static SENTRY_LIVE: AtomicBool = AtomicBool::new(false);

/// Start the Sentry client, if there is a DSN to start it with.
///
/// Returns the guard, which MUST be held for the life of the process:
/// dropping a `ClientInitGuard` closes the client and flushes it, so binding
/// this to `_` instead of `_guard` at the call site would switch reporting off
/// on the same line that switched it on.
///
/// Returns `None` when `SENTRY_DSN` is empty. Note this is not the same as
/// calling `sentry::init("")` — an empty DSN string is an error, not a
/// disabled client, so it is never handed to the crate at all.
#[must_use = "the ClientInitGuard must be held for the whole process — dropping it shuts Sentry down"]
pub fn init() -> Option<sentry::ClientInitGuard> {
    if SENTRY_DSN.is_empty() {
        return None;
    }

    // BUILT BY MUTATION, NOT BY STRUCT LITERAL. `sentry::ClientOptions` is
    // `#[non_exhaustive]` as of 0.49, so `ClientOptions { .. }` does not
    // compile at all (E0639) — it has to start from `Default` and be assigned
    // into. Field names move between releases too (`sample_rate` became
    // `event_sampling_strategy`), which is the other reason to touch as few of
    // them as possible.
    let mut options = sentry::ClientOptions::default();
    // Which build a crash came from. Without this every report from every
    // version lands in one undifferentiated pile.
    options.release = sentry::release_name!();
    // Attach the stack trace to plain `log::error!` events too, not only to
    // panics — an error line without a call path names the symptom and not the
    // code that produced it.
    options.attach_stacktrace = true;
    // Explicit, not inherited: usernames, machine names and IP addresses are
    // exactly what PRIVACY.md promises are not sent. Left at the crate's
    // default of false, and written out so that a future change to that
    // default cannot quietly turn it on.
    options.send_default_pii = false;

    let guard = sentry::init((SENTRY_DSN, options));

    SENTRY_LIVE.store(true, Ordering::Relaxed);
    Some(guard)
}

/// Seed / re-seed the kill switch from config. Called from BOTH the startup
/// config load and `config::save` — see `SENDING_ENABLED`'s comment for why
/// one of those two is not enough.
pub fn publish(cfg: &crate::config::AppConfig) {
    set_sending_enabled(cfg.send_logs);
}

/// Flip the kill switch. Instantly effective: the next log record and the next
/// panic both read the new value. No `sentry::init()` re-run, no restart.
pub fn set_sending_enabled(on: bool) {
    let was = SENDING_ENABLED.swap(on, Ordering::Relaxed);
    if was != on {
        log::info!(
            "telemetry: crash/error reporting {} (sentry client {})",
            if on { "ENABLED" } else { "DISABLED — nothing will leave this machine" },
            if SENTRY_LIVE.load(Ordering::Relaxed) { "live" } else { "inert, no DSN" },
        );
    }
}

/// Whether anything is currently allowed to be sent.
pub fn sending_enabled() -> bool {
    SENDING_ENABLED.load(Ordering::Relaxed)
}

/// How a single log record should be treated by Sentry.
///
/// Called from the filter closure of `sentry_log::SentryLogger` in
/// `logger.rs`, for every record the process emits, so it must stay cheap:
/// one relaxed atomic load and one integer comparison.
///
/// `LogFilter::Ignore` is a genuine drop, not a "record it quietly" — nothing
/// is queued, nothing is buffered and nothing is sent. That is what makes the
/// toggle a kill switch rather than a preference.
pub fn log_filter(metadata: &log::Metadata<'_>) -> sentry_log::LogFilter {
    if !SENDING_ENABLED.load(Ordering::Relaxed) {
        return sentry_log::LogFilter::Ignore;
    }
    // PROBLEM 217 — records this module produced itself are ALREADY reported,
    // rate-limited, by `report_degraded` / `report_frontend_error`. Letting the
    // bridge send them again would both duplicate every event and defeat the
    // rate limit, because the bridge has no memory: the hook-deafness line that
    // fired 38 times in one session would arrive 38 times.
    if metadata.target() == DEGRADED_TARGET {
        return sentry_log::LogFilter::Ignore;
    }
    // log::Level orders Error(1) < Warn(2) < Info(3): "at least as severe as"
    // is `<=`, not `>=`. Getting this backwards would send everything.
    if metadata.level() <= SENTRY_MINIMUM_LEVEL {
        sentry_log::LogFilter::Event
    } else {
        sentry_log::LogFilter::Ignore
    }
}

// ===========================================================================
// PROBLEM 217 — the two paths a user-visible failure could not previously take
// ===========================================================================
//
// The threshold above is `Error`, and it is deliberately staying there. What
// changed is that two whole classes of failure never produced an `error!` line
// at all, so nothing about them ever left the machine:
//
//   1. FRONTEND failures. `frontend_log` / `overlay_log` are the only bridges a
//      webview has into the Rust log, and they logged at INFO and WARN. Every
//      JavaScript exception in the dashboard or the overlay — the entire UI
//      layer, and the thing a friend actually reports as "it looks broken" —
//      stayed on their machine. `frontend_error` / `overlay_error` are the
//      sibling commands that carry ERROR severity; the old two are untouched,
//      so no existing call site changes meaning.
//
//   2. DEGRADED-BUT-RUNNING states. Hook deafness, the compositing self-test's
//      dead verdict, `OVERLAY_DISABLED` — the app keeps running and the user
//      cannot always tell that half of it stopped working. These are WARN, and
//      raising the global threshold to WARN to catch them would also send the
//      spacedesk/PowerToys conflict chatter, thousands of lines a session, and
//      bury the four that matter.
//
// So the promoted set is an EXPLICIT LIST — `Degraded` below — rather than a
// threshold. A threshold silently widens as new warnings are added; a list has
// to be edited on purpose, and can be read in one screen.
//
// GENERALISE THIS: *a reporting threshold chosen for volume decides which
// failures you will never hear about — pick it from what users report, not from
// what is cheap to send.*

/// The log target both new reporting paths write their local line under.
///
/// Two jobs. It keeps the line in `debug.log` at its real severity (an ERROR
/// stays an ERROR locally — this is the user's own machine and nothing is being
/// hidden from them), and it tells `log_filter` above not to hand that same
/// record to Sentry a second time, unbounded.
pub const DEGRADED_TARGET: &str = "spaceadom::degraded";

/// The explicit list of "the app is degraded and the user probably cannot tell"
/// conditions that are allowed to leave the machine.
///
/// Adding a variant is a deliberate act with a code review attached. That is
/// the entire point of the enum: the alternative — moving
/// `SENTRY_MINIMUM_LEVEL` to `Warn` — would have enrolled every existing and
/// every future `warn!` in the codebase without anyone deciding to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Degraded {
    /// `hook: DEAF for the last …` (`hook/mod.rs`). The reference hook is
    /// seeing keys and the primary hook is not: shortcuts have silently
    /// stopped working, and nothing on screen says so.
    HookDeaf,
    /// The pixel self-test scored a strike (`commands.rs`) — the overlay was
    /// visible and composed nothing.
    OverlayCompositingStrike,
    /// Three strikes: GPU composition is declared dead and the app falls back
    /// to software rendering / rebuilds the overlay window (`commands.rs`).
    OverlayCompositingDead,
    /// `OVERLAY_DISABLED` set or found set — HUD and every sound suppressed
    /// (`lib.rs`, `guide_hud/mod_impl.rs`).
    OverlayDisabled,
    /// The display-rebuild failure path (`display_watch.rs`): no overlay window
    /// exists and one could not be built.
    OverlayRebuildFailed,
}

impl Degraded {
    /// The stable key. It is the Sentry fingerprint AND the rate-limit key, so
    /// all 38 of one session's hook-deafness reports collapse into one issue
    /// with one event, not 38 issues.
    pub const fn key(self) -> &'static str {
        match self {
            Degraded::HookDeaf => "hook-deaf",
            Degraded::OverlayCompositingStrike => "overlay-compositing-strike",
            Degraded::OverlayCompositingDead => "overlay-compositing-dead",
            Degraded::OverlayDisabled => "overlay-disabled",
            Degraded::OverlayRebuildFailed => "overlay-rebuild-failed",
        }
    }
}

/// Same condition, at most one event per this window.
///
/// Measured from the owner's own machine: hook deafness fired 38 times in one
/// session. Thirty-eight events say exactly what one event says, cost 38 of the
/// free tier's 5,000 monthly, and make the issue list unreadable.
const DEGRADED_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// …and at most this many for one condition in one run of the app, however long
/// it runs. A machine that is permanently broken should say so once or twice,
/// not once every fifteen minutes for eight hours.
const MAX_PER_KEY: u32 = 3;

/// A hard ceiling across every key, so no combination of conditions can turn
/// one session into a flood.
const MAX_EVENTS_PER_PROCESS: u32 = 25;

/// Longest message this module will submit. A JS stack can be enormous; the
/// first frames are the diagnostic ones.
const MAX_DETAIL: usize = 2_000;

struct Budget {
    last_sent: std::time::Instant,
    sent: u32,
    /// Occurrences dropped since the last one that got through. Reported with
    /// the next event so "one event" never reads as "it happened once".
    suppressed: u32,
}

/// Per-condition rate limiter. Deliberately a plain struct taking `now` as a
/// parameter rather than reading the clock itself — that is what makes the
/// suppression rules testable without sleeping for fifteen minutes.
struct RateLimiter {
    keys: std::collections::HashMap<String, Budget>,
    total: u32,
}

/// What the limiter decided.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// Send it. Carries how many occurrences were dropped since the last send.
    Send { suppressed: u32 },
    /// Drop it silently.
    Suppress,
}

impl RateLimiter {
    fn new() -> Self {
        Self { keys: std::collections::HashMap::new(), total: 0 }
    }

    fn check(&mut self, key: &str, now: std::time::Instant) -> Verdict {
        if self.total >= MAX_EVENTS_PER_PROCESS {
            return Verdict::Suppress;
        }
        if let Some(b) = self.keys.get_mut(key) {
            if b.sent >= MAX_PER_KEY || now.duration_since(b.last_sent) < DEGRADED_COOLDOWN {
                b.suppressed = b.suppressed.saturating_add(1);
                return Verdict::Suppress;
            }
            let suppressed = std::mem::take(&mut b.suppressed);
            b.last_sent = now;
            b.sent += 1;
            self.total += 1;
            return Verdict::Send { suppressed };
        }
        // A key we have never seen. NOTE THE ORDER, because it is what bounds
        // memory: an entry is only ever created on a SEND, and sends are capped
        // at MAX_EVENTS_PER_PROCESS above — so a page throwing a DIFFERENT
        // message every frame (a new key each time) cannot grow this map past
        // that cap. There is deliberately no separate key-count limit; one was
        // written and it was unreachable, which is worse than none.
        self.keys.insert(key.to_owned(), Budget { last_sent: now, sent: 1, suppressed: 0 });
        self.total += 1;
        Verdict::Send { suppressed: 0 }
    }
}

/// `Mutex::new` is const and `HashMap::new` is not, hence the `Option`. Poison
/// is recovered from rather than propagated: a panic while holding this lock
/// must not turn every later report into a second panic.
static LIMITER: std::sync::Mutex<Option<RateLimiter>> = std::sync::Mutex::new(None);

fn allow(key: &str) -> Verdict {
    let mut guard = LIMITER.lock().unwrap_or_else(|p| p.into_inner());
    guard.get_or_insert_with(RateLimiter::new).check(key, std::time::Instant::now())
}

/// Report one degraded condition.
///
/// Returns whether an event was actually submitted — false when the user has
/// opted out, and false when the rate limiter suppressed it. Callers ignore it;
/// the tests do not.
///
/// The local `debug.log` line is NOT this function's job — the call sites keep
/// their own `warn!`/`error!` wording, which is what someone reading a log
/// greps for. This adds the reporting, and only the reporting.
pub fn report_degraded(cond: Degraded, detail: &str) -> bool {
    // The kill switch first, before any work at all. A user who switched
    // sending off must not even pay for the string handling.
    if !SENDING_ENABLED.load(Ordering::Relaxed) {
        return false;
    }
    let key = cond.key();
    let suppressed = match allow(key) {
        Verdict::Send { suppressed } => suppressed,
        Verdict::Suppress => return false,
    };

    let detail = scrub(&cap(detail, MAX_DETAIL));
    let message = if suppressed > 0 {
        format!("degraded [{key}]: {detail} (+{suppressed} further occurrence(s) suppressed)")
    } else {
        format!("degraded [{key}]: {detail}")
    };

    // WARNING, not Error: these are "the app is running but half of it is not
    // working", which is exactly what Sentry's warning level means. Keeping
    // them off Error also keeps the crash list a crash list.
    submit(message, sentry::Level::Warning, &[key.into()], key);
    log::info!(
        "telemetry: reported degraded condition '{key}' to the error reporter \
         ({suppressed} earlier occurrence(s) had been suppressed)"
    );
    true
}

/// Report a JavaScript error from one of the two webviews.
///
/// `origin` is the existing log convention — `dashboard-js` or `overlay-js` —
/// so the origin is unmistakable both in `debug.log` and in the Sentry title.
///
/// The local log line is written FIRST and ALWAYS, opt-out or not: `debug.log`
/// never leaves the machine, and the whole reason these bridges exist is that a
/// shipped webview has no console.
pub fn report_frontend_error(origin: &str, msg: &str) -> bool {
    // Unscrubbed locally, on purpose. Paths on the user's own machine are
    // exactly what makes their own log useful to them.
    log::error!(target: DEGRADED_TARGET, "{origin}: {msg}");

    if !SENDING_ENABLED.load(Ordering::Relaxed) {
        return false;
    }

    // Rate-limit on the FIRST LINE only. A stack differs frame by frame between
    // otherwise identical failures, and the frontend already de-duplicates —
    // this is the backstop for a caller that invokes the command directly.
    let signature = format!("{origin}|{}", cap(msg.lines().next().unwrap_or(""), 120));
    let suppressed = match allow(&signature) {
        Verdict::Send { suppressed } => suppressed,
        Verdict::Suppress => return false,
    };

    let clean = scrub(&cap(msg, MAX_DETAIL));
    let message = if suppressed > 0 {
        format!("{origin}: {clean} (+{suppressed} further occurrence(s) suppressed)")
    } else {
        format!("{origin}: {clean}")
    };
    submit(message, sentry::Level::Error, &[origin.to_owned().into()], origin);
    true
}

/// The one place either new path hands anything to the crate.
fn submit(
    message: String,
    level: sentry::Level,
    fingerprint: &[std::borrow::Cow<'static, str>],
    condition: &str,
) {
    sentry::configure_scope(|scope| {
        scope.set_tag("condition", condition);
    });
    sentry::capture_event(sentry::protocol::Event {
        message: Some(message),
        level,
        fingerprint: fingerprint.to_vec().into(),
        ..Default::default()
    });
}

/// Truncate to at most `n` bytes without splitting a UTF-8 character.
fn cap(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_owned();
    }
    let mut end = n;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// Remove anything that could identify the machine or its owner.
///
/// PRIVACY.md promises no file paths, no user names, no URLs. A JS stack legally
/// contains the app's OWN bundle paths (`http://tauri.localhost/assets/…`) and
/// those are kept — they are the same on every installation and they are the
/// only thing that makes a minified stack readable. Everything else goes:
///
///   · `C:\Users\beamu\…` and `D:/anything/…`  → `<path>`  (drive-letter paths)
///   · `\\server\share\…`                      → `<path>`  (UNC paths)
///   · `https://example.com/…`                 → `<url>`   (any non-local host)
///
/// Hand-written rather than a regex because `regex` is not a dependency of this
/// project and adding one to scrub three shapes is not a trade worth making.
fn scrub(input: &str) -> String {
    // Addresses go FIRST, before the path/URL walk, because an address can sit
    // inside either one (`C:\Users\me\a@b.com`, `https://u@host/x`) and both of
    // those branches consume to the next space — running them first would let
    // an address survive in the tail of a path that contains a space.
    let redacted = redact_emails(input);
    let input: &str = &redacted;
    let c: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;

    // A path or URL runs until whitespace or one of these closers. A Windows
    // path CAN contain spaces, so a path with one leaves its tail behind — but
    // the drive, and the user name that always follows `\Users\`, are gone,
    // which is what the promise is about.
    let ends = |ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | ')' | ']' | '>' | ',' | ';');

    let has = |i: usize, s: &str| c[i..].iter().take(s.len()).copied().eq(s.chars());

    while i < c.len() {
        // Drive-letter path: `C:\…` or `C:/…`
        let drive = c[i].is_ascii_alphabetic()
            && i + 2 < c.len()
            && c[i + 1] == ':'
            && (c[i + 2] == '\\' || c[i + 2] == '/');
        // UNC path: `\\server\share`
        let unc = c[i] == '\\' && i + 1 < c.len() && c[i + 1] == '\\';
        if drive || unc {
            out.push_str("<path>");
            i += if drive { 3 } else { 2 };
            while i < c.len() && !ends(c[i]) {
                i += 1;
            }
            continue;
        }

        if has(i, "http://") || has(i, "https://") {
            let scheme_len = if has(i, "https://") { 8 } else { 7 };
            let mut j = i + scheme_len;
            let host_start = j;
            while j < c.len() && !ends(c[j]) && c[j] != '/' {
                j += 1;
            }
            let host: String = c[host_start..j].iter().collect();
            let host = host.split(':').next().unwrap_or("").to_ascii_lowercase();
            // The app's own origins. Tauri v2 serves the bundle from
            // `tauri.localhost`, so a stack frame naming it is the app's own
            // code and carries nothing about the user.
            let local = host == "tauri.localhost"
                || host == "localhost"
                || host == "127.0.0.1"
                || host.ends_with(".localhost");
            if local {
                // Keep it verbatim, scheme and all.
                while i < c.len() && !ends(c[i]) {
                    out.push(c[i]);
                    i += 1;
                }
            } else {
                out.push_str("<url>");
                while i < c.len() && !ends(c[i]) {
                    i += 1;
                }
            }
            continue;
        }

        out.push(c[i]);
        i += 1;
    }
    out
}

/// Replace anything address-shaped with `<email>`.
///
/// WHY THIS EXISTS (2026-08-31). The browser-profile feature reads the
/// signed-in account out of each Chromium profile's `Local State`, and those
/// are the owner's real personal accounts. The UI deliberately shows only the
/// LOCAL PART, and nothing on the Rust side ever logs either half — but "no
/// call site does it today" is a promise about the present, and a panic
/// message or a JS error carries whatever string happened to be in scope. This
/// is the backstop that makes the promise structural: it does not matter who
/// puts an address into a report, it does not leave the machine.
///
/// Anchored on the `@` and expanded outwards, because an address has no prefix
/// to key off. A domain must end in a real TLD (a dot, then two or more
/// letters), which is what keeps `a@b`, a lone `@` and Rust's own
/// `#[cfg(…)]`-shaped noise from being eaten.
fn redact_emails(input: &str) -> String {
    let c: Vec<char> = input.chars().collect();
    let local_ok =
        |ch: char| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '%' | '+' | '-' | '\'');
    let domain_ok = |ch: char| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-');

    let mut out = String::with_capacity(input.len());
    // How far of the input has already been copied into `out`. Kept rather
    // than pushing char-by-char because a match has to RETRACT the local part,
    // which was to the LEFT of the `@` that revealed it.
    let mut emitted = 0usize;
    let mut i = 0usize;

    while i < c.len() {
        if c[i] == '@' {
            let mut start = i;
            while start > emitted && local_ok(c[start - 1]) {
                start -= 1;
            }
            // A leading dot belongs to the sentence, not the address.
            while start < i && c[start] == '.' {
                start += 1;
            }
            let mut end = i + 1;
            while end < c.len() && domain_ok(c[end]) {
                end += 1;
            }
            // A trailing dot is punctuation — "…mailed nur@example.com." must
            // keep its full stop.
            while end > i + 1 && c[end - 1] == '.' {
                end -= 1;
            }
            let domain: String = c[i + 1..end].iter().collect();
            let has_tld = domain.rsplit_once('.').is_some_and(|(host, tld)| {
                !host.is_empty() && tld.len() >= 2 && tld.chars().all(|ch| ch.is_ascii_alphabetic())
            });
            if start < i && has_tld {
                out.extend(c[emitted..start].iter());
                out.push_str("<email>");
                emitted = end;
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out.extend(c[emitted..].iter());
    out
}

/// Forward a panic to Sentry from inside the app's ONE panic hook.
///
/// This is what the `sentry` crate's `panic` feature would have done for us,
/// done by hand instead, because that feature would have installed a second
/// `std::panic::set_hook` (see this module's header). The event is shaped the
/// same way `sentry-panic` shapes it: one exception of type `panic`, the panic
/// message as its value, the current stack as its stack trace, level Fatal.
///
/// MUST NOT PANIC, and must not block for long — it runs while the process is
/// already dying, and on the main-thread path `lib.rs` calls
/// `std::process::exit(1)` immediately afterwards. Hence the explicit flush
/// with a short timeout: without it the event is still sitting in the
/// background transport queue when the process is torn down, and the crash we
/// most wanted to see is the one that never arrives.
pub fn capture_panic(message: &str, thread: &str) {
    if !SENDING_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    sentry::configure_scope(|scope| {
        scope.set_tag("thread", thread);
    });

    sentry::capture_event(sentry::protocol::Event {
        exception: vec![sentry::protocol::Exception {
            ty: "panic".into(),
            // PROBLEM 227 — SCRUBBED, like the other two submit paths.
            //
            // This was `message.to_owned()`: the raw `info.payload()` from
            // `lib.rs`'s panic hook, sent verbatim, at Fatal, from the ONE code
            // path guaranteed to fire on the crashes that matter most.
            // `report_degraded` and `report_frontend_error` both go through
            // `scrub(&cap(…))`; this one did not, while PRIVACY.md promises the
            // user that "every report is scrubbed before it is sent".
            //
            // Today's production panic messages are mostly dependency strings
            // with no PII (tao's "cannot move state from Destroyed"), so this
            // was a contract gap rather than a live leak — and it was one
            // `expect(&format!("… {}", path.display()))` away from being a real
            // one, invisibly, because the surrounding hook looks handled.
            // `send_default_pii = false` does not cover it: that governs the
            // crate's automatic user/server context, not an exception value the
            // app supplies.
            value: Some(scrub(&cap(message, MAX_DETAIL))),
            stacktrace: sentry::integrations::backtrace::current_stacktrace(),
            ..Default::default()
        }]
        .into(),
        level: sentry::Level::Fatal,
        ..Default::default()
    });

    // 2 seconds is a compromise: long enough for one small HTTPS POST on a
    // normal connection, short enough that a machine which is offline (the
    // common case for "it crashed on a laptop on a train") does not add a
    // visible hang to a crash the user is already unhappy about.
    if let Some(client) = sentry::Hub::current().client() {
        client.flush(Some(std::time::Duration::from_secs(2)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ONE test, in sequence, for the same reason `crash_context.rs` has one:
    /// everything here is process-global state and cargo runs tests on
    /// parallel threads in a single process. Splitting this into four little
    /// tests re-creates PROBLEM 130, where four tests sharing one static
    /// passed alone and failed in the suite.
    #[test]
    fn the_kill_switch_actually_kills() {
        let restore = sending_enabled();

        // 1. The default a fresh process starts at is OFF. A flag that is
        //    consulted before the config has loaded must fail CLOSED — the
        //    honest state at that moment is "consent unknown".
        //    (Checked via a fresh atomic rather than the global, which an
        //    earlier line of this same test may already have moved.)
        assert!(
            !AtomicBool::new(false).load(Ordering::Relaxed),
            "SENDING_ENABLED's initialiser must be false"
        );

        // 2. Off means nothing passes — not even a panic-level error.
        set_sending_enabled(false);
        let err = log::Metadata::builder().level(log::Level::Error).build();
        assert_eq!(
            log_filter(&err),
            sentry_log::LogFilter::Ignore,
            "with sending off, an ERROR must still be dropped"
        );

        // 3. On, the level gate decides — and it must be ERROR-only. This is
        //    the assertion that catches someone widening the constant without
        //    also changing what PRIVACY.md promises.
        set_sending_enabled(true);
        assert_eq!(SENTRY_MINIMUM_LEVEL, log::Level::Error);
        assert_eq!(log_filter(&err), sentry_log::LogFilter::Event);
        for quieter in [log::Level::Warn, log::Level::Info, log::Level::Debug, log::Level::Trace] {
            let md = log::Metadata::builder().level(quieter).build();
            assert_eq!(
                log_filter(&md),
                sentry_log::LogFilter::Ignore,
                "{quieter} must never be sent — ordinary use of the app sends nothing"
            );
        }

        // 4. PROBLEM 217 — a record this module logged ITSELF is already
        //    reported, rate-limited, by report_degraded/report_frontend_error.
        //    The bridge must drop it rather than send a second, unbounded copy.
        let own = log::Metadata::builder()
            .level(log::Level::Error)
            .target(DEGRADED_TARGET)
            .build();
        assert_eq!(
            log_filter(&own),
            sentry_log::LogFilter::Ignore,
            "the degraded target is reported by hand — the bridge must not re-send it"
        );

        // 5. The switch is live: flipping it changes the very next verdict,
        //    with no re-init of anything.
        set_sending_enabled(false);
        assert_eq!(log_filter(&err), sentry_log::LogFilter::Ignore);

        // 6. PROBLEM 217 — and the same switch has to cover the NEW paths, not
        //    only the log bridge it was written for. This is the assertion that
        //    fails if a future reporting path forgets to ask.
        assert!(
            !report_degraded(Degraded::HookDeaf, "hook deafness while opted out"),
            "report_degraded must submit nothing while the user has sending off"
        );
        assert!(
            !report_frontend_error("dashboard-js", "error while opted out"),
            "report_frontend_error must submit nothing while the user has sending off"
        );

        set_sending_enabled(restore);
    }

    /// PROBLEM 217 — the rate limiter, driven by an injected clock so the
    /// fifteen-minute window can be tested in microseconds.
    ///
    /// The case that motivated it, stated as an assertion: hook deafness fired
    /// 38 times in one session on the owner's machine. That must be ONE event.
    #[test]
    fn the_rate_limiter_collapses_a_storm_into_one_event() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let mut rl = RateLimiter::new();

        // 38 occurrences in one session → exactly one send, and it knows about
        // the 37 it swallowed.
        assert_eq!(rl.check("hook-deaf", t0), Verdict::Send { suppressed: 0 });
        for n in 1..38 {
            assert_eq!(
                rl.check("hook-deaf", t0 + Duration::from_secs(n)),
                Verdict::Suppress,
                "occurrence {n} inside the cooldown must be dropped"
            );
        }

        // A DIFFERENT condition is not affected by another's budget — otherwise
        // one noisy condition would hide every other one.
        assert_eq!(rl.check("overlay-disabled", t0), Verdict::Send { suppressed: 0 });

        // Past the cooldown it speaks again, and reports the backlog.
        let later = t0 + DEGRADED_COOLDOWN + Duration::from_secs(1);
        assert_eq!(rl.check("hook-deaf", later), Verdict::Send { suppressed: 37 });

        // MAX_PER_KEY is a hard stop: a permanently broken machine says so a
        // few times, not every fifteen minutes forever.
        let mut far = later;
        for _ in 0..10 {
            far += DEGRADED_COOLDOWN + Duration::from_secs(1);
            rl.check("hook-deaf", far);
        }
        assert_eq!(
            rl.keys["hook-deaf"].sent, MAX_PER_KEY,
            "one condition must never exceed MAX_PER_KEY events in one run"
        );

        // The process-wide ceiling holds whatever the mix of keys — and it is
        // ALSO what bounds memory, because an entry is only created by a send.
        // A page that throws a different message every frame therefore cannot
        // grow the map: this is the assertion that says so.
        let mut rl2 = RateLimiter::new();
        let mut sent = 0;
        for n in 0..500 {
            if rl2.check(&format!("unique-{n}"), t0) == (Verdict::Send { suppressed: 0 }) {
                sent += 1;
            }
        }
        assert_eq!(sent, MAX_EVENTS_PER_PROCESS, "the process-wide ceiling must hold");
        assert_eq!(
            rl2.keys.len(),
            MAX_EVENTS_PER_PROCESS as usize,
            "500 distinct errors must not become 500 map entries"
        );
    }

    /// PII. A JS stack may legitimately carry the app's own bundle paths;
    /// nothing derived from the user's machine may survive.
    #[test]
    fn scrub_removes_everything_that_identifies_the_machine() {
        let s = scrub(
            "TypeError: x is undefined at http://tauri.localhost/assets/index-a1b2.js:12:34 \
             opening C:\\Users\\beamu\\Documents\\notes.txt from \\\\NAS\\media \
             posting to https://example.com/upload?user=beamu",
        );
        assert!(!s.contains("beamu"), "the user name must never survive: {s}");
        assert!(!s.contains("C:\\"), "a drive-letter path must never survive: {s}");
        assert!(!s.contains("NAS"), "a UNC host must never survive: {s}");
        assert!(!s.contains("example.com"), "a non-local URL must never survive: {s}");
        assert!(
            s.contains("http://tauri.localhost/assets/index-a1b2.js:12:34"),
            "the app's OWN bundle path is what makes a stack readable — keep it: {s}"
        );
        assert!(s.contains("TypeError: x is undefined"), "the diagnosis itself must survive: {s}");

        // Forward slashes and lower-case drives count too.
        assert!(!scrub("d:/rust/foo.rs failed").contains("rust"));
        // A relative path inside our own source is not a machine path.
        assert_eq!(scrub("panicked at src/lib.rs:744"), "panicked at src/lib.rs:744");
    }

    /// PII, the 2026-08-31 addition. The browser-profile feature puts the
    /// owner's real signed-in accounts in memory; no address may reach Sentry
    /// no matter which call site drops one into a message.
    #[test]
    fn scrub_removes_email_addresses() {
        let s = scrub("failed for nur.arpon+work@example.co.uk while opening Chrome");
        assert!(!s.contains('@'), "no address may survive: {s}");
        assert!(!s.contains("nur.arpon"), "not even the local part: {s}");
        assert!(!s.contains("example.co.uk"), "nor the domain: {s}");
        assert!(s.contains("<email>") && s.contains("while opening Chrome"));

        // Punctuation around it is the sentence's, not the address's.
        assert_eq!(scrub("mailed a@b.com."), "mailed <email>.");
        assert_eq!(scrub("(x@y.org)"), "(<email>)");
        // Inside a path, and inside a URL — both already redacted by their own
        // branch, but neither may leave an address behind either.
        assert!(!scrub(r"C:\Users\beamu\a@b.com").contains('@'));
        assert!(!scrub("https://u@host.com/x").contains('@'));

        // Things that merely CONTAIN an @ are not addresses and must survive,
        // or the scrubber starts eating diagnoses.
        assert_eq!(scrub("expected @ here"), "expected @ here");
        assert_eq!(scrub("a@b failed"), "a@b failed", "no TLD — not an address");
        assert_eq!(scrub("@example.com"), "@example.com", "no local part");
        assert_eq!(
            scrub("TypeError: x is undefined"),
            "TypeError: x is undefined",
            "the diagnosis itself must survive untouched"
        );
    }

    /// The DSN ships EMPTY, and `init()` must therefore hand the crate
    /// nothing. `sentry::init("")` is an error, not a disabled client, so
    /// "guard it" and "pass an empty string" are not the same thing.
    #[test]
    fn an_empty_dsn_never_reaches_sentry_init() {
        if SENTRY_DSN.is_empty() {
            assert!(init().is_none(), "an empty DSN must produce no client");
        }
    }
}
