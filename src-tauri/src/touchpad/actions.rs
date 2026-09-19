//! TOUCHPAD T2 — the three edge actions, in-process and instant.
//!
//! A slide ticks at 30–60 Hz, so none of this may shell out to PowerShell:
//!
//! * **Volume** — Core Audio `IAudioEndpointVolume` on the default render
//!   endpoint (the same interface `boss_key.rs` mutes through). Read the
//!   master scalar, set `clamp(cur + k·Δtravel)`, read back for the readout.
//! * **Brightness** — WMI in-process via COM (`IWbemServices` +
//!   `WmiMonitorBrightnessMethods.WmiSetBrightness`), the same class
//!   `actions/brightness.rs` used to drive through PowerShell; that module now
//!   shares `set_brightness`/`get_brightness` here so there is one path.
//! * **Scrub** — ←/→ taps through `hook::send_keys_checked` (cookie
//!   `0x7A7A7A7A`, PROBLEM 227), at a rate `scrub_rate(travel, sensitivity,
//!   band_length)`. RATE-based (continuous, a Touch Bar scrubber) — kept that
//!   way in 1.0.122; 1.0.130 capped it (8 taps/s at sensitivity 5, flat past
//!   40 % of the band). Also the automatic FALLBACK inside a "Video seek"
//!   gesture that finds no seekable media session (`seek.rs`) — never a
//!   replacement for the user's own choice of Scrub in the list.
//! * **Seek** (1.0.130) — `seek.rs`: the media session's position follows the
//!   finger in pure proportion.
//! * **Presets** (1.0.130) — `presets.rs`: named chord pairs driven like
//!   Chords (or once per slide).
//! * **Chords** ("Any shortcut", 1.0.122) — STEP-based: `chord_steps` quantises
//!   the signed travel into steps (`chord_step_size(sensitivity)` of the pad's
//!   short side, past the same dead zone as scrub) and `touchpad::mod` sends
//!   the forward/backward chord once per new step. Both go out through one
//!   path, `send_chord` → `engine::actions::chord::send_batch`.
//!
//! COM must be initialised on the calling thread first; `touchpad::mod`'s
//! reader thread does that once at start-up.
#![cfg(windows)]

use windows::core::{BSTR, VARIANT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoSetProxyBlanket, CLSCTX_ALL, CLSCTX_INPROC_SERVER, EOAC_NONE,
    RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE,
};

// ---------------------------------------------------------------------------
// Scrub rate (pure — the one line a test can referee)
// ---------------------------------------------------------------------------

/// Dead zone before any scrubbing begins: the fraction of the pad a finger
/// must travel from entry before the first ←/→ tap.
pub const SCRUB_DEAD_ZONE: f32 = 0.03;
/// The slowest continuous tap rate (taps per second), just past the dead zone.
pub const SCRUB_MIN_RATE: f32 = 2.0;
/// The fraction of the BAND'S LENGTH at which the rate reaches its cap and
/// goes FLAT (owner, 2026-09-20 00:58: "if you go much farther it becomes too
/// fast, out of control" — 1.0.122's curve kept climbing to 25 taps/s at full
/// pad travel; now nothing past ~40 % of the band is any faster).
pub const SCRUB_RAMP_FRACTION: f32 = 0.40;

/// The cap by sensitivity — 3 taps/s at 1, **8 at 5 (the default)**, 15 at 10,
/// piecewise linear (`seek::sens_scale`). Pure.
///
/// | sens | 1 | 2    | 3   | 4    | 5 | 6   | 7    | 8    | 9    | 10 |
/// |------|---|------|-----|------|---|-----|------|------|------|----|
/// | cap  | 3 | 4.25 | 5.5 | 6.75 | 8 | 9.4 | 10.8 | 12.2 | 13.6 | 15 |
pub fn scrub_cap(sensitivity: u8) -> f32 {
    super::seek::sens_scale(sensitivity, 3.0, 8.0, 15.0)
}

