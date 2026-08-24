/// engine/actions/pip.rs — Picture-in-Picture 4-corner cycle.
/// Mirrors V11 TogglePiP(), rebuilt 2026-08-24 (PROBLEM 167).
///
/// WHAT CHANGED AND WHY — read before "restoring" any of it.
///
/// The owner's report: *"pip isn't working properly… it behaves oddly and
/// doesn't go to the 4 corners properly either, loses its title bar and won't
/// come back."* Then, live: *"I tried PiP, it didn't work for the Claude
/// desktop app, then space hud sound appeared but showed nothing, it appeared
/// behind Claude."* That last sentence is the whole bug in one line — see §3.
///
/// 1. NO MORE STYLE SURGERY. The old code stripped WS_CAPTION|WS_THICKFRAME
///    to make a clean borderless tile. On classic windows that looked good. On
///    Electron/Chromium windows — Claude, Discord, VS Code, Spotify, i.e. most
///    of what this owner actually uses — those bits are not what draws the
///    frame, so stripping them changed nothing visible while still arming the
///    failure below. Owner's call, 2026-08-24: "stop stripping entirely —
///    corner-snap only." PiP is now move + resize + stay-on-top. Nothing about
///    another program's window structure is modified, so there is nothing that
///    can fail to be put back.
///
/// 2. THE ORPHAN. `restore_window` was reachable ONLY on the 5th tap, and this
///    cache is memory-only. Restart Spaceadom — or lose the entry any other
///    way — and that window stayed borderless, topmost and half-size FOREVER,
///    with no route back and no visible sign of what had happened to it. There
///    was no restore-on-exit handler and no `IsWindow` validation, so a closed
///    window's entry also sat in the map until the process died, ready for
///    Windows to hand its recycled HWND to something innocent.
///    Now: entries are validated every pass, `restore_all()` runs on exit, and
///    `rescue_orphan()` repairs a window the OLD build stripped.
///
/// 3. THE TOPMOST POLLUTION — why the Guide HUD vanished behind Claude.
///    PiP marks its window HWND_TOPMOST and (before this) only cleared it on
///    the 5th tap. A window that could not be cycled out of therefore stayed
///    topmost for the rest of its life. The overlay's own "re-assert topmost
///    on every show" could not climb back over it, because that call was a
///    no-op (PROBLEM 168). So one failed PiP permanently hid the HUD behind
///    an ordinary-looking app. Both halves are fixed; this half is: topmost is
///    cleared on restore, on rescue, and on exit.
///
/// 4. THE MONITOR IS THE CURSOR'S, DELIBERATELY. Corners are computed from
///    `MonitorFromPoint(GetCursorPos())` while the window comes from
///    `GetForegroundWindow()`, so with two displays PiP throws the window onto
///    the screen you are pointing at. That reads as a bug and is not one —
///    owner's explicit decision, 2026-08-24, when offered the alternative:
///    "the monitor the cursor is on — keep as is." Do not "fix" it.
///
/// 5. THE ANIMATION ACTUALLY RUNS NOW. The spring's exit test read
///    `(x - tx).abs() < 0.5 && vx.abs() < 0.5` — X ONLY. Top-Right → Bottom-
///    Right is a purely vertical hop, so x was already at target with vx = 0
///    and the loop broke on iteration 1: that tap snapped with no motion while
///    its neighbours glided. Both axes are tested now. Concurrent taps also
///    used to spawn overlapping threads that fought over one window, which is
///    the "behaves oddly" — a generation counter now retires the older one.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Clone)]
pub struct PipEntry {
    pub original_x: i32,
    pub original_y: i32,
    pub original_w: i32,
    pub original_h: i32,
    pub was_maximized: bool,
    /// 0=TopLeft, 1=TopRight, 2=BottomRight, 3=BottomLeft
    pub position_index: u8,
}

pub type PipCache = Arc<Mutex<HashMap<isize, PipEntry>>>;

