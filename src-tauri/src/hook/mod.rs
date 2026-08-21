/// hook/mod.rs — Dedicated Win32 keyboard hook thread.
///
/// Architecture guarantees:
/// • Runs on a completely isolated OS thread with its own Win32 message pump.
/// • Uses `GetMessage` (blocking) → 0 % CPU when idle.
/// • Communicates events to the engine via a `crossbeam_channel` SPSC sender.
/// • Never touches Tauri/WebView2 on the critical path.

pub mod fullscreen;
pub mod conflicts;
pub mod conflict_close;

use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Public event type sent to the async engine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum KeyCombo {
    Alpha(char),      // Space + a–z
    Special(String),  // Space + F1–F12, Enter, Tab, Left, Right (user-configurable)
    Escape,           // Space + Esc   → Boss Key
    Backtick,         // Space + `     → PiP
    Comma,            // Space + ,     → Focus Engine
    RightAlt,         // Space + RAlt  → Profile Cycle
    UpArrow,          // Space + Up    → Scroll-Top (double-tap)
    DownArrow,        // Space + Down  → Scroll-Bottom (double-tap)
    Period,           // Space + .     → Bypass Toggle
    Backspace,        // Space + ⌫     → Force Close (Alt+F4)
}

#[derive(Debug, Clone)]
pub enum HookEvent {
    SpaceDown,
    SpaceUp { modifier_fired: bool },
    KeyCombo(KeyCombo),
    WheelUp,
    WheelDown,
}

// ---------------------------------------------------------------------------
// Shared atomic state (written by hook thread, read by engine)
// ---------------------------------------------------------------------------

/// `true` when Space is held down as a modifier.
pub static MODIFIER_ACTIVE: AtomicBool = AtomicBool::new(false);
/// `true` when Bypass mode is active (hook passes Space through without interception).
pub static BYPASS_MODE: AtomicBool = AtomicBool::new(false);
/// `true` when a fullscreen game is active — hook passes everything through.
pub static FULLSCREEN_ACTIVE: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Suppression counters — LOCK-FREE ATOMICS ONLY (PROBLEM 58).
//
// The hook callback must never touch the logger: log4rs writes synchronously
// to disk, and disk I/O inside a WH_KEYBOARD_LL callback makes Windows evict
// the hook once it overruns LowLevelHooksTimeout. These counters cost a single
// atomic increment; `drain_hook_diagnostics()` turns them into log lines from
// the ENGINE thread, where blocking is harmless.
// ---------------------------------------------------------------------------
use std::sync::atomic::AtomicU32;
static SUPPRESS_FULLSCREEN: AtomicU32 = AtomicU32::new(0);
static SUPPRESS_BYPASS: AtomicU32 = AtomicU32::new(0);
static ROLLOVER_HITS: AtomicU32 = AtomicU32::new(0);
static STUCK_MODIFIER: AtomicU32 = AtomicU32::new(0);
static UNMAPPED_KEYS: AtomicU32 = AtomicU32::new(0);

/// PROBLEM 104 — the counter that answers "does the hook see ANY key?".
///
/// The user reports that with the Spaceadom window focused, nothing works at
/// all: no Guide HUD, no toasts, no launches. Every existing counter only
/// records keys the hook DECIDED something about, so a hook that never fires
/// and a hook that fires and passes everything through look identical — both
/// leave zeros everywhere. This is incremented on entry, before any branch,
/// so a still-zero value is proof the callback is not being invoked.
static KB_EVENTS_SEEN: AtomicU32 = AtomicU32::new(0);
/// Of those, how many arrived while OUR OWN window held the foreground. If
/// this stays 0 while the total climbs, Windows is not delivering our own
/// window's keystrokes to our hook — which is the user's exact symptom.
static KB_EVENTS_OWN_FG: AtomicU32 = AtomicU32::new(0);
static DROPPED_EVENTS: AtomicU32 = AtomicU32::new(0);

