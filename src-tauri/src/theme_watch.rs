//! theme_watch.rs — REVIEW FIXES 2026-09-05 (H4). **The one place that
//! answers "does Windows want dark app surfaces?" for the whole app.**
//!
//! ---------------------------------------------------------------------------
//! THE BUG THIS EXISTS FOR
//! ---------------------------------------------------------------------------
//! `tauri.conf.json` pinned the `settings` window to `"theme": "Light"`. Tauri
//! forwards that to WebView2 as `SetPreferredColorScheme(Light)`, and
//! `prefers-color-scheme` inside a webview reports **the scheme the webview was
//! told to prefer**, not the scheme the user chose. So on a dark machine:
//!
//!   - the DASHBOARD's `matchMedia("(prefers-color-scheme: dark)")` answered
//!     `false`, permanently, by configuration;
//!   - the OVERLAY, which had no such pin, answered `true`;
//!   - the theme `"auto"` therefore rendered Earthy in one window and Starry
//!     night in the other, at the same moment, on one screen — exactly the
//!     failure CLAUDE.md's "ONE setting drives everything" rule exists to
//!     prevent;
//!   - and `settings-panel.ts`'s theme-pill handler resolved `"auto"` through
//!     the dashboard's wrong answer and then PERSISTED `dark_mode: false` from
//!     it, writing the wrong half into `config.json` for Rust and the overlay
//!     to believe.
//!
//! The rule was already shared (`src/theme-resolve.ts`). What was not shared
//! was the INPUT. This module makes the input single-sourced: Rust reads the
//! registry — which no window configuration can lie to — and both webviews
//! consume the identical bool.
//!
//! ---------------------------------------------------------------------------
//! HOW THE CHANGE IS DETECTED, AND WHY IT IS A POLL
//! ---------------------------------------------------------------------------
//! Three mechanisms were considered. The one chosen is a 2-second poll of the
//! same registry value [`crate::config::os_prefers_dark`] already reads.
//!
//! **REJECTED — a message-only window receiving `WM_SETTINGCHANGE`
//! ("ImmersiveColorSet").** This is the mechanism most references reach for
//! first and it does not work in the shape it is usually written. The theme
//! change is announced by broadcasting `WM_SETTINGCHANGE`, and a broadcast
//! reaches **top-level** windows only — a message-only window (`HWND_MESSAGE`)
//! is documented as not receiving broadcast messages at all. A watcher built
//! that way compiles, runs, logs "watching", and silently never fires: a check
//! that cannot produce a positive result, which is the same class of trap
//! CLAUDE.md records for ASCII markers and for the `%LOCALAPPDATA%` sandbox
//! test. Making it work means a real top-level window with its own class and
//! its own message pump, which is the version-fragile, input-adjacent thing
//! `display_watch.rs` already refused to do for `WM_DISPLAYCHANGE` — in an app
//! whose whole value depends on a `WH_KEYBOARD_LL` hook staying healthy, a new
//! window procedure on a new thread is not a cheap addition.
//!
//! **REJECTED — `RegNotifyChangeKeyValue` on the Personalize key.** Correct,
//! event-driven, and genuinely the tidy answer on paper. It is not taken here
//! for one concrete reason: **it cannot be verified from this machine's agent
//! shell.** That shell runs inside an MSIX container which virtualises HKCU
//! (CLAUDE.md, PROBLEM 143), so a registry write made to test the notification
//! lands in the container's private hive and the notification under test may
//! or may not be the thing that answered. Shipping a Win32 primitive whose
//! only proof would be "it compiled" is how this project has been burned
//! before. The poll below reaches the same registry value through the same
//! already-tested function, and its correctness question reduces to a `!=`
//! comparison that IS unit-tested ([`Watcher`] below).
//!
//! **CHOSEN — poll every 2s.** It is this repository's own established
//! precedent, for a near-identical problem and with the reasoning already
//! written down: `display_watch.rs` rejected `WM_DISPLAYCHANGE` in favour of
//! polling because "a directory-free integer comparison every couple of
//! seconds costs nothing measurable and cannot destabilise anything". The cost
//! here is one `RegOpenKeyEx` + one `RegQueryValueEx` per 2 s on a dedicated
//! thread that does nothing else. The worst-case latency a user can observe is
//! 2 s between flipping Windows' "Choose your mode" and the app following —
//! against a mechanism that, in this codebase, would be unprovable.
//!
//! ---------------------------------------------------------------------------
//! WHAT IT EMITS
//! ---------------------------------------------------------------------------
//! `os-theme-changed` with `{ "dark": bool }`, via the global `emit` — never
//! `emit_to`, which has never delivered in this app (CLAUDE.md, window rules).
//! Both windows listen; `src/os-theme.ts` is the single consumer in each.
//!
//! Paired with `#[tauri::command] get_os_prefers_dark`, because an event that
//! fires only on CHANGE leaves a freshly-created window — a first launch, or
//! the overlay `display_watch.rs` rebuilds when the monitors move — wearing
//! whatever its module defaulted to. Both halves, every time; the same rule
//! the theme bool, the band count and the ring layout already follow.

