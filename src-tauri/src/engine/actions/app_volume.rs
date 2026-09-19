//! "App volume" (special ids `app_volume_down` / `app_volume_up`, 1.0.126) —
//! nudge the AUDIO SESSION of the app in front by ±10 percentage points and
//! say so: "🔉 Brave 40%" / "🔊 Brave 60%". Owner ask 2026-09-19. Seeded on
//! Space+`-` and Space+`=` like every other special.
//!
//! WHAT IT MOVES, AND WHAT IT NEVER TOUCHES. Windows' own volume keys move
//! the MASTER (`IAudioEndpointVolume`, what `touchpad::actions::add_volume`
//! drives). This moves the per-app slider in the Volume Mixer — the
//! `ISimpleAudioVolume` of each audio SESSION that belongs to the app in
//! front — and nothing else. The master is never read or written here.
//!
//! HOW THE APP IS FOUND. The foreground window's process (`GetForegroundWindow`
//! → `GetWindowThreadProcessId` → `QueryFullProcessImageNameW`, the same
//! plumbing as `hook::exclusions::foreground_info`). Browsers, Discord and
//! Spotify play from HELPER processes (a renderer, a utility process), whose
//! pid is not the window's — so every running process with the SAME EXE NAME
//! as the foreground exe counts (`CreateToolhelp32Snapshot`, the enumerator
//! `hook::conflicts::detect` uses), and the sessions of all of them are
//! matched (`matching_pids`, pure, tested).
//!
//! HOW THE SESSIONS ARE WALKED. `IMMDeviceEnumerator::EnumAudioEndpoints(
//! eRender, DEVICE_STATE_ACTIVE)` — EVERY active render endpoint, not only
//! the default, because with SteelSeries Sonar installed an app very often
//! plays on a non-default Sonar channel (owner, 2026-09-19) — then per
//! endpoint `IAudioSessionManager2` → `IAudioSessionEnumerator`; each
//! `IAudioSessionControl2::GetProcessId` is compared with the matched pids;
//! an expired session is skipped; each match's `ISimpleAudioVolume` gets
//! read, `next_pct` (pure, tested) applied, written. The toast shows the
//! foreground pid's own session level when it has one, else the first
//! matched session's.
//!
//! FAILURE IS A TOAST, NEVER A PANIC. The shell or the desktop in front, or
//! Spaceadom itself: "No app in front". The app has no session on any active
//! render endpoint: "🔇 <App> isn't playing anything". Any COM error is logged with the
//! HRESULT and toasted as the same "isn't playing" line — there is nothing
//! the user can do about it from the keyboard. No elevation is needed or
//! requested; the Mixer does this as the user, and so do we. Reversible by
//! the owner's rule in CLAUDE.md: the other key puts it back.
//!
//! The COM calls run on the engine thread; `CoInitializeEx(MTA)` is called
//! per press, exactly as `audio_output::next_speaker` does.

/// One press moves the slider by this many percentage points.
pub const DELTA_PCT: i32 = 10;

/// The foreground is the desktop, the shell, or Spaceadom itself.
pub const NO_APP_TOAST: &str = "No app in front";

/// The percent after applying `delta` points to `cur` (0..=100), clamped.
/// Pure.
pub fn next_pct(cur: i32, delta: i32) -> u8 {
    (cur + delta).clamp(0, 100) as u8
}

/// A Core Audio scalar (0.0..=1.0) as a whole percent. Pure.
pub fn scalar_to_pct(level: f32) -> i32 {
    if !level.is_finite() {
        return 0;
    }
    (level.clamp(0.0, 1.0) * 100.0).round() as i32
}

/// PURE — every pid whose exe name equals the foreground exe's (case-
/// insensitive), plus the foreground pid itself, in snapshot order with the
/// foreground pid first and no duplicates. `snapshot` is `(pid, exe file
/// name)` as Toolhelp reports it (`brave.exe`, no path). An empty `fg_exe`
/// matches by pid alone — a name that could not be read must not match every
/// process whose name also could not be read.
pub fn matching_pids(fg_pid: u32, fg_exe: &str, snapshot: &[(u32, String)]) -> Vec<u32> {
    let want = fg_exe.trim().to_lowercase();
    let mut out = vec![fg_pid];
    if want.is_empty() {
        return out;
    }
    for (pid, name) in snapshot {
        if *pid != 0 && !out.contains(pid) && name.trim().eq_ignore_ascii_case(&want) {
            out.push(*pid);
        }
    }
    out
}