/// Report and reset the hook's suppression counters. Called from the ENGINE
/// thread (never the hook thread). Silent when everything is zero, so a
/// healthy log stays clean.
pub fn drain_hook_diagnostics() {
    let fs = SUPPRESS_FULLSCREEN.swap(0, Ordering::Relaxed);
    let by = SUPPRESS_BYPASS.swap(0, Ordering::Relaxed);
    let ro = ROLLOVER_HITS.swap(0, Ordering::Relaxed);
    let st = STUCK_MODIFIER.swap(0, Ordering::Relaxed);
    let un = UNMAPPED_KEYS.swap(0, Ordering::Relaxed);
    let dr = DROPPED_EVENTS.swap(0, Ordering::Relaxed);
    let rh = HOOK_REINSTALLS.swap(0, Ordering::Relaxed);
    // PROBLEM 104 — reported at most once a minute. This function drains on
    // every Space RELEASE, so logging unconditionally wrote a line every few
    // seconds while typing: the same log-noise problem the watchdog had, in a
    // line that was added to diagnose it. The counters keep accumulating
    // between reports, so nothing is lost — only the printing is throttled.
    {
        static LAST_SEEN_REPORT: AtomicU64 = AtomicU64::new(0);
        let now = tick_count();
        let last = LAST_SEEN_REPORT.load(Ordering::Relaxed);
        if now.saturating_sub(last) >= 60_000 {
            let seen = KB_EVENTS_SEEN.swap(0, Ordering::Relaxed);
            let own = KB_EVENTS_OWN_FG.swap(0, Ordering::Relaxed);
            if seen > 0 {
                LAST_SEEN_REPORT.store(now, Ordering::Relaxed);
                log::info!(
                    "hook: saw {seen} key event(s) in the last minute, {own} of them while the Spaceadom window itself had focus"
                );
            }
        }
    }
    if fs == 0 && by == 0 && ro == 0 && st == 0 && un == 0 && dr == 0 && rh == 0 {
        return;
    }
    log::info!(
        "hook diagnostics — fullscreen-suppressed:{fs} bypass-suppressed:{by} \
         typed-not-command(rollover):{ro} stuck-modifier-resets:{st} unmapped-keys:{un} \
         dropped-events:{dr} watchdog-reinstalls:{rh}"
    );
    if ro > 0 {
        // The advice here used to say "set a SLOWER typing speed (a slower
        // setting narrows the window)". Both halves were backwards: a slower
        // setting WIDENS the window (16800/wpm), which produces MORE of these,
        // so following it made the reported problem worse.
        log::info!(
            "hook: {ro} key(s) landed INSIDE the rollover window and were typed instead of \
             treated as a shortcut. Hold Space slightly longer before the letter, or set a \
             FASTER 'Typing speed' in Settings — a faster setting NARROWS the window, so a \
             shorter hold counts as a command."
        );
    }

    // PROBLEM 95 — the Space-down→key-down delay distribution from REAL typing.
    // This is the measurement that decides whether the window is safe for THIS
    // person's hands; it cannot be obtained by simulating keystrokes.
    let window = ROLLOVER_MS.load(Ordering::Relaxed);
    let typed: Vec<u32> = MARGIN_TYPED.iter().map(|b| b.swap(0, Ordering::Relaxed)).collect();
    let cmd: Vec<u32> = MARGIN_COMMAND.iter().map(|b| b.swap(0, Ordering::Relaxed)).collect();
    if typed.iter().chain(cmd.iter()).any(|&n| n > 0) {
        let fmt = |v: &[u32]| {
            v.iter()
                .enumerate()
                .filter(|(_, &n)| n > 0)
                .map(|(i, &n)| {
                    let lo = i as u64 * MARGIN_BUCKET_MS;
                    if i == MARGIN_BUCKETS - 1 {
                        format!("{lo}+ms:{n}")
                    } else {
                        format!("{lo}-{}ms:{n}", lo + MARGIN_BUCKET_MS - 1)
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        log::info!(
            "hook margins (window {window}ms) — TYPED [{}] | COMMAND [{}]",
            fmt(&typed),
            fmt(&cmd)
        );
        // The danger sign: ordinary typing arriving within one bucket of the
        // threshold. One heavier-thumbed day and those become commands.
        let near = window.saturating_sub(MARGIN_BUCKET_MS) / MARGIN_BUCKET_MS;
        let close: u32 = typed.iter().skip(near as usize).sum();
        if close > 0 {
            log::warn!(
                "hook: {close} keystroke(s) came within {}ms of being treated as a command \
                 while typing. If shortcuts ever fire mid-sentence, set a SLOWER 'Typing \
                 speed' in Settings — that WIDENS the window and pushes ordinary typing \
                 further from the threshold.",
                MARGIN_BUCKET_MS
            );
        }
    }
}
/// `true` when the current Space-down has already been aborted (another key hit in rollover window).
static SPACE_ABORTED: AtomicBool = AtomicBool::new(false);
/// `true` when we actually swallowed the current Space-down. If we passed it
/// through (Ctrl/Alt/Win held, bypass, fullscreen), the matching Space-up must
/// pass through too — otherwise we inject a phantom space the user never typed.
static SPACE_INTERCEPTED: AtomicBool = AtomicBool::new(false);
/// Timestamp (ms) when Space was pressed down.
static SPACE_DOWN_TS: AtomicU64 = AtomicU64::new(0);
/// PROBLEM 95 — how close does REAL typing come to the command threshold?
///
/// Simulated keystrokes could not answer this: injected input never reliably
/// reached the hook from the test harness, and a thumb is exactly the thing a
/// simulation guesses at. So the app measures it on live typing instead.
///
/// One bucket per 40 ms of Space-down→key-down delay, counted separately for
/// the two verdicts. A single `fetch_add` per event: no allocation, no lock,
/// no logging — the hook callback must still return in microseconds.
///
/// Read it in debug.log as `hook margins`. What to look for: TYPED counts
/// piling up in the buckets just under the window mean the user's ordinary
/// typing is skimming the threshold, and one heavier day would tip it into
/// firing commands mid-sentence.
pub const MARGIN_BUCKET_MS: u64 = 40;
pub const MARGIN_BUCKETS: usize = 10; // 0-39 … 360+
pub static MARGIN_TYPED: [AtomicU32; MARGIN_BUCKETS] =
    [const { AtomicU32::new(0) }; MARGIN_BUCKETS];
pub static MARGIN_COMMAND: [AtomicU32; MARGIN_BUCKETS] =
    [const { AtomicU32::new(0) }; MARGIN_BUCKETS];

#[inline(always)]
fn record_margin(bucketed: &[AtomicU32; MARGIN_BUCKETS], held_ms: u64) {
    let i = ((held_ms / MARGIN_BUCKET_MS) as usize).min(MARGIN_BUCKETS - 1);
    bucketed[i].fetch_add(1, Ordering::Relaxed);
}

/// Timestamp (ms) of last alpha-key press (for rollover detection).
static LAST_ALPHA_TS: AtomicU64 = AtomicU64::new(0);
/// Rollover window in milliseconds (configurable, default 50).
pub static ROLLOVER_MS: AtomicU64 = AtomicU64::new(50);
/// The hook's Win32 thread ID — needed to post WM_QUIT on teardown.
static HOOK_THREAD_ID: AtomicU64 = AtomicU64::new(0);

// --- PROBLEM 65/66 — hook liveness (the eviction watchdog) -----------------
/// GetTickCount64 ms of the last genuine event seen by the KEYBOARD hook.
/// Stamped before any filtering so a fully-bypassed keystroke still counts.
static LAST_KB_EVENT: AtomicU64 = AtomicU64::new(0);
/// PROBLEM 78 — tick of the watchdog's last reinstall, for its 60s cooldown.
static WATCHDOG_LAST_REINSTALL: AtomicU64 = AtomicU64::new(0);
/// Same for the MOUSE hook. Kept separate: the two hooks are evicted
/// independently, and our keyboard callback is the heavy one — a dead
/// keyboard hook with a live mouse hook is the realistic failure.
static LAST_MS_EVENT: AtomicU64 = AtomicU64::new(0);
/// PROBLEM 132 — consecutive watchdog reinstalls with NO hook event in
/// between. The 2026-08-17 outage ran 20 unbroken minutes at one reinstall a
/// minute, each logging `reinstall ok: true`, because re-hooking is the only
/// move the watchdog had. A hook proc only fires on the thread that installed
/// it, so if THAT THREAD's message pump is the thing that is wedged,
/// SetWindowsHookEx on it can never help — it succeeds and delivers nothing.
/// PROBLEM 134 - "is OUR window the foreground?", sampled OFF the hook path.
/// Refreshed on the watchdog's 3s timer, read by the callback as one atomic.
static FG_IS_SELF: AtomicBool = AtomicBool::new(false);
static BLIND_REINSTALLS: AtomicU32 = AtomicU32::new(0);
/// PROBLEM 132 — how many alarms fired while OUR OWN window held the
/// foreground. This is the owner's exact repeated report ("shortcuts do not
/// work inside the app"), and until now it was the ONE case the watchdog could
/// not describe: the UIPI discriminator skips self-focus, so it fell straight
/// through to the eviction verdict with no evidence either way.
static BLIND_WHILE_OWN_FG: AtomicU32 = AtomicU32::new(0);
/// Set by the watchdog, read by the message pump: tear this whole thread down
/// so the PROBLEM 82 supervisor rebuilds it with a fresh pump and fresh hooks.
static ESCALATE_RESTART: AtomicBool = AtomicBool::new(false);

/// PROBLEM 161 — let the USER ask for the repair the watchdog performs.
///
/// The dashboard shows a banner when `HOOK_INSTALLED` is false, and its "Try
/// again" button lands here. It sets the same escalation flag the watchdog
/// uses after two failed re-hooks, so the hook THREAD is rebuilt rather than
/// the hook merely re-installed on a thread that may itself be wedged — which
/// is the distinction PROBLEM 132 was about: re-hooking from a jammed thread
/// produces something that looks healthy and receives nothing.
pub fn request_hook_rebuild() {
    log::info!("hook: rebuild requested by the user from the dashboard banner");
    ESCALATE_RESTART.store(true, Ordering::Relaxed);
}
/// `true` while the WH_KEYBOARD_LL hook is believed installed. Set by the
/// hook thread; read by get_hook_status so the dashboard tells the truth
/// (it used to hardcode `installed: true`).
pub static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
/// Count of watchdog reinstalls, drained into the log by the engine thread.
pub static HOOK_REINSTALLS: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Win32 Virtual Key constants we care about
// ---------------------------------------------------------------------------
const VK_SPACE: u16 = 0x20;
const VK_ESCAPE: u16 = 0x1B;
const VK_OEM_3: u16 = 0xC0;  // backtick / ~
const VK_OEM_COMMA: u16 = 0xBC;
const VK_OEM_PERIOD: u16 = 0xBE;
const VK_RMENU: u16 = 0xA5;  // Right Alt
const VK_UP: u16 = 0x26;
const VK_DOWN: u16 = 0x28;
const VK_BACK: u16 = 0x08;   // Backspace → Force Close
const VK_LEFT: u16 = 0x25;
const VK_RIGHT: u16 = 0x27;
const VK_RETURN: u16 = 0x0D;   // Enter
const VK_TAB: u16 = 0x09;
// F1–F12
const VK_F1: u16 = 0x70;
const VK_F12: u16 = 0x7B;
// WM message values
const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_SYSKEYUP: u32 = 0x0105;
// Hook type
const WH_KEYBOARD_LL: i32 = 13;
const WH_MOUSE_LL: i32 = 14;
// SendInput constants
const _INPUT_KEYBOARD: u32 = 1;
const _KEYEVENTF_KEYUP: u32 = 0x0002;
// Mouse hook WM values
const WM_MOUSEWHEEL: u32 = 0x020A;
const _WHEEL_DELTA: i32 = 120;

// ---------------------------------------------------------------------------
// Thread-local sender (set once when the hook thread starts)
// ---------------------------------------------------------------------------
thread_local! {
    static EVENT_TX: std::cell::RefCell<Option<Sender<HookEvent>>> =
        const { std::cell::RefCell::new(None) };
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Spawn the isolated keyboard + mouse hook thread.
/// Returns immediately; the hook runs until `stop_hook()` is called.
/// Deliberate shutdown flag — set by stop_hook() so the respawn supervisor
/// can tell "the app is exiting" from "the hook thread DIED" (PROBLEM 82).
pub static HOOK_SHUTDOWN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn spawn_hook_thread(tx: Sender<HookEvent>, rollover_ms: u64) {
    ROLLOVER_MS.store(rollover_ms, Ordering::Relaxed);
    // PROBLEM 95 — say which window is in force. Without this line the log
    // cannot answer "why did a shortcut not fire" or "why did one fire while
    // typing": the single number that decides both was invisible.
    log::info!(
        "hook: rollover window {rollover_ms}ms — Space must be held at least this long \
         before a letter for it to count as a command; anything quicker is typed"
    );

    // PROBLEM 82 — the hook thread is the app. If it panics (a driver feeds a
    // malformed event, an OS call fails somewhere unexpected), Space+key is
    // dead until the user restarts the process, and NOTHING says so. The
    // supervisor loop below catches the panic, logs it loudly, and restarts
    // the whole hook thread body — with a 2s pause and a 5-restart/10-min cap
    // so a persistent crash cannot become a spin loop.
    std::thread::Builder::new()
        .name("st-hook-supervisor".into())
        .spawn(move || {
            let mut restarts: Vec<std::time::Instant> = Vec::new();
            loop {
                let tx2 = tx.clone();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    hook_thread_main(tx2);
                }));
                if HOOK_SHUTDOWN.load(Ordering::Relaxed) {
                    return; // clean exit (tray Exit / self-restart)
                }
                match result {
                    Ok(()) => {
                        // hook_thread_main returned without shutdown — the
                        // message pump ended unexpectedly (WM_QUIT from a
                        // foreign source). Treat like a crash: restart.
                        log::error!("hook: thread exited unexpectedly — restarting it");
                    }
                    Err(_) => {
                        log::error!(
                            "hook: THREAD PANICKED (payload in the panic-hook entry above) — \
                             restarting it so Space+key keeps working"
                        );
                    }
                }
                let now = std::time::Instant::now();
                restarts.retain(|t| now.duration_since(*t).as_secs() < 600);
                restarts.push(now);
                if restarts.len() > 5 {
                    log::error!(
                        "hook: 5 restarts inside 10 minutes — giving up to avoid a crash loop. \
                         Space+key is DEAD until the app is restarted."
                    );
                    return;
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        })
        .expect("failed to spawn hook supervisor");
}

/// Signal the hook thread to uninstall hooks and exit. Sets HOOK_SHUTDOWN
/// first so the supervisor (PROBLEM 82) knows this exit is deliberate and
/// does not restart the thread.
pub fn stop_hook() {
    HOOK_SHUTDOWN.store(true, Ordering::SeqCst);
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW;
        let tid = HOOK_THREAD_ID.load(Ordering::Relaxed) as u32;
        if tid != 0 {
            let _ = PostThreadMessageW(tid, 0x0012 /*WM_QUIT*/, None, None);
        }
    }
}

// ---------------------------------------------------------------------------
// Hook thread main body
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn hook_thread_main(tx: Sender<HookEvent>) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, KillTimer, SetTimer, TranslateMessage,
        UnhookWindowsHookEx, MSG, WM_TIMER,
    };
    use windows::Win32::System::Threading::GetCurrentThreadId;

    // Store sender in thread-local
    EVENT_TX.with(|cell| *cell.borrow_mut() = Some(tx));

    unsafe {
        HOOK_THREAD_ID.store(GetCurrentThreadId() as u64, Ordering::Relaxed);

        // PROBLEM 134 - raise this thread above the UI.
        //
        // Windows evicts a low-level hook whose callback does not RETURN inside
        // LowLevelHooksTimeout (1000ms cap since Win10 1709). That deadline is
        // wall-clock: a callback that is merely waiting for a CPU slice misses
        // it exactly like a slow one. At normal priority this thread competes with
        // WebView2's renderer, and on this owner's machine the overlay runs in
        // SOFTWARE mode (--disable-gpu, GPU composition is dead here), so the
        // dashboard is composited on the CPU - a 1766x964 window since PROBLEM
        // 123 grew it to 92% of the work area. The busiest moment is precisely
        // when that window is focused, which is precisely the owner's report:
        // "while I am using the app, nothing fires."
        //
        // ABOVE_NORMAL, deliberately not TIME_CRITICAL: this thread must beat a
        // rendering pass, not the kernel. The callback is bounded work (atomics
        // and a channel send), so it cannot monopolise anything even if it is
        // scheduled aggressively.
        {
            use windows::Win32::System::Threading::{
                GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL,
            };
            match SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL) {
                Ok(()) => log::info!(
                    "hook: thread priority raised to ABOVE_NORMAL so a WebView2 render                      pass cannot starve the callback past LowLevelHooksTimeout                      (PROBLEM 134)"
                ),
                Err(e) => log::warn!(
                    "hook: could not raise thread priority ({e}) - continuing at normal                      priority; a heavy UI frame may still evict the hook (PROBLEM 134)"
                ),
            }
        }

        // PROBLEM 66 — SetWindowsHookExW used to be .expect()ed: on a machine
        // where install fails (AV/policy blocking global hooks), the hook
        // thread PANICKED silently and the app sat in the tray doing nothing,
        // with the dashboard still claiming everything was fine.
        let (mut kb_hook, mut ms_hook) = install_hooks();
        if kb_hook.is_invalid() {
            log::error!(
                "hook: SetWindowsHookExW(WH_KEYBOARD_LL) FAILED — Space+key cannot work. \
                 Usually security software or policy blocking global hooks. \
                 The watchdog will keep retrying."
            );
        } else {
            log::info!("hook: WH_KEYBOARD_LL + WH_MOUSE_LL installed");
        }

        // PROBLEM 65 — the eviction watchdog. Windows silently EVICTS a
        // low-level hook whose callback overruns LowLevelHooksTimeout (300ms
        // default); nothing tells us, GetMessageW keeps pumping an empty
        // queue, and Space+key just dies while the log looks healthy. A hook
        // proc only fires on the thread that installed it, so the reinstall
        // must happen HERE — a thread-queue timer wakes the blocking pump.
        //
        // NULL-hwnd SetTimer IGNORES the id you pass and returns a fresh
        // system id; WM_TIMER carries THAT id. Compare against the RETURN
        // VALUE or the watchdog silently never fires.
        let timer_id = SetTimer(None, 0, 3000, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_TIMER && msg.wParam.0 == timer_id {
                watchdog_check(&mut kb_hook, &mut ms_hook);
                // PROBLEM 132 - returning here IS the repair, not a failure.
                // HOOK_SHUTDOWN stays false, and that is precisely what tells
                // the supervisor this exit was not deliberate, so it rebuilds
                // the thread immediately with a fresh message queue.
                if ESCALATE_RESTART.swap(false, Ordering::Relaxed) {
                    let _ = KillTimer(None, timer_id);
                    let _ = UnhookWindowsHookEx(kb_hook);
                    let _ = UnhookWindowsHookEx(ms_hook);
                    HOOK_INSTALLED.store(false, Ordering::Relaxed);
                    return;
                }
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = KillTimer(None, timer_id);
        UnhookWindowsHookEx(kb_hook).ok();
        UnhookWindowsHookEx(ms_hook).ok();
        HOOK_INSTALLED.store(false, Ordering::Relaxed);
        log::info!("hook: hooks removed, thread exiting");
    }
}

/// Install (or re-install) both hooks. Never panics: a failed install leaves
/// an invalid HHOOK for the watchdog to retry. Updates HOOK_INSTALLED so the
/// dashboard reports the truth.
#[cfg(windows)]
unsafe fn install_hooks() -> (
    windows::Win32::UI::WindowsAndMessaging::HHOOK,
    windows::Win32::UI::WindowsAndMessaging::HHOOK,
) {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowsHookExW, WINDOWS_HOOK_ID};
    let kb = SetWindowsHookExW(WINDOWS_HOOK_ID(WH_KEYBOARD_LL), Some(kb_hook_proc), None, 0)
        .unwrap_or_default();
    let ms = SetWindowsHookExW(WINDOWS_HOOK_ID(WH_MOUSE_LL), Some(ms_hook_proc), None, 0)
        .unwrap_or_default();
    HOOK_INSTALLED.store(!kb.is_invalid(), Ordering::Relaxed);
    let now = tick_count();
    LAST_KB_EVENT.store(now, Ordering::Relaxed);
    LAST_MS_EVENT.store(now, Ordering::Relaxed);
    (kb, ms)
}

/// Milliseconds since the OS last saw ANY user input (keyboard or mouse).
/// GetLastInputInfo reports in 32-bit GetTickCount space — compare there,
/// never against GetTickCount64.
#[cfg(windows)]
fn millis_since_last_input() -> u64 {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    unsafe {
        let mut lii = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut lii).as_bool() {
            (GetTickCount() as u64).wrapping_sub(lii.dwTime as u64) & 0xFFFF_FFFF
        } else {
            u64::MAX
        }
    }
}