/// Taps per second for a signed `travel` (fraction of the pad along the band's
/// axis) at `sensitivity` (1..=10) on a band `band_length` long (fraction of
/// the edge, 0.30..=1.0). The SIGN of `travel` picks the direction (the
/// caller reads it); the rate uses the magnitude. Dead zone `SCRUB_DEAD_ZONE`
/// of the pad, then linear from `SCRUB_MIN_RATE` up to `scrub_cap` at
/// `SCRUB_RAMP_FRACTION × band_length`, then FLAT at the cap however far the
/// finger goes. Pure.
pub fn scrub_rate(travel: f32, sensitivity: u8, band_length: f32) -> f32 {
    let a = travel.abs();
    if !a.is_finite() || a < SCRUB_DEAD_ZONE {
        return 0.0;
    }
    let cap = scrub_cap(sensitivity);
    let floor = SCRUB_MIN_RATE.min(cap);
    let len = if band_length.is_finite() { band_length.clamp(0.30, 1.0) } else { 0.80 };
    let ramp_end = (SCRUB_RAMP_FRACTION * len).max(SCRUB_DEAD_ZONE + 1e-3);
    let t = ((a - SCRUB_DEAD_ZONE) / (ramp_end - SCRUB_DEAD_ZONE)).clamp(0.0, 1.0);
    floor + (cap - floor) * t
}

// ---------------------------------------------------------------------------
// Chord steps ("Any shortcut" — pure, tested)
// ---------------------------------------------------------------------------

/// The same dead zone as scrub, so the two feel alike at the start.
pub const CHORD_DEAD_ZONE: f32 = SCRUB_DEAD_ZONE;
/// Step size at sensitivity 1 and 10, as fractions of the pad's SHORT side.
pub const CHORD_STEP_SLOW: f32 = 0.12;
pub const CHORD_STEP_FAST: f32 = 0.02;

/// How far (fraction of the pad's short side) the finger travels per step:
/// 1 = one step per 12 %, 10 = one per 2 %, linear between. Pure.
pub fn chord_step_size(sensitivity: u8) -> f32 {
    let t = (sensitivity.clamp(1, 10) - 1) as f32 / 9.0;
    CHORD_STEP_SLOW + (CHORD_STEP_FAST - CHORD_STEP_SLOW) * t
}

/// The signed step count for a signed `travel` (already in short-side units,
/// `invert` already applied): 0 inside the dead zone, then one step per
/// `chord_step_size` beyond it, sign from the travel. Pure. The caller keeps
/// the last count it acted on and sends the forward chord for each +1 and the
/// backward chord for each −1, so a slide back undoes a slide out.
pub fn chord_steps(travel: f32, sensitivity: u8) -> i32 {
    let a = travel.abs();
    if !a.is_finite() || a < CHORD_DEAD_ZONE {
        return 0;
    }
    let n = ((a - CHORD_DEAD_ZONE) / chord_step_size(sensitivity)).floor() as i32 + 1;
    if travel < 0.0 {
        -n
    } else {
        n
    }
}

/// The k in `cur + k·Δtravel` for the analogue actions (volume, brightness).
/// `Δtravel` is a fraction of the pad, so a full-pad slide at the default
/// sensitivity moves the value by roughly `k`. Pure.
pub fn analogue_gain(sensitivity: u8) -> f32 {
    // Full-pad slide ≈ 1.0 (100 %) of range at sensitivity 6; scaled linearly.
    let sens = (sensitivity.clamp(1, 10) as f32) / 6.0;
    1.6 * sens
}

// ---------------------------------------------------------------------------
// Volume — Core Audio (same interface as boss_key::set_system_mute)
// ---------------------------------------------------------------------------

/// The default render endpoint's `IAudioEndpointVolume`, or `None`.
fn endpoint_volume() -> Option<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume> {
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    // SAFETY: plain COM object creation; the caller has CoInitialize'd.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
    }
}

