//! session_end.rs — PROBLEM 224. Own `WM_ENDSESSION` before tao sees it.
//!
//! # The crash this exists to remove
//!
//! Thirty recorded crashes, all one line:
//!
//! ```text
//! PANIC on thread 'main' at ...\tao-0.35.3\...\event_loop\runner.rs:371:25:
//!     cannot move state from Destroyed
//! ```
//!
//! `runner.rs:371` is the last arm of tao's state machine:
//! `(Destroyed, _) => panic!("cannot move state from Destroyed")`. Something
//! moved the runner to `Destroyed` and then a window message arrived and tried
//! to move it somewhere else.
//!
//! The thing that moves it to `Destroyed` is tao's own `WM_ENDSESSION`
//! handler, in `thread_event_target_callback` (tao 0.35.3,
//! `platform_impl/windows/event_loop.rs:2384`):
//!
//! ```text
//! win32wm::WM_ENDSESSION => {
//!   if wparam.0 == TRUE.0 as usize {
//!     subclass_input.event_loop_runner.loop_destroyed();   // -> Destroyed
//!   }
//!   // Note: after we return 0 here, Windows will shut us down
//!   LRESULT(0)
//! }
//! ```
//!
//! Read that comment again: **"after we return 0 here, Windows will shut us
//! down."** tao is not making a mistake, it is making a BET — that the process
//! dies before any further message reaches a window proc. The bet is only
//! wrong because we survive it. Windows does not kill a process the instant it
//! returns from `WM_ENDSESSION`; it kills it when the whole session-end
//! sequence is done, which for a Restart Manager close (an .msi upgrading a
//! running app) may never happen at all — the installer only wanted the exe
//! closed. In that gap the settings/overlay window gets one more message, wry's
//! `parent_subclass_proc` chains it into tao's `public_window_callback`, tao
//! asks the runner to change state, and the runner is already `Destroyed`.
//!
//! That whole path is legible in the recorded backtrace, and it is the reason
//! the obvious fix does not work — see "Why not a msg_hook" below:
//!
//! ```text
//!  24: GetMessageW                 <- the app is parked in its message pump
//!  23: NtUserGetMessage
//!  22: KiUserCallbackDispatcher    <- the kernel dispatches a SENT message
//!  20: SendMessageW
//!  19: CallWindowProcW
//!  17: DefSubclassProc
//!  16: <wry::webview2::InnerWebView>::parent_subclass_proc
//!  15: DefSubclassProc
//!   9: core::panicking::panic_fmt
//! ```
//!
//! # The fix: make tao's assumption true
//!
//! Take `WM_ENDSESSION` first and exit from inside the handler. tao's runner is
//! then never told anything, so it can never be asked to move out of
//! `Destroyed`. No dependency patch, no panic suppression, no `catch_unwind`
//! (which could not work anyway: the panic crosses an `extern "system"`
//! boundary, where unwinding aborts).
//!
//! `SetWindowSubclass` puts the newest subclass at the HEAD of the chain, so a
//! subclass installed after tao's and wry's runs before both of them.
//!
//! # Why every thread window, not just the two Tauri ones
//!
//! tao's `Tao Thread Event Target` window is created first, is a real
//! top-level window (`WS_POPUP | WS_VISIBLE`, layered + tool-window so the user
//! never sees it), and is therefore in the broadcast set for `WM_ENDSESSION` —
//! and it is the ONE window whose handler calls `loop_destroyed()`. Guarding
//! only `settings` and `overlay` would still lose the race whenever Windows
//! happened to reach the event target first. `EnumThreadWindows` covers all
//! three, plus the tray's hidden window, in one pass.
//!
//! **`EnumThreadWindows(GetCurrentThreadId())`, never `EnumWindows`.** Widening
//! this to every window on the desktop is the class of mistake that once
//! minimised explorer.exe's shell windows and broke the owner's touchpad
//! (NATIVE_SAFETY.md).
//!
//! # Why not a `msg_hook`
//!
//! tao exposes `EventLoopBuilderExtWindows::with_msg_hook`, and it is the first
//! thing anyone reaches for. It cannot work here, and it fails SILENTLY: it
//! compiles, the tests stay green, and nothing changes. tao's message hook only
//! sees messages that come out of `GetMessageW`. `WM_QUERYENDSESSION` and
//! `WM_ENDSESSION` are **sent**, not posted — the kernel delivers them straight
//! into the window procedure through `KiUserCallbackDispatcher`, which is
//! exactly what frames 22-24 of the backtrace above show. A posted-message hook
//! is on the wrong path entirely.
//!
//! # Free side effect
//!
//! `RunEvent::Exit`'s `pip::restore_all()` (PROBLEM 167) currently runs — when
//! it runs at all — with the runner already `Destroyed`, and can be cut short
//! by the panic. Doing the teardown here moves it to a point where the runner
//! is healthy, so PiP windows stranded at a quarter size with no title bar stop
//! depending on a coin flip. The Tauri-side call is untouched: `restore_all`
//! drains its own map, so whichever path arrives second restores nothing twice.
//!
//! # Rules for anything added to this file
//!
//! 1. **You are on the main thread inside a SENT message.** Windows allows
//!    roughly `HungAppTimeout` (5 s by default) before it force-kills the
//!    process at shutdown. Do no blocking work here beyond the teardown.
//! 2. **No `std::panic::set_hook`.** There is exactly one, in `lib.rs`
//!    (CLAUDE.md; PROBLEM 131). This module must not install another, and must
//!    not silence the existing one.
//! 3. **No heap data in `dwRefData`.** It is deliberately `0`, so there is
//!    nothing to free when a window is destroyed and no way to leave a
//!    dangling pointer behind. `RemoveWindowSubclass` is never needed.
//! 4. Nothing in the subclass procedure may panic: it is `extern "system"`, and
//!    a panic across that boundary aborts the process without a log line.

