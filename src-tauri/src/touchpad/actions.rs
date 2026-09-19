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
//!   `0x7A7A7A7A`, PROBLEM 227), at a rate `scrub_rate(travel, sensitivity)`.
//!   RATE-based (continuous, a Touch Bar scrubber) — kept that way in 1.0.122.
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
/// The slowest and fastest continuous tap rates (taps per second).
pub const SCRUB_MIN_RATE: f32 = 2.0;
pub const SCRUB_MAX_RATE: f32 = 25.0;

/// Taps per second for a signed `travel` at `sensitivity` (1..=10). The SIGN
/// of `travel` picks the direction (the caller reads it); the rate uses the
/// magnitude. Dead zone `SCRUB_DEAD_ZONE` of the pad, then linear from
/// `SCRUB_MIN_RATE` to `SCRUB_MAX_RATE` as the magnitude runs to full travel,
/// with sensitivity reaching the top rate sooner. Pure.
pub fn scrub_rate(travel: f32, sensitivity: u8) -> f32 {
    let a = travel.abs();
    if !a.is_finite() || a < SCRUB_DEAD_ZONE {
        return 0.0;
    }
    let sens = (sensitivity.clamp(1, 10) as f32) / 6.0; // 1.0 at the default 6
    let span = (1.0 - SCRUB_DEAD_ZONE).max(1e-3);
    let t = (((a - SCRUB_DEAD_ZONE) / span) * sens).clamp(0.0, 1.0);
    SCRUB_MIN_RATE + (SCRUB_MAX_RATE - SCRUB_MIN_RATE) * t
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
    u32::try_from(&v).ok().map(|n| n.min(255) as u8)
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
    let services = wmi_services()?;
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
        let methods = first_instance(&services, "WmiMonitorBrightnessMethods")?;
        let path = get_bstr(&methods, "__PATH")?;

        // The class object (not the instance) owns the method signatures.
        let class = first_instance(&services, "WmiMonitorBrightnessMethods")?;
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
            .ok()?;
        let in_sig = in_sig?;
        let in_params = in_sig.SpawnInstance(0).ok()?;

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
            .ok()?;
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
        assert_eq!(scrub_rate(0.0, 6), 0.0);
        assert_eq!(scrub_rate(0.02, 6), 0.0, "inside the dead zone");
        assert_eq!(scrub_rate(-0.02, 6), 0.0, "sign does not escape the dead zone");
        // Just past the dead zone → near the minimum rate.
        let low = scrub_rate(0.04, 6);
        assert!(low >= SCRUB_MIN_RATE && low < SCRUB_MIN_RATE + 2.0, "{low}");
        // Full travel → the maximum.
        assert!((scrub_rate(1.0, 6) - SCRUB_MAX_RATE).abs() < 1e-4);
        assert!((scrub_rate(-1.0, 6) - SCRUB_MAX_RATE).abs() < 1e-4, "magnitude, not sign");
    }

    #[test]
    fn scrub_rate_is_monotonic_and_sensitivity_reaches_full_sooner() {
        assert!(scrub_rate(0.2, 6) < scrub_rate(0.5, 6));
        assert!(scrub_rate(0.5, 6) <= scrub_rate(0.5, 10), "more sensitive is faster");
        assert!(scrub_rate(0.5, 3) <= scrub_rate(0.5, 6), "less sensitive is slower");
        // Every rate stays inside the documented band.
        for s in 1..=10u8 {
            for i in 0..=20 {
                let r = scrub_rate(i as f32 / 20.0, s);
                assert!(r == 0.0 || (SCRUB_MIN_RATE..=SCRUB_MAX_RATE).contains(&r), "s={s} r={r}");
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