/// Current master volume as a percent 0..=100, or `None`.
pub fn get_volume_pct() -> Option<u8> {
    let vol = endpoint_volume()?;
    // SAFETY: reads a scalar.
    let scalar = unsafe { vol.GetMasterVolumeLevelScalar().ok()? };
    Some((scalar.clamp(0.0, 1.0) * 100.0).round() as u8)
}

/// Nudge the master volume by `delta_frac` of full range and return the new
/// percent 0..=100. `delta_frac` is `k·Δtravel`.
pub fn add_volume(delta_frac: f32) -> Option<u8> {
    let vol = endpoint_volume()?;
    // SAFETY: read-modify-write of the endpoint scalar.
    unsafe {
        let cur = vol.GetMasterVolumeLevelScalar().ok()?;
        let next = (cur + delta_frac).clamp(0.0, 1.0);
        // GUID_NULL context: the change is not attributed to a session.
        vol.SetMasterVolumeLevelScalar(next, std::ptr::null()).ok()?;
        let read = vol.GetMasterVolumeLevelScalar().unwrap_or(next);
        Some((read.clamp(0.0, 1.0) * 100.0).round() as u8)
    }
}

// ---------------------------------------------------------------------------
// Brightness — WMI over COM (shared with engine::actions::brightness)
// ---------------------------------------------------------------------------

/// `root\wmi` `IWbemServices` with the proxy blanket set, or `None`.
fn wmi_services() -> Option<IWbemServices> {
    // SAFETY: standard WMI connect sequence; the caller has CoInitialize'd.
    unsafe {
        let locator: IWbemLocator =
            CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).ok()?;
        let services = locator
            .ConnectServer(
                &BSTR::from("root\\wmi"),
                &BSTR::new(),
                &BSTR::new(),
                &BSTR::new(),
                0,
                &BSTR::new(),
                None,
            )
            .ok()?;
        CoSetProxyBlanket(
            &services,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            windows::core::PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
        .ok()?;
        Some(services)
    }
}

/// The first instance of `class`, or `None`. `flags` is the enumeration flag.
unsafe fn first_instance(services: &IWbemServices, class: &str) -> Option<IWbemClassObject> {
    let query = format!("SELECT * FROM {class}");
    let enumerator = services
        .ExecQuery(
            &BSTR::from("WQL"),
            &BSTR::from(query.as_str()),
            WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
            None,
        )
        .ok()?;
    let mut row: [Option<IWbemClassObject>; 1] = [None];
    let mut returned = 0u32;
    // WBEM_INFINITE = -1.
    let _ = enumerator.Next(-1, &mut row, &mut returned);
    if returned == 0 {
        return None;
    }
    row[0].take()
}

/// Read a `u8`-valued property off a WMI object.
unsafe fn get_u8(obj: &IWbemClassObject, name: &str) -> Option<u8> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut v = VARIANT::default();
    obj.Get(windows::core::PCWSTR(wide.as_ptr()), 0, &mut v, None, None).ok()?;
    // The class declares CurrentBrightness as uint8; VARIANT carries it as an
    // integer, so route through u32 (the widest integer TryFrom windows-core
    // gives a VARIANT) and narrow.
    // VariantToUInt32 coerces VT_UI1 (the class's uint8) as well as VT_UI4.
    if let Ok(n) = u32::try_from(&v) {
        return Some(n.min(255) as u8);
    }
    let r = i32::try_from(&v).ok().map(|n| n.clamp(0, 255) as u8);
    if r.is_none() {
        log::warn!("touchpad: brightness — property {name} is not an integer VARIANT");
    }
    r
}

/// Read a BSTR-valued property (used for `__PATH`).
unsafe fn get_bstr(obj: &IWbemClassObject, name: &str) -> Option<BSTR> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut v = VARIANT::default();
    obj.Get(windows::core::PCWSTR(wide.as_ptr()), 0, &mut v, None, None).ok()?;
    BSTR::try_from(&v).ok()
}