/// ONE cache for the process.
///
/// It used to be a fresh map per `EngineState`, which was fine while the only
/// reader was the engine. `restore_all()` runs from the Tauri exit handler,
/// which has no engine handle at all — and a restore-on-exit that cannot see
/// the entries is not a restore. Sharing one map keeps `new_cache()`'s
/// signature and gives shutdown something to read.
fn global_cache() -> &'static PipCache {
    static CACHE: OnceLock<PipCache> = OnceLock::new();
    CACHE.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

/// Handle to the shared PiP cache.
pub fn new_cache() -> PipCache {
    global_cache().clone()
}

/// Bumped by every new animation. A running spring loop that finds the counter
/// has moved past its own ticket snaps to ITS OWN target and stops, so a
/// superseded flight neither fights the new one nor abandons its window
/// half-way to a corner.
static ANIM_GEN: AtomicU64 = AtomicU64::new(0);

/// Toggle PiP on the current foreground window.
/// Returns a notification message string.
pub fn toggle_pip(cache: &PipCache) -> String {
    #[cfg(windows)]
    unsafe {
        toggle_pip_win32(cache)
    }
    #[cfg(not(windows))]
    {
        let _ = cache;
        String::from("PiP not supported on this platform")
    }
}

#[cfg(windows)]
unsafe fn toggle_pip_win32(cache: &PipCache) -> String {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST},
        UI::WindowsAndMessaging::{
            GetCursorPos, GetForegroundWindow, GetWindowRect, IsIconic, IsZoomed, SetWindowPos,
            ShowWindow, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_NOACTIVATE, SW_RESTORE,
        },
    };

    let hwnd = GetForegroundWindow();
    if hwnd.0.is_null() {
        log::warn!("pip: no foreground window — nothing to toggle");
        return String::new();
    }

    // Never act on our own dashboard or overlay. Stripping/moving Spaceadom's
    // own window from inside Spaceadom is not a feature anyone asked for, and
    // the overlay is click-through and NoActivate so it should never be
    // foreground in the first place — if it somehow is, that is a bug to log,
    // not a window to tile.
    if is_own_window(hwnd) {
        log::warn!("pip: the foreground window belongs to Spaceadom — refusing to PiP ourselves");
        return "PiP needs another app's window".to_string();
    }

    // PROBLEM 167 — repair a window the OLD PiP left stripped. This is a
    // legacy path only: nothing this build does can create one.
    if !cache
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains_key(&(hwnd.0 as isize))
        && looks_orphaned(hwnd)
    {
        rescue_orphan(hwnd);
        return "🔧 Window frame repaired — press again for PiP".to_string();
    }

    // A minimised window has a meaningless rect; restore it before measuring.
    if IsIconic(hwnd).as_bool() {
        let _ = ShowWindow(hwnd, SW_RESTORE);
    }

    // Corners come from the CURSOR's monitor, on purpose — see §4 in the
    // header. This is what lets PiP throw a window onto the screen you are
    // pointing at.
    let mut cursor_pos = POINT::default();
    GetCursorPos(&mut cursor_pos).ok();
    let monitor = MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST);
    let mut mon_info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(monitor, &mut mon_info).as_bool() {
        log::warn!("pip: GetMonitorInfoW failed for hwnd {:?}", hwnd);
        return String::new();
    }

    // PiP is positioned against the WORK area (rcWork, excludes the taskbar),
    // not the raw monitor bounds — so rcMonitor's dimensions are unused.
    let wa = mon_info.rcWork;
    let mon_w = wa.right - wa.left;
    let mon_h = wa.bottom - wa.top;

    // PiP size = 50% × 50% of the work area (i.e. 25% of its area)
    let pip_w = mon_w / 2;
    let pip_h = mon_h / 2;

    let mut cache_lock = cache.lock().unwrap_or_else(|p| p.into_inner());

    // Drop entries whose windows are gone BEFORE looking ours up. Windows
    // recycles HWND values, so a dead entry left in the map can be inherited
    // by an unrelated new window — which then starts its life at "corner 2"
    // or gets "restored" to a stranger's geometry. That is a large part of
    // "doesn't go to the 4 corners properly".
    prune_dead(&mut cache_lock);

    let hwnd_key = hwnd.0 as isize;

    if let Some(entry) = cache_lock.get_mut(&hwnd_key) {
        entry.position_index += 1;
        let idx = entry.position_index;

        if idx >= 4 {
            // 5th tap: put it back where it came from and let go of it.
            log::info!("pip: restoring hwnd {hwnd_key:#x} to its original frame");
            let entry = entry.clone();
            cache_lock.remove(&hwnd_key);
            drop(cache_lock);
            restore_window(hwnd, &entry);
            return "↩️ Frame Restored".to_string();
        }

        let (x, y) = corner_position(idx, wa.left, wa.top, pip_w, pip_h, mon_w, mon_h);
        log::info!("pip: hwnd {hwnd_key:#x} → corner {idx} at ({x},{y}) {pip_w}x{pip_h}");
        drop(cache_lock);
        animate_to(hwnd, x, y, pip_w, pip_h);
        corner_label(idx)
    } else {
        // Enter PiP for the first time.
        let maximized = IsZoomed(hwnd).as_bool();

        // Un-maximise FIRST, then measure. The old code measured before the
        // restore, so `original_*` held the MAXIMISED rect and the 5th tap
        // "restored" a window to bounds it had never had in its normal state.
        if maximized {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }

        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            log::warn!("pip: GetWindowRect failed for hwnd {hwnd_key:#x} — not entering PiP");
            return String::new();
        }

        log::info!(
            "pip: entering PiP for hwnd {hwnd_key:#x} (was {}x{} at ({},{}), maximized={maximized})",
            rect.right - rect.left,
            rect.bottom - rect.top,
            rect.left,
            rect.top
        );

        cache_lock.insert(
            hwnd_key,
            PipEntry {
                original_x: rect.left,
                original_y: rect.top,
                original_w: rect.right - rect.left,
                original_h: rect.bottom - rect.top,
                was_maximized: maximized,
                position_index: 0,
            },
        );
        drop(cache_lock);

        // Stay-on-top is the ONE thing PiP still changes about the window, and
        // it is the point of the feature. Every exit route clears it again:
        // the 5th tap, `rescue_orphan`, and `restore_all` at shutdown.
        // No SWP_FRAMECHANGED — nothing about the frame is being altered any
        // more, and asking for a frame recalculation we do not need is how you
        // make a Chromium window flicker.
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );

        let (x, y) = corner_position(0, wa.left, wa.top, pip_w, pip_h, mon_w, mon_h);
        animate_to(hwnd, x, y, pip_w, pip_h);

        "📺 PiP: Top-Left".into()
    }
}