/// PROBLEM 65 — decide whether the hooks were silently evicted, and reinstall.
///
/// Runs in the message pump on the WM_TIMER branch (every 3s), NEVER inside a
/// hook callback, so the Win32 calls and any logging here cannot trip
/// LowLevelHooksTimeout. Logging happens only AFTER the dead hooks are
/// unhooked, so a slow disk write can never delay a still-live callback.
///
/// Two rules, because the hooks die independently:
/// 1. BOTH silent >8s while the OS saw input <2s ago → both evicted (or the
///    callback overran and took the pair down). Fast reinstall.
/// 2. Keyboard silent >120s while the MOUSE hook is provably alive (<8s) and
///    the OS saw input <2s ago → the keyboard hook alone was evicted. The
///    long window exists because "mouse active, no typing" is a normal way
///    to read a page; the price of the occasional false positive is a sub-ms
///    unhook/rehook, which is harmless.
/// PROBLEM 132 — WHICH window has the foreground, by name.
///
/// The watchdog used to log "Usually means an elevated window has focus
/// (UIPI)" on every alarm. That sentence is wrong by construction: the code
/// immediately above it RULES ELEVATION OUT before it can be reached. Weeks of
/// investigation went past this line and believed it. A log that asserts a
/// cause the code already excluded is worse than one that says nothing —
/// it is a signpost pointing away from the answer.
///
/// Safe here and ONLY here: this runs on the WM_TIMER branch of the pump, not
/// in a hook callback, so an OpenProcess round-trip cannot trip
/// LowLevelHooksTimeout.
#[cfg(windows)]
fn foreground_desc() -> String {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return "<none - secure desktop or desktop switch>".to_string();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        if pid == 0 {
            return "<foreground window reports no pid>".to_string();
        }
        let is_self = pid == std::process::id();
        let mut name = String::new();
        // LIMITED, not PROCESS_QUERY_INFORMATION: this one SUCCEEDS against an
        // elevated process from medium integrity, which is the point — we want
        // the name even when the window is the reason we are deaf.
        if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut buf = [0u16; 260];
            let mut len = buf.len() as u32;
            if QueryFullProcessImageNameW(
                h,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok()
            {
                let full = String::from_utf16_lossy(&buf[..len as usize]);
                name = full.rsplit(std::path::MAIN_SEPARATOR).next().unwrap_or("").to_string();
            }
            let _ = CloseHandle(h);
        }
        if name.is_empty() {
            name = format!("pid {pid}");
        }
        if is_self {
            format!("{name} <- SPACEADOM'S OWN WINDOW")
        } else {
            format!("{name} (pid {pid})")
        }
    }
}

