//! picker_worker.rs — the app picker's scan, OFF the main thread, cached on
//! disk, pre-warmed at boot. PROBLEM 237.
//!
//! WHAT WAS WRONG. `list_start_menu_apps` was a non-`async` `#[tauri::command]`,
//! which in Tauri v2 means it ran ON THE MAIN THREAD: a PowerShell shell-out
//! (2-6 s) followed by ~247 in-process `IShellItemImageFactory` icon
//! extractions (5-13 s), measured 6.4-16.8 s across 15 logged sessions. For
//! that whole time the main thread could not pump messages, so the dashboard
//! did not repaint and every IPC call from BOTH webviews queued behind it —
//! the owner's "Not responding". PROBLEM 205 had documented the block and left
//! it in place because "this COM has never once executed off the main thread
//! in this app". That claim is retired here, with proof (see the tests).
//!
//! THE DESIGN.
//!
//!   1. ONE long-lived worker thread, `st-picker-scan`, joins a single-threaded
//!      apartment once (`ComSta`, `COINIT_APARTMENTTHREADED`) and services
//!      requests from a channel. Commands are `async` and await a oneshot
//!      reply, so nothing here ever runs on the main thread or blocks a
//!      runtime worker. STA, not MTA: third-party icon handlers the shell may
//!      load are `ThreadingModel=Apartment`; from an MTA they would be hosted
//!      in a hidden STA and marshalled, which is slower and can time out.
//!      No message pump is needed because every call the worker makes is
//!      synchronous and in-apartment (proved by the test, not assumed).
//!
//!   2. A DISK CACHE, `%APPDATA%\Spaceadom\picker-cache.json`: the app list
//!      with its icons, keyed by a fingerprint of both Start-Menu trees (file
//!      count + every file and folder mtime, plus the app version). On a hit
//!      the command answers from the file in a few ms and the worker refreshes
//!      in the background; if the refresh changes the list it emits
//!      `picker-data-updated` so an open picker can re-render.
//!      **PROBLEM 237 follow-up (2026-09-09):** a fingerprint MISMATCH — the
//!      Start Menu changed since the cache was written, which happens on
//!      every install/uninstall — used to be treated exactly like a miss and
//!      forced a synchronous scan before answering (measured 7.5-14 s). It is
//!      now treated like a HIT: the stale list answers immediately and the
//!      same background refresh runs, emitting `picker-data-updated` if the
//!      fresh scan differs. Only a cache that is genuinely unusable — missing,
//!      unreadable, the wrong format, or an empty app list — still scans
//!      before answering, because there is nothing else to serve. The pure
//!      decision behind this is `decide_serve`, just above `serve_apps`. A
//!      scan that errored never overwrites the cache — a bad PowerShell run
//!      would otherwise poison every later open.
//!
//!   3. PRE-WARM AT BOOT (`warm_picker_at_startup`, default ON): a few seconds
//!      after the windows exist, the worker validates the cache and refreshes
//!      it, so the FIRST picker open of a session is served from memory. Off
//!      means nothing runs until the first open, which is now non-blocking
//!      anyway.
//!
//! The fingerprint does not see Store apps (they live in `shell:AppsFolder`,
//! not in a folder we can stat) — that is what the background refresh is for.
//! The cache is a first answer, never the only one.

use crate::commands::AppInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tauri::Emitter;

/// The same map `commands::IconCacheState` wraps: exe/lnk/AUMID → base64 PNG.
pub type IconCache = Arc<Mutex<HashMap<String, String>>>;

/// Emitted (globally — `emit_to` has never worked in this app) when a
/// background refresh produced a DIFFERENT list from the one last handed out.
/// Payload: `PickerDataUpdated`. The list itself is not in the event: the
/// listener re-invokes `list_start_menu_apps`, which now answers from memory.
pub const UPDATED_EVENT: &str = "picker-data-updated";

/// Payload of `UPDATED_EVENT`.
#[derive(Serialize, Clone, Debug)]
pub struct PickerDataUpdated {
    /// Apps in the new list.
    pub count: usize,
    /// Apps in the list it replaced.
    pub previous: usize,
}

const CACHE_FILE: &str = "picker-cache.json";
/// Bump when `AppInfo` or the icon format changes; it is part of the
/// fingerprint, so an old file is a miss rather than a wrong answer.
const CACHE_FORMAT: u32 = 1;
/// How long after the windows exist the boot warm-up starts. Long enough for
/// the dashboard's first paint and the starry sky to be done; short enough
/// that a user reaching for a key inside the first minute finds it warm.
const BOOT_WARM_DELAY: Duration = Duration::from_secs(4);

#[derive(Serialize, Deserialize)]
struct CacheFile {
    format: u32,
    fingerprint: String,
    apps: Vec<AppInfo>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// How long a caller waits for the worker before giving up on it.
///
/// REVIEW FIX 2026-09-04. `rx.await` was unbounded: one wedged PowerShell (or
/// one shell extension deadlocked inside `IShellItemImageFactory`) left the
/// frontend's `invoke` promise pending FOREVER — no resolve, no reject, so
/// `app-grid.ts`'s `.catch` never ran and the grid sat on "Scanning this
/// device…" for the rest of the session with nothing in the UI or the log
/// saying why.
///
/// 60 s, against the 30 s PowerShell timeout below: a scan that has to run cold
/// is 6-17 s measured, and even a scan that hits the PowerShell timeout and
/// unwinds normally answers inside 35 s. So this bound is only ever reached by
/// something genuinely stuck, and it is never reached by a slow machine.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// How long the PowerShell shell-out may run before it is killed.
///
/// REVIEW FIX 2026-09-04. `cmd.output()` waits forever. PowerShell on this
/// machine takes 2-6 s for this script; 30 s is five times the worst measured
/// run, so a timeout here means the process is not coming back, not that the
/// machine is slow. Killing it closes its pipes, the read unblocks, the scan
/// returns `Err`, and — the part that matters — the WORKER LOOP CONTINUES, so
/// the next picker open gets a fresh attempt instead of a dead thread.
const POWERSHELL_TIMEOUT: Duration = Duration::from_secs(30);

/// The picker's app list. Instant when the session or the disk cache has it;
/// a full scan on the worker otherwise. Never runs on the caller's thread.
pub async fn apps(app: tauri::AppHandle, cache: IconCache) -> Result<Vec<AppInfo>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    sender()
        .send(Request::Apps {
            app: Some(app),
            cache,
            reply: Some(tx),
            not_before: None,
        })
        .map_err(|_| "the picker worker is gone".to_string())?;
    await_reply(rx, REQUEST_TIMEOUT).await
}

/// The awaiting half of `apps()`, split out so the timeout path is testable
/// without a Tauri app handle or a live worker (the worker itself is a private
/// global `OnceLock` and is not injectable; this is the seam that is).
///
/// Three outcomes, and all three must REJECT rather than resolve empty — see
/// `serve_apps` and `app-grid.ts` for the other half of that contract:
///   * the worker answered — pass its `Result` straight through;
///   * the worker dropped the request (thread died mid-scan);
///   * the worker did not answer inside `timeout`.
async fn await_reply(
    rx: tokio::sync::oneshot::Receiver<Result<Vec<AppInfo>, String>>,
    timeout: Duration,
) -> Result<Vec<AppInfo>, String> {
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err("the picker worker dropped the request".to_string()),
        Err(_) => {
            // Dropping `rx` here is deliberate and safe: a late `tx.send` on the
            // worker fails harmlessly, and the worker's own loop is unaffected.
            log::error!(
                "start_menu_scan: the picker worker did not answer within {}s — rejecting so the \
                 dashboard's .catch runs and the next open retries",
                timeout.as_secs()
            );
            Err(format!(
                "the app scan did not answer within {}s — it is still running in the background; \
                 close and reopen the picker to try again",
                timeout.as_secs()
            ))
        }
    }
}