/// True when this HWND belongs to the Spaceadom process.
#[cfg(windows)]
unsafe fn is_own_window(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    pid != 0 && pid == GetCurrentProcessId()
}

/// Forget every entry whose window no longer exists.
#[cfg(windows)]
fn prune_dead(map: &mut HashMap<isize, PipEntry>) {
    use windows::Win32::UI::WindowsAndMessaging::IsWindow;
    map.retain(|k, _| {
        let alive =
            unsafe { IsWindow(windows::Win32::Foundation::HWND(*k as *mut _)).as_bool() };
        if !alive {
            log::debug!("pip: dropping cache entry for closed window {k:#x}");
        }
        alive
    });
}

/// Does this window look like one the OLD PiP stripped and abandoned?
///
/// Deliberately narrow. A plain "borderless and topmost" test would also match
/// games, media players in fullscreen and every app with custom chrome, and
/// handing WS_CAPTION to a Chromium window that never had one would wreck its
/// layout. So the signature has to include the geometry the old PiP produced
/// and nothing else does by coincidence: half the work area on BOTH axes,
/// parked within a few pixels of one of the four corners.
#[cfg(windows)]
unsafe fn looks_orphaned(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, GetWindowRect, GWL_EXSTYLE, GWL_STYLE, WS_CAPTION, WS_EX_TOPMOST,
        WS_THICKFRAME,
    };

    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;

    // Must be topmost AND missing both bits the old code removed.
    if ex_style & WS_EX_TOPMOST.0 == 0 {
        return false;
    }
    if style & WS_CAPTION.0 != 0 || style & WS_THICKFRAME.0 != 0 {
        return false;
    }

    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_err() {
        return false;
    }
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(monitor, &mut mi).as_bool() {
        return false;
    }
    let wa = mi.rcWork;
    let mon_w = wa.right - wa.left;
    let mon_h = wa.bottom - wa.top;
    let (pip_w, pip_h) = (mon_w / 2, mon_h / 2);

    // 4px of slack: integer halving and DPI rounding move things by a pixel or
    // two, but nothing legitimate lands this close to the signature by chance.
    const SLACK: i32 = 4;
    let near = |a: i32, b: i32| (a - b).abs() <= SLACK;

    let w = rect.right - rect.left;
    let h = rect.bottom - rect.top;
    if !near(w, pip_w) || !near(h, pip_h) {
        return false;
    }

    let corners = [
        (wa.left, wa.top),
        (wa.left + mon_w - pip_w, wa.top),
        (wa.left + mon_w - pip_w, wa.top + mon_h - pip_h),
        (wa.left, wa.top + mon_h - pip_h),
    ];
    corners
        .iter()
        .any(|(cx, cy)| near(rect.left, *cx) && near(rect.top, *cy))
}