/// Current internal-panel brightness as a percent 0..=100, or `None` when the
/// machine has no `WmiMonitorBrightness` instance (a desktop, an external-only
/// setup).
pub fn get_brightness() -> Option<u8> {
    let Some(services) = wmi_services() else {
        log::warn!("touchpad: brightness — could not connect to root/wmi (CoInitialize on this thread?)");
        return None;
    };
    // SAFETY: read-only WMI query.
    unsafe {
        let inst = first_instance(&services, "WmiMonitorBrightness")?;
        get_u8(&inst, "CurrentBrightness")
    }
}

/// Set the internal panel to `pct` (clamped 0..=100) via
/// `WmiMonitorBrightnessMethods.WmiSetBrightness(0, pct)`. Returns the value
/// set, or `None` on any failure (no panel, WMI refused).
pub fn set_brightness_abs(pct: u8) -> Option<u8> {
    let pct = pct.min(100);
    let services = wmi_services()?;
    // SAFETY: the WMI ExecMethod sequence — get the methods instance, read its
    // __PATH, build the in-params from the class's in-signature, ExecMethod.
    unsafe {
        let Some(methods) = first_instance(&services, "WmiMonitorBrightnessMethods") else {
            log::warn!("touchpad: brightness — no WmiMonitorBrightnessMethods instance (no internal panel?)");
            return None;
        };
        let Some(path) = get_bstr(&methods, "__PATH") else {
            log::warn!("touchpad: brightness — the methods instance has no __PATH");
            return None;
        };

        // The CLASS object owns the method signatures — an instance from
        // ExecQuery does not, and GetMethod on it fails (WBEM_E_INVALID_OPERATION),
        // which is why 1.0.120–122 never changed brightness while PowerShell's
        // WMI on the same laptop did (owner, 2026-09-19 18:20).
        let mut class: Option<IWbemClassObject> = None;
        if let Err(e) = services.GetObject(
            &BSTR::from("WmiMonitorBrightnessMethods"),
            WBEM_GENERIC_FLAG_TYPE(0),
            None,
            Some(&mut class),
            None,
        ) {
            log::warn!("touchpad: brightness — GetObject(class) failed: {e}");
            return None;
        }
        let class = class?;
        let mut in_sig: Option<IWbemClassObject> = None;
        class
            .GetMethod(
                windows::core::PCWSTR(
                    "WmiSetBrightness\0".encode_utf16().collect::<Vec<u16>>().as_ptr(),
                ),
                0,
                &mut in_sig,
                &mut None,
            )
            .map_err(|e| log::warn!("touchpad: brightness — GetMethod failed: {e}"))
            .ok()?;
        let in_sig = in_sig?;
        let in_params = in_sig
            .SpawnInstance(0)
            .map_err(|e| log::warn!("touchpad: brightness — SpawnInstance failed: {e}"))
            .ok()?;

        let timeout: VARIANT = 0u32.into();
        in_params
            .Put(
                windows::core::PCWSTR("Timeout\0".encode_utf16().collect::<Vec<u16>>().as_ptr()),
                0,
                &timeout,
                0,
            )
            .ok()?;
        let brightness: VARIANT = (pct as u32).into();
        in_params
            .Put(
                windows::core::PCWSTR(
                    "Brightness\0".encode_utf16().collect::<Vec<u16>>().as_ptr(),
                ),
                0,
                &brightness,
                0,
            )
            .ok()?;

        services
            .ExecMethod(
                &path,
                &BSTR::from("WmiSetBrightness"),
                WBEM_GENERIC_FLAG_TYPE(0),
                None,
                &in_params,
                None,
                None,
            )
            .map_err(|e| log::warn!("touchpad: brightness — ExecMethod(WmiSetBrightness) failed: {e}"))
            .ok()?;
        log::info!("touchpad: brightness set to {pct}% via WMI COM");
        Some(pct)
    }
}

