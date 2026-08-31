/// display_watch.rs — rebuild the overlay when the display setup changes.
///
/// PROBLEM 117, measured live 2026-08-16.
///
/// SYMPTOM: after the app had been running 7h10m, holding Space produced sound
/// and launched applications but drew nothing. The Guide HUD and the toasts had
/// both stopped appearing. "Software overlay" was already on, and was verified
/// live: `--disable-gpu` WAS present on the WebView2 process. The overlay
/// reported itself perfectly healthy the whole time:
///
///     overlay_fit_hud: asked 1144x572 -> clamped 1144x572 @ (281,247);
///     monitor 1707x1067 at (0,0) scale 1.5; GOT size Ok((1144.0, 572.0))
///     pos Ok((281.0, 247.0)); visible Ok(true)
///
/// A screen capture of that exact rectangle, taken at the moment the app logged
/// the window as shown, contained ZERO HUD-coloured pixels against 666 in the
/// baseline taken seconds earlier. The window existed, was correctly placed and
/// claimed to be visible, and composed nothing.
///
/// ROOT CAUSE: in that same run the monitor the app saw had changed underneath
/// it — 117 log entries reported `1707x1067 @1.5` and 106 reported
/// `1920x1080 @1`, interleaved across the session. A transparent, layered,
/// always-on-top window whose composition was set up against one display
/// arrangement does not necessarily survive that arrangement changing. Nothing
/// in the app noticed, because every readback Rust can perform still answers
/// "fine".
///
/// Restarting the application restored both surfaces immediately (233 overlay
/// draws in the next 40 minutes). That is also the whole of the "self-healing"
/// reported since 2026-08-13: it never healed, it got restarted.
///
/// THE FIX: watch the display topology and rebuild the overlay window when it
/// changes, which is what a restart was doing by accident.
///
/// WHY POLLING AND NOT WM_DISPLAYCHANGE: the message is delivered to top-level
/// windows, so receiving it means subclassing a window Tauri and WebView2 both
/// own. That is version-fragile, and a mistake there breaks input for the whole
/// app. A directory-free integer comparison every couple of seconds costs
/// nothing measurable and cannot destabilise anything.
///
/// PORTABILITY: this must behave on any x64 Windows machine, not just the
/// developer's. It assumes no particular monitor count, resolution, scale
/// factor or GPU; it reads whatever Tauri reports and only reacts to CHANGE.
/// A machine whose display never changes simply never triggers it.
///
/// ---------------------------------------------------------------------------
/// PROBLEM 214 — TWO REBUILDS RACED, captured live 2026-08-28.
///
/// PROBLEM 117 fixed DETECTING the change. PROBLEM 118 fixed the REBUILD. What
/// neither considered is TWO REBUILDS OVERLAPPING, which is what plugging a
/// monitor in actually produces on this machine — one physical plug-in emitted
/// 1 display → 2 → 1 inside five seconds:
///
///     18:51:03.226 [WARN]  configuration CHANGED — was [1 display], now [2]
///     18:51:06.547 [WARN]  configuration CHANGED — was [2 displays], now [1]
///     18:51:08.216 [INFO]  overlay: configured (on-demand, click-through)
///     18:51:08.216 [INFO]  overlay rebuilt for the new display configuration
///     18:51:08.389 [ERROR] overlay REBUILD FAILED (a webview with label
///                          `overlay` already exists) — the HUD and toasts
///                          cannot appear until the next display change or a
///                          restart
///
/// The `REBUILDING` guard was there and did not hold, because
/// `AppHandle::run_on_main_thread` POSTS a closure to the event loop and
/// returns immediately — it does not wait for it. The old code therefore
/// queued the build closure, called `done()` (clearing `REBUILDING`) and
/// exited, all before the window it had asked for existed. The second display
/// change then walked straight through an unlocked door, found the label free
/// (the first rebuild's replacement had not been built yet), queued a SECOND
/// build behind the first, and lost the label race by 173 ms. Its failure path
/// set `OVERLAY_DISABLED`, which switched off an overlay that had just been
/// built correctly and was working. Shortcuts kept working (they are Rust);
/// the HUD and every sound died, because both live in that webview page and
/// every path into it is gated on that one flag.
///
/// FOUR separate defects, fixed separately:
///
///   1. SERIALISE. The guard now covers the WHOLE rebuild: every hop onto the
///      main thread is made with `on_main_thread_blocking`, which waits for the
///      closure to actually run. A request arriving mid-rebuild sets `PENDING`
///      and is run once, afterwards, by the same thread — queued, never
///      concurrent.
///   2. COALESCE. A single plug-in is a BURST. The watcher no longer rebuilds
///      on the first change it sees; it waits for the configuration to HOLD
///      STILL for `STABLE_POLLS` consecutive polls. His transitions were 3.3 s
///      apart, so a fixed short debounce would have fired between them —
///      stability is the correct test, not a timer.
///   3. "ALREADY EXISTS" IS SUCCESS. If the build fails and a webview labelled
///      `overlay` is nevertheless there, that IS the thing we were trying to
///      create. Adopt it: re-run `configure_overlay_window` on it (which clears
///      `OVERLAY_DISABLED`) and log a warning. Never disable an overlay because
///      it exists.
///   4. SELF-HEAL. Every poll, if the overlay is missing OR `OVERLAY_DISABLED`
///      is set, a rebuild is attempted on a backoff. The old error text
///      literally admitted the app was stuck "until the next display change or
///      a restart"; on a machine where the trigger fires daily that is not an
///      acceptable resting state. Nothing here ever requires a restart again.
///
/// GENERALISE: *a recovery path that can itself fail must be idempotent and
/// self-healing, or it becomes the new failure.* And: *a guard released by a
/// call that only REQUESTS work guards nothing* — the same `close()`-versus-
/// `destroy()` trap PROBLEM 118 recorded, one level up.
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::Manager;