use std::sync::atomic::{AtomicBool, Ordering};

/// tao's hidden per-thread message window (tao 0.35.3,
/// `THREAD_EVENT_TARGET_WINDOW_CLASS`). Named here because its presence in the
/// enumeration is the ONE thing that makes this guard worth having: it is the
/// window whose `WM_ENDSESSION` handler sets `Destroyed`. `install` logs an
/// ERROR if it is not found, because a guard that missed it is a guard that
/// still loses the race — silently.
pub const TAO_EVENT_TARGET_CLASS: &str = "Tao Thread Event Target";

/// `uIdSubclass` for our procedure. A subclass is identified by the
/// (procedure, id) PAIR, so this cannot collide with tao's 0/1 or wry's — but a
/// distinctive value makes the entry recognisable in a debugger. Re-installing
/// with the same pair REPLACES the entry rather than adding a second one, which
/// is what makes `install` idempotent and safe to call on every overlay
/// rebuild.
const SUBCLASS_ID: usize = 0x5A5E_0E4D;

/// Windows has told us the session is ending and has NOT yet said it was
/// cancelled. Set from `WM_QUERYENDSESSION`, cleared by `WM_ENDSESSION(FALSE)`.
///
/// This is a diagnostic, not a gate: it is what lets the `WM_ENDSESSION(TRUE)`
/// line say whether the shutdown was announced (Restart Manager and a normal
/// logoff both query first) or arrived unannounced.
static SESSION_ENDING: AtomicBool = AtomicBool::new(false);

/// One-shot latch for the teardown. `WM_ENDSESSION` goes to EVERY top-level
/// window in the process, and every one of them now runs this procedure, so
/// without the latch a slow teardown could be re-entered from the next window.
static ENDING_HANDLED: AtomicBool = AtomicBool::new(false);

/// True once `WM_QUERYENDSESSION` has been seen and not cancelled.
pub fn session_ending() -> bool {
    SESSION_ENDING.load(Ordering::SeqCst)
}

// ---------------------------------------------------------------------------
// Pure part — no Win32, unit-tested
// ---------------------------------------------------------------------------