/// Nudge brightness by `delta` percentage points and return the new value.
/// Shared by `engine::actions::brightness::adjust` so both paths are COM.
pub fn add_brightness(delta: i32) -> Option<u8> {
    let cur = get_brightness()? as i32;
    let next = (cur + delta).clamp(0, 100) as u8;
    set_brightness_abs(next)
}

// ---------------------------------------------------------------------------
// Scrub — ←/→ taps, cookie-tagged so our own hook passes them through
// ---------------------------------------------------------------------------

pub const VK_LEFT: u16 = 0x25;
pub const VK_RIGHT: u16 = 0x27;

/// Press one chord (downs in order, ups in reverse, ONE `send_keys_checked`
/// batch, cookie `0x7A7A7A7A` — PROBLEM 227: a partial insert leaves nothing
/// latched). The scrub taps and the "Any shortcut" steps both come here. An
/// empty or over-long chord sends nothing.
pub fn send_chord(keys: &[u16]) {
    if keys.is_empty() || keys.len() > crate::engine::actions::chord::MAX_KEYS {
        return;
    }
    // SAFETY: SendInput of cookie-tagged keyboard events; not the hook
    // callback, so logging inside send_keys_checked is fine.
    unsafe {
        crate::engine::actions::chord::send_batch(keys);
    }
}