/// One monitor's comparable fingerprint: x, y, width, height, scale-percent.
pub type Mon = (i32, i32, u32, u32, i64);

/// Set for the WHOLE duration of a rebuild — see PROBLEM 214. Before that fix
/// this was released the instant the build was *queued*, which is why two
/// rebuilds could overlap despite the guard existing.
static REBUILDING: AtomicBool = AtomicBool::new(false);

/// A rebuild was requested while one was already running. The running thread
/// picks this up and runs exactly one more pass — queue, never concurrency.
static PENDING: AtomicBool = AtomicBool::new(false);

/// Set by `heal_now()` when something noticed the overlay is unusable and
/// wants the healer to skip its backoff (the Guide HUD does this when a hold
/// finds `OVERLAY_DISABLED` set).
static HEAL_ASAP: AtomicBool = AtomicBool::new(false);

/// How often to compare. Display changes are human-scale events; two seconds is
/// far below any speed a person can perceive, and the check is a handful of
/// integer comparisons.
const POLL: Duration = Duration::from_secs(2);

/// How many CONSECUTIVE polls must report the same configuration before it is
/// treated as settled.
///
/// Measured from the owner's 2026-08-28 log: one physical plug-in produced
/// transitions 3.3 s apart. Three polls at 2 s means the configuration must
/// hold still for at least 4 s — comfortably past that 3.3 s gap, so the whole
/// storm collapses into ONE rebuild, while the worst-case latency from the last
/// transition to the rebuild stays under 8 s.
///
/// This replaces the old fixed 1.2 s `SETTLE` sleep, which was shorter than the
/// gap between his own transitions and therefore could not coalesce them.
const STABLE_POLLS: u32 = 3;

/// How long to wait for a closure we posted to the main thread to actually run.
/// Generous: building a webview on a cold or busy machine is not instant. If it
/// expires we log and carry on rather than hanging the watcher forever — and
/// the "already exists → adopt" branch makes a late-arriving duplicate build
/// harmless, which is the whole point of making this path idempotent.
const MAIN_THREAD_TIMEOUT: Duration = Duration::from_secs(20);

/// A comparable fingerprint of every monitor: position, size and scale.
/// Scale is quantised to whole percent because f64 has no useful equality.
fn topology(app: &tauri::AppHandle) -> Vec<Mon> {
    let Ok(monitors) = app.available_monitors() else {
        return Vec::new();
    };
    let mut v: Vec<Mon> = monitors
        .iter()
        .map(|m| {
            let p = m.position();
            let s = m.size();
            (p.x, p.y, s.width, s.height, (m.scale_factor() * 100.0).round() as i64)
        })
        .collect();
    // available_monitors() gives no ordering guarantee; sort so that the same
    // physical arrangement always produces the same fingerprint.
    v.sort_unstable();
    v
}

// ---------------------------------------------------------------------------
// PURE PART 1 — the debounce / coalescing decision (PROBLEM 214, defect 2)
// ---------------------------------------------------------------------------