/// The toast for a successful nudge: "🔊 Brave 60%" going up, "🔉 Brave 40%"
/// going down. Pure.
pub fn toast_text(app: &str, pct: u8, delta: i32) -> String {
    let glyph = if delta < 0 { "🔉" } else { "🔊" };
    format!("{glyph} {app} {pct}%")
}

/// The app in front has no audio session on the default output. Pure.
pub fn nothing_playing_toast(app: &str) -> String {
    format!("🔇 {app} isn't playing anything")
}

/// Nudge the app in front by `delta` points and return the toast line. Every
/// branch returns a toast; nothing here panics on a COM error. NEVER called
/// from a test that is not `#[ignore]` — it really changes a session's slider.
pub fn adjust(delta: i32) -> String {
    #[cfg(windows)]
    {
        win::adjust(delta)
    }
    #[cfg(not(windows))]
    {
        let _ = delta;
        NO_APP_TOAST.to_string()
    }
}

#[cfg(windows)]
pub(crate) mod win {
    use super::{matching_pids, next_pct, nothing_playing_toast, scalar_to_pct, toast_text, NO_APP_TOAST};
    use windows::core::{Interface, Result};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Media::Audio::{
        eRender, AudioSessionStateExpired, IAudioSessionControl2, IAudioSessionManager2, IMMDevice,
        IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId};