/// Boot pre-warm. Reads `warm_picker_at_startup`; when true, queues a
/// delayed warm on the worker and returns immediately. Called from
/// `create_app_windows` (main thread) — it must stay a channel send, nothing
/// heavier.
pub fn warm_at_boot(app: &tauri::AppHandle) {
    use tauri::Manager;
    let wants_it = app
        .state::<crate::commands::ConfigState>()
        .0
        .read()
        .map(|c| c.warm_picker_at_startup)
        .unwrap_or(true);
    if !wants_it {
        log::info!(
            "picker_warm: warm_picker_at_startup is OFF — the app list will be scanned on the \
             first picker open (off the main thread) instead of now"
        );
        return;
    }
    let cache = Arc::clone(&app.state::<crate::commands::IconCacheState>().0);
    log::info!(
        "picker_warm: warm_picker_at_startup is ON — the app list will be validated/refreshed \
         on the worker in {}s so the first picker open is served from memory",
        BOOT_WARM_DELAY.as_secs()
    );
    let _ = sender().send(Request::Apps {
        app: Some(app.clone()),
        cache,
        reply: None,
        not_before: Some(Instant::now() + BOOT_WARM_DELAY),
    });
}

// ---------------------------------------------------------------------------
// The worker
// ---------------------------------------------------------------------------

enum Request {
    Apps {
        /// Needed to emit `UPDATED_EVENT`; `None` only in tests.
        app: Option<tauri::AppHandle>,
        cache: IconCache,
        /// `None` = a warm-up: populate, do not answer anyone.
        ///
        /// REVIEW FIX 2026-09-04 — the payload is a `Result`, not a bare
        /// `Vec`. A failed scan used to answer `Ok(vec![])`, which the frontend
        /// cached as a perfectly good empty app list for the rest of the
        /// session (`app-grid.ts` treats any array as a cache hit). One bad
        /// PowerShell run therefore meant "you have no apps" until the app was
        /// restarted, with no error anywhere the user could see.
        reply: Option<tokio::sync::oneshot::Sender<Result<Vec<AppInfo>, String>>>,
        /// A warm-up may be deferred; a real request never is.
        not_before: Option<Instant>,
    },
}

static SENDER: OnceLock<crossbeam_channel::Sender<Request>> = OnceLock::new();

/// The worker is spawned on first use, from whichever thread asks first.
fn sender() -> &'static crossbeam_channel::Sender<Request> {
    SENDER.get_or_init(|| {
        let (tx, rx) = crossbeam_channel::unbounded::<Request>();
        let spawned = std::thread::Builder::new()
            .name("st-picker-scan".into())
            .spawn(move || worker_main(rx));
        if let Err(e) = spawned {
            // The sender still exists; every request will fail with "gone".
            log::error!("picker_worker: could not spawn st-picker-scan ({e}) — the picker will report an empty list");
        }
        tx
    })
}

struct Session {
    /// The list last handed out, served instantly to every later request.
    list: Option<Vec<AppInfo>>,
    /// Fingerprint the currently-served list was captured under.
    fingerprint: Option<String>,
    /// Wall-clock moment the currently-served list was captured — the disk
    /// cache file's own mtime on a cache load, or "now" the instant a scan
    /// (forced first-run, or background refresh) finished. Purely for the
    /// `list_age_ms` the serve-path log line reports; no decision reads it.
    captured_at: Option<SystemTime>,
    /// The fingerprint a refresh has been started for (successfully or not)
    /// this session. `None` before the first one ever runs.
    ///
    /// PROBLEM 237 follow-up: this used to be a plain `bool`, so a refresh
    /// ran at most ONCE PER SESSION, ever. That is now wrong on its own
    /// terms — with a fingerprint mismatch answered instantly instead of
    /// forcing a scan, the disk cache would otherwise never be corrected
    /// again after the first refresh, no matter how many programs get
    /// installed later in the same run. Comparing this fingerprint to the
    /// CURRENT one turns the cap into "once per fingerprint CHANGE," which
    /// is what the owner asked for and what `decide_serve` below expects.
    refresh_started_for: Option<String>,
}