/// What one poll means. Kept as data rather than side effects so the decision
/// can be tested without a display, a window or a running app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observation {
    /// Nothing to do: an empty (mid-transition) read, or the configuration is
    /// the one we already built against and has not moved.
    Ignored,
    /// A configuration we have not seen before. The stability clock restarts.
    Changed,
    /// Same as last poll, but not yet still for long enough. Carries the run
    /// length so the log can say how close it is.
    Settling(u32),
    /// Held still long enough — rebuild against this.
    Settled(Vec<Mon>),
}

/// Collapses a burst of display transitions into one rebuild.
///
/// The rule is STABILITY, not a timer: a configuration must be reported
/// identically `needed` polls in a row before it counts. A plug-in that walks
/// 1 display → 2 → 1 restarts the clock at every step and produces exactly one
/// rebuild at the end.
///
/// Note that a round trip back to the ORIGINAL configuration still rebuilds.
/// That is deliberate: PROBLEM 117 is about the overlay's composition being
/// established against an arrangement that then changed underneath it, and it
/// changed whether or not it changed back.
#[derive(Debug)]
pub struct Coalescer {
    built_against: Vec<Mon>,
    last_seen: Vec<Mon>,
    dirty: bool,
    agreed: u32,
    needed: u32,
}

impl Coalescer {
    pub fn new(initial: Vec<Mon>, needed: u32) -> Self {
        Self {
            built_against: initial.clone(),
            last_seen: initial,
            dirty: false,
            agreed: 0,
            needed: needed.max(1),
        }
    }

    /// The configuration the overlay was last built against.
    pub fn built_against(&self) -> &[Mon] {
        &self.built_against
    }

    /// The most recent non-empty reading.
    pub fn last_seen(&self) -> &[Mon] {
        &self.last_seen
    }

    pub fn observe(&mut self, now: Vec<Mon>) -> Observation {
        // Windows reports intermediate, sometimes empty states while a mode
        // change is in progress. An empty list must never be read as "all
        // monitors disappeared" — and it must not reset the stability run
        // either, or a flickering read could stall the rebuild forever.
        if now.is_empty() {
            return Observation::Ignored;
        }
        if now != self.last_seen {
            self.last_seen = now;
            self.dirty = true;
            self.agreed = 1;
            return Observation::Changed;
        }
        if !self.dirty {
            return Observation::Ignored;
        }
        self.agreed += 1;
        if self.agreed >= self.needed {
            self.dirty = false;
            self.agreed = 0;
            self.built_against = self.last_seen.clone();
            return Observation::Settled(self.built_against.clone());
        }
        Observation::Settling(self.agreed)
    }
}

// ---------------------------------------------------------------------------
// PURE PART 2 — the "already exists → adopt" branch (PROBLEM 214, defect 3)
// ---------------------------------------------------------------------------

/// The three possible ends of a rebuild's build step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildOutcome {
    /// A fresh window was created.
    Built,
    /// The builder refused because the label was taken — which means the thing
    /// we wanted EXISTS. Adopt and reconfigure it. This is a SUCCESS.
    Adopted,
    /// The build failed and no window with that label is there. Only this state
    /// justifies `OVERLAY_DISABLED`, because nothing could be shown either way.
    Failed,
}

/// Pure classification of the build step. Split out from the Tauri call so the
/// branch that cost the owner his HUD can be exercised by a unit test — the
/// exact lesson PROBLEM 118 wrote down and PROBLEM 214 had to learn again.
pub fn classify_build(built_ok: bool, existing_present: bool) -> BuildOutcome {
    if built_ok {
        BuildOutcome::Built
    } else if existing_present {
        BuildOutcome::Adopted
    } else {
        BuildOutcome::Failed
    }
}

// ---------------------------------------------------------------------------
// PURE PART 3 — the self-heal trigger (PROBLEM 214, defect 4)
// ---------------------------------------------------------------------------

/// Is the overlay in a state where the HUD and toasts cannot appear?
///
/// Either symptom is fatal on its own: a missing window has nothing to render
/// into, and `OVERLAY_DISABLED` makes every show path return early even when
/// the window is perfectly healthy (which is exactly what PROBLEM 214 did).
pub fn should_self_heal(overlay_disabled: bool, window_present: bool) -> bool {
    overlay_disabled || !window_present
}