/// Give a window that the OLD PiP stripped its frame back.
///
/// We cannot know its original size — that died with the build that took it —
/// so this repairs what is actually broken (no title bar to drag, no edge to
/// resize, stuck above everything) and leaves it where it sits. The user can
/// then move and size it normally, which is precisely what they could not do.
#[cfg(windows)]
unsafe fn rescue_orphan(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, SetWindowLongW, SetWindowPos, GWL_STYLE, HWND_NOTOPMOST, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_CAPTION, WS_THICKFRAME,
    };

    log::warn!(
        "pip: hwnd {:#x} carries the signature of a window an OLDER build stripped and \
         never restored (borderless + topmost + exactly a quarter-screen corner tile). \
         Giving its title bar and resize edge back and clearing stay-on-top. This path \
         is legacy repair only — this build never strips a window's frame.",
        hwnd.0 as isize
    );

    let style = GetWindowLongW(hwnd, GWL_STYLE);
    SetWindowLongW(
        hwnd,
        GWL_STYLE,
        style | WS_CAPTION.0 as i32 | WS_THICKFRAME.0 as i32,
    );
    // SWP_FRAMECHANGED IS required here, unlike the PiP path: the non-client
    // area genuinely changed size, and without it the window keeps drawing
    // with its old frame metrics until something else forces a recalculation.
    let _ = SetWindowPos(
        hwnd,
        HWND_NOTOPMOST,
        0,
        0,
        0,
        0,
        SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    );
}

/// Put every window PiP is still holding back the way it was found.
///
/// Called from the Tauri exit handler. Without it, quitting Spaceadom left
/// every PiP'd window pinned above everything else at a quarter size, with the
/// only control that could release them gone — the owner's "loses its title
/// bar and won't come back". Best-effort by design: shutdown must not be able
/// to fail, so every step is ignore-on-error and the map is cleared regardless.
pub fn restore_all() {
    #[cfg(windows)]
    {
        let cache = global_cache();
        let mut map = cache.lock().unwrap_or_else(|p| p.into_inner());
        if map.is_empty() {
            return;
        }
        log::info!("pip: restoring {} window(s) before exit", map.len());
        for (key, entry) in map.iter() {
            let hwnd = windows::Win32::Foundation::HWND(*key as *mut _);
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::IsWindow;
                if IsWindow(hwnd).as_bool() {
                    restore_window(hwnd, entry);
                }
            }
        }
        map.clear();
    }
}