/// What a window message means to this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMsg {
    /// `WM_QUERYENDSESSION` — Windows is ASKING. Answer yes and change nothing:
    /// a query can still be cancelled, and an app that answers no is the app
    /// that blocks a Windows update.
    Query,
    /// `WM_ENDSESSION` with `wParam == TRUE` — it is actually happening.
    Ending,
    /// `WM_ENDSESSION` with `wParam == FALSE` — a previous query was cancelled.
    Cancelled,
    /// Everything else — chain to the rest of the subclass chain untouched.
    Other,
}

/// The message numbers, spelled out rather than imported.
///
/// They are `const`s in `windows::Win32::UI::WindowsAndMessaging` too, and
/// `constants_match_win32` below asserts these agree with those. Keeping our
/// own copies is what lets `classify` be a plain function that a unit test can
/// call on any platform — the alternative is a `#[cfg(windows)]` test, i.e. no
/// test at all on the day someone runs `cargo test` elsewhere.
pub const WM_QUERYENDSESSION: u32 = 0x0011;
pub const WM_ENDSESSION: u32 = 0x0016;

/// `lParam` flags on both messages.
pub const ENDSESSION_CLOSEAPP: usize = 0x0000_0001;
pub const ENDSESSION_CRITICAL: usize = 0x4000_0000;
pub const ENDSESSION_LOGOFF: usize = 0x8000_0000;

/// Classify one message. Pure; the whole decision table in four lines.
pub fn classify(msg: u32, wparam: usize) -> SessionMsg {
    if msg == WM_QUERYENDSESSION {
        return SessionMsg::Query;
    }
    if msg == WM_ENDSESSION {
        // wParam is a BOOL: any non-zero value means the session really is
        // ending. Comparing against 1 would be wrong — Win32 BOOLs are only
        // guaranteed non-zero for true.
        return if wparam != 0 { SessionMsg::Ending } else { SessionMsg::Cancelled };
    }
    SessionMsg::Other
}

/// Turn the `lParam` of a session message into something a log reader can act
/// on. This is the difference between "the app exited" and "msiexec asked the
/// app to close so it could upgrade it", which is the case that produced the
/// 2026-08-30 crash.
///
/// `0` is not a flag and not an error: a plain shutdown or restart sends no
/// bits at all.
pub fn describe_reason(lparam: usize) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if lparam & ENDSESSION_CLOSEAPP != 0 {
        parts.push("ENDSESSION_CLOSEAPP (an installer or the Restart Manager wants this exe closed so it can be replaced)");
    }
    if lparam & ENDSESSION_LOGOFF != 0 {
        parts.push("ENDSESSION_LOGOFF (the user is signing out)");
    }
    if lparam & ENDSESSION_CRITICAL != 0 {
        parts.push("ENDSESSION_CRITICAL (this shutdown cannot be blocked)");
    }
    if parts.is_empty() {
        return format!("lParam 0x{lparam:X} — a system shutdown or restart");
    }
    format!("lParam 0x{lparam:X} — {}", parts.join(" + "))
}

// ---------------------------------------------------------------------------
// Win32 part
// ---------------------------------------------------------------------------

/// Counts gathered while enumerating, so `install` can say what it actually
/// guarded instead of claiming success it did not verify.
#[cfg(windows)]
#[derive(Default)]
struct Tally {
    seen: usize,
    guarded: usize,
    tao_target: bool,
    classes: Vec<String>,
}