/// Backoff between heal attempts, expressed in POLL ticks. First attempt is
/// immediate; then 2, 5, 15 and finally 30 ticks (4 s, 10 s, 30 s, 60 s at a
/// 2 s poll). Capped rather than giving up: a machine that is mid-mode-change,
/// or waiting on a WebView2 that is not serviceable yet, gets better on its own
/// and must be retried forever — never left needing a restart.
pub fn heal_backoff_polls(attempts: u32) -> u32 {
    match attempts {
        0 => 0,
        1 => 2,
        2 => 5,
        3 => 15,
        _ => 30,
    }
}

/// Drives `heal_backoff_polls` across successive polls. Pure and time-free:
/// one call per poll tick, so a test can run an hour of backoff instantly.
#[derive(Debug, Default)]
pub struct Healer {
    attempts: u32,
    waited: u32,
}

impl Healer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget the backoff — the next sick poll heals immediately. Called when
    /// a rebuild has just been ordered for another reason, and when something
    /// user-visible (a Space hold) has just discovered the overlay is dead.
    pub fn reset(&mut self) {
        self.attempts = 0;
        self.waited = 0;
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// One poll tick. Returns true when a heal should be attempted NOW.
    pub fn poll(&mut self, needs_heal: bool) -> bool {
        if !needs_heal {
            self.reset();
            return false;
        }
        if self.waited < heal_backoff_polls(self.attempts) {
            self.waited += 1;
            return false;
        }
        self.waited = 0;
        self.attempts = self.attempts.saturating_add(1);
        true
    }
}