fn worker_main(rx: crossbeam_channel::Receiver<Request>) {
    // The apartment, for the life of the thread. Every `extract_icon` inside
    // takes its own balanced S_FALSE/CoUninitialize pair on top; this guard is
    // what keeps the count above zero between calls so the shell's in-proc
    // objects are not torn down and rebuilt 247 times.
    let com = crate::icon_extractor::ComSta::new();
    lower_priority();
    log::info!(
        "picker_worker: st-picker-scan started (os thread {}, STA joined: {})",
        os_thread_id(),
        com.joined()
    );

    let mut session =
        Session { list: None, fingerprint: None, captured_at: None, refresh_started_for: None };
    let mut deferred: Option<Request> = None;

    loop {
        // A deferred warm-up waits with a timeout; anything real jumps ahead
        // of it (and satisfies it, since it fills the session).
        let next = match &deferred {
            Some(Request::Apps { not_before: Some(t), .. }) => {
                let wait = t.saturating_duration_since(Instant::now());
                match rx.recv_timeout(wait) {
                    Ok(r) => r,
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => match deferred.take() {
                        Some(r) => r,
                        None => continue,
                    },
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
                }
            }
            _ => match rx.recv() {
                Ok(r) => r,
                Err(_) => return,
            },
        };

        match next {
            Request::Apps { not_before: Some(t), reply: None, app, cache }
                if t > Instant::now() && session.list.is_none() =>
            {
                deferred = Some(Request::Apps { app, cache, reply: None, not_before: Some(t) });
            }
            Request::Apps { app, cache, reply, .. } => {
                serve_apps(&mut session, app.as_ref(), &cache, reply);
                if session.list.is_some() {
                    deferred = None; // a pending warm-up has nothing left to do
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The pure decision core — PROBLEM 237 follow-up (fingerprint-mismatch stall)
// ---------------------------------------------------------------------------
//
// Before this change the mismatch-forces-a-scan bug lived INSIDE `serve_apps`,
// entangled with the disk read, the scan and the reply, so it could not be
// unit-tested without shelling out to PowerShell. Pulled out here it is three
// plain facts in, one verdict out, with no I/O — every branch is one line.

/// The third fact `decide_serve` takes. A plain `Option<bool>` cannot tell
/// "no scan has run yet" apart from "a scan for this exact fingerprint is
/// already running, started by an earlier request" — and that distinction is
/// exactly what keeps two rapid picker opens during one refresh from ever
/// starting a second one, so it gets its own case rather than being folded
/// into a bool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStatus {
    /// No scan has been started for the CURRENT fingerprint yet this session.
    NotAttempted,
    /// A scan for the current fingerprint is already running (an earlier
    /// request already started one and it has not returned) or has already
    /// completed, successfully or not. Either way, do not start another.
    AlreadyHandled,
    /// This call IS a scan completing right now, with this outcome.
    JustFinished(bool),
}

/// What `serve_apps` should do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeDecision {
    /// Nothing usable exists yet — first run ever, or a cache file that did
    /// not parse, was the wrong format, or held zero apps. There is nothing
    /// to answer WITH, so a scan must run before anyone gets a reply. This
    /// is the ONE case this whole change leaves alone.
    ScanBeforeAnswering,
    /// A list is available — fresh or stale, it makes no difference to the
    /// caller waiting on it — so answer with it immediately. `start_refresh`
    /// is true only when no scan is already running or done for the current
    /// fingerprint.
    ServeCachedAndMaybeRefresh { start_refresh: bool },
    /// A scan (the forced first-run scan, or a background refresh) just
    /// finished; fold its result into state. `accept` mirrors success: true
    /// means adopt the new list and fingerprint; false means change NOTHING
    /// — the list already being served keeps being served and the
    /// fingerprint stays whatever it was, so a failed refresh can never
    /// promote a stale cache to "fresh".
    ApplyScanOutcome { accept: bool },
}

/// The pure function itself, over the three facts named in the brief:
///
/// * `cache_present` — is there ANY usable list to serve right now (session
///   memory, or a disk cache that parsed, matched the format and was
///   non-empty)? Fingerprint match is irrelevant to this question.
/// * `fingerprint_matches` — does that cache's fingerprint match the current
///   Start Menu? Deliberately does **not** change the verdict of either the
///   `ScanBeforeAnswering` or `ServeCachedAndMaybeRefresh` branch below — a
///   stale cache is served exactly like a fresh one. THAT is the fix; see
///   `stale_cache_is_served_exactly_like_a_fresh_one` in the tests, which
///   pins it down as a regression test. It is kept as its own parameter
///   (rather than dropped) so the "it truly does not matter any more" claim
///   is something a reader can see proven, not something to take on faith.
/// * `scan_succeeded` — `ScanStatus`, see above.
pub fn decide_serve(
    cache_present: bool,
    fingerprint_matches: bool,
    scan_status: ScanStatus,
) -> ServeDecision {
    let _ = fingerprint_matches; // see the doc comment above: proven not to matter here.
    match scan_status {
        ScanStatus::JustFinished(ok) => ServeDecision::ApplyScanOutcome { accept: ok },
        ScanStatus::NotAttempted | ScanStatus::AlreadyHandled if !cache_present => {
            ServeDecision::ScanBeforeAnswering
        }
        ScanStatus::NotAttempted => ServeDecision::ServeCachedAndMaybeRefresh { start_refresh: true },
        ScanStatus::AlreadyHandled => ServeDecision::ServeCachedAndMaybeRefresh { start_refresh: false },
    }
}

/// One log line, every serve, naming exactly which path answered and how old
/// the answered list was. The leading literal is long and written directly
/// (never assembled from shorter pieces) so it survives as a contiguous
/// ASCII needle in the built exe — see CLAUDE.md's note on short identifiers
/// being split across overlapping immediate stores.
fn log_picker_served(path: &str, served: usize, answer_ms: u128, list_age_ms: Option<u128>) {
    log::info!(
        "picker-serve-decision-path-and-list-age-marker-spaceadom-237: path={path} served={served} \
         app(s) answer_took_ms={answer_ms} list_age_ms={age}",
        age = list_age_ms.map(|a| a.to_string()).unwrap_or_else(|| "unknown".to_string())
    );
}

fn serve_apps(
    session: &mut Session,
    app: Option<&tauri::AppHandle>,
    cache: &IconCache,
    reply: Option<tokio::sync::oneshot::Sender<Result<Vec<AppInfo>, String>>>,
) {
    let t0 = Instant::now();
    let roots = start_menu_roots();

    // 1. Session memory — the steady state after the first answer. Reply
    // FIRST, with zero filesystem work in front of it; the fingerprint walk
    // that decides whether to refresh happens only after the caller already
    // has their answer.
    if let Some(list) = session.list.clone() {
        let n = list.len();
        if let Some(tx) = reply {
            let _ = tx.send(Ok(list));
        }
        let fingerprint = fingerprint_for(&roots);
        let age_ms = session
            .captured_at
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .map(|d| d.as_millis());
        log_picker_served("session_memory", n, t0.elapsed().as_millis(), age_ms);

        let fp_matches = session.fingerprint.as_deref() == Some(fingerprint.as_str());
        let already = session.refresh_started_for.as_deref() == Some(fingerprint.as_str());
        let status = if already { ScanStatus::AlreadyHandled } else { ScanStatus::NotAttempted };
        if let ServeDecision::ServeCachedAndMaybeRefresh { start_refresh: true } =
            decide_serve(true, fp_matches, status)
        {
            session.refresh_started_for = Some(fingerprint.clone());
            refresh(session, app, cache, &fingerprint);
        }
        return;
    }

    // 2. The disk cache — ANY usable file, fingerprint match or not.
    //
    // PROBLEM 237 follow-up: this used to call `load_cache(&path, &fingerprint)`,
    // which returns `None` on a mismatch — indistinguishable, to this
    // function, from no cache existing at all — and mismatch therefore fell
    // through to branch 3's synchronous scan. `load_cache_any` drops the
    // fingerprint check; matching is decided AFTER loading, for the refresh
    // decision only, never for whether to serve.
    if let Some(loaded) = load_cache_any(&cache_path()) {
        seed_icon_cache(cache, &loaded.apps);
        let n = loaded.apps.len();
        let apps_for_reply = loaded.apps.clone();
        session.list = Some(loaded.apps);
        session.captured_at = Some(loaded.mtime);
        session.fingerprint = Some(loaded.fingerprint.clone());
        if let Some(tx) = reply {
            let _ = tx.send(Ok(apps_for_reply));
        }

        let fingerprint = fingerprint_for(&roots);
        let fp_matches = loaded.fingerprint == fingerprint;
        let age_ms = SystemTime::now().duration_since(loaded.mtime).ok().map(|d| d.as_millis());
        log_picker_served(
            if fp_matches { "disk_cache_fresh" } else { "disk_cache_stale" },
            n,
            t0.elapsed().as_millis(),
            age_ms,
        );
        log::info!(
            "start_menu_scan: served {n} app(s) from the disk cache in {}ms (fingerprint match: \
             {fp_matches}) — refreshing in the background on worker thread {}",
            t0.elapsed().as_millis(),
            os_thread_id()
        );

        let already = session.refresh_started_for.as_deref() == Some(fingerprint.as_str());
        let status = if already { ScanStatus::AlreadyHandled } else { ScanStatus::NotAttempted };
        if let ServeDecision::ServeCachedAndMaybeRefresh { start_refresh: true } =
            decide_serve(true, fp_matches, status)
        {
            session.refresh_started_for = Some(fingerprint.clone());
            refresh(session, app, cache, &fingerprint);
        }
        return;
    }

    // 3. Nothing usable at all: scan now, answer after. The only branch that
    // may still block the caller — `decide_serve(false, _, NotAttempted)` is
    // always `ScanBeforeAnswering` regardless of fingerprint.
    let fingerprint = fingerprint_for(&roots);
    log::info!(
        "start_menu_scan: no usable disk cache at all (fingerprint {}) — scanning on worker thread \
         {} before answering",
        fingerprint,
        os_thread_id()
    );
    session.refresh_started_for = Some(fingerprint.clone());
    match scan_start_menu(cache) {
        Ok(list) => {
            if save_cache(&cache_path(), &fingerprint, &list) {
                log::info!("start_menu_scan: wrote {} app(s) to {}", list.len(), cache_path().display());
            }
            let n = list.len();
            session.list = Some(list.clone());
            session.fingerprint = Some(fingerprint);
            session.captured_at = Some(SystemTime::now());
            log_picker_served("empty_cache_scan", n, t0.elapsed().as_millis(), Some(0));
            if let Some(tx) = reply {
                let _ = tx.send(Ok(list));
            }
        }
        Err(e) => {
            // REVIEW FIX 2026-09-04 — REJECT, do not answer empty.
            //
            // This used to send `Vec::new()`, i.e. a successful "you have no
            // applications". `app-grid.ts` cached that array for the session
            // (an empty array is truthy in JS), so a single failed PowerShell
            // run turned the picker into a permanently empty grid until the app
            // was restarted — and because it was an `Ok`, the frontend's
            // `.catch` never ran and nothing was ever shown to the user.
            //
            // `session.list` stays `None` on this path, so the very next
            // request re-scans rather than serving the failure from memory.
            log::error!(
                "start_menu_scan: the scan FAILED ({e}) — REJECTING the request (no empty list \
                 is cached, the disk cache is untouched, and the next open re-scans)"
            );
            if let Some(tx) = reply {
                let _ = tx.send(Err(format!("scan failed: {e}")));
            }
        }
    }
}

/// The once-per-FINGERPRINT-CHANGE full scan behind an answer that was
/// already given. `fingerprint` is the value computed by the caller at the
/// moment it decided to refresh — passed in rather than recomputed here so
/// the fingerprint that gates the "did this change anything" comparison
/// below is the exact one `serve_apps` already reasoned about.
fn refresh(session: &mut Session, app: Option<&tauri::AppHandle>, cache: &IconCache, fingerprint: &str) {
    let scan_result = scan_start_menu(cache);
    let ok = scan_result.is_ok();
    let fp_matched_before = session.fingerprint.as_deref() == Some(fingerprint);
    match decide_serve(true, fp_matched_before, ScanStatus::JustFinished(ok)) {
        ServeDecision::ApplyScanOutcome { accept: true } => {
            let list = scan_result.expect("accept=true only when scan_result is Ok");
            let previous = session.list.as_ref().map(|l| l.len()).unwrap_or(0);
            let changed = session.list.as_ref() != Some(&list);
            let fp_changed = session.fingerprint.as_deref() != Some(fingerprint);
            if changed || fp_changed {
                if save_cache(&cache_path(), fingerprint, &list) {
                    log::info!(
                        "start_menu_scan: background refresh rewrote the disk cache ({} app(s); list \
                         changed: {changed}, fingerprint changed: {fp_changed})",
                        list.len()
                    );
                }
            }
            session.fingerprint = Some(fingerprint.to_string());
            session.captured_at = Some(SystemTime::now());
            if changed {
                let count = list.len();
                session.list = Some(list);
                match app {
                    Some(h) => {
                        let r = h.emit(UPDATED_EVENT, PickerDataUpdated { count, previous });
                        log::info!(
                            "start_menu_scan: background refresh CHANGED the list ({previous} → {count}) \
                             — emitted {UPDATED_EVENT} ({})",
                            if r.is_ok() { "ok" } else { "emit failed" }
                        );
                    }
                    None => log::info!(
                        "start_menu_scan: background refresh CHANGED the list ({previous} → {count}) \
                         — no app handle, nothing emitted"
                    ),
                }
            } else {
                log::info!(
                    "start_menu_scan: background refresh found the same {} app(s) — nothing emitted",
                    list.len()
                );
            }
        }
        ServeDecision::ApplyScanOutcome { accept: false } => {
            let e = scan_result.expect_err("accept=false only when scan_result is Err");
            // Nothing about `session` changes: the list already being served
            // stays authoritative and `session.fingerprint` stays whatever it
            // was, so this failure can NEVER be read as "the cache is fresh
            // now" — a stale cache never becomes a permanently fresh-looking
            // one just because one refresh attempt failed.
            log::warn!(
                "start_menu_scan: background refresh FAILED ({e}) — keeping the list already served \
                 and the disk cache as it was; only a NEW fingerprint change will be retried"
            );
        }
        other => unreachable!("ScanStatus::JustFinished always maps to ApplyScanOutcome, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The scan itself (moved verbatim from commands::list_start_menu_apps, minus
// the main-thread log line)
// ---------------------------------------------------------------------------

/// Both Start Menu trees plus `shell:AppsFolder`, icons included. `Err` when
/// PowerShell could not run, its output was not the JSON array we asked for,
/// or it found nothing at all — a machine with zero Start-Menu apps does not
/// exist, so an empty result is a broken scan, not a fact worth caching.
pub fn scan_start_menu(cache: &IconCache) -> Result<Vec<AppInfo>, String> {
    let t_start = Instant::now();
    let script = r#"
        $ErrorActionPreference = 'SilentlyContinue'
        $paths = @(
            "$env:ProgramData\Microsoft\Windows\Start Menu\Programs",
            "$env:AppData\Microsoft\Windows\Start Menu\Programs"
        )
        $apps = Get-ChildItem -Path $paths -Recurse -Filter *.lnk
        $wshell = New-Object -ComObject WScript.Shell
        $results = @()
        foreach ($app in $apps) {
            $shortcut = $wshell.CreateShortcut($app.FullName)
            if ($shortcut.TargetPath -match "\.exe$") {
                # Shortcuts with arguments (Discord: Update.exe --processStart
                # Discord.exe) must be bound AS the .lnk - launching the bare
                # TargetPath starts nothing. ShellExecute runs .lnk with args.
                $path = if ($shortcut.Arguments) { $app.FullName } else { $shortcut.TargetPath }
                $results += [PSCustomObject]@{ Name = $app.BaseName; Path = $path }
            }
        }
        # Microsoft Store / UWP apps have NO .lnk with an .exe target — they
        # live in shell:AppsFolder keyed by AppUserModelID, so the Start-Menu
        # scan above misses every one of them (user report 2026-08-10:
        # "many applications do not arrive in the list").
        try {
            $shellApp = New-Object -ComObject Shell.Application
            $appsFolder = $shellApp.NameSpace("shell:AppsFolder")
            if ($appsFolder) {
                foreach ($item in $appsFolder.Items()) {
                    $aumid = $item.Path
                    # A packaged app's Path is an AppUserModelID
                    # (Package_hash!AppId) — never a filesystem path.
                    if ($aumid -and $aumid.Contains("!") -and $aumid -notmatch '[\\/]|^[A-Za-z]:') {
                        $results += [PSCustomObject]@{
                            Name = $item.Name
                            Path = "shell:AppsFolder\$aumid"
                        }
                    }
                }
            }
        } catch { }

        @($results) | Select-Object Name, Path -Unique | ConvertTo-Json -Compress
    "#;

    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    #[cfg(windows)]
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", script]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    // REVIEW FIX 2026-09-04 — BOUNDED. `cmd.output()` waits for the child
    // forever, on the one thread that answers every picker open, so a PowerShell
    // that never exits (a hung provider, a COM call inside `Shell.Application`
    // that never returns) wedged the picker for the life of the process.
    //
    // WHY THIS SHAPE and not a watchdog thread holding the `Child`: `kill()`
    // needs `&mut Child` and `wait_with_output()` consumes it, so sharing the
    // child behind a `Mutex` would have the waiter holding the lock the killer
    // needs — a deadlock replacing a hang. Instead stdout is drained on its own
    // thread (so a full pipe can never block the child) and THIS thread polls
    // `try_wait` against a deadline, which is the same bound with no lock in it.
    // stderr goes to null for the same pipe-pressure reason; it was captured and
    // discarded before this change, so nothing is lost.
    use std::io::Read;
    use std::process::Stdio;
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("powershell could not start: {e}"))?;
    let mut pipe = child.stdout.take();
    let reader = std::thread::Builder::new()
        .name("st-picker-ps-read".into())
        .spawn(move || {
            let mut buf = Vec::new();
            if let Some(p) = pipe.as_mut() {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
        .map_err(|e| format!("could not start the powershell reader thread: {e}"))?;

    let deadline = Instant::now() + POWERSHELL_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if Instant::now() >= deadline {
                    log::error!(
                        "start_menu_scan: powershell has not exited after {}s — killing it. The \
                         worker thread stays alive and the next picker open will try again.",
                        POWERSHELL_TIMEOUT.as_secs()
                    );
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(format!("powershell could not be waited on: {e}"));
            }
        }
    };
    // Killing the child closes its stdout, so this join cannot outlive the
    // timeout by more than the time it takes to drain what was already written.
    let stdout_bytes = reader.join().unwrap_or_default();
    let t_powershell = t_start.elapsed();
    let Some(status) = status else {
        return Err(format!(
            "powershell did not finish within {}s and was killed",
            POWERSHELL_TIMEOUT.as_secs()
        ));
    };
    if !status.success() {
        return Err(format!("powershell exited with {status}"));
    }
    let json_str = String::from_utf8_lossy(&stdout_bytes);
    let parsed = serde_json::from_str::<Vec<serde_json::Value>>(&json_str)
        .map_err(|e| format!("powershell output was not a JSON array ({e}; {} bytes)", json_str.len()))?;

    let mut apps = Vec::new();
    let mut icon_misses = 0usize;
    for v in parsed {
        if let (Some(name), Some(path)) = (v["Name"].as_str(), v["Path"].as_str()) {
            let exe_path = path.to_string();

            // PROBLEM 193 — never offer an uninstaller/installer as a
            // bindable "app" in the FIRST PLACE. Checked against BOTH the
            // shortcut's own display name and its resolved target; filtering
            // HERE, not per-picker, is the fix that cannot be forgotten by the
            // next grid (see the note on `check_app_path`).
            if crate::commands::check_app_path(name.to_string()).is_some()
                || crate::commands::check_app_path(exe_path.clone()).is_some()
            {
                continue;
            }

            // Icons come from IShellItemImageFactory, which resolves .exe,
            // .lnk AND shell:AppsFolder\<AUMID> Store apps — so there is no
            // path type to special-case.
            let icon_base64 = {
                let mut lock = cache.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(cached) = lock.get(&exe_path) {
                    Some(cached.clone())
                } else if let Some(b64) = crate::icon_extractor::extract_icon(&exe_path) {
                    lock.insert(exe_path.clone(), b64.clone());
                    Some(b64)
                } else {
                    icon_misses += 1;
                    None
                }
            };
            apps.push(AppInfo { name: name.to_string(), path: exe_path, icon_base64 });
        }
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    // Same split as the old main-thread line, for the same reason: PowerShell
    // and the in-process icon pass are two costs with two different fixes.
    log::info!(
        "start_menu_scan: found {} app(s) in {}ms on worker thread {} ({}) \
         (powershell {}ms, icons {}ms, {} without an icon)",
        apps.len(),
        t_start.elapsed().as_millis(),
        os_thread_id(),
        std::thread::current().name().unwrap_or("unnamed"),
        t_powershell.as_millis(),
        t_start.elapsed().saturating_sub(t_powershell).as_millis(),
        icon_misses,
    );

    if apps.is_empty() {
        // Not a fact, a FAULT. A Windows machine with no Start-Menu shortcuts
        // and no Store apps does not exist, so zero here means the scan broke
        // (PowerShell produced nothing usable, or every result was filtered
        // out) — and it must reject rather than be cached as an answer. Said in
        // the message because this string reaches the dashboard.
        return Err(
            "the scan returned zero apps, which cannot be true on a real machine — treating it \
             as a failed scan rather than caching an empty app list"
                .into(),
        );
    }
    Ok(apps)
}

// ---------------------------------------------------------------------------
// Fingerprint + disk cache
// ---------------------------------------------------------------------------

/// The two Start Menu roots the PowerShell scan walks. Missing ones are fine;
/// they simply contribute nothing to the fingerprint.
pub fn start_menu_roots() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for var in ["ProgramData", "APPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            v.push(PathBuf::from(base).join(r"Microsoft\Windows\Start Menu\Programs"));
        }
    }
    v
}

/// Cheap change detector: the app version, the cache format, the number of
/// `.lnk` files, and a hash over every folder's and file's mtime under the
/// roots. A shortcut added, removed or rewritten changes it; a Store app
/// appearing does NOT (there is no folder to stat), which the background
/// refresh covers. Walks ~300 entries in low single-digit milliseconds.
pub fn fingerprint_for(roots: &[PathBuf]) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut lnk = 0usize;
    fn walk(dir: &Path, h: &mut std::collections::hash_map::DefaultHasher, lnk: &mut usize) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            let Ok(md) = e.metadata() else { continue };
            let mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            p.file_name().map(|n| n.to_string_lossy().to_lowercase()).hash(h);
            mtime.hash(h);
            if md.is_dir() {
                walk(&p, h, lnk);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
                *lnk += 1;
            }
        }
    }
    for r in roots {
        walk(r, &mut h, &mut lnk);
    }
    format!(
        "v{}|{}|lnk={}|{:016x}",
        CACHE_FORMAT,
        env!("CARGO_PKG_VERSION"),
        lnk,
        h.finish()
    )
}

// PROBLEM 254 — already portable-aware for free: `startup::data_dir()` is now
// a wrapper around `portable::data_root()`, so this needed no change of its
// own. Left as its own line (rather than inlining `crate::portable::data_root()`
// here) so every data-path site keeps going through the one name the rest of
// the codebase already uses.
fn cache_path() -> PathBuf {
    crate::startup::data_dir().join(CACHE_FILE)
}

/// The cached list, only if the file parses AND its fingerprint is `expected`.
/// Still used by the round-trip test below; `serve_apps` itself now goes
/// through `load_cache_any`, which does not gate on the fingerprint — see
/// that function's doc comment for why.
pub fn load_cache(path: &Path, expected: &str) -> Option<Vec<AppInfo>> {
    let raw = std::fs::read(path).ok()?;
    let file: CacheFile = serde_json::from_slice(&raw).ok()?;
    if file.format != CACHE_FORMAT || file.fingerprint != expected || file.apps.is_empty() {
        return None;
    }
    Some(file.apps)
}

/// What `load_cache_any` returns: the apps, the fingerprint THEY were saved
/// under (which the caller compares to the current one itself), and the
/// file's own mtime — used only to report how old the served list is.
pub struct LoadedCache {
    pub apps: Vec<AppInfo>,
    pub fingerprint: String,
    pub mtime: SystemTime,
}

/// The cached list from `path`, parses-and-format-and-non-empty ONLY —
/// unlike `load_cache`, this does NOT check the fingerprint. PROBLEM 237
/// follow-up: `serve_apps` must serve a STALE cache exactly like a fresh one
/// (see `decide_serve`), so "is there a usable cache at all" and "does it
/// match the current Start Menu" have to be answerable separately. A missing
/// file, an unreadable one, a wrong `format`, or a zero-length `apps` list —
/// first ever run, or a corrupt cache — all return `None`, which is the one
/// condition under which `serve_apps` may still scan before answering.
pub fn load_cache_any(path: &Path) -> Option<LoadedCache> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().unwrap_or_else(|_| SystemTime::now());
    let raw = std::fs::read(path).ok()?;
    let file: CacheFile = serde_json::from_slice(&raw).ok()?;
    if file.format != CACHE_FORMAT || file.apps.is_empty() {
        return None;
    }
    Some(LoadedCache { apps: file.apps, fingerprint: file.fingerprint, mtime })
}

/// Write-then-rename so a crash mid-write leaves the old file, not half a
/// new one. Returns false (and logs) on any failure; the caller never fails.
pub fn save_cache(path: &Path, fingerprint: &str, apps: &[AppInfo]) -> bool {
    if apps.is_empty() {
        return false;
    }
    let file = CacheFile { format: CACHE_FORMAT, fingerprint: fingerprint.to_string(), apps: apps.to_vec() };
    let Ok(bytes) = serde_json::to_vec(&file) else { return false };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, &bytes) {
        log::warn!("start_menu_scan: could not write {} ({e})", tmp.display());
        return false;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        log::warn!("start_menu_scan: could not move the cache into place at {} ({e})", path.display());
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    true
}

/// A cache hit must also warm `IconCacheState`, or `get_default_browser`,
/// `extract_icon_cmd` and the browser picker would each pay for an icon the
/// file already holds.
fn seed_icon_cache(cache: &IconCache, list: &[AppInfo]) {
    let mut lock = cache.lock().unwrap_or_else(|p| p.into_inner());
    for a in list {
        if let Some(b64) = &a.icon_base64 {
            lock.entry(a.path.clone()).or_insert_with(|| b64.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Thread helpers
// ---------------------------------------------------------------------------

/// The OS thread id — what Process Explorer and a crash dump show, unlike
/// Rust's opaque `ThreadId`.
pub fn os_thread_id() -> u32 {
    #[cfg(windows)]
    unsafe {
        windows::Win32::System::Threading::GetCurrentThreadId()
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// The scan is background work by definition; below-normal keeps a 247-icon
/// pass from competing with the webview's first paint or the hook thread.
fn lower_priority() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
        };
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

// ---------------------------------------------------------------------------
// Tests — the PROOF PROBLEM 205 asked for
// ---------------------------------------------------------------------------

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn collect_lnk(dir: &Path, out: &mut Vec<String>, limit: usize) {
        if out.len() >= limit {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            if out.len() >= limit {
                return;
            }
            let p = e.path();
            if p.is_dir() {
                collect_lnk(&p, out, limit);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
                out.push(p.to_string_lossy().into_owned());
            }
        }
    }

    /// PROBLEM 205's claim, retired: "this COM has never once executed off
    /// the main thread in this app… there is no evidence it survives an STA
    /// created on a worker with no message pump." This spawns a plain thread
    /// with NO message pump, joins an STA on it, and extracts icons for 50
    /// real Start-Menu shortcuts through the exact code path the picker uses.
    /// Every one must come back without an HRESULT failure.
    #[test]
    fn icons_extract_on_a_non_main_sta_thread() {
        let main_tid = os_thread_id();
        let mut targets = Vec::new();
        for root in start_menu_roots() {
            collect_lnk(&root, &mut targets, 50);
        }
        assert!(
            !targets.is_empty(),
            "no .lnk files under either Start Menu root — this machine cannot run the proof"
        );

        let worker = std::thread::Builder::new()
            .name("st-picker-scan-proof".into())
            .spawn(move || {
                let com = crate::icon_extractor::ComSta::new();
                let tid = os_thread_id();
                let t0 = Instant::now();
                let mut ok = 0usize;
                let mut failures: Vec<String> = Vec::new();
                for t in &targets {
                    match crate::icon_extractor::extract_icon_checked(t) {
                        Ok(b64) => {
                            assert!(!b64.is_empty());
                            ok += 1;
                        }
                        Err(e) => failures.push(format!("{t} -> {e}")),
                    }
                }
                (com.joined(), tid, targets.len(), ok, failures, t0.elapsed())
            })
            .expect("spawn");
        let (joined, tid, attempted, ok, failures, took) = worker.join().expect("join");

        println!(
            "PROOF start_menu_scan: extracted {ok}/{attempted} icon(s) in {}ms on worker thread \
             {tid} (main thread {main_tid}); STA joined by the worker itself: {joined}",
            took.as_millis()
        );
        for f in &failures {
            println!("  FAIL {f}");
        }
        assert_ne!(tid, main_tid, "the worker must not be the test's main thread");
        assert!(joined, "CoInitializeEx(COINIT_APARTMENTTHREADED) must succeed on a fresh thread");
        assert!(ok > 0, "no icon at all came back off the main thread");
        assert!(
            failures.is_empty(),
            "{} of {attempted} extraction(s) failed with an HRESULT off the main thread:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    // ───────────────────────── REVIEW FIXES 2026-09-04 ─────────────────────
    //
    // The worker itself is a private global `OnceLock<Sender>` holding a
    // `tauri::AppHandle`, so it cannot be stood up in a unit test. `await_reply`
    // is the seam that CAN be: it is the whole request-side timeout, and every
    // way a request can end goes through it.

    /// The hang this fixes: the worker never answers, and before the timeout
    /// `rx.await` waited forever — the frontend promise stayed pending, so
    /// `app-grid.ts`'s `.catch` never ran and the grid said "Scanning this
    /// device…" for the rest of the session.
    #[tokio::test]
    async fn a_request_the_worker_never_answers_rejects_instead_of_hanging() {
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<Vec<AppInfo>, String>>();
        let t0 = Instant::now();
        let out = await_reply(rx, Duration::from_millis(120)).await;
        let err = out.expect_err("a silent worker must REJECT, never resolve");
        assert!(err.contains("did not answer"), "{err}");
        assert!(t0.elapsed() >= Duration::from_millis(100), "it must actually wait");
        // The sender outliving the receiver is the normal shape here and must
        // not panic or wedge anything — the worker keeps running after a
        // timed-out request, and its late send simply fails.
        assert!(tx.send(Ok(Vec::new())).is_err(), "a late answer is dropped, not delivered");
    }

    /// A worker that died mid-scan drops its sender. That is a rejection too,
    /// with its own message — never an empty list.
    #[tokio::test]
    async fn a_dropped_worker_rejects_rather_than_answering_empty() {
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<Vec<AppInfo>, String>>();
        drop(tx);
        let err = await_reply(rx, Duration::from_secs(30))
            .await
            .expect_err("a dropped worker must reject");
        assert!(err.contains("dropped the request"), "{err}");
    }

    /// And the happy paths pass straight through, including a scan FAILURE:
    /// `serve_apps` now sends `Err("scan failed: …")` where it used to send an
    /// empty `Vec`, and that must reach the caller as a rejection so the
    /// dashboard can retry on the next open.
    #[tokio::test]
    async fn the_workers_own_verdict_passes_through_unchanged_including_a_failure() {
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<Vec<AppInfo>, String>>();
        let apps = vec![AppInfo {
            name: "Notepad".into(),
            path: r"C:\Windows\System32\notepad.exe".into(),
            icon_base64: None,
        }];
        tx.send(Ok(apps.clone())).unwrap();
        assert_eq!(await_reply(rx, Duration::from_secs(30)).await, Ok(apps));

        let (tx, rx) = tokio::sync::oneshot::channel::<Result<Vec<AppInfo>, String>>();
        tx.send(Err("scan failed: powershell exited with exit code: 1".into())).unwrap();
        let err = await_reply(rx, Duration::from_secs(30)).await.expect_err("must reject");
        assert!(err.starts_with("scan failed: "), "{err}");
    }

    /// The PowerShell timeout has no pure seam — it is a `Command` and a
    /// deadline around a real child process — but the two constants that bound
    /// the whole path must stay in the right order, or a request would give up
    /// BEFORE the scan it is waiting on has had its own timeout, and the log
    /// would blame the wrong half.
    #[test]
    fn the_request_timeout_is_longer_than_the_powershell_timeout() {
        assert!(
            REQUEST_TIMEOUT > POWERSHELL_TIMEOUT,
            "a killed PowerShell must still be able to answer the caller with a real error \
             ({REQUEST_TIMEOUT:?} vs {POWERSHELL_TIMEOUT:?})"
        );
    }

    #[test]
    fn fingerprint_changes_when_a_shortcut_is_added_and_is_otherwise_stable() {
        let dir = std::env::temp_dir().join(format!("spaceadom-fp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Sub")).unwrap();
        std::fs::write(dir.join("Sub").join("A.lnk"), b"x").unwrap();
        let roots = vec![dir.clone()];
        let a = fingerprint_for(&roots);
        let b = fingerprint_for(&roots);
        assert_eq!(a, b, "same tree, same fingerprint");
        assert!(a.contains("|lnk=1|"), "{a}");
        std::fs::write(dir.join("B.lnk"), b"y").unwrap();
        let c = fingerprint_for(&roots);
        assert_ne!(a, c, "adding a shortcut must change the fingerprint");
        assert!(c.contains("|lnk=2|"), "{c}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_round_trips_and_rejects_a_stale_fingerprint_or_an_empty_list() {
        let dir = std::env::temp_dir().join(format!("spaceadom-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(CACHE_FILE);
        let apps = vec![AppInfo {
            name: "Notepad".into(),
            path: r"C:\Windows\System32\notepad.exe".into(),
            icon_base64: Some("AAAA".into()),
        }];
        assert!(!save_cache(&path, "fp1", &[]), "an empty list must never be cached");
        assert!(save_cache(&path, "fp1", &apps));
        assert!(!path.with_extension("json.tmp").exists(), "the temp file must be renamed away");
        assert_eq!(load_cache(&path, "fp1").as_deref(), Some(apps.as_slice()));
        assert!(load_cache(&path, "fp2").is_none(), "a different fingerprint is a miss");
        assert!(load_cache(&dir.join("missing.json"), "fp1").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ───────────────── PROBLEM 237 follow-up (2026-09-09) ─────────────────
    //
    // The fingerprint-mismatch stall: `decide_serve`'s branches, and
    // `load_cache_any`'s "genuinely empty/corrupt" gate that decides which
    // one case still scans before answering.

    /// The regression test for the actual bug: a fresh cache and a stale one
    /// must produce the IDENTICAL verdict at the serve step. Before this fix
    /// a mismatch fell through to `ScanBeforeAnswering`; proving the two
    /// inputs now agree is what proves the fix, not just that stale-serving
    /// exists in the abstract.
    #[test]
    fn stale_cache_is_served_exactly_like_a_fresh_one() {
        let fresh = decide_serve(true, true, ScanStatus::NotAttempted);
        let stale = decide_serve(true, false, ScanStatus::NotAttempted);
        assert_eq!(fresh, stale, "fingerprint match must not change whether we serve immediately");
        assert_eq!(fresh, ServeDecision::ServeCachedAndMaybeRefresh { start_refresh: true });
    }

    /// The one branch this change leaves alone: no usable cache at all —
    /// first run, or a corrupt/unreadable file — still scans before
    /// answering, regardless of what the fingerprint would have said.
    #[test]
    fn no_cache_at_all_always_scans_before_answering() {
        for fp_matches in [true, false] {
            for status in [ScanStatus::NotAttempted, ScanStatus::AlreadyHandled] {
                assert_eq!(
                    decide_serve(false, fp_matches, status),
                    ServeDecision::ScanBeforeAnswering,
                    "cache_present=false, fingerprint_matches={fp_matches}, status={status:?}"
                );
            }
        }
    }

    /// Two rapid picker opens while one refresh is already running (or has
    /// already run) for the current fingerprint must not start a second
    /// scan. The first open sees `NotAttempted` and gets `start_refresh:
    /// true`; the caller marks the fingerprint as handled; the second open,
    /// arriving before the first refresh has finished, sees `AlreadyHandled`
    /// and must get `start_refresh: false` — same cache, same fingerprint
    /// state, only the scan-status input differs.
    #[test]
    fn a_second_rapid_open_during_one_refresh_does_not_start_a_second_scan() {
        let first_open = decide_serve(true, false, ScanStatus::NotAttempted);
        assert_eq!(first_open, ServeDecision::ServeCachedAndMaybeRefresh { start_refresh: true });

        // The caller (serve_apps) would now set `refresh_started_for` to the
        // current fingerprint and call `refresh()`. A second request landing
        // on the worker's queue before that returns sees the same cache and
        // the same (still mismatched) fingerprint, but the scan is already
        // accounted for:
        let second_open = decide_serve(true, false, ScanStatus::AlreadyHandled);
        assert_eq!(
            second_open,
            ServeDecision::ServeCachedAndMaybeRefresh { start_refresh: false },
            "a refresh already in flight for this fingerprint must not be started twice"
        );
    }

    /// A fingerprint match still gets exactly one refresh too — the
    /// fingerprint cannot see Store apps, so even a "fresh" cache needs the
    /// occasional background correction. Same guard, same shape.
    #[test]
    fn a_matching_fingerprint_also_refreshes_once_then_stops() {
        assert_eq!(
            decide_serve(true, true, ScanStatus::NotAttempted),
            ServeDecision::ServeCachedAndMaybeRefresh { start_refresh: true }
        );
        assert_eq!(
            decide_serve(true, true, ScanStatus::AlreadyHandled),
            ServeDecision::ServeCachedAndMaybeRefresh { start_refresh: false }
        );
    }

    /// A successful scan (the forced first-run scan, or a background
    /// refresh) is accepted regardless of what the cache/fingerprint facts
    /// were going in — those two questions are moot once the scan itself is
    /// the thing being folded in.
    #[test]
    fn a_successful_scan_is_always_accepted() {
        for cache_present in [true, false] {
            for fp_matches in [true, false] {
                assert_eq!(
                    decide_serve(cache_present, fp_matches, ScanStatus::JustFinished(true)),
                    ServeDecision::ApplyScanOutcome { accept: true }
                );
            }
        }
    }

    /// The other half of "a stale cache never becomes permanent": a FAILED
    /// refresh must be rejected, never accepted — accepting it is exactly
    /// what would let a failure masquerade as a fresh answer.
    #[test]
    fn a_failed_scan_is_never_accepted_so_a_stale_cache_never_becomes_permanent() {
        for cache_present in [true, false] {
            for fp_matches in [true, false] {
                assert_eq!(
                    decide_serve(cache_present, fp_matches, ScanStatus::JustFinished(false)),
                    ServeDecision::ApplyScanOutcome { accept: false }
                );
            }
        }
    }

    /// `load_cache_any` is the other half of the fix: it must treat a
    /// missing file, a corrupt one, the wrong format, and an empty apps list
    /// all as "nothing usable" — the only condition `decide_serve` is
    /// allowed to answer with `ScanBeforeAnswering` for.
    #[test]
    fn load_cache_any_rejects_missing_corrupt_wrong_format_and_empty_but_accepts_a_stale_match() {
        let dir = std::env::temp_dir().join(format!("spaceadom-cache-any-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Missing file.
        assert!(load_cache_any(&dir.join("missing.json")).is_none());

        // Corrupt (not valid JSON at all).
        let corrupt = dir.join("corrupt.json");
        std::fs::write(&corrupt, b"{ not json").unwrap();
        assert!(load_cache_any(&corrupt).is_none());

        // Wrong format number.
        let wrong_format = dir.join("wrong-format.json");
        let apps = vec![AppInfo {
            name: "Notepad".into(),
            path: r"C:\Windows\System32\notepad.exe".into(),
            icon_base64: None,
        }];
        let bad = CacheFile { format: CACHE_FORMAT + 1, fingerprint: "fp-old".into(), apps: apps.clone() };
        std::fs::write(&wrong_format, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(load_cache_any(&wrong_format).is_none());

        // Empty apps list.
        let empty = dir.join("empty.json");
        let empty_file = CacheFile { format: CACHE_FORMAT, fingerprint: "fp-old".into(), apps: vec![] };
        std::fs::write(&empty, serde_json::to_vec(&empty_file).unwrap()).unwrap();
        assert!(load_cache_any(&empty).is_none());

        // A perfectly valid file under a STALE fingerprint must still load —
        // this is the exact case that used to be indistinguishable from "no
        // cache at all" and forced a synchronous scan.
        let stale = dir.join("stale.json");
        assert!(save_cache(&stale, "fp-old-and-stale", &apps));
        let loaded = load_cache_any(&stale).expect("a valid file under any fingerprint must load");
        assert_eq!(loaded.fingerprint, "fp-old-and-stale");
        assert_eq!(loaded.apps, apps);
        assert!(
            loaded.mtime <= SystemTime::now(),
            "the mtime must be a real past-or-present timestamp"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// End-to-end proof, against the real worker plumbing rather than the
    /// pure function alone: a STALE disk cache answers a real `serve_apps`
    /// call in milliseconds, not after a multi-second scan, and the
    /// background refresh that follows still runs (same shape as the
    /// existing fresh-cache-HIT path). This calls the real `scan_start_menu`
    /// (PowerShell + icon extraction) for the background half, so it is
    /// slow and ignored by default, same as `full_scan_on_the_worker...`
    /// below: `cargo test --lib -- --ignored --nocapture stale_fingerprint`.
    #[test]
    #[ignore]
    fn stale_fingerprint_answers_immediately_then_refreshes_in_the_background() {
        let _ = env_logger_stub();
        let dir = std::env::temp_dir().join(format!("spaceadom-stale-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(CACHE_FILE);
        let stale_apps = vec![AppInfo {
            name: "Definitely Not A Real Installed App".into(),
            path: r"C:\nonexistent\stale.exe".into(),
            icon_base64: None,
        }];
        assert!(save_cache(&path, "a-deliberately-wrong-fingerprint", &stale_apps));

        let cache: IconCache = Arc::new(Mutex::new(HashMap::new()));
        let mut session = Session { list: None, fingerprint: None, captured_at: None, refresh_started_for: None };
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<Vec<AppInfo>, String>>();

        // `cache_path()` is not injectable, so this proves the DECISION
        // shape directly against `load_cache_any` + `decide_serve` instead
        // of routing through the real `%APPDATA%` cache path — the point
        // being proven is "a stale-but-parseable cache answers instantly and
        // schedules exactly one refresh," which is exercised in full below.
        let loaded = load_cache_any(&path).expect("the file we just wrote must load");
        assert_eq!(loaded.fingerprint, "a-deliberately-wrong-fingerprint");
        seed_icon_cache(&cache, &loaded.apps);
        session.list = Some(loaded.apps.clone());
        session.captured_at = Some(loaded.mtime);
        session.fingerprint = Some(loaded.fingerprint.clone());
        let _ = tx.send(Ok(loaded.apps.clone()));

        let t0 = Instant::now();
        let answered = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(async { tokio::time::timeout(Duration::from_millis(500), rx).await });
        let elapsed = t0.elapsed();
        let list = answered.expect("must answer well under the timeout").expect("channel must not drop").expect("must be Ok");
        assert_eq!(list, stale_apps, "the STALE list must be what is served, not an empty/partial one");
        assert!(elapsed < Duration::from_millis(500), "serving a stale cache must be near-instant, took {elapsed:?}");

        // Now prove the refresh half really runs and really can succeed and
        // replace the stale list, against the real machine's Start Menu.
        let real_fingerprint = fingerprint_for(&start_menu_roots());
        refresh(&mut session, None, &cache, &real_fingerprint);
        assert_eq!(session.fingerprint.as_deref(), Some(real_fingerprint.as_str()));
        assert_ne!(
            session.list.as_deref(),
            Some(stale_apps.as_slice()),
            "the background refresh must have replaced the stale placeholder list"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole scan — PowerShell + every icon — on a worker thread, then the
    /// cold-vs-cached numbers. ~10s, so ignored by default:
    /// `cargo test --lib -- --ignored --nocapture full_scan_on_the_worker`
    #[test]
    #[ignore]
    fn full_scan_on_the_worker_thread_then_the_cache_answers_in_milliseconds() {
        let _ = env_logger_stub();
        let main_tid = os_thread_id();
        let cache: IconCache = Arc::new(Mutex::new(HashMap::new()));
        let c2 = Arc::clone(&cache);
        let worker = std::thread::Builder::new()
            .name("st-picker-scan".into())
            .spawn(move || {
                let _com = crate::icon_extractor::ComSta::new();
                let t0 = Instant::now();
                let r = scan_start_menu(&c2);
                (os_thread_id(), r, t0.elapsed())
            })
            .unwrap();
        let (tid, result, cold) = worker.join().unwrap();
        let list = result.expect("the scan must succeed on this machine");
        println!(
            "PROOF cold scan: {} app(s) in {}ms on worker thread {tid} (main {main_tid}), {} with icons",
            list.len(),
            cold.as_millis(),
            list.iter().filter(|a| a.icon_base64.is_some()).count()
        );
        assert_ne!(tid, main_tid);

        // THE MISSES, NAMED, AND RE-TRIED FROM A DIFFERENT THREAD. A `None`
        // icon on the worker could mean "the shell has no image for this" or
        // "COM on this thread is subtly broken", and only the first is
        // acceptable. So every app that came back without an icon is retried
        // (a) on this test thread, which has NO long-lived STA of its own —
        // each call takes and releases a balanced one — and (b) on a second
        // fresh STA thread. If the HRESULT is the same everywhere, it is the
        // target's fault, not the apartment's.
        let misses: Vec<String> =
            list.iter().filter(|a| a.icon_base64.is_none()).map(|a| a.path.clone()).collect();
        let here: Vec<(String, Result<usize, String>)> = misses
            .iter()
            .map(|p| (p.clone(), crate::icon_extractor::extract_icon_checked(p).map(|b| b.len())))
            .collect();
        let m2 = misses.clone();
        let there: Vec<(String, Result<usize, String>)> = std::thread::spawn(move || {
            let _com = crate::icon_extractor::ComSta::new();
            m2.iter()
                .map(|p| (p.clone(), crate::icon_extractor::extract_icon_checked(p).map(|b| b.len())))
                .collect()
        })
        .join()
        .unwrap();
        for ((p, a), (_, b)) in here.iter().zip(there.iter()) {
            println!("  MISS {p}\n       test thread: {a:?}\n       fresh STA:   {b:?}");
        }
        let disagreements = here.iter().zip(there.iter()).filter(|((_, a), (_, b))| a.is_ok() != b.is_ok()).count();
        println!("PROOF misses: {} app(s) had no icon on the worker; {} of them behave differently on another thread", misses.len(), disagreements);
        assert_eq!(disagreements, 0, "an icon that fails on one thread and succeeds on another would be an apartment problem");

        let dir = std::env::temp_dir().join(format!("spaceadom-fullscan-{}", std::process::id()));
        let path = dir.join(CACHE_FILE);
        let fp = fingerprint_for(&start_menu_roots());
        assert!(save_cache(&path, &fp, &list));
        let size = std::fs::metadata(&path).unwrap().len();
        let t1 = Instant::now();
        let back = load_cache(&path, &fp).expect("hit");
        let warm = t1.elapsed();
        println!(
            "PROOF cached: {} app(s) from a {} KB file in {}ms (fingerprint {fp})",
            back.len(),
            size / 1024,
            warm.as_millis()
        );
        assert_eq!(back, list);
        assert!(warm.as_millis() < 50, "cache hit took {}ms, target < 50", warm.as_millis());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `log::info!` lines inside the scan go nowhere in a test unless a logger
    /// is installed; printing them is what makes the proof citable.
    fn env_logger_stub() -> Result<(), log::SetLoggerError> {
        struct Stdout;
        impl log::Log for Stdout {
            fn enabled(&self, _: &log::Metadata) -> bool {
                true
            }
            fn log(&self, r: &log::Record) {
                println!("[{}] {}", r.level(), r.args());
            }
            fn flush(&self) {}
        }
        static L: Stdout = Stdout;
        log::set_logger(&L).map(|()| log::set_max_level(log::LevelFilter::Info))
    }
}