/// Install the guard on every top-level window owned by the CALLING thread.
///
/// **Must be called on the thread that owns the app's windows** — the main
/// thread. `EnumThreadWindows` is thread-scoped, so calling this from a worker
/// finds nothing and installs nothing; that is why `display_watch`'s rebuild
/// hops to the main thread before calling it, and why a zero count is logged as
/// an ERROR rather than passing quietly.
///
/// Idempotent: re-installing the same (procedure, id) pair replaces the
/// existing entry. Call it whenever a window is created or rebuilt — a rebuilt
/// overlay is a NEW HWND and carries none of the old window's subclasses.
#[cfg(windows)]
pub fn install() {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::EnumThreadWindows;

    let tid = unsafe { GetCurrentThreadId() };
    let mut tally = Tally::default();
    unsafe {
        let _ = EnumThreadWindows(
            tid,
            Some(enum_proc),
            LPARAM(&mut tally as *mut Tally as isize),
        );
    }

    let list = tally.classes.join(", ");
    if tally.seen == 0 {
        log::error!(
            "session: WM_ENDSESSION guard NOT armed — EnumThreadWindows(thread {tid}) found no \
             top-level windows. Either this ran before the event loop existed or it ran off the \
             main thread, and in both cases tao will set Destroyed at shutdown and the \
             'cannot move state from Destroyed' panic is back (PROBLEM 224)."
        );
    } else if !tally.tao_target {
        log::error!(
            "session: WM_ENDSESSION guard armed on {}/{} main-thread window(s) [{}] but tao's \
             '{}' window was NOT among them — that is the one window whose handler sets \
             Destroyed, so the guard cannot prevent PROBLEM 224's panic in this state.",
            tally.guarded,
            tally.seen,
            list,
            TAO_EVENT_TARGET_CLASS
        );
    } else {
        log::info!(
            "session: WM_ENDSESSION guard armed on {}/{} main-thread window(s) [{}] — tao's \
             event target included, so a shutdown, a sign-out or an .msi upgrade over the \
             running app now exits from our handler instead of panicking in tao (PROBLEM 224).",
            tally.guarded,
            tally.seen,
            list
        );
    }
}

#[cfg(not(windows))]
pub fn install() {}

/// `EnumThreadWindows` callback. `extern "system"`, so it must not panic;
/// everything in it is either infallible or slice-bounded.
#[cfg(windows)]
unsafe extern "system" fn enum_proc(
    hwnd: windows::Win32::Foundation::HWND,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::BOOL {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::UI::Shell::SetWindowSubclass;
    use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;

    let tally = &mut *(lparam.0 as *mut Tally);
    tally.seen += 1;

    // 128 is comfortably above the 64-character maximum for a registered class
    // name (RegisterClassEx rejects longer), so this never truncates a real
    // class. `GetClassNameW` returns the count WITHOUT the terminator, and 0 on
    // failure.
    let mut buf = [0u16; 128];
    let n = GetClassNameW(hwnd, &mut buf).clamp(0, buf.len() as i32) as usize;
    let class = if n == 0 {
        "<class name unavailable>".to_string()
    } else {
        String::from_utf16_lossy(&buf[..n])
    };
    if class == TAO_EVENT_TARGET_CLASS {
        tally.tao_target = true;
    }

    // dwRefData is deliberately 0 — see rule 3 in this module's header.
    let ok = SetWindowSubclass(hwnd, Some(session_end_proc), SUBCLASS_ID, 0).as_bool();
    if ok {
        tally.guarded += 1;
        tally.classes.push(class);
    } else {
        tally.classes.push(format!("{class} (SUBCLASS FAILED)"));
    }

    BOOL(1) // keep enumerating
}

/// The guard itself. Runs ahead of wry's and tao's procedures because it was
/// installed last.
#[cfg(windows)]
unsafe extern "system" fn session_end_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    _uid_subclass: usize,
    _ref_data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::LRESULT;
    use windows::Win32::UI::Shell::DefSubclassProc;

    match classify(msg, wparam.0) {
        // The overwhelmingly common case, and the first line of the function
        // for that reason: one integer comparison and straight back into the
        // chain.
        SessionMsg::Other => DefSubclassProc(hwnd, msg, wparam, lparam),

        SessionMsg::Query => {
            // Log once per shutdown attempt, not once per window: the message
            // is broadcast to all of them and they all land here.
            if !SESSION_ENDING.swap(true, Ordering::SeqCst) {
                log::warn!(
                    "session: WM_QUERYENDSESSION — {}. Answering YES and tearing NOTHING down \
                     yet; a query can still be cancelled, and the app that answers no is the \
                     app that blocks a Windows update (PROBLEM 224).",
                    describe_reason(lparam.0 as usize)
                );
            }
            // TRUE. Deliberately NOT chained: DefWindowProc's own answer to
            // WM_QUERYENDSESSION is TRUE, so this changes nothing except that
            // no other procedure in the chain can veto the shutdown. tao does
            // not handle this message at all (the arm is commented out in
            // event_loop.rs), so nothing downstream is being denied.
            LRESULT(1)
        }

        SessionMsg::Cancelled => {
            if SESSION_ENDING.swap(false, Ordering::SeqCst) {
                log::warn!(
                    "session: WM_ENDSESSION(FALSE) — the shutdown was CANCELLED. Nothing had \
                     been torn down, so there is nothing to undo and the app carries on."
                );
            }
            // Chain: tao's own handler ignores the FALSE case, and this is the
            // path where the process keeps living, so nothing may be skipped.
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }

        SessionMsg::Ending => {
            if ENDING_HANDLED.swap(true, Ordering::SeqCst) {
                // Another window already started the teardown. Answer and get
                // out of the way; the first one is about to exit the process.
                return LRESULT(0);
            }
            log::warn!(
                "session: WM_ENDSESSION(TRUE) — {}; announced by a prior WM_QUERYENDSESSION: \
                 {}. Taking the exit HERE, ahead of tao: tao sets its runner to Destroyed in \
                 this handler and bets the process dies before another message arrives, and \
                 surviving that bet is the whole of PROBLEM 224.",
                describe_reason(lparam.0 as usize),
                SESSION_ENDING.load(Ordering::SeqCst)
            );
            teardown();
            log::warn!(
                "session: teardown finished — exiting with code 0 from inside the \
                 WM_ENDSESSION handler. tao's runner is never told the session ended, so it \
                 can never be asked to move out of Destroyed (PROBLEM 224)."
            );
            std::process::exit(0);
        }
    }
}