    /// The app in front, as this feature needs it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Target {
        pub pid: u32,
        /// `brave.exe` — the file name, lowercased, as Toolhelp spells it.
        pub exe: String,
        /// `focus::short_name_for(stem)` — "Brave", "Word", "Notepad".
        pub name: String,
    }

    /// READ-ONLY: the foreground window's process, or `None` for the desktop,
    /// the shell hosts, Spaceadom itself, and anything unreadable (the same
    /// rules as `focus::is_shell_surface`, so the ring's pill and this toast
    /// agree on what counts as "an app in front").
    pub unsafe fn foreground_target() -> Option<Target> {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == std::process::id() {
            return None;
        }
        let mut cls = [0u16; 128];
        let n = GetClassNameW(hwnd, &mut cls);
        let class = String::from_utf16_lossy(&cls[..n.max(0) as usize]);
        let path = crate::hook::exclusions::process_path_for_pid(pid);
        if path.is_empty() {
            return None;
        }
        let stem = crate::hook::exclusions::normalize_stem(&path);
        if stem.is_empty()
            || stem == crate::hook::exclusions::own_stem()
            || crate::engine::focus::is_shell_surface(&stem, &class)
        {
            return None;
        }
        let exe = path.rsplit(['\\', '/']).next().unwrap_or(&path).to_lowercase();
        Some(Target { pid, exe, name: crate::engine::focus::short_name_for(&stem) })
    }

    /// READ-ONLY: every running process as `(pid, exe file name lowercased)`.
    /// Empty when the snapshot cannot be taken — the caller then matches the
    /// foreground pid alone.
    pub unsafe fn process_snapshot() -> Vec<(u32, String)> {
        let mut out = Vec::new();
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                out.push((entry.th32ProcessID, name));
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        out
    }

    /// One audio session on one active render endpoint, as read.
    #[derive(Debug, Clone)]
    pub struct Session {
        /// Index of the endpoint in `EnumAudioEndpoints(eRender, ACTIVE)` order.
        pub endpoint: usize,
        /// The endpoint's friendly name (`audio_output::win::device_name`).
        pub endpoint_name: String,
        pub pid: u32,
        /// `AudioSessionState` as an integer: 0 inactive, 1 active, 2 expired.
        pub state: i32,
        pub volume: ISimpleAudioVolume,
    }

    /// READ-ONLY: every session on ONE endpoint. A session whose control
    /// cannot be cast is skipped, not fatal.
    unsafe fn sessions_on(device: &IMMDevice, endpoint: usize, endpoint_name: &str, out: &mut Vec<Session>) -> Result<()> {
        let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
        let list = manager.GetSessionEnumerator()?;
        let n = list.GetCount()?;
        for i in 0..n {
            let Ok(ctl) = list.GetSession(i) else { continue };
            let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
            let pid = ctl2.GetProcessId().unwrap_or(0);
            let state = ctl.GetState().map(|s| s.0).unwrap_or(AudioSessionStateExpired.0);
            let Ok(volume) = ctl.cast::<ISimpleAudioVolume>() else { continue };
            out.push(Session { endpoint, endpoint_name: endpoint_name.to_string(), pid, state, volume });
        }
        Ok(())
    }

    /// READ-ONLY: every session on EVERY active render endpoint, in endpoint
    /// order, expired ones included so a listing can show them; the caller
    /// filters. An endpoint whose session manager refuses is logged and
    /// skipped; the error returned is only the enumerator's own. The caller
    /// has `CoInitializeEx`'d this thread.
    pub unsafe fn sessions_on_active_endpoints() -> Result<Vec<Session>> {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let coll = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        let n = coll.GetCount()?;
        let mut out = Vec::new();
        for i in 0..n {
            let Ok(device) = coll.Item(i) else { continue };
            let name = crate::engine::actions::audio_output::win::device_name(&device);
            if let Err(e) = sessions_on(&device, i as usize, &name, &mut out) {
                log::warn!("app_volume: could not walk sessions on endpoint {i} ({name:?}): {e:?}");
            }
        }
        Ok(out)
    }

    /// WRITES the matched sessions' sliders. Returns the new percent to toast
    /// — the foreground pid's own session when it has one, else the first
    /// helper's — or `None` when no live session belongs to `pids`.
    pub(crate) unsafe fn nudge_sessions(sessions: &[Session], pids: &[u32], delta: i32) -> Option<u8> {
        let mut toast: Option<(bool, u8)> = None;
        for s in sessions {
            if s.pid == 0 || !pids.contains(&s.pid) || s.state == AudioSessionStateExpired.0 {
                continue;
            }
            let cur = match s.volume.GetMasterVolume() {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("app_volume: GetMasterVolume failed for pid {}: {e:?}", s.pid);
                    continue;
                }
            };
            let cur_pct = scalar_to_pct(cur);
            let next = next_pct(cur_pct, delta);
            if let Err(e) = s.volume.SetMasterVolume(next as f32 / 100.0, std::ptr::null()) {
                log::warn!("app_volume: SetMasterVolume({next}%) failed for pid {}: {e:?}", s.pid);
                continue;
            }
            log::info!(
                "app_volume: session pid {} on endpoint {} ({:?}) {cur_pct}% → {next}% (state {})",
                s.pid, s.endpoint, s.endpoint_name, s.state
            );
            let is_fg = s.pid == pids[0];
            match toast {
                None => toast = Some((is_fg, next)),
                Some((false, _)) if is_fg => toast = Some((true, next)),
                _ => {}
            }
        }
        toast.map(|(_, pct)| pct)
    }

    pub fn adjust(delta: i32) -> String {
        // SAFETY: plain Win32/COM queries and calls on the engine thread;
        // every result is checked and every failure becomes a toast.
        unsafe {
            let Some(target) = foreground_target() else {
                log::info!("app_volume: no app in front (desktop, shell or ourselves) — nothing changed");
                return NO_APP_TOAST.to_string();
            };
            let snapshot = process_snapshot();
            let pids = matching_pids(target.pid, &target.exe, &snapshot);
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let sessions = match sessions_on_active_endpoints() {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("app_volume: could not enumerate the active render endpoints: {e:?}");
                    return nothing_playing_toast(&target.name);
                }
            };
            match nudge_sessions(&sessions, &pids, delta) {
                Some(pct) => {
                    log::info!(
                        "app_volume: {} ({}, {} pid(s) of {} session(s)) moved by {delta:+} to {pct}% (app-volume-session-slider-moved-spaceadom-126)",
                        target.name,
                        target.exe,
                        pids.len(),
                        sessions.len()
                    );
                    toast_text(&target.name, pct, delta)
                }
                None => {
                    log::info!(
                        "app_volume: {} ({}) has no live session on any active render endpoint — {} pid(s) matched, {} session(s) seen",
                        target.name,
                        target.exe,
                        pids.len(),
                        sessions.len()
                    );
                    nothing_playing_toast(&target.name)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(rows: &[(u32, &str)]) -> Vec<(u32, String)> {
        rows.iter().map(|(p, n)| (*p, n.to_string())).collect()
    }

    #[test]
    fn matching_pids_takes_the_foreground_and_every_same_named_helper() {
        let s = snap(&[
            (0, "[System Process]"),
            (4, "System"),
            (100, "brave.exe"),
            (101, "brave.exe"),
            (102, "Brave.EXE"),
            (200, "discord.exe"),
            (300, "explorer.exe"),
        ]);
        assert_eq!(matching_pids(100, "brave.exe", &s), vec![100, 101, 102], "case-insensitive, no duplicate of the fg pid");
        assert_eq!(matching_pids(200, "discord.exe", &s), vec![200]);
        assert_eq!(matching_pids(999, "brave.exe", &s), vec![999, 100, 101, 102], "fg pid first even when it is not in the snapshot");
        assert_eq!(matching_pids(300, "notepad.exe", &s), vec![300], "no helpers: the fg pid alone");
    }

    #[test]
    fn matching_pids_with_no_name_or_no_snapshot_matches_by_pid_alone() {
        let s = snap(&[(1, ""), (2, ""), (3, "x.exe")]);
        assert_eq!(matching_pids(3, "", &s), vec![3], "an unreadable name must not match the other unreadable names");
        assert_eq!(matching_pids(7, "x.exe", &[]), vec![7]);
        assert_eq!(matching_pids(7, "x.exe", &snap(&[(0, "x.exe")])), vec![7], "pid 0 is never a helper");
    }

    #[test]
    fn next_pct_clamps_at_both_ends() {
        assert_eq!(next_pct(50, DELTA_PCT), 60);
        assert_eq!(next_pct(50, -DELTA_PCT), 40);
        assert_eq!(next_pct(95, 10), 100);
        assert_eq!(next_pct(100, 10), 100);
        assert_eq!(next_pct(5, -10), 0);
        assert_eq!(next_pct(0, -10), 0);
        assert_eq!(next_pct(0, 10), 10);
    }

    #[test]
    fn scalars_round_to_whole_percents() {
        assert_eq!(scalar_to_pct(0.0), 0);
        assert_eq!(scalar_to_pct(1.0), 100);
        assert_eq!(scalar_to_pct(0.404), 40);
        assert_eq!(scalar_to_pct(0.605), 61);
        assert_eq!(scalar_to_pct(1.7), 100, "clamped");
        assert_eq!(scalar_to_pct(f32::NAN), 0);
        // A round trip through a press lands on a whole percent.
        assert_eq!(next_pct(scalar_to_pct(0.4), DELTA_PCT), 50);
    }

    #[test]
    fn the_toasts_name_the_app_and_the_new_level() {
        assert_eq!(toast_text("Brave", 40, -10), "🔉 Brave 40%");
        assert_eq!(toast_text("Brave", 60, 10), "🔊 Brave 60%");
        assert_eq!(nothing_playing_toast("Brave"), "🔇 Brave isn't playing anything");
        assert_eq!(NO_APP_TOAST, "No app in front");
    }

    /// READ-ONLY on the machine that runs it: lists every audio session on
    /// every ACTIVE render endpoint with its pid, exe and current slider, and
    /// what the foreground app would match. Never calls `SetMasterVolume`.
    /// `cargo test --release --lib list_audio_sessions -- --ignored --nocapture`
    #[test]
    #[ignore = "reads this machine's audio sessions; run by hand with --nocapture"]
    #[cfg(windows)]
    fn list_audio_sessions_on_this_machine() {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let sessions = win::sessions_on_active_endpoints().expect("endpoint enumerator");
            let snapshot = win::process_snapshot();
            let name_of = |pid: u32| snapshot.iter().find(|(p, _)| *p == pid).map(|(_, n)| n.clone()).unwrap_or_else(|| "?".into());
            eprintln!("audio sessions across all active render endpoints: {}", sessions.len());
            for (i, s) in sessions.iter().enumerate() {
                let vol = s.volume.GetMasterVolume().map(scalar_to_pct).unwrap_or(-1);
                let state = match s.state { 0 => "inactive", 1 => "active", 2 => "expired", _ => "?" };
                eprintln!("  [{i}] ep{} {:<34} pid {:>6} {:<28} {:<8} {vol}%", s.endpoint, s.endpoint_name, s.pid, name_of(s.pid), state);
            }
            match win::foreground_target() {
                Some(t) => {
                    let pids = matching_pids(t.pid, &t.exe, &snapshot);
                    eprintln!("foreground: {:?} pid {} exe {} → {} matching pid(s): {pids:?}", t.name, t.pid, t.exe, pids.len());
                }
                None => eprintln!("foreground: none (desktop, shell or ourselves)"),
            }
        }
    }
}