/// Send one ←/→ tap (down+up). `forward` = Right.
pub fn scrub_tap(forward: bool) {
    send_chord(&[if forward { VK_RIGHT } else { VK_LEFT }]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_rate_has_a_dead_zone_then_climbs_to_the_cap() {
        let len = 0.8; // the top band's default length
        assert_eq!(scrub_rate(0.0, 5, len), 0.0);
        assert_eq!(scrub_rate(0.02, 5, len), 0.0, "inside the dead zone");
        assert_eq!(scrub_rate(-0.02, 5, len), 0.0, "sign does not escape the dead zone");
        assert_eq!(scrub_rate(f32::NAN, 5, len), 0.0);
        // Just past the dead zone → near the minimum rate.
        let low = scrub_rate(0.04, 5, len);
        assert!(low >= SCRUB_MIN_RATE && low < SCRUB_MIN_RATE + 1.0, "{low}");
        // At 40 % of the band (0.32 of the pad) → the cap, 8 taps/s at 5.
        assert!((scrub_rate(0.32, 5, len) - 8.0).abs() < 1e-4);
        assert!((scrub_rate(-0.32, 5, len) - 8.0).abs() < 1e-4, "magnitude, not sign");
    }

    /// Owner, 2026-09-20 00:58: past ~40 % of the band the rate is FLAT — no
    /// faster however far the finger goes — and the cap is 8 taps/s at the
    /// default sensitivity (3 … 15 over 1..10).
    #[test]
    fn scrub_rate_is_flat_past_forty_percent_of_the_band_and_capped_per_sensitivity() {
        let len = 0.8;
        for s in 1..=10u8 {
            let cap = scrub_cap(s);
            for a in [0.32f32, 0.4, 0.5, 0.75, 1.0] {
                assert!((scrub_rate(a, s, len) - cap).abs() < 1e-4, "s={s} a={a}: {} != {cap}", scrub_rate(a, s, len));
            }
        }
        // The documented cap table.
        for (s, want) in [(1u8, 3.0f32), (2, 4.25), (3, 5.5), (4, 6.75), (5, 8.0), (6, 9.4), (7, 10.8), (8, 12.2), (9, 13.6), (10, 15.0)] {
            assert!((scrub_cap(s) - want).abs() < 1e-4, "s={s}: {} != {want}", scrub_cap(s));
        }
        // The ramp end follows the band: a full-length band reaches the cap at
        // 0.40 of the pad, a 0.30 band at 0.12.
        assert!(scrub_rate(0.32, 5, 1.0) < 8.0 - 1e-3, "a longer band ramps longer");
        assert!((scrub_rate(0.40, 5, 1.0) - 8.0).abs() < 1e-4);
        assert!((scrub_rate(0.12, 5, 0.30) - 8.0).abs() < 1e-4, "a short band ramps fast");
    }

    #[test]
    fn scrub_rate_is_monotonic_and_sensitivity_only_raises_the_cap() {
        let len = 0.8;
        assert!(scrub_rate(0.1, 5, len) < scrub_rate(0.2, 5, len));
        assert!(scrub_rate(0.5, 5, len) <= scrub_rate(0.5, 10, len), "more sensitive is faster");
        assert!(scrub_rate(0.5, 3, len) <= scrub_rate(0.5, 5, len), "less sensitive is slower");
        // Every rate stays inside [min, cap] and never decreases with travel.
        for s in 1..=10u8 {
            let cap = scrub_cap(s);
            let mut last = 0.0f32;
            for i in 0..=40 {
                let r = scrub_rate(i as f32 / 40.0, s, len);
                assert!(r == 0.0 || (SCRUB_MIN_RATE.min(cap)..=cap + 1e-4).contains(&r), "s={s} r={r}");
                assert!(r >= last - 1e-5, "s={s} i={i}: {r} < {last}");
                last = r;
            }
        }
    }

    #[test]
    fn chord_steps_quantise_travel_past_the_dead_zone_in_both_signs() {
        // Dead zone, both signs.
        assert_eq!(chord_steps(0.0, 6), 0);
        assert_eq!(chord_steps(0.029, 6), 0);
        assert_eq!(chord_steps(-0.029, 6), 0);
        assert_eq!(chord_steps(f32::NAN, 6), 0);
        // Just past the dead zone: the first step, in the travel's sign.
        assert_eq!(chord_steps(0.031, 6), 1);
        assert_eq!(chord_steps(-0.031, 6), -1);
        // Sensitivity 1: one step per 12 % → 0.03 + 0.12 = 0.15 is step 2.
        assert!((chord_step_size(1) - 0.12).abs() < 1e-6);
        assert_eq!(chord_steps(0.149, 1), 1);
        assert_eq!(chord_steps(0.151, 1), 2);
        assert_eq!(chord_steps(-0.151, 1), -2);
        // Sensitivity 10: one step per 2 % → 0.03 + 0.02·k.
        assert!((chord_step_size(10) - 0.02).abs() < 1e-6);
        assert_eq!(chord_steps(0.049, 10), 1);
        assert_eq!(chord_steps(0.051, 10), 2);
        assert_eq!(chord_steps(0.231, 10), 11);
        // Monotonic in travel and in sensitivity; a full slide never explodes.
        for s in 1..=10u8 {
            let mut last = 0;
            for i in 0..=100 {
                let n = chord_steps(i as f32 / 100.0, s);
                assert!(n >= last, "s={s} i={i}");
                last = n;
            }
            assert!(chord_steps(0.5, s) <= chord_steps(0.5, 10));
            assert!(chord_steps(1.0, s) <= 49);
        }
        // Invert is applied upstream by the gesture (travel sign flips), so
        // the quantiser only has to be odd: steps(-t) == -steps(t).
        for i in 0..=20 {
            let t = i as f32 / 20.0;
            assert_eq!(chord_steps(-t, 6), -chord_steps(t, 6));
        }
    }

    #[test]
    fn analogue_gain_scales_with_sensitivity() {
        assert!(analogue_gain(1) < analogue_gain(6));
        assert!(analogue_gain(6) < analogue_gain(10));
        assert!(analogue_gain(6) > 0.0);
    }
}

// ---------------------------------------------------------------------------
// Brightness WORKER — one hidden PowerShell per slide (2026-09-19)
// ---------------------------------------------------------------------------
//
// The in-process WMI COM path above failed silently on the owner's laptop
// (1.0.120–1.0.123: the toast read 0 or a stale 60 and the panel never
// moved) while `Get-CimInstance root/wmi WmiMonitorBrightness` in PowerShell
// worked every time. So the reader starts ONE hidden `powershell.exe` when a
// brightness band goes live, reads the current value from its first output
// line, feeds it absolute percentages on stdin while the finger slides, and
// closes it on lift. Start-up (~300 ms) is paid once per slide, never per tick.

/// A live brightness session. `current()` is the value the worker last set
/// (or read at start); `add()` clamps and sends; `stop()` closes the process.
pub struct BrightnessWorker {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    current: u8,
}

impl BrightnessWorker {
    /// The script: print the current brightness once, then set every integer
    /// line read from stdin until "q".
    pub fn script() -> &'static str {
        "$ErrorActionPreference='SilentlyContinue'; \
         $b = Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightness | Select-Object -First 1; \
         if (-not $b) { Write-Output 'none'; exit 3 }; \
         $m = Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightnessMethods | Select-Object -First 1; \
         Write-Output ([int]$b.CurrentBrightness); \
         while ($true) { $l = [Console]::In.ReadLine(); if ($null -eq $l -or $l -eq 'q') { break }; \
           $n = 0; if ([int]::TryParse($l, [ref]$n)) { \
             Invoke-CimMethod -InputObject $m -MethodName WmiSetBrightness -Arguments @{Timeout=0; Brightness=[Math]::Max(0,[Math]::Min(100,$n))} | Out-Null } }"
    }

    /// Spawn the worker and read the starting value. `None` (with a warn)
    /// when there is no internal panel or PowerShell could not start.
    pub fn start() -> Option<Self> {
        #[cfg(windows)]
        {
            use std::io::BufRead;
            use std::os::windows::process::CommandExt;
            use std::process::{Command, Stdio};
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let mut child = Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", Self::script()])
                .creation_flags(CREATE_NO_WINDOW)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| log::warn!("touchpad: brightness worker — could not start PowerShell: {e}"))
                .ok()?;
            let stdin = child.stdin.take()?;
            let stdout = child.stdout.take()?;
            let mut first = String::new();
            let _ = std::io::BufReader::new(stdout).read_line(&mut first);
            let current = match first.trim().parse::<u8>() {
                Ok(v) => v.min(100),
                Err(_) => {
                    log::warn!("touchpad: brightness worker — no internal panel (first line {:?})", first.trim());
                    let _ = child.kill();
                    return None;
                }
            };
            log::info!("touchpad: brightness worker started at {current}%");
            Some(Self { child, stdin, current })
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    pub fn current(&self) -> u8 {
        self.current
    }

    /// Nudge by `delta` points; returns the new value it asked for.
    pub fn add(&mut self, delta: i32) -> Option<u8> {
        use std::io::Write;
        let next = (self.current as i32 + delta).clamp(0, 100) as u8;
        if next == self.current {
            return Some(next);
        }
        if let Err(e) = writeln!(self.stdin, "{next}") {
            log::warn!("touchpad: brightness worker — write failed: {e}");
            return None;
        }
        let _ = self.stdin.flush();
        self.current = next;
        Some(next)
    }

    /// Close the worker (tells it to quit, then makes sure it is gone).
    pub fn stop(mut self) {
        use std::io::Write;
        let _ = writeln!(self.stdin, "q");
        let _ = self.stdin.flush();
        drop(self.stdin);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(400);
        while std::time::Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                log::info!("touchpad: brightness worker closed at {}%", self.current);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let _ = self.child.kill();
        log::info!("touchpad: brightness worker killed at {}%", self.current);
    }
}

#[cfg(test)]
mod worker_tests {
    use super::BrightnessWorker;
    #[test]
    fn the_worker_script_reads_once_then_sets_from_stdin() {
        let s = BrightnessWorker::script();
        assert!(s.contains("WmiMonitorBrightness"));
        assert!(s.contains("WmiSetBrightness"));
        assert!(s.contains("ReadLine"));
        assert!(s.contains("Brightness=[Math]::Max(0,[Math]::Min(100,$n))"));
    }
}