/// Everything that must happen before the process goes, and nothing else.
///
/// The budget is `HungAppTimeout` — 5 s by default — for the whole
/// `WM_ENDSESSION` handler, shared with the log writes around it. Both calls
/// below are bounded:
///
/// * `pip::restore_all` returns immediately when nothing is cornered (the
///   normal case) and otherwise makes a handful of `SetWindowPos`/`ShowWindow`
///   calls on windows it has already checked with `IsWindow`.
/// * `stop_hook` posts one thread message and returns.
///
/// **Config is deliberately absent.** `config::save` runs synchronously at
/// every mutation and the file is never held open, so there is nothing pending
/// to flush — and writing the owner's config from a shutdown path would be a
/// way to corrupt it, not a way to protect it.
#[cfg(windows)]
fn teardown() {
    // PROBLEM 167 — hand back every window PiP is still holding. A stranded PiP
    // window is topmost and a quarter size with no title bar, and the only
    // control that released it is the app that is about to stop existing.
    crate::engine::actions::pip::restore_all();

    // Uninstall WH_KEYBOARD_LL / WH_MOUSE_LL deliberately, so the supervisor
    // (PROBLEM 82) does not read the hook thread's exit as a fault and restart
    // it while the process is winding down.
    crate::hook::stop_hook();

    // PROBLEM 253 — THIS EXIT IS NOT A CRASH, and something has to say so.
    //
    // `safe_mode` counts a launch as failed unless the app either stays alive
    // thirty seconds or exits deliberately, and everything about the path this
    // function is on is deliberate: Windows is signing the user out, shutting
    // down, or letting an installer close us so it can replace the exe. Every
    // one of those routinely happens inside the first thirty seconds — a logon
    // autostarts the app and an update arrives; a user signs in and straight
    // back out — and three of them in a row would put a perfectly healthy app
    // into safe mode and tell its owner it had crashed three times.
    //
    // It is HERE and not in `lib.rs`'s `RunEvent` handler because that handler
    // never runs on this path: the whole point of this module is that we
    // `std::process::exit(0)` from inside the `WM_ENDSESSION` handler, ahead of
    // tao, so no Tauri exit event is ever produced.
    //
    // Bounded, as rule 1 in this module's header requires: two atomic loads and
    // at most one write of a ~60-byte JSON file.
    crate::safe_mode::note_clean_exit();

    log::info!("session: PiP windows restored and the keyboard hook told to stop");
}