use tauri::{AppHandle, Emitter};

/// How often the registry value is re-read. See the module header for why this
/// is a poll at all. Two seconds is `display_watch.rs`'s own interval, chosen
/// for the same reason: fast enough that nobody notices the lag, slow enough
/// that the cost is unmeasurable.
const POLL: std::time::Duration = std::time::Duration::from_secs(2);

/// The event every window listens for. One name, stated once, so the Rust
/// emitter and `src/os-theme.ts` cannot drift apart silently.
pub(crate) const EVENT: &str = "os-theme-changed";

/// The payload. A struct rather than a bare bool so the field has a NAME on
/// the JavaScript side — `e.payload.dark` reads as an assertion, `e.payload`
/// reads as a mystery, and a later addition (say, the accent colour) would
/// otherwise be a breaking change to every listener.
#[derive(Clone, serde::Serialize)]
struct OsThemePayload {
    dark: bool,
}

/// **Does Windows want dark app surfaces?** — the frontend's seed.
///
/// One bool, collapsing [`crate::config::os_prefers_dark`]'s `Option` through
/// the SAME fallback [`crate::config::resolve_theme`] applies to `None`:
/// unanswerable is daylight. That collapse belongs here rather than in the
/// frontend, so there is exactly one place in the app that decides what an
/// unreadable registry value means — the alternative is a `null` crossing the
/// IPC boundary and two webviews each inventing their own answer to it, which
/// is a smaller copy of the bug this whole module exists to remove.
#[tauri::command]
pub fn get_os_prefers_dark() -> bool {
    crate::config::os_prefers_dark().unwrap_or(false)
}

/// The pure half: remember the last answer, and say when it MOVED.
///
/// Split out from the thread so the only logic in here can be tested. The
/// thread around it is a `sleep` and an `emit`; this is the part that can be
/// wrong, and the two ways it can be wrong are both regressions somebody would
/// otherwise ship: re-emitting an unchanged value (which would run the
/// dashboard's 450ms cross-fade every 2 seconds forever), and treating an
/// unreadable registry value as a CHANGE to light (which would drag a Starry
/// night user into daylight the moment a transient read failed).
pub(crate) struct Watcher {
    dark: bool,
}

impl Watcher {
    /// Seeded with the value at startup, so the first poll can only report a
    /// genuine change. The windows get this same value from
    /// [`get_os_prefers_dark`] as they boot.
    pub(crate) fn new(initial: Option<bool>) -> Self {
        Self {
            dark: initial.unwrap_or(false),
        }
    }

    /// Feed it a fresh reading. `Some(dark)` when the app must be told;
    /// `None` when nothing changed.
    ///
    /// An unreadable value (`None`) is folded through the SAME
    /// `unwrap_or(false)` as everywhere else rather than being ignored: a key
    /// that has genuinely been deleted means daylight by the app's own
    /// documented rule, and a rule with an exception in the watcher and no
    /// exception in the resolver is two rules.
    pub(crate) fn observe(&mut self, now: Option<bool>) -> Option<bool> {
        let now = now.unwrap_or(false);
        if now == self.dark {
            return None;
        }
        self.dark = now;
        Some(now)
    }
}