/// Ask the watcher to drop its heal backoff and try again on the next poll.
///
/// Called from the Guide HUD when a Space hold finds the overlay switched off:
/// the user has just told us, by doing the thing, that they need it now.
pub fn heal_now() {
    HEAL_ASAP.store(true, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// The rebuild itself
// ---------------------------------------------------------------------------

/// Run `f` on the main thread AND WAIT FOR IT TO FINISH.
///
/// `AppHandle::run_on_main_thread` posts to the event loop and returns
/// immediately. The old rebuild treated it as if it had run, released the
/// `REBUILDING` guard and returned — which is PROBLEM 214's root cause. Every
/// step of a rebuild that must be finished before the next one starts goes
/// through here.
///
/// MUST NOT be called from the main thread: it would deadlock waiting for a
/// queue that only the caller can drain. The only caller is the
/// `st-overlay-rebuild` worker thread.
fn on_main_thread_blocking<F, T>(app: &tauri::AppHandle, what: &str, f: F) -> Option<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    if let Err(e) = app.run_on_main_thread(move || {
        let _ = tx.send(f());
    }) {
        log::error!("display: could not reach the main thread to {what} ({e})");
        return None;
    }
    match rx.recv_timeout(MAIN_THREAD_TIMEOUT) {
        Ok(v) => Some(v),
        Err(_) => {
            log::error!(
                "display: the main thread did not {what} within {}s — carrying on without \
                 waiting. The closure may still run later; the rebuild is idempotent \
                 (an existing overlay is adopted, never treated as a failure) so a late \
                 arrival is harmless (PROBLEM 214).",
                MAIN_THREAD_TIMEOUT.as_secs()
            );
            None
        }
    }
}

/// Destroy the overlay window and build a replacement with identical properties.
///
/// THREE BUGS HAVE LIVED HERE. Written down because all three are easy to write
/// again:
///
/// 1. (1.0.33) `close()` is a REQUEST, not an action. It returns immediately and
///    the window goes away some time later, so building the replacement in the
///    same breath failed with `a webview with label 'overlay' already exists`.
///    `destroy()` is the immediate form, and the label is polled until it is
///    genuinely free before rebuilding.
///
/// 2. (1.0.33) Setting `OVERLAY_DISABLED` when the rebuild failed made the app
///    STRICTLY WORSE than having no fix at all. The flag is now set ONLY when
///    the window is genuinely gone AND could not be replaced.
///
/// 3. (1.0.89, PROBLEM 214) The `REBUILDING` guard was released before the work
///    it guarded had happened, because `run_on_main_thread` only queues. Two
///    rebuilds overlapped, the loser hit "already exists", and its failure path
///    disabled an overlay the winner had just built correctly. The guard now
///    spans the whole rebuild, and "already exists" is adopted, not failed.
///
/// The waiting is done OFF the main thread. Blocking the main thread would
/// freeze the dashboard and the tray for the duration.
pub fn rebuild_overlay(app: &tauri::AppHandle) {
    if REBUILDING.swap(true, Ordering::SeqCst) {
        // PROBLEM 214, defect 1. A second display change arriving mid-rebuild
        // must QUEUE. Running it now is the bug.
        PENDING.store(true, Ordering::SeqCst);
        log::info!(
            "display: a rebuild is already in flight — this request is queued behind it \
             rather than run concurrently (PROBLEM 214)"
        );
        return;
    }
    // PROBLEM 131 — the leading hypothesis for the 14 crashes is a window
    // message arriving after its host window was destroyed. This is the ONE
    // place the app deliberately destroys a live window, so recording it makes
    // the hypothesis testable from a crash log alone: if a crash report shows a
    // rebuild moments earlier, that is the answer; if the counter is 0 in every
    // report, the hypothesis is dead and should be written off in the notes.
    crate::crash_context::note_overlay_rebuild();
    crate::crash_context::note_display_event("overlay rebuild started (display topology changed)");
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("st-overlay-rebuild".into())
        .spawn(move || {
            loop {
                rebuild_once(&app);
                if !PENDING.swap(false, Ordering::SeqCst) {
                    break;
                }
                log::info!(
                    "display: a display change arrived while the last rebuild was running — \
                     running the queued rebuild now (PROBLEM 214)"
                );
            }
            REBUILDING.store(false, Ordering::SeqCst);
            // Tiny window between the swap above and this store: if a request
            // landed in it, honour it with a fresh call rather than losing it.
            if PENDING.swap(false, Ordering::SeqCst) {
                rebuild_overlay(&app);
            }
        })
        .is_ok();

    if !spawned {
        log::error!("display: could not spawn the rebuild thread");
        PENDING.store(false, Ordering::SeqCst);
        REBUILDING.store(false, Ordering::SeqCst);
    }
}

/// One rebuild pass. Always called with `REBUILDING` held.
fn rebuild_once(app: &tauri::AppHandle) {
    // ---- 1. tear the old one down, on the main thread, and WAIT ----
    let a = app.clone();
    if on_main_thread_blocking(app, "destroy the old overlay", move || {
        crate::guide_hud::hide_guide_hud();
        if let Some(old) = a.get_webview_window("overlay") {
            let _ = old.hide();
            if let Err(e) = old.destroy() {
                log::error!("display: could not destroy the old overlay ({e})");
            }
        }
    })
    .is_none()
    {
        return;
    }

    // ---- 2. wait for the label to actually free up ----
    let mut gone = false;
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(100));
        if app.get_webview_window("overlay").is_none() {
            gone = true;
            break;
        }
    }
    if !gone {
        // The old window outlived its own destroy request. It is still there,
        // so it is still usable — leave it alone and say so. Do NOT disable
        // the overlay: that was PROBLEM 118's bug 2.
        log::error!(
            "display: the old overlay did not go away within 4s — keeping it rather \
             than switching the HUD off. It may be bound to the previous display."
        );
        return;
    }

    // ---- 3. build the replacement, on the main thread, and WAIT ----
    // Mirrors tauri.conf.json field-for-field, exactly as the PROBLEM 81
    // rebuild path does. A replacement missing any of these is an opaque,
    // decorated, focus-stealing rectangle.
    let a2 = app.clone();
    let outcome = on_main_thread_blocking(app, "build the overlay", move || {
        let built = tauri::WebviewWindowBuilder::new(
            &a2,
            "overlay",
            tauri::WebviewUrl::App("overlay.html".into()),
        )
        .visible(false)
        .title("Spaceadom Overlay")
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .shadow(false)
        .inner_size(600.0, 460.0)
        .build();

        let err = built.as_ref().err().map(|e| e.to_string());
        // PROBLEM 214, defect 3 — "already exists" means the thing we wanted
        // IS THERE. Ask, rather than assuming the build error is fatal.
        let existing = if built.is_err() { a2.get_webview_window("overlay") } else { None };
        let outcome = classify_build(built.is_ok(), existing.is_some());

        match (outcome, built, existing) {
            (BuildOutcome::Built, Ok(w), _) => {
                crate::configure_overlay_window(&w);
                log::info!("display: overlay rebuilt for the new display configuration");
            }
            (BuildOutcome::Adopted, _, Some(w)) => {
                // Do NOT disable. Reconfiguring is what makes this idempotent:
                // configure_overlay_window re-applies click-through and clears
                // OVERLAY_DISABLED on success.
                crate::configure_overlay_window(&w);
                log::warn!(
                    "display: the overlay window already existed ({}) — ADOPTED and \
                     reconfigured it instead of failing. This is the state that used to \
                     switch the HUD and every sound off until a restart (PROBLEM 214).",
                    err.as_deref().unwrap_or("no error text")
                );
            }
            _ => {
                // Genuinely gone and not replaceable. The flag is honest here:
                // nothing could be shown either way. The watcher's self-heal
                // retries on a backoff and success clears it — no restart.
                // PROBLEM 217 — the target takes this line off the automatic
                // log bridge and hands it to `report_degraded` instead, which
                // rate-limits it. Severity and wording in debug.log unchanged.
                // It matters here specifically: the watcher RETRIES on a
                // backoff, so a permanently broken rebuild used to submit one
                // event per retry, forever.
                log::error!(
                    target: crate::telemetry::DEGRADED_TARGET,
                    "display: overlay REBUILD FAILED ({}) and no overlay window exists — \
                     the HUD and toasts cannot appear until this heals. The display \
                     watcher will keep retrying on a backoff; no restart is required \
                     (PROBLEM 214).",
                    err.as_deref().unwrap_or("no error text")
                );
                crate::guide_hud::OVERLAY_DISABLED.store(true, Ordering::Relaxed);
                crate::telemetry::report_degraded(
                    crate::telemetry::Degraded::OverlayRebuildFailed,
                    &format!(
                        "overlay REBUILD FAILED ({}) and no overlay window exists — the HUD \
                         and toasts cannot appear until the self-heal succeeds",
                        err.as_deref().unwrap_or("no error text")
                    ),
                );
            }
        }
        outcome
    });

    if outcome.is_none() {
        log::error!(
            "display: the overlay build never reported back — the self-heal poll will \
             re-check and rebuild if it is genuinely missing (PROBLEM 214)"
        );
    }

    // ---- 4. re-arm the WM_ENDSESSION guard on the NEW overlay HWND ----
    //
    // PROBLEM 224. A rebuilt overlay is a brand-new window: step 1 destroyed
    // the old HWND and every subclass on it went with it. Without this the
    // guard silently degrades one window at a time — and this app rebuilds the
    // overlay on every display change, which for this owner is a routine daily
    // event (CLAUDE.md: he plugs a second display in and out through the day).
    //
    // ON THE MAIN THREAD, and that is the whole reason this is a hop rather
    // than a bare call: `rebuild_once` runs on the `st-display-watch` thread,
    // and `session_end::install` uses `EnumThreadWindows(GetCurrentThreadId())`
    // — from here that enumerates the WATCHER thread's windows, of which there
    // are none. It would install nothing, return quietly, and the only symptom
    // would be a shutdown crash weeks later. (`install` logs an ERROR on a
    // zero count for the same reason: a check that cannot produce a negative
    // is not a check.)
    //
    // Deliberately AFTER the `outcome.is_none()` branch: whether the rebuild
    // built, adopted or failed, whatever overlay window exists now is the one
    // that needs guarding. PROBLEM 214's serialisation is untouched — this runs
    // inside the same `REBUILDING` hold, adds no new window operation, and
    // cannot fail the rebuild.
    if on_main_thread_blocking(app, "re-arm the WM_ENDSESSION guard", || {
        crate::session_end::install()
    })
    .is_none()
    {
        log::error!(
            "display: the WM_ENDSESSION guard could not be re-armed after the overlay \
             rebuild — the app still works, but a shutdown or an .msi upgrade may hit \
             PROBLEM 224's tao panic until the next rebuild or restart re-arms it"
        );
    }
}

