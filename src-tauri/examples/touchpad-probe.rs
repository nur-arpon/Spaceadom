//! Touchpad T1 — the hardware proof. READ-ONLY.
//!
//!     cargo run --release --example touchpad-probe            # the probe
//!     cargo run --release --example touchpad-probe -- --list  # devices + caps only, no log
//!     cargo run --release --example touchpad-probe -- --band 0.15
//!     cargo run --release --example touchpad-probe -- --fingers 1 --edges right
//!
//! T1b: `--fingers N` (default 2) sets how many contacts arm a band. N=2 is
//! the T1 rule (two tips, same edge, moving). N=1 and N=3 arm ON LANDING:
//! the band goes live the moment exactly N contacts are down and every one
//! of them FIRST touched inside the band (a contact that lands outside and
//! drifts in never arms), before any movement — and while live the hook
//! eats WM_MOUSEMOVE as well as wheel messages, so the pointer holds still.
//! Per entry it prints the cursor at entry/exit and the furthest the cursor
//! got during the hold. `--edges right|left|top|bottom[,..]` restricts the
//! bands (default all four).
//!
//! Mechanism 1: a message-only raw-input sink (`touchpad::raw::run_sink`)
//! parses every Precision Touchpad HID report and prints the contacts.
//! Mechanism 2: a `WH_MOUSE_LL` hook on a SECOND thread returns `LRESULT(1)`
//! for WM_MOUSEWHEEL / WM_MOUSEHWHEEL while `raw::BAND_LIVE` is set (two
//! tips, both in one edge band, moving). It counts eaten vs passed ticks and
//! prints both every 5 s with the start/end leak figures.
//!
//! It never injects input, never moves the cursor, never changes
//! brightness or volume, never touches config.json. Output goes to stdout
//! and to `%APPDATA%\Spaceadom\touchpad-probe.log`, at most 20 lines/s.
//! Ctrl+C to stop.
#![cfg(windows)]

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use std::collections::HashMap;

use space_toggle_os_lib::touchpad::raw::{self, AxisCaps, Contact, DeviceInfo, Edge, Report};
use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetCursorPos, GetMessageW, SetWindowsHookExW,
    TranslateMessage, MSG, WH_MOUSE_LL, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
};

// --- Mechanism 2 counters: written by the LL callback, read by the summary.
static WHEEL_EATEN: AtomicU64 = AtomicU64::new(0);
static WHEEL_PASSED: AtomicU64 = AtomicU64::new(0);
static MOUSE_EVENTS: AtomicU64 = AtomicU64::new(0);
/// Tick of the most recent PASSED wheel tick (0 = none).
static LAST_PASSED_AT: AtomicU64 = AtomicU64::new(0);
/// Tick of the first EATEN wheel tick since the band last went live (0 = none yet).
static FIRST_EATEN_AT: AtomicU64 = AtomicU64::new(0);
/// A wheel tick passed within LEAK_WINDOW_MS AFTER the band went live —
/// impossible by construction (the flag was set), kept as a sanity counter.
static LEAK_AFTER_ENTRY: AtomicU64 = AtomicU64::new(0);
/// A wheel tick passed within LEAK_WINDOW_MS after the band went DEAD
/// (the tail of the gesture leaking out as the fingers lift).
static LEAK_AFTER_EXIT: AtomicU64 = AtomicU64::new(0);
static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
/// T1b — set once at start-up when `--fingers` is not 2: while BAND_LIVE the
/// hook eats WM_MOUSEMOVE too, so the pointer must hold still.
static EAT_MOVES: AtomicBool = AtomicBool::new(false);
static MOVES_EATEN: AtomicU64 = AtomicU64::new(0);
const LEAK_WINDOW_MS: u64 = 100;