#[cfg(windows)]
unsafe fn watchdog_check(
    kb: &mut windows::Win32::UI::WindowsAndMessaging::HHOOK,
    ms: &mut windows::Win32::UI::WindowsAndMessaging::HHOOK,
) {
    use windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx;

    // PROBLEM 134 - sample the foreground here, on the timer, so the hook
    // callback never has to. This branch already runs Win32 calls safely.
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        let fg = GetForegroundWindow();
        let mut is_self = false;
        if !fg.0.is_null() {
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut pid));
            is_self = pid == std::process::id();
        }
        FG_IS_SELF.store(is_self, Ordering::Relaxed);
    }

    let user_input_ms = millis_since_last_input();
    if user_input_ms >= 2_000 {
        // PROBLEM 101 — THE ROOT CAUSE OF 260 FALSE ALARMS.
        //
        // Returning early is not enough. The silence CLOCKS keep running while
        // the user is away — asleep, reading, out of the room — so the moment
        // they touch the mouse again the watchdog compares a stale keyboard
        // timer against a fresh "user is active" signal and concludes the hook
        // must be dead. Measured: "kb hook silent 1825375ms / mouse 547ms" —
        // 30 minutes of not typing, mouse alive half a second ago.
        //
        // Idle time is not evidence about the hook, so it must not accumulate.
        // Re-stamping here means silence is only ever counted while the user
        // was actually PRESENT, which is the only silence that means anything.
        let now = tick_count();
        LAST_KB_EVENT.store(now, Ordering::Relaxed);
        LAST_MS_EVENT.store(now, Ordering::Relaxed);
        return;
    }

    // PROBLEM 78 — while an ELEVATED window has focus (UAC prompt, admin
    // terminal, an installer), Windows UIPI delivers NOTHING to a
    // non-elevated hook, but GetLastInputInfo still updates — the user is
    // typing into the elevated window. That is the app's documented,
    // accepted limitation, NOT an eviction, and it produced a storm of
    // reinstalls at ERROR level during this machine's own install sessions
    // ("kb silent 9000ms / mouse 9000ms, user active 0ms ago" × 7).
    // Discriminator: OpenProcess(PROCESS_QUERY_INFORMATION) on the
    // foreground process FAILS with access denied from medium integrity
    // against an elevated process (the LIMITED flavour would succeed —
    // deliberately not used here).
    {
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION};
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        let fg = GetForegroundWindow();
        // PROBLEM 101 — a NULL foreground window is the UAC secure desktop (or
        // a desktop switch). The hook is deaf there BY DESIGN, and this block
        // had no `else`, so null fell straight through to the eviction verdict
        // — turning every UAC prompt into a false alarm plus a pointless
        // reinstall. Treat it like elevated focus: expected silence.
        if fg.0.is_null() {
            return;
        }
        {
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut pid));
            // PROBLEM 132 — when the foreground IS us, the elevation test below
            // is skipped and we fall through to "evicted". That is the owner's
            // exact repeated symptom, so COUNT it rather than losing it.
            if pid != 0 && pid == std::process::id() {
                BLIND_WHILE_OWN_FG.fetch_add(1, Ordering::Relaxed);
            }
            if pid != 0 && pid != std::process::id() {
                match OpenProcess(PROCESS_QUERY_INFORMATION, false, pid) {
                    Ok(h) => {
                        let _ = windows::Win32::Foundation::CloseHandle(h);
                    }
                    Err(_) => return, // elevated focus — UIPI silence is normal
                }
            }
        }
    }

    let now = tick_count();

    // PROBLEM 78 — cooldown. A real eviction is fixed by ONE reinstall; if
    // silence persists after that, reinstalling again 3s later cannot help
    // and a repeating cause (UIPI edge, another hook ahead of us swallowing
    // events) turns the watchdog into an ERROR-spam loop. One reinstall per
    // minute is fast enough for real evictions and bounds the noise.
    let last = WATCHDOG_LAST_REINSTALL.load(Ordering::Relaxed);
    if last != 0 && now.saturating_sub(last) < 60_000 {
        return;
    }

    let kb_silence = now.saturating_sub(LAST_KB_EVENT.load(Ordering::Relaxed));
    let ms_silence = now.saturating_sub(LAST_MS_EVENT.load(Ordering::Relaxed));

    // PROBLEM 101 — the `kb_dead` branch is DELETED. It read:
    //     kb_dead = kb_silence > 120_000 && ms_silence < 8_000
    // i.e. "the mouse hook is delivering but nobody has typed for 2 minutes",
    // which it treated as proof the keyboard hook had died. That is simply a
    // description of reading a page. It accounted for 95 of 255 alarms; for
    // those, the MEASURED median mouse silence was 79ms and 78 of 95 had a
    // mouse event within one second — the hooks were provably installed and
    // delivering at the instant it declared them dead. The branch has zero
    // power to distinguish an evicted hook from a person not typing, so no
    // threshold could have rescued it.
    //
    // `both_dead` survives: neither hook seeing anything for 8s while the user
    // is demonstrably active IS anomalous. Note it usually means UIPI deafness
    // (an elevated window has focus and this app runs unelevated) rather than
    // eviction — a reinstall cannot cure that, which is why the log line below
    // no longer claims it can.
    let both_dead = kb_silence > 8_000 && ms_silence > 8_000;
    if !both_dead {
        // Events are arriving: whatever was wrong has cleared. Reset the
        // streak so escalation only ever fires for CONTINUOUS blindness.
        BLIND_REINSTALLS.store(0, Ordering::Relaxed);
        return;
    }
    WATCHDOG_LAST_REINSTALL.store(now, Ordering::Relaxed);

    // Unhook FIRST (dead handles unhook harmlessly), log after — the old
    // hooks are gone by the time the disk write happens.
    let _ = UnhookWindowsHookEx(*kb);
    let _ = UnhookWindowsHookEx(*ms);
    // A mid-hold eviction must not leave the Space latch stuck.
    MODIFIER_ACTIVE.store(false, Ordering::Relaxed);
    SPACE_INTERCEPTED.store(false, Ordering::Relaxed);
    SPACE_ABORTED.store(false, Ordering::Relaxed);

    let fg = foreground_desc();
    let (nkb, nms) = install_hooks();
    *kb = nkb;
    *ms = nms;
    HOOK_REINSTALLS.fetch_add(1, Ordering::Relaxed);
    // PROBLEM 101 — WARN, not ERROR, and it no longer asserts a cause it
    // cannot know. 260 of these were logged at ERROR in two days with not one
    // demonstrable eviction among them, which made the log's error channel
    // useless for finding real faults. It also claimed "silent eviction" as
    // fact; the likelier cause is UIPI deafness (an elevated window has focus
    // while this app runs unelevated), which a reinstall cannot fix. Say what
    // was OBSERVED and leave the diagnosis open.
    log::warn!(
        "hook: WATCHDOG — user active {user_input_ms}ms ago but NEITHER hook saw anything \
         (kb {kb_silence}ms / mouse {ms_silence}ms). Foreground: {fg}. Elevation was \
         ALREADY ruled out above, so this is NOT UIPI. Re-hooking. reinstall ok: {}",
        !nkb.is_invalid()
    );

    // PROBLEM 132 - ESCALATE. Re-hooking was the ONLY repair this watchdog
    // had, and on 2026-08-17 it ran for 20 unbroken minutes: one alarm a
    // minute, every one reporting `reinstall ok: true`, while the owner had no
    // shortcuts at all. That "ok" only means SetWindowsHookEx returned a
    // handle. It says nothing about whether events will ARRIVE, because a hook
    // proc fires on the thread that INSTALLED it - so if this thread's message
    // pump is what is wedged, a fresh hook on the same wedged pump is a fresh
    // hook that never fires. Repeating it every minute forever is a repair
    // that cannot work, logging success each time.
    //
    // After two consecutive blind reinstalls (~2 min) take the bigger move and
    // end this thread. The PROBLEM 82 supervisor reads an unexpected pump exit
    // as a crash and rebuilds it from scratch - new thread, new message queue,
    // new hooks - which is the only repair that survives a wedged pump. That
    // supervisor's own 5-restarts-per-10-minutes cap bounds this, so escalation
    // cannot become a spin loop.
    let streak = BLIND_REINSTALLS.fetch_add(1, Ordering::Relaxed) + 1;
    if streak >= 2 {
        BLIND_REINSTALLS.store(0, Ordering::Relaxed);
        let own = BLIND_WHILE_OWN_FG.swap(0, Ordering::Relaxed);
        log::error!(
            "hook: {streak} reinstalls in a row and STILL no events - re-hooking has failed, \
             so the ENTIRE hook thread is being restarted (fresh message pump). \
             Foreground: {fg}. Alarms while Spaceadom's OWN window had focus: {own}."
        );
        ESCALATE_RESTART.store(true, Ordering::Relaxed);
    }
}