/// Start the watcher. Safe to call once, from setup.
pub fn start(app: tauri::AppHandle) {
    if std::thread::Builder::new()
        .name("st-display-watch".into())
        .spawn(move || {
            let mut coalescer = Coalescer::new(topology(&app), STABLE_POLLS);
            let mut healer = Healer::new();
            log::info!(
                "display: watching {} monitor(s) for configuration changes — a change must \
                 hold still for {} consecutive {}s polls before the overlay is rebuilt, so \
                 one plug-in produces ONE rebuild (PROBLEM 214)",
                coalescer.built_against().len(),
                STABLE_POLLS,
                POLL.as_secs()
            );
            loop {
                std::thread::sleep(POLL);

                match coalescer.observe(topology(&app)) {
                    Observation::Ignored => {}
                    Observation::Changed => {
                        log::warn!(
                            "display: configuration CHANGED — was {:?}, now {:?}. Waiting for \
                             it to settle before rebuilding; a single plug-in emits several \
                             transitions seconds apart and two overlapping rebuilds are what \
                             broke the HUD (PROBLEM 117/118/214).",
                            coalescer.built_against(),
                            coalescer.last_seen()
                        );
                    }
                    Observation::Settling(n) => {
                        log::info!(
                            "display: configuration steady for {n}/{STABLE_POLLS} polls — \
                             still waiting for it to settle"
                        );
                    }
                    Observation::Settled(cfg) => {
                        log::warn!(
                            "display: configuration settled at {cfg:?} — rebuilding the overlay \
                             ONCE. Without this the HUD and toasts stop painting while still \
                             reporting themselves visible (PROBLEM 117)."
                        );

                        // The dashboard has the same problem in a different
                        // shape: if it is open on a display that has just been
                        // unplugged, it is now sitting at coordinates no
                        // monitor covers, and the only way back is to close and
                        // reopen it from the tray. `ensure_on_screen`
                        // (PROBLEM 83) already knows how to fix that; it simply
                        // was never called while a window was open, because
                        // until now nothing told the app the displays moved.
                        let a = app.clone();
                        let _ = app.run_on_main_thread(move || {
                            if let Some(win) = a.get_webview_window("settings") {
                                if win.is_visible().unwrap_or(false) {
                                    crate::ensure_on_screen(&win);
                                }
                            }
                        });

                        rebuild_overlay(&app);
                        healer.reset();
                        continue;
                    }
                }

                // ---- self-heal (PROBLEM 214, defect 4) ----
                //
                // The overlay can end up unusable without any display change:
                // a rebuild that genuinely failed, a click-through call that
                // failed at startup, a WebView2 that was not serviceable yet at
                // a cold boot. Every one of those used to persist until the
                // user restarted the app. None of them do now.
                if REBUILDING.load(Ordering::SeqCst) {
                    continue;
                }
                if HEAL_ASAP.swap(false, Ordering::Relaxed) {
                    healer.reset();
                }
                let disabled = crate::guide_hud::OVERLAY_DISABLED.load(Ordering::Relaxed);
                let present = app.get_webview_window("overlay").is_some();
                if healer.poll(should_self_heal(disabled, present)) {
                    log::warn!(
                        "display: the overlay is unusable (disabled={disabled}, \
                         window_present={present}) — self-healing, attempt {}. The user must \
                         never have to restart the app for this (PROBLEM 214).",
                        healer.attempts()
                    );
                    rebuild_overlay(&app);
                }
            }
        })
        .is_err()
    {
        log::error!(
            "display: could not spawn the watcher thread — the overlay will still stop \
             painting after a display change until the app is restarted"
        );
    }
}