#[cfg(windows)]
fn corner_position(
    idx: u8,
    left: i32,
    top: i32,
    pip_w: i32,
    pip_h: i32,
    mon_w: i32,
    mon_h: i32,
) -> (i32, i32) {
    match idx {
        1 => (left + mon_w - pip_w, top),                 // Top-Right
        2 => (left + mon_w - pip_w, top + mon_h - pip_h), // Bottom-Right
        3 => (left, top + mon_h - pip_h),                 // Bottom-Left
        _ => (left, top),                                 // Top-Left (fallback)
    }
}

#[cfg(windows)]
fn corner_label(idx: u8) -> String {
    match idx {
        0 => "📐 PiP: Top-Left".into(),
        1 => "📐 PiP: Top-Right".into(),
        2 => "📐 PiP: Bottom-Right".into(),
        3 => "📐 PiP: Bottom-Left".into(),
        _ => "📐 PiP".into(),
    }
}

/// Animate a window to a target rect with a spring, on a short-lived thread so
/// the engine actor is never blocked.
#[cfg(windows)]
fn animate_to(hwnd: windows::Win32::Foundation::HWND, tx: i32, ty: i32, tw: i32, th: i32) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, IsWindow, SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
    };

    let hwnd_raw = hwnd.0 as isize;
    let ticket = ANIM_GEN.fetch_add(1, Ordering::SeqCst) + 1;

    let spawned = std::thread::Builder::new()
        .name("st-pip-anim".into())
        .spawn(move || {
            let h = || windows::Win32::Foundation::HWND(hwnd_raw as *mut _);
            // A null insert-after with SWP_NOZORDER: the z-order is untouched,
            // so the topmost flag set when PiP was entered survives the flight.
            let place = |x: i32, y: i32| unsafe {
                let _ = SetWindowPos(
                    h(),
                    windows::Win32::Foundation::HWND(std::ptr::null_mut()),
                    x,
                    y,
                    tw,
                    th,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            };

            // Spring constants (120fps tuned)
            let k: f64 = 0.18;
            let c: f64 = 0.42;

            let mut rect = RECT::default();
            unsafe { GetWindowRect(h(), &mut rect).ok() };

            let mut x = rect.left as f64;
            let mut y = rect.top as f64;
            let mut vx = 0f64;
            let mut vy = 0f64;

            for _ in 0..120 {
                // Superseded by a newer tap: land on OUR target and stand down,
                // rather than fighting the new flight for the same window or
                // abandoning this one part-way to a corner.
                if ANIM_GEN.load(Ordering::SeqCst) != ticket {
                    place(tx, ty);
                    return;
                }
                // The window can be closed mid-flight; SetWindowPos on a dead
                // HWND is harmless but pointless, and the loop would keep
                // running for a second for nothing.
                if !unsafe { IsWindow(h()).as_bool() } {
                    return;
                }

                let fx = -k * (x - tx as f64) - c * vx;
                let fy = -k * (y - ty as f64) - c * vy;
                vx += fx;
                vy += fy;
                x += vx;
                y += vy;

                place(x.round() as i32, y.round() as i32);

                // BOTH axes. Testing X alone (what this did until PROBLEM 167)
                // meant a purely vertical hop — Top-Right to Bottom-Right —
                // satisfied the test on iteration 1 and snapped with no motion
                // at all, while the horizontal hops either side of it glided.
                let settled_x = (x - tx as f64).abs() < 0.5 && vx.abs() < 0.5;
                let settled_y = (y - ty as f64).abs() < 0.5 && vy.abs() < 0.5;
                if settled_x && settled_y {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(8));
            }

            // Snap to exact target — the spring gets close, not exact.
            if ANIM_GEN.load(Ordering::SeqCst) == ticket {
                place(tx, ty);
            }
        });

    // A thread that will not spawn must not lose the window: place it directly.
    // Same lesson as PROBLEM 124 — a spawn failure is plausible on someone
    // else's machine and must degrade to "no animation", never to "no PiP".
    if let Err(e) = spawned {
        log::warn!("pip: could not spawn the animation thread ({e}) — placing the window directly");
        unsafe {
            let _ = SetWindowPos(hwnd, windows::Win32::Foundation::HWND(std::ptr::null_mut()), tx, ty, tw, th, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}

/// Restore a window to the position, size and z-order it had before PiP.
///
/// No style work: this build never changed any. `HWND_NOTOPMOST` is the part
/// that matters most — leaving a window pinned above everything is what hid
/// the Guide HUD behind it (§3).
#[cfg(windows)]
unsafe fn restore_window(hwnd: windows::Win32::Foundation::HWND, entry: &PipEntry) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, HWND_NOTOPMOST, SWP_NOACTIVATE, SW_SHOWMAXIMIZED,
    };

    // Stop any flight still in the air for this window, or it will keep
    // dragging the window back toward a corner after we have restored it.
    ANIM_GEN.fetch_add(1, Ordering::SeqCst);

    let _ = SetWindowPos(
        hwnd,
        HWND_NOTOPMOST,
        entry.original_x,
        entry.original_y,
        entry.original_w,
        entry.original_h,
        SWP_NOACTIVATE,
    );

    if entry.was_maximized {
        let _ = ShowWindow(hwnd, SW_SHOWMAXIMIZED);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_are_the_four_corners_of_the_work_area() {
        // A 1920x1040 work area at origin (0,0): tiles are 960x520.
        let (l, t, w, h) = (0, 0, 1920, 1040);
        let (pw, ph) = (w / 2, h / 2);
        assert_eq!(corner_position(0, l, t, pw, ph, w, h), (0, 0));
        assert_eq!(corner_position(1, l, t, pw, ph, w, h), (960, 0));
        assert_eq!(corner_position(2, l, t, pw, ph, w, h), (960, 520));
        assert_eq!(corner_position(3, l, t, pw, ph, w, h), (0, 520));
    }

    #[test]
    fn corners_respect_a_non_zero_monitor_origin() {
        // A second display to the LEFT of the primary has negative coordinates,
        // and a taskbar makes the work area's top non-zero. Both were correct
        // before; this pins them so a future "simplification" cannot regress
        // the two-monitor case the owner actually runs.
        let (l, t, w, h) = (-1920, 48, 1920, 992);
        let (pw, ph) = (w / 2, h / 2);
        assert_eq!(corner_position(0, l, t, pw, ph, w, h), (-1920, 48));
        assert_eq!(corner_position(1, l, t, pw, ph, w, h), (-960, 48));
        assert_eq!(corner_position(2, l, t, pw, ph, w, h), (-960, 544));
        assert_eq!(corner_position(3, l, t, pw, ph, w, h), (-1920, 544));
    }

    /// The vertical hop is the one the old exit test broke on. This asserts
    /// the SHAPE of the bug rather than the animation: corner 1 → corner 2
    /// changes only Y, so any convergence test that ignores Y is satisfied
    /// before the window has moved.
    #[test]
    fn top_right_to_bottom_right_is_a_purely_vertical_move() {
        let (l, t, w, h) = (0, 0, 1920, 1040);
        let (pw, ph) = (w / 2, h / 2);
        let a = corner_position(1, l, t, pw, ph, w, h);
        let b = corner_position(2, l, t, pw, ph, w, h);
        assert_eq!(a.0, b.0, "x must not change on this hop");
        assert_ne!(a.1, b.1, "y must change on this hop");
    }

    #[test]
    fn the_cache_is_shared_not_per_caller() {
        // restore_all() reads the global map from the exit handler, which has
        // no engine handle. If new_cache() ever goes back to handing out fresh
        // maps, shutdown silently restores nothing.
        let a = new_cache();
        let b = new_cache();
        a.lock().unwrap().insert(
            0x1234,
            PipEntry {
                original_x: 1,
                original_y: 2,
                original_w: 3,
                original_h: 4,
                was_maximized: false,
                position_index: 0,
            },
        );
        assert!(
            b.lock().unwrap().contains_key(&0x1234),
            "new_cache() must hand out the SAME map, or restore_all() sees nothing"
        );
        a.lock().unwrap().clear();
    }
}