#[cfg(not(windows))]
fn hook_thread_main(_tx: Sender<HookEvent>) {
    log::warn!("hook: non-Windows platform — keyboard hook is a no-op");
}

// ---------------------------------------------------------------------------
// Keyboard HOOKPROC
// ---------------------------------------------------------------------------

#[cfg(windows)]
unsafe extern "system" fn kb_hook_proc(
    n_code: i32,
    w_param: windows::Win32::Foundation::WPARAM,
    l_param: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{CallNextHookEx, KBDLLHOOKSTRUCT};
    use windows::Win32::Foundation::LRESULT;

    if n_code < 0 {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    let ks = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
    let vk = ks.vkCode as u16;
    let msg = w_param.0 as u32;
    let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
    let now = tick_count();
    // PROBLEM 65 — liveness stamp for the eviction watchdog. One lock-free
    // store; runs for EVERY callback (even injected/bypassed events count —
    // being called at all is the proof of life).
    LAST_KB_EVENT.store(now, Ordering::Relaxed);

    const MAGIC_INJECTED: usize = 0x7A7A7A7A;
    // --- Ignore OUR OWN synthetic inputs to prevent infinite loops ---
    //
    // NOTE: we deliberately test only our magic cookie, NOT the generic
    // LLKHF_INJECTED flag. Blanket-ignoring LLKHF_INJECTED silently disables
    // SpaceToggle for anyone using AutoHotkey, macro keyboards, the on-screen
    // keyboard, Remote Desktop, or laptop drivers that stamp INJECTED onto
    // genuinely physical keystrokes. The magic cookie is sufficient to break
    // the feedback loop, because every key we synthesise carries it.
    if ks.dwExtraInfo == MAGIC_INJECTED {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    // ===================================================================
    // NO log:: CALLS BEYOND THIS POINT — PROBLEM 58, and it broke the app.
    //
    // log4rs writes SYNCHRONOUSLY to a file. Doing that inside a
    // WH_KEYBOARD_LL callback puts disk I/O on the hook path, and Windows
    // enforces `LowLevelHooksTimeout` (300ms default, HKCU\Control Panel\
    // Desktop): a callback that overruns it gets the hook SILENTLY EVICTED.
    // The process keeps running, the startup line still says "hooks
    // installed", and every keystroke simply stops arriving.
    //
    // That is exactly what happened: PROBLEM 48 added eight log::info! calls
    // here to explain suppressions. On a fast SSD dev machine the writes
    // absorbed fine; on a tester's Vostro 5471 they did not, and Space+key
    // and the Guide HUD both died while the log looked perfectly healthy.
    // logger.rs line 43 already warned about this in writing.
    //
    // Diagnostics here MUST be lock-free atomics only. The engine thread
    // reads them and does the logging safely off the hook path.
    // ===================================================================

    // PROBLEM 104 — count EVERY key before any decision. Two atomics and one
    // GetForegroundWindow/GetWindowThreadProcessId pair; no allocation, no
    // lock, no logging, so the callback still returns in microseconds.
    KB_EVENTS_SEEN.fetch_add(1, Ordering::Relaxed);
    {
        // PROBLEM 134 - this used to call GetForegroundWindow +
        // GetWindowThreadProcessId HERE, on every keystroke. Both are Win32
        // window queries, and the rule for this callback (skill reference
        // win32-keyboard-hook.md, section 2) is absolute: "read the event,
        // check your dwExtraInfo tag, consult an atomic or lock-free
        // structure, decide pass-or-suppress, return." Window queries are not
        // on that list. They enter win32k and contend on USER32 state that the
        // foreground application's UI thread also touches - so the cost is
        // paid exactly when that thread is busiest, which is when OUR OWN
        // dashboard is focused and rendering.
        //
        // The old comment claimed "no allocation, no lock, no logging, so the
        // callback still returns in microseconds" and was believed for that
        // reason. It counted the wrong costs: the lock it takes is inside the
        // window manager, not in our code.
        //
        // Now a plain atomic read. FG_IS_SELF is refreshed off the hook path,
        // on the watchdog's WM_TIMER branch - stale by up to 3s, which is
        // irrelevant for a per-minute diagnostic counter and free here.
        if FG_IS_SELF.load(Ordering::Relaxed) {
            KB_EVENTS_OWN_FG.fetch_add(1, Ordering::Relaxed);
        }
    }

    // --- Fullscreen: pass everything through immediately ---
    if FULLSCREEN_ACTIVE.load(Ordering::Relaxed) {
        SUPPRESS_FULLSCREEN.fetch_add(1, Ordering::Relaxed);
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    // --- Bypass Mode: pass everything through immediately (except Space + .) ---
    if BYPASS_MODE.load(Ordering::Relaxed) {
        if is_down {
            SUPPRESS_BYPASS.fetch_add(1, Ordering::Relaxed);
        }
        // Still allow Space + . to toggle bypass mode OFF!
        if vk == VK_OEM_PERIOD && is_down && (windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(VK_SPACE as i32) as u16 & 0x8000) != 0 {
            send_event(HookEvent::KeyCombo(KeyCombo::Period));
            return LRESULT(1);
        }
        return CallNextHookEx(None, n_code, w_param, l_param);
    }


    // --- Track last alpha-key timestamp for rollover window ---
    if is_down && is_alpha_or_digit(vk) {
        LAST_ALPHA_TS.store(now, Ordering::Relaxed);
        if MODIFIER_ACTIVE.load(Ordering::Relaxed) {
            // Modifier is active — this key should be intercepted as a combo
            // (handled in the combo dispatch below)
        }
    }

    // ===================================================================
    // SPACE DOWN
    // ===================================================================
    if vk == VK_SPACE && is_down {
        // Ctrl+Space, Alt+Space and Win+Space are real OS/app shortcuts
        // (IME switch, IDE autocomplete, window menu, layout switcher).
        // Never swallow them — hand them straight to the OS.
        if other_modifier_down() {
            return CallNextHookEx(None, n_code, w_param, l_param);
        }
        if !MODIFIER_ACTIVE.load(Ordering::Relaxed) {
            MODIFIER_ACTIVE.store(true, Ordering::Relaxed);
            SPACE_ABORTED.store(false, Ordering::Relaxed);
            SPACE_DOWN_TS.store(now, Ordering::Relaxed);
            send_event(HookEvent::SpaceDown);
        }
        SPACE_INTERCEPTED.store(true, Ordering::Relaxed);
        // Always suppress Space down to prevent auto-repeat leaking to the OS
        return LRESULT(1);
    }

    // ===================================================================
    // SPACE UP
    // ===================================================================
    if vk == VK_SPACE && is_up {
        // If we never swallowed the matching down-stroke, this up-stroke
        // belongs to the OS. Injecting here would duplicate the space.
        if !SPACE_INTERCEPTED.swap(false, Ordering::Relaxed) {
            return CallNextHookEx(None, n_code, w_param, l_param);
        }

        let modifier_fired = MODIFIER_ACTIVE.load(Ordering::Relaxed);
        MODIFIER_ACTIVE.store(false, Ordering::Relaxed);

        // If no modifier action was taken, pass a real Space through
        if !SPACE_ABORTED.load(Ordering::Relaxed) {
            inject_space();
        }

        send_event(HookEvent::SpaceUp { modifier_fired });
        return LRESULT(1);
    }

    // ===================================================================
    // COMBO KEYS (only when modifier is active)
    // ===================================================================
    if MODIFIER_ACTIVE.load(Ordering::Relaxed) && is_down {
        // --- FAILSAFE: has the modifier been latched on for an absurd time? ---
        //
        // THIS USED TO CALL GetAsyncKeyState(VK_SPACE) AND IT BROKE EVERY
        // SHORTCUT IN THE APP. Do not put it back. Reason:
        //
        // We suppress Space-down by returning LRESULT(1), so the keystroke
        // never propagates and Windows never marks Space as pressed in its
        // key-state table. GetAsyncKeyState therefore reports Space as UP even
        // while the user is physically holding it. The guard concluded
        // "modifier stuck", reset it, and let the letter through as plain
        // typing — so Space+F typed "f" instead of opening Explorer, every
        // single time. Confirmed on real hardware: the log filled with
        // "MODIFIER_ACTIVE stuck. Auto-correcting." on genuine keypresses.
        //
        // We are the only component that knows Space is down, because we are
        // the one hiding it. So trust our own bookkeeping and bound it by time
        // instead. The original worry was a dropped Space-UP event latching the
        // modifier on forever; a timeout covers that without lying about the
        // key state.
        // Generous on purpose: people hold Space and READ the guide HUD.
        // A latched modifier only mistypes until the user taps Space again,
        // so err on the side of never interrupting a real hold.
        const MAX_MODIFIER_HOLD_MS: u64 = 30_000;
        let latched_ms = now.saturating_sub(SPACE_DOWN_TS.load(Ordering::Relaxed));
        if latched_ms > MAX_MODIFIER_HOLD_MS {
            STUCK_MODIFIER.fetch_add(1, Ordering::Relaxed);
            MODIFIER_ACTIVE.store(false, Ordering::Relaxed);
            SPACE_INTERCEPTED.store(false, Ordering::Relaxed);
            return CallNextHookEx(None, n_code, w_param, l_param);
        }
        // Check rollover: if the alpha key hit within rollover_ms of Space↓,
        // treat it as normal typing — abort modifier and pass both keys through.
        let space_ts = SPACE_DOWN_TS.load(Ordering::Relaxed);
        let rollover = ROLLOVER_MS.load(Ordering::Relaxed);
        // saturating_sub: SPACE_DOWN_TS can legitimately be 0 or stale if
        // MODIFIER_ACTIVE was forced on by a path that never stamped it.
        let held_ms = now.saturating_sub(space_ts);
        let in_rollover = rollover > 0 && is_alpha_or_digit(vk) && held_ms < rollover;

        if in_rollover {
            // Typing rollover — this is prose, not a command.
            //
            // LOG IT (PROBLEM 48). This path silently turns an intended
            // shortcut into typed text. If a user's rollover_ms is set too
            // high for how fast they press, EVERY shortcut lands here and the
            // app looks completely dead while the log stays empty. Logged at
            // info with the actual numbers so the cause is a one-line read;
            // the first 5 and then every 20th, to bound the volume for a fast
            // typist while still showing the pattern.
            ROLLOVER_HITS.fetch_add(1, Ordering::Relaxed);
            record_margin(&MARGIN_TYPED, held_ms); // PROBLEM 95
            MODIFIER_ACTIVE.store(false, Ordering::Relaxed);
            SPACE_ABORTED.store(true, Ordering::Relaxed);

            // Emit BOTH keystrokes ourselves in a single atomic SendInput
            // batch, and suppress the original.
            //
            // The previous version injected the space and then let the real
            // key through via CallNextHookEx. That races: the real key is
            // already being delivered on this hook thread, while our injected
            // space goes to the BACK of the input queue — so fast typists got
            // "hte" instead of "the". Ordering is only guaranteed if we own
            // both events.
            inject_space_then_key(vk);
            return LRESULT(1);
        }

        // --- Map VK to combo variant ---
        let combo_opt: Option<KeyCombo> = match vk {
            VK_ESCAPE => Some(KeyCombo::Escape),
            VK_OEM_3 => Some(KeyCombo::Backtick),
            VK_OEM_COMMA => Some(KeyCombo::Comma),
            VK_OEM_PERIOD => Some(KeyCombo::Period),

            VK_RMENU => Some(KeyCombo::RightAlt),
            VK_UP    => Some(KeyCombo::UpArrow),
            VK_DOWN  => Some(KeyCombo::DownArrow),
            VK_BACK  => Some(KeyCombo::Backspace),
            v if is_alpha_vk(v) => {
                let ch = vk_to_char(v);
                ch.map(KeyCombo::Alpha)
            }
            // Special keys (F1–F12, Enter, Tab, Left, Right) —
            // only dispatch if the user has bound them in special_keys config.
            VK_RETURN => Some(KeyCombo::Special("enter".into())),
            VK_TAB    => Some(KeyCombo::Special("tab".into())),
            VK_LEFT   => Some(KeyCombo::Special("left".into())),
            VK_RIGHT  => Some(KeyCombo::Special("right".into())),
            v if (VK_F1..=VK_F12).contains(&v) => {
                let n = v - VK_F1 + 1;
                Some(KeyCombo::Special(format!("f{n}")))
            }
            _ => None,
        };

        if let Some(combo) = combo_opt {
            // NO LOGGING HERE — see the PROBLEM 58 banner above. The engine
            // logs "combo Space+X received" the moment it handles this event.
            record_margin(&MARGIN_COMMAND, held_ms); // PROBLEM 95
            SPACE_ABORTED.store(true, Ordering::Relaxed);
            send_event(HookEvent::KeyCombo(combo));
            return LRESULT(1); // suppress key
        }

        if is_alpha_or_digit(vk) {
            UNMAPPED_KEYS.fetch_add(1, Ordering::Relaxed);
        }
    }

    // Default: pass through
    CallNextHookEx(None, n_code, w_param, l_param)
}

// ---------------------------------------------------------------------------
// Mouse HOOKPROC (wheel events)
// ---------------------------------------------------------------------------

#[cfg(windows)]
unsafe extern "system" fn ms_hook_proc(
    n_code: i32,
    w_param: windows::Win32::Foundation::WPARAM,
    l_param: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{CallNextHookEx, MSLLHOOKSTRUCT};
    use windows::Win32::Foundation::LRESULT;

    if n_code < 0 {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }
    // PROBLEM 65 — liveness stamp (see kb_hook_proc). Must be BEFORE the
    // MODIFIER_ACTIVE early-return or the watchdog only sees mouse life
    // while Space is held.
    LAST_MS_EVENT.store(tick_count(), Ordering::Relaxed);
    if !MODIFIER_ACTIVE.load(Ordering::Relaxed) {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    if w_param.0 as u32 == WM_MOUSEWHEEL {
        let ms = &*(l_param.0 as *const MSLLHOOKSTRUCT);
        let delta = (ms.mouseData >> 16) as i16;
        SPACE_ABORTED.store(true, Ordering::Relaxed);
        if delta > 0 {
            send_event(HookEvent::WheelUp);
        } else {
            send_event(HookEvent::WheelDown);
        }
        return LRESULT(1); // suppress scroll
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn send_event(event: HookEvent) {
    EVENT_TX.with(|cell| {
        if let Some(tx) = cell.borrow().as_ref() {
            if let Err(e) = tx.try_send(event) {
                // Counter, not a log call: send_event() runs ON THE HOOK PATH,
                // and this fires precisely when the system is already under
                // load — the worst possible moment to add disk I/O and get the
                // hook evicted (PROBLEM 58). Reported by drain_hook_diagnostics.
                let _ = e;
                DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

/// Build one synthetic keyboard INPUT stamped with our magic cookie so the
/// hook recognises it as self-generated and passes it through.
#[cfg(windows)]
fn kbd_input(vk: u16, keyup: bool) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: if keyup { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0x7A7A7A7A,
            },
        },
    }
}

/// Inject a synthetic Space key press+release via SendInput.
#[cfg(windows)]
unsafe fn inject_space() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};
    let inputs = [kbd_input(VK_SPACE, false), kbd_input(VK_SPACE, true)];
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
}

/// Emit Space followed by `vk` as ONE ordered SendInput batch.
/// Used by the typing-rollover path, where ordering must be guaranteed.
#[cfg(windows)]
unsafe fn inject_space_then_key(vk: u16) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};
    let inputs = [
        kbd_input(VK_SPACE, false),
        kbd_input(VK_SPACE, true),
        kbd_input(vk, false),
        kbd_input(vk, true),
    ];
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
}

#[cfg(not(windows))]
unsafe fn inject_space() {}

#[cfg(not(windows))]
unsafe fn inject_space_then_key(_vk: u16) {}

/// True if Ctrl, Alt or Win is physically held right now.
///
/// Shift is deliberately excluded: Shift+Space is a plain space in most apps,
/// and treating it as pass-through would break the modifier while capitalising.
fn other_modifier_down() -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        const VK_CONTROL: i32 = 0x11;
        const VK_MENU: i32 = 0x12; // Alt
        const VK_LWIN: i32 = 0x5B;
        const VK_RWIN: i32 = 0x5C;
        [VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
            .iter()
            .any(|&k| (GetAsyncKeyState(k) as u16 & 0x8000) != 0)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// PROBLEM 99 — the monotonic clock, for callers outside this module (the
/// undo buffer needs an age). Deliberately the SAME source the hook uses, so
/// timings recorded here can be compared with hook timings directly.
pub fn tick_count_pub() -> u64 {
    tick_count()
}

fn tick_count() -> u64 {
    #[cfg(windows)]
    unsafe {
        windows::Win32::System::SystemInformation::GetTickCount64()
    }
    #[cfg(not(windows))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

fn is_alpha_or_digit(vk: u16) -> bool {
    (0x41..=0x5A).contains(&vk) // A–Z
        || (0x30..=0x39).contains(&vk) // 0–9
}

fn is_alpha_vk(vk: u16) -> bool {
    (0x41..=0x5A).contains(&vk)
}

fn vk_to_char(vk: u16) -> Option<char> {
    if (0x41..=0x5A).contains(&vk) {
        char::from_u32((vk as u32) + 32) // map A→a, etc.
    } else {
        None
    }
}