// ---------------------------------------------------------------------------
// Tests — the pure parts only. Everything here is a branch a user reaches ONLY
// after something has already gone wrong, which is exactly the kind PROBLEM 118
// says must not ship unexercised.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn one() -> Vec<Mon> {
        vec![(0, 0, 2560, 1600, 150)]
    }
    fn two() -> Vec<Mon> {
        vec![(0, 0, 1920, 1080, 100), (1920, 0, 2560, 1600, 150)]
    }

    // ---- debounce / coalescing ----

    #[test]
    fn steady_configuration_never_rebuilds() {
        let mut c = Coalescer::new(one(), 3);
        for _ in 0..20 {
            assert_eq!(c.observe(one()), Observation::Ignored);
        }
    }

    #[test]
    fn an_empty_read_is_ignored_and_does_not_reset_the_run() {
        // Windows reports empty lists mid-mode-change. Treating one as "every
        // monitor vanished" would rebuild forever; treating it as a change
        // would stall the run.
        let mut c = Coalescer::new(one(), 3);
        assert_eq!(c.observe(two()), Observation::Changed);
        assert_eq!(c.observe(Vec::new()), Observation::Ignored);
        assert_eq!(c.observe(two()), Observation::Settling(2));
        assert_eq!(c.observe(Vec::new()), Observation::Ignored);
        assert_eq!(c.observe(two()), Observation::Settled(two()));
    }

    #[test]
    fn a_change_needs_the_full_run_before_it_rebuilds() {
        let mut c = Coalescer::new(one(), 3);
        assert_eq!(c.observe(two()), Observation::Changed);
        assert_eq!(c.observe(two()), Observation::Settling(2));
        assert_eq!(c.observe(two()), Observation::Settled(two()));
        // and it does not fire again for the same configuration
        assert_eq!(c.observe(two()), Observation::Ignored);
    }

    #[test]
    fn the_owners_plug_in_storm_produces_exactly_one_rebuild() {
        // 2026-08-28, %APPDATA%\Spaceadom\debug.log: 1 display -> 2 -> 1 within
        // five seconds. The old code rebuilt twice and the second one killed
        // the HUD. Polling at 2s, the sequence a watcher would see is:
        let mut c = Coalescer::new(one(), 3);
        let feed = [one(), two(), two(), one(), one(), one(), one(), one()];
        let mut rebuilds = 0;
        for cfg in feed {
            if let Observation::Settled(_) = c.observe(cfg) {
                rebuilds += 1;
            }
        }
        assert_eq!(rebuilds, 1, "a single plug-in must coalesce into ONE rebuild");
    }

    #[test]
    fn a_round_trip_back_to_the_original_still_rebuilds() {
        // PROBLEM 117 is about composition established against an arrangement
        // that CHANGED. It changed whether or not it changed back.
        let mut c = Coalescer::new(one(), 2);
        assert_eq!(c.observe(two()), Observation::Changed);
        assert_eq!(c.observe(one()), Observation::Changed);
        assert_eq!(c.observe(one()), Observation::Settled(one()));
    }

    #[test]
    fn monitor_order_does_not_create_a_phantom_change() {
        // available_monitors() promises no ordering; topology() sorts. Assert
        // the invariant the sort exists to provide.
        let mut a = two();
        let mut b = two();
        b.reverse();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b);
    }

    // ---- "already exists" -> adopt ----

    #[test]
    fn a_successful_build_is_built() {
        assert_eq!(classify_build(true, false), BuildOutcome::Built);
    }

    #[test]
    fn already_exists_is_adopted_not_failed() {
        // THE PROBLEM 214 BRANCH. The build failed with "a webview with label
        // `overlay` already exists" and the window was right there. Treating
        // that as failure is what switched the HUD and every sound off.
        assert_eq!(classify_build(false, true), BuildOutcome::Adopted);
        assert_ne!(classify_build(false, true), BuildOutcome::Failed);
    }

    #[test]
    fn a_build_failure_with_no_window_is_the_only_real_failure() {
        assert_eq!(classify_build(false, false), BuildOutcome::Failed);
    }

    // ---- self-heal trigger ----

    #[test]
    fn a_healthy_overlay_does_not_heal() {
        assert!(!should_self_heal(false, true));
    }

    #[test]
    fn either_symptom_alone_triggers_a_heal() {
        // PROBLEM 214's exact end state: the window EXISTS and is fine, and
        // the flag alone makes the HUD unreachable.
        assert!(should_self_heal(true, true));
        // and the plain missing-window case
        assert!(should_self_heal(false, false));
        assert!(should_self_heal(true, false));
    }

    #[test]
    fn the_first_heal_is_immediate_then_backs_off() {
        let mut h = Healer::new();
        assert!(h.poll(true), "the first sick poll must heal at once");
        assert_eq!(h.attempts(), 1);
        // 2 quiet polls before attempt 2
        assert!(!h.poll(true));
        assert!(!h.poll(true));
        assert!(h.poll(true));
        assert_eq!(h.attempts(), 2);
    }

    #[test]
    fn healing_stops_the_moment_the_overlay_is_well_again() {
        let mut h = Healer::new();
        assert!(h.poll(true));
        assert!(!h.poll(false));
        assert_eq!(h.attempts(), 0, "recovery must clear the backoff");
        // and a later relapse heals immediately rather than resuming the backoff
        assert!(h.poll(true));
    }

    #[test]
    fn the_backoff_is_capped_and_never_gives_up() {
        // A machine mid-mode-change, or one whose WebView2 is not serviceable
        // yet, gets better on its own. Giving up would recreate the exact
        // "until a restart" state PROBLEM 214 exists to remove.
        for attempts in 0..1000u32 {
            assert!(heal_backoff_polls(attempts) <= 30);
        }
        let mut h = Healer::new();
        let mut heals = 0;
        for _ in 0..1000 {
            if h.poll(true) {
                heals += 1;
            }
        }
        assert!(heals > 20, "it must keep retrying forever, got {heals}");
    }
}