/// Start the watcher. Safe to call once, from setup.
///
/// Runs even in safe mode, unlike `display_watch::start`. The reason that one
/// is skipped does not apply here: `display_watch` REBUILDS an overlay window
/// and safe mode deliberately has none, whereas this thread only reads a
/// registry value and emits an event. A safe-mode launch still shows the
/// dashboard, and a dashboard in the wrong palette while the user is trying to
/// work out why the app is in safe mode is one more confusing thing for no
/// gain.
pub fn start(app: AppHandle) {
    if std::thread::Builder::new()
        .name("st-theme-watch".into())
        .spawn(move || {
            let initial = crate::config::os_prefers_dark();
            let mut watcher = Watcher::new(initial);
            log::info!(
                "theme: watching the Windows app light/dark setting every {}s \
                 (HKCU Themes\\Personalize\\AppsUseLightTheme); it currently reads {:?}, so \
                 an \"auto\" theme resolves to {}. Both windows are told through the global \
                 '{EVENT}' event — this is the ONE reading of that setting in the app, \
                 because prefers-color-scheme inside a webview reports what the webview was \
                 configured to prefer rather than what Windows was asked for.",
                POLL.as_secs(),
                initial,
                if initial.unwrap_or(false) {
                    "starry"
                } else {
                    "earthy"
                }
            );
            loop {
                std::thread::sleep(POLL);
                if let Some(dark) = watcher.observe(crate::config::os_prefers_dark()) {
                    log::info!(
                        "theme: Windows' app mode changed — dark={dark}. Emitting '{EVENT}' to \
                         every window so the dashboard and the overlay move together; a theme \
                         of \"auto\" now resolves to {}.",
                        if dark { "starry" } else { "earthy" }
                    );
                    if let Err(e) = app.emit(EVENT, OsThemePayload { dark }) {
                        log::error!(
                            "theme: could not emit '{EVENT}' ({e}) — the windows keep the \
                             palette they already had, and the next change will try again."
                        );
                    }
                }
            }
        })
        .is_err()
    {
        log::error!(
            "theme: could not start the '{EVENT}' watcher thread. An \"auto\" theme still \
             resolves correctly at launch (get_os_prefers_dark is a plain command and is \
             unaffected), but a light/dark change made while the app is running will not be \
             followed until it is restarted."
        );
    }
}

/// The whole of what can be wrong in here. The thread is a `sleep` and an
/// `emit`; the decision is [`Watcher::observe`], and both of its failure modes
/// are regressions with a visible cost, so both are pinned.
#[cfg(test)]
mod tests {
    use super::*;

    /// A restatement of the current value is NOT a change. Emitting one every
    /// 2 seconds would run `applyLook()`'s 450ms cross-fade on the dashboard
    /// forever — the animation is gated on the RESOLVED theme differing, but
    /// the event still costs two windows a repaint, and any future listener
    /// that is not so careful would flicker.
    #[test]
    fn an_unchanged_reading_emits_nothing() {
        let mut w = Watcher::new(Some(true));
        assert_eq!(w.observe(Some(true)), None);
        assert_eq!(w.observe(Some(true)), None);
    }

    /// Both directions, and the value that is emitted is the NEW one.
    #[test]
    fn a_change_is_reported_once_in_each_direction() {
        let mut w = Watcher::new(Some(false));
        assert_eq!(w.observe(Some(true)), Some(true));
        assert_eq!(w.observe(Some(true)), None, "the same value is not a change");
        assert_eq!(w.observe(Some(false)), Some(false));
        assert_eq!(w.observe(Some(false)), None);
    }

    /// An unreadable registry value is DAYLIGHT, not "ignore this poll" and
    /// not "dark" — the same `unwrap_or(false)` `config::resolve_theme`
    /// applies to `os_dark == None`, and the same one `get_os_prefers_dark`
    /// hands the frontend. Three places, one answer.
    #[test]
    fn an_unreadable_value_is_daylight_like_everywhere_else() {
        let mut w = Watcher::new(None);
        assert_eq!(
            w.observe(None),
            None,
            "None seeded as daylight, so a second None is not a change"
        );

        let mut w = Watcher::new(Some(true));
        assert_eq!(
            w.observe(None),
            Some(false),
            "a value that became unreadable resolves to daylight, exactly as \
             config::resolve_theme does with None — a rule with an exception in the watcher \
             and none in the resolver is two rules"
        );
    }

    /// A watcher seeded from an unanswerable read must still notice the first
    /// real answer. This is the first-launch case on a machine whose
    /// Personalize key has never been written (a fresh install that has never
    /// opened the Personalisation page — see `config::os_prefers_dark`'s doc).
    #[test]
    fn a_seed_of_none_still_sees_the_first_real_dark_reading() {
        let mut w = Watcher::new(None);
        assert_eq!(w.observe(Some(true)), Some(true));
    }

    /// The command collapses the `Option` the same way, and cannot panic. It
    /// reads this machine's real setting, so the VALUE is whatever the owner
    /// chose — what is asserted is that asking is safe and that the answer
    /// feeds the shared rule without inventing a third palette.
    #[test]
    fn the_command_answers_and_agrees_with_the_resolver() {
        let dark = get_os_prefers_dark();
        assert_eq!(
            dark,
            crate::config::os_prefers_dark().unwrap_or(false),
            "the command must not apply a different fallback from the one the resolver uses"
        );
        let resolved = crate::config::resolve_theme(crate::config::THEME_AUTO, Some(dark));
        assert!(
            resolved == "earthy" || resolved == "starry",
            "\"auto\" resolved to {resolved}, which is not one of the two palettes it may ever \
             produce"
        );
    }
}