// ---------------------------------------------------------------------------
// Tests — the pure parts only. The subclass procedure itself needs a live
// message pump and is exercised by the harness in
// scripts/verify-session-end.ps1 (see V14_FIXES_AND_CODE.md PROBLEM 224).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers this module keys off are copied from Win32 rather than
    /// imported, so that `classify` stays platform-free. This test is the thing
    /// that stops the copies drifting: it compares them with the `windows`
    /// crate's own constants. If a crate upgrade ever renumbers one, this fails
    /// here instead of failing as a shutdown crash on the owner's machine.
    #[test]
    #[cfg(windows)]
    fn constants_match_win32() {
        use windows::Win32::UI::WindowsAndMessaging as w;
        assert_eq!(WM_QUERYENDSESSION, w::WM_QUERYENDSESSION);
        assert_eq!(WM_ENDSESSION, w::WM_ENDSESSION);
        assert_eq!(ENDSESSION_CLOSEAPP, w::ENDSESSION_CLOSEAPP as usize);
        assert_eq!(ENDSESSION_LOGOFF, w::ENDSESSION_LOGOFF as usize);
    }

    #[test]
    fn query_is_never_an_exit() {
        // The trap this guards: exiting on the QUERY would kill the app every
        // time a shutdown was proposed and then cancelled.
        assert_eq!(classify(WM_QUERYENDSESSION, 0), SessionMsg::Query);
        assert_eq!(classify(WM_QUERYENDSESSION, 1), SessionMsg::Query);
    }

    #[test]
    fn endsession_true_ends_and_false_cancels() {
        assert_eq!(classify(WM_ENDSESSION, 1), SessionMsg::Ending);
        assert_eq!(classify(WM_ENDSESSION, 0), SessionMsg::Cancelled);
    }

    #[test]
    fn any_nonzero_wparam_means_ending() {
        // A Win32 BOOL is only guaranteed non-zero for true. `wparam == 1`
        // would read a TRUE of 2 or -1 as "cancelled" and let the panic back
        // in on exactly the shutdown that matters.
        for w in [1usize, 2, 42, usize::MAX] {
            assert_eq!(classify(WM_ENDSESSION, w), SessionMsg::Ending, "wparam {w}");
        }
    }

    #[test]
    fn ordinary_messages_are_left_alone() {
        // WM_CLOSE, WM_DESTROY, WM_PAINT, WM_TIMER: everything that is not one
        // of the two session messages must chain untouched.
        for msg in [0x0010u32, 0x0002, 0x000F, 0x0113, 0x0000] {
            assert_eq!(classify(msg, 1), SessionMsg::Other, "msg 0x{msg:04X}");
        }
    }

    #[test]
    fn reason_names_the_restart_manager() {
        // The 2026-08-30 crash: msiexec upgrading a running app. If the log
        // cannot say that, the next report is "it crashed at 15:09" again.
        let s = describe_reason(ENDSESSION_CLOSEAPP);
        assert!(s.contains("ENDSESSION_CLOSEAPP"), "{s}");
        assert!(s.contains("0x1"), "{s}");
    }

    #[test]
    fn reason_handles_logoff_shutdown_and_combinations() {
        assert!(describe_reason(ENDSESSION_LOGOFF).contains("ENDSESSION_LOGOFF"));
        // 0 is a plain shutdown/restart, not an error or an unknown value.
        let plain = describe_reason(0);
        assert!(plain.contains("shutdown or restart"), "{plain}");
        assert!(!plain.contains("ENDSESSION_"), "{plain}");
        // Flags combine; both must survive.
        let both = describe_reason(ENDSESSION_CLOSEAPP | ENDSESSION_LOGOFF);
        assert!(both.contains("ENDSESSION_CLOSEAPP"), "{both}");
        assert!(both.contains("ENDSESSION_LOGOFF"), "{both}");
    }

    #[test]
    fn reason_never_panics_on_an_unknown_value() {
        // It is read inside a message handler that must not panic.
        for l in [usize::MAX, 0x2000_0000, 0x0000_00FF] {
            let _ = describe_reason(l);
        }
    }
}