unsafe extern "system" fn ms_hook_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code < 0 {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }
    MOUSE_EVENTS.fetch_add(1, Ordering::Relaxed);
    let msg = w_param.0 as u32;
    if msg == WM_MOUSEMOVE
        && EAT_MOVES.load(Ordering::Relaxed)
        && raw::BAND_LIVE.load(Ordering::Relaxed)
    {
        MOVES_EATEN.fetch_add(1, Ordering::Relaxed);
        return LRESULT(1);
    }
    if msg == WM_MOUSEWHEEL || msg == WM_MOUSEHWHEEL {
        let now = GetTickCount64();
        if raw::BAND_LIVE.load(Ordering::Relaxed) {
            WHEEL_EATEN.fetch_add(1, Ordering::Relaxed);
            let _ = FIRST_EATEN_AT.compare_exchange(0, now, Ordering::Relaxed, Ordering::Relaxed);
            return LRESULT(1);
        }
        WHEEL_PASSED.fetch_add(1, Ordering::Relaxed);
        LAST_PASSED_AT.store(now, Ordering::Relaxed);
        let left = raw::BAND_LEFT_AT.load(Ordering::Relaxed);
        if left != 0 && now.saturating_sub(left) <= LEAK_WINDOW_MS {
            LEAK_AFTER_EXIT.fetch_add(1, Ordering::Relaxed);
        }
        let entered = raw::BAND_ENTERED_AT.load(Ordering::Relaxed);
        if entered != 0 && now.saturating_sub(entered) <= LEAK_WINDOW_MS && entered > left {
            LEAK_AFTER_ENTRY.fetch_add(1, Ordering::Relaxed);
        }
    }
    CallNextHookEx(None, n_code, w_param, l_param)
}

fn spawn_mouse_hook_thread() {
    std::thread::Builder::new()
        .name("touchpad-probe-mouse-ll".into())
        .spawn(|| unsafe {
            match SetWindowsHookExW(WH_MOUSE_LL, Some(ms_hook_proc), None, 0) {
                Ok(h) if !h.is_invalid() => HOOK_INSTALLED.store(true, Ordering::Relaxed),
                other => {
                    eprintln!("WH_MOUSE_LL could not be installed: {other:?}");
                    return;
                }
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("spawn mouse hook thread");
}

// --- Output: stdout + log file, rate-limited.
struct Out {
    file: Option<File>,
}

static OUT: Mutex<Out> = Mutex::new(Out { file: None });

fn say(line: &str) {
    println!("{line}");
    if let Ok(mut o) = OUT.lock() {
        if let Some(f) = o.file.as_mut() {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn log_path() -> Option<std::path::PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(std::path::PathBuf::from(appdata).join("Spaceadom").join("touchpad-probe.log"))
}

fn describe(d: &DeviceInfo) -> String {
    format!(
        "vid 0x{:04X} pid 0x{:04X} ver 0x{:X} usage 0x{:02X}/0x{:02X} {}",
        d.vendor_id, d.product_id, d.version, d.usage_page, d.usage, d.name
    )
}

fn print_pad(d: &DeviceInfo) {
    say(&format!("device found: {}", describe(d)));
    let Some(p) = d.pad.as_ref() else {
        say("  (no preparsed data — cannot read caps)");
        return;
    };
    say(&format!(
        "  input report {} bytes, {} link collections, {} value caps, {} button caps, finger collections {:?}, contact count max {:?}",
        p.input_report_bytes, p.link_collections, p.input_value_caps, p.input_button_caps,
        p.finger_collections, p.contact_count_max
    ));
    match (&p.x, &p.y) {
        (Some(x), Some(y)) => {
            let mm = p
                .size_mm()
                .map(|(w, h)| format!("{w:.1} x {h:.1} mm"))
                .unwrap_or_else(|| "size in mm unknown (no linear unit in the descriptor)".into());
            say(&format!(
                "  pad size: logical X {}..{} Y {}..{} ({} x {} units); physical X {}..{} Y {}..{} unit 0x{:X} exp {} => {}",
                x.logical_min, x.logical_max, y.logical_min, y.logical_max,
                x.logical_span(), y.logical_span(),
                x.physical_min, x.physical_max, y.physical_min, y.physical_max,
                x.unit, raw::unit_exponent(x.unit_exp), mm
            ));
        }
        _ => say("  X/Y value caps not found inside a Finger collection"),
    }
    say("  input value caps:");
    for l in &p.value_cap_lines {
        say(l);
    }
}

fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    // SAFETY: plain Win32 read of the cursor position.
    let _ = unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

fn dist(a: (i32, i32), b: (i32, i32)) -> f64 {
    (((a.0 - b.0) as f64).powi(2) + ((a.1 - b.1) as f64).powi(2)).sqrt()
}

fn parse_edges(s: &str) -> Vec<Edge> {
    let mut v = Vec::new();
    for part in s.split(',') {
        match part.trim().to_ascii_lowercase().as_str() {
            "right" => v.push(Edge::Right),
            "left" => v.push(Edge::Left),
            "top" => v.push(Edge::Top),
            "bottom" => v.push(Edge::Bottom),
            "all" => return vec![Edge::Left, Edge::Right, Edge::Top, Edge::Bottom],
            other => eprintln!("--edges: unknown edge '{other}' ignored"),
        }
    }
    if v.is_empty() {
        vec![Edge::Left, Edge::Right, Edge::Top, Edge::Bottom]
    } else {
        v
    }
}

/// T1b — arm ON LANDING for `fingers` contacts (1 or 3): live while exactly
/// `fingers` tips are down, every one of them first touched inside an
/// allowed edge band, and all of them are inside the SAME allowed band now.
/// No movement requirement — the band is live from the landing report.
struct LandingTracker {
    fingers: usize,
    band: f64,
    edges: Vec<Edge>,
    x: AxisCaps,
    y: AxisCaps,
    /// Per contact id currently down: the edge it LANDED in (None = landed
    /// outside every allowed band, and stays None until the tip lifts).
    landed: HashMap<u32, Option<Edge>>,
    live: bool,
    edge: Option<Edge>,
}

impl LandingTracker {
    fn edge_at(&self, c: &Contact) -> Option<Edge> {
        raw::edge_of(self.x.fraction(c.x), self.y.fraction(c.y), self.band)
            .filter(|e| self.edges.contains(e))
    }

    fn feed(&mut self, rep: &Report, now: u64) -> bool {
        let tips: Vec<Contact> = rep.tips().copied().collect();
        // Forget lifted contacts; record where new ones landed.
        self.landed.retain(|id, _| tips.iter().any(|c| c.id == *id));
        for c in &tips {
            if !self.landed.contains_key(&c.id) {
                let e = self.edge_at(c);
                self.landed.insert(c.id, e);
            }
        }
        let live = tips.len() == self.fingers && {
            let now_edges: Vec<Option<Edge>> = tips.iter().map(|c| self.edge_at(c)).collect();
            let first = now_edges.first().copied().flatten();
            first.is_some()
                && now_edges.iter().all(|e| *e == first)
                && tips.iter().all(|c| self.landed.get(&c.id).copied().flatten() == first)
        };
        if live {
            self.edge = tips.first().and_then(|c| self.edge_at(c));
        }
        if live != self.live {
            self.live = live;
            if live {
                raw::BAND_ENTERED_AT.store(now, Ordering::Relaxed);
            } else {
                raw::BAND_LEFT_AT.store(now, Ordering::Relaxed);
            }
            raw::BAND_LIVE.store(live, Ordering::Relaxed);
        }
        live
    }
}

/// One tracker per `--fingers` value; N=2 is raw::BandTracker unchanged
/// (with the `--edges` filter laid over its verdict).
enum Arming {
    Two { t: raw::BandTracker, edges: Vec<Edge> },
    Landing(LandingTracker),
}

impl Arming {
    fn feed(&mut self, rep: &Report, now: u64) -> bool {
        match self {
            Arming::Two { t, edges } => {
                let live = t.feed(rep, now);
                if live && !t.edge.is_some_and(|e| edges.contains(&e)) {
                    // A disallowed edge: overrule the verdict for this run.
                    if raw::BAND_LIVE.swap(false, Ordering::Relaxed) {
                        raw::BAND_LEFT_AT.store(now, Ordering::Relaxed);
                    }
                    return false;
                }
                live
            }
            Arming::Landing(t) => t.feed(rep, now),
        }
    }
    fn edge(&self) -> Option<Edge> {
        match self {
            Arming::Two { t, .. } => t.edge,
            Arming::Landing(t) => t.edge,
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let list_only = args.iter().any(|a| a == "--list");
    let mut band = 0.12f64;
    if let Some(i) = args.iter().position(|a| a == "--band") {
        if let Some(v) = args.get(i + 1).and_then(|s| s.parse::<f64>().ok()) {
            band = v.clamp(0.02, 0.5);
        }
    }
    let mut fingers = 2usize;
    if let Some(i) = args.iter().position(|a| a == "--fingers") {
        if let Some(v) = args.get(i + 1).and_then(|s| s.parse::<usize>().ok()) {
            fingers = v.clamp(1, 5);
        }
    }
    let mut edges = parse_edges("all");
    if let Some(i) = args.iter().position(|a| a == "--edges") {
        if let Some(v) = args.get(i + 1) {
            edges = parse_edges(v);
        }
    }
    let eat_moves = fingers != 2;
    EAT_MOVES.store(eat_moves, Ordering::Relaxed);

    if !list_only {
        if let Some(p) = log_path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            match OpenOptions::new().create(true).append(true).open(&p) {
                Ok(f) => {
                    OUT.lock().unwrap().file = Some(f);
                    println!("log: {}", p.display());
                }
                Err(e) => println!("log could not be opened at {}: {e}", p.display()),
            }
        }
    }

    say(&format!(
        "touchpad-probe T1b — read-only — band {:.2} — fingers {} ({}) — edges {:?} — {}",
        band,
        fingers,
        if fingers == 2 { "T1 rule: two tips, same edge, moving; eats wheel" } else { "arm on landing; eats wheel AND WM_MOUSEMOVE while live" },
        edges,
        chrono_free_stamp()
    ));

    let devices = raw::enumerate();
    say(&format!("raw-input HID devices: {}", devices.len()));
    for d in &devices {
        if !d.is_touch_pad() {
            say(&format!("  hid: {}", describe(d)));
        }
    }
    let pads: Vec<&DeviceInfo> = devices.iter().filter(|d| d.is_touch_pad()).collect();
    if pads.is_empty() {
        say("no Precision Touchpad — the feature would show its unavailable state");
        return;
    }
    for p in &pads {
        print_pad(p);
    }
    if list_only {
        return;
    }

    let pad = pads[0].clone();
    let caps = pad.pad.clone().expect("pad caps");
    let (Some(x), Some(y)) = (caps.x.clone(), caps.y.clone()) else {
        say("cannot run: X/Y caps missing — nothing to normalise the band against");
        return;
    };

    spawn_mouse_hook_thread();
    std::thread::sleep(Duration::from_millis(50));
    say(&format!(
        "WH_MOUSE_LL hook: {}",
        if HOOK_INSTALLED.load(Ordering::Relaxed) { "installed on its own thread" } else { "NOT installed (mechanism 2 cannot be measured)" }
    ));
    say(&match fingers {
        1 => "fingers: ONE finger landing inside the edge band, then sliding along it over a long web page. Watch: did the pointer move at all, and did the page scroll?  Ctrl+C to stop.".to_string(),
        2 => "fingers: 1) one finger anywhere  2) two fingers in the middle  3) two fingers sliding UP the RIGHT edge over a long web page  4) two fingers along the TOP edge.  Ctrl+C to stop.".to_string(),
        n => format!("fingers: {n} fingers landing inside the edge band, then sliding along it. Watch: did Task View / the desktop switch / any {n}-finger gesture fire?  Ctrl+C to stop."),
    });

    // Per-report state lives in the closure; the sink calls it on the main
    // thread (the sink's pump runs here).
    let mut tracker = if fingers == 2 {
        Arming::Two { t: raw::BandTracker::new(x, y, band), edges: edges.clone() }
    } else {
        Arming::Landing(LandingTracker {
            fingers,
            band,
            edges: edges.clone(),
            x,
            y,
            landed: HashMap::new(),
            live: false,
            edge: None,
        })
    };
    let mut last_line = Instant::now() - Duration::from_secs(1);
    let mut last_summary = Instant::now();
    let mut reports_since: u64 = 0;
    let mut max_contacts: usize = 0;
    let mut reports_total: u64 = 0;
    let mut band_entries: u64 = 0;
    let mut first_eaten_ms: Vec<u64> = Vec::new();
    let mut was_live = false;
    let mut entry_cursor = (0i32, 0i32);
    let mut max_cursor_dist = 0f64;
    let mut moves_at_entry = 0u64;

    let on_report = Box::new(move |rep: &raw::Report| {
        let now = unsafe { GetTickCount64() };
        reports_since += 1;
        reports_total += 1;
        let tips = rep.tips().count();
        max_contacts = max_contacts.max(tips);
        let live = tracker.feed(rep, now);
        if live && was_live {
            max_cursor_dist = max_cursor_dist.max(dist(entry_cursor, cursor()));
        }
        if live && !was_live {
            band_entries += 1;
            FIRST_EATEN_AT.store(0, Ordering::Relaxed);
            entry_cursor = cursor();
            max_cursor_dist = 0.0;
            moves_at_entry = MOVES_EATEN.load(Ordering::Relaxed);
            // A wheel tick that passed just BEFORE the flag went up is the
            // start-of-gesture leak: Windows synthesised the wheel from the
            // same slide before our parse flagged the band.
            let lp = LAST_PASSED_AT.load(Ordering::Relaxed);
            let pre_leak = lp != 0 && now.saturating_sub(lp) <= LEAK_WINDOW_MS;
            say(&format!(
                "band LIVE  edge={:?}  cursor at entry ({}, {})  {}",
                tracker.edge(),
                entry_cursor.0,
                entry_cursor.1,
                if pre_leak { format!("(a wheel tick PASSED {} ms before entry — start leak)", now - lp) } else { "(no wheel tick in the 100 ms before entry)".into() }
            ));
        } else if !live && was_live {
            let fe = FIRST_EATEN_AT.load(Ordering::Relaxed);
            let entered = raw::BAND_ENTERED_AT.load(Ordering::Relaxed);
            if fe != 0 {
                first_eaten_ms.push(fe.saturating_sub(entered));
            }
            let exit_cursor = cursor();
            let moved = dist(entry_cursor, exit_cursor);
            say(&format!(
                "band DEAD  edge={:?}  held {} ms  first eaten tick after {}  cursor at exit ({}, {})  moved {:.0} px (max {:.0} px during hold)  moves eaten this hold {}",
                tracker.edge(),
                now.saturating_sub(entered),
                if fe != 0 { format!("{} ms", fe.saturating_sub(entered)) } else { "never (no wheel tick eaten)".into() },
                exit_cursor.0,
                exit_cursor.1,
                moved,
                max_cursor_dist.max(moved),
                MOVES_EATEN.load(Ordering::Relaxed).saturating_sub(moves_at_entry)
            ));
        }
        was_live = live;

        // Per-report line, at most ~10/s so the summary fits inside 20/s.
        if last_line.elapsed() >= Duration::from_millis(100) {
            last_line = Instant::now();
            let mut s = format!(
                "contacts={}",
                rep.contact_count.map(|c| c.to_string()).unwrap_or_else(|| "?".into())
            );
            for c in &rep.contacts {
                s.push_str(&format!("  {}:{},{}({})", c.id, c.x, c.y, if c.tip { "tip" } else { "up" }));
            }
            if live {
                s.push_str("  [BAND]");
            }
            say(&s);
        }

        if last_summary.elapsed() >= Duration::from_secs(5) {
            let secs = last_summary.elapsed().as_secs_f64();
            last_summary = Instant::now();
            let rate = reports_since as f64 / secs;
            reports_since = 0;
            say(&format!(
                "summary: {:.1} reports/s ({} total)  max contacts {}  wheel eaten {} passed {}  moves eaten {}  mouse events {}  band entries {}  first_eaten_ms {:?}  leak: after-entry {} after-exit {}",
                rate, reports_total, max_contacts,
                WHEEL_EATEN.load(Ordering::Relaxed), WHEEL_PASSED.load(Ordering::Relaxed),
                MOVES_EATEN.load(Ordering::Relaxed),
                MOUSE_EVENTS.load(Ordering::Relaxed), band_entries, first_eaten_ms,
                LEAK_AFTER_ENTRY.load(Ordering::Relaxed), LEAK_AFTER_EXIT.load(Ordering::Relaxed)
            ));
        }
    });

    // A summary while the pad is idle: the sink only calls back on reports,
    // so a timer thread prints the wheel counters too.
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(5));
        say(&format!(
            "idle-summary: wheel eaten {} passed {}  moves eaten {}  mouse events {}  band live now {}",
            WHEEL_EATEN.load(Ordering::Relaxed), WHEEL_PASSED.load(Ordering::Relaxed),
            MOVES_EATEN.load(Ordering::Relaxed),
            MOUSE_EVENTS.load(Ordering::Relaxed), raw::BAND_LIVE.load(Ordering::Relaxed)
        ));
    });

    match raw::run_sink(&pad, on_report) {
        Ok(()) => say("sink loop ended"),
        Err(e) => say(&format!("sink failed: {e}")),
    }
}

/// A timestamp without pulling a date crate into the example: seconds since
/// the Unix epoch plus the tick count, enough to line the log up with debug.log.
fn chrono_free_stamp() -> String {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix {unix} tick {}", unsafe { GetTickCount64() })
}
