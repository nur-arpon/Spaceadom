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
//! * **Scrub** — ←/→ taps through `send_chord` (cookie `0x7A7A7A7A`,
//!   PROBLEM 227). MOVEMENT-based since 1.0.131 (owner, 2026-09-20 01:15:
//!   "scrubbing lightly did nothing, then holding kept going forward forward
//!   forward even at the slowest setting"): one tap per STEP of travel along
//!   the band, direction from the sign, nothing while the finger is still,
//!   the other arrow when it comes back — exactly the Chords step engine
//!   (`quantise_steps`) with a step measured in MILLIMETRES
//!   (`scrub_step_mm`: 12 mm at sensitivity 1, 6 mm at 5, 2 mm at 10). The
//!   rate machinery (`scrub_rate`, `scrub_cap`, the ramp) is gone. Also the
//!   automatic FALLBACK inside a "Video seek" gesture that finds no seekable
//!   media session (`seek.rs`) — same step model — never a replacement for
//!   the user's own choice of Scrub in the list.
//! * **Seek** (1.0.130) — `seek.rs`: the media session's position follows the
//!   finger in pure proportion.
//! * **Presets** (1.0.130) — `presets.rs`: named chord pairs driven like
//!   Chords (or once per slide).
//! * **Chords** ("Any shortcut", 1.0.122) — STEP-based: `chord_steps` quantises
//!   the signed travel into steps (`chord_step_size(sensitivity)` of the pad's
//!   short side, past the same dead zone as scrub) and `touchpad::mod` sends
//!   the forward/backward chord once per new step (`step_towards`). Scrub and
//!   chords go out through one path, `send_chord` →
//!   `engine::actions::chord::send_batch`.
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
// Steps (pure — the one quantiser scrub and chords share)
// ---------------------------------------------------------------------------

/// Dead zone before any stepping begins: the fraction of the pad's SHORT side
/// a finger must travel from its landing point before the first step. Scrub
/// and chords share it, so the two feel alike at the start.
pub const SCRUB_DEAD_ZONE: f32 = 0.03;

/// The signed step count for a signed `travel` past a `dead_zone`, one step
/// per `step` beyond it — the FIRST step fires on leaving the dead zone, the
/// next every `step` after that. 0 inside the dead zone or for garbage
/// (NaN, a non-positive step). Odd in `travel`: `steps(−t) == −steps(t)`.
/// Pure. The caller keeps the count it has acted on and brings it up or
/// down to this target (`step_towards`), so a slide back undoes a slide out.
pub fn quantise_steps(travel: f32, dead_zone: f32, step: f32) -> i32 {
    let a = travel.abs();
    if !a.is_finite() || a < dead_zone || !(step > 0.0) || !step.is_finite() {
        return 0;
    }
    let n = ((a - dead_zone) / step).floor() as i32 + 1;
    if travel < 0.0 {
        -n
    } else {
        n
    }
}

/// Bring `sent` to `target` one step at a time, calling `send(true)` for
/// each +1 and `send(false)` for each −1 — the movement model: a target that
/// has not changed sends nothing (a still finger), a target below the count
/// sends the backward key (the finger came back). Returns how many sends
/// went out. Pure apart from `send`.
pub fn step_towards(sent: &mut i32, target: i32, mut send: impl FnMut(bool)) -> u32 {
    let mut n = 0;
    while *sent < target {
        send(true);
        *sent += 1;
        n += 1;
    }
    while *sent > target {
        send(false);
        *sent -= 1;
        n += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// Scrub steps (pure — millimetres per ←/→ tap)
// ---------------------------------------------------------------------------

/// Millimetres of finger travel per ←/→ tap at sensitivity 1, 5 and 10.
pub const SCRUB_STEP_SLOW_MM: f32 = 12.0;
pub const SCRUB_STEP_MID_MM: f32 = 6.0;
pub const SCRUB_STEP_FAST_MM: f32 = 2.0;
/// The short side assumed when the HID descriptor gives no physical size
/// (`PadCaps::size_mm` = `None`), so the mm table still means something:
/// a typical Precision pad is 60–80 mm tall.
pub const SCRUB_FALLBACK_SHORT_MM: f32 = 60.0;

/// Millimetres per tap by sensitivity — 12 mm at 1, **6 mm at 5 (the
/// default)**, 2 mm at 10, piecewise linear (`seek::sens_scale`). Pure.
///
/// | sens | 1  | 2    | 3   | 4   | 5 | 6   | 7   | 8   | 9   | 10 |
/// |------|----|------|-----|-----|---|-----|-----|-----|-----|----|
/// | mm   | 12 | 10.5 | 9   | 7.5 | 6 | 5.2 | 4.4 | 3.6 | 2.8 | 2  |
pub fn scrub_step_mm(sensitivity: u8) -> f32 {
    super::seek::sens_scale(sensitivity, SCRUB_STEP_SLOW_MM, SCRUB_STEP_MID_MM, SCRUB_STEP_FAST_MM)
}

/// The scrub step as a fraction of the pad's SHORT side — what the quantiser
/// wants, since travel arrives in short-side units (`travel_short_side`).
/// `short_mm` is the pad's physical short side when the descriptor gives one;
/// `None` (or a bad value) falls back to `SCRUB_FALLBACK_SHORT_MM`, so the
/// table then reads 12/60 = 0.20 … 6/60 = 0.10 … 2/60 = 0.033 of the short
/// side. Pure.
pub fn scrub_step_size(sensitivity: u8, short_mm: Option<f32>) -> f32 {
    let mm = match short_mm {
        Some(v) if v.is_finite() && v > 0.0 => v,
        _ => SCRUB_FALLBACK_SHORT_MM,
    };
    scrub_step_mm(sensitivity) / mm
}

/// The signed ←/→ tap target for a signed `travel` in short-side units:
/// `quantise_steps` with the scrub dead zone and `scrub_step_size`. Pure.
/// Scrub IS `Chords { →, ← }` with this step table.
pub fn scrub_steps(travel: f32, sensitivity: u8, short_mm: Option<f32>) -> i32 {
    quantise_steps(travel, SCRUB_DEAD_ZONE, scrub_step_size(sensitivity, short_mm))
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
    quantise_steps(travel, CHORD_DEAD_ZONE, chord_step_size(sensitivity))
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
// Scrub's keys — ←/→, cookie-tagged so our own hook passes them through
// ---------------------------------------------------------------------------

pub const VK_LEFT: u16 = 0x25;
pub const VK_RIGHT: u16 = 0x27;
/// Scrub's chords (one → / ← tap each): scrub is `Chords { →, ← }`.
pub const SCRUB_FORWARD: &[u16] = &[VK_RIGHT];
pub const SCRUB_BACKWARD: &[u16] = &[VK_LEFT];

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

#[cfg(test)]
mod tests {
    use super::*;

    /// 1.0.131 (owner, 2026-09-20 01:15): scrub is MOVEMENT — one tap per
    /// step of travel, in millimetres by sensitivity, nothing for a light
    /// touch inside the dead zone, the first tap on leaving it.
    #[test]
    fn the_scrub_step_table_is_millimetres_by_sensitivity() {
        for (s, want) in [(1u8, 12.0f32), (2, 10.5), (3, 9.0), (4, 7.5), (5, 6.0), (6, 5.2), (7, 4.4), (8, 3.6), (9, 2.8), (10, 2.0)] {
            assert!((scrub_step_mm(s) - want).abs() < 1e-4, "s={s}: {} != {want}", scrub_step_mm(s));
        }
        assert_eq!(scrub_step_mm(0), scrub_step_mm(1), "clamped below");
        assert_eq!(scrub_step_mm(99), scrub_step_mm(10), "clamped above");
        for s in 1..10u8 {
            assert!(scrub_step_mm(s) > scrub_step_mm(s + 1), "finer with sensitivity at {s}");
        }
        // On a 74 mm short side, 6 mm is 0.081 of it; with no size known the
        // 60 mm fallback makes the table 0.20 / 0.10 / 0.033.
        assert!((scrub_step_size(5, Some(74.0)) - 6.0 / 74.0).abs() < 1e-6);
        assert!((scrub_step_size(1, None) - 0.20).abs() < 1e-6);
        assert!((scrub_step_size(5, None) - 0.10).abs() < 1e-6);
        assert!((scrub_step_size(10, None) - 2.0 / 60.0).abs() < 1e-6);
        assert_eq!(scrub_step_size(5, Some(0.0)), scrub_step_size(5, None), "a zero size is unknown");
        assert_eq!(scrub_step_size(5, Some(f32::NAN)), scrub_step_size(5, None), "NaN is unknown");
    }

    #[test]
    fn scrub_steps_are_nothing_for_a_light_touch_one_on_leaving_the_dead_zone_then_one_per_step() {
        let mm = Some(74.0f32); // the short side; a step at 5 = 6 mm = 0.0811
        let step = scrub_step_size(5, mm);
        let dz = SCRUB_DEAD_ZONE;
        // Light travel below the dead zone (≈ 2.2 mm on this pad) → no tap.
        assert_eq!(scrub_steps(0.0, 5, mm), 0);
        assert_eq!(scrub_steps(dz - 1e-4, 5, mm), 0, "just inside the dead zone");
        assert_eq!(scrub_steps(-(dz - 1e-4), 5, mm), 0);
        assert_eq!(scrub_steps(f32::NAN, 5, mm), 0);
        // Leaving the dead zone → exactly ONE tap, and still one for the
        // whole first step.
        assert_eq!(scrub_steps(dz + 1e-4, 5, mm), 1);
        assert_eq!(scrub_steps(dz + step * 0.5, 5, mm), 1, "halfway through the first step: still one");
        assert_eq!(scrub_steps(dz + step - 1e-4, 5, mm), 1, "the whole first step: still one");
        // Exactly one more step → a second tap; then one per step.
        assert_eq!(scrub_steps(dz + step + 1e-4, 5, mm), 2);
        assert_eq!(scrub_steps(dz + 4.0 * step + 1e-4, 5, mm), 5);
        // Backward is the mirror.
        assert_eq!(scrub_steps(-(dz + 1e-4), 5, mm), -1);
        assert_eq!(scrub_steps(-(dz + 4.0 * step + 1e-4), 5, mm), -5);
        // The owner's pad: 119 mm long, a full 80 % band is 95 mm ≈ 15 taps
        // at 6 mm — never runaway. Travel arrives in short-side units, so a
        // 95 mm slide on a 74 mm short side is 1.28.
        assert_eq!(scrub_steps(95.0 / 74.0, 5, mm), 16);
        assert_eq!(scrub_steps(95.0 / 74.0, 1, mm), 8, "12 mm per tap at 1");
        assert_eq!(scrub_steps(95.0 / 74.0, 10, mm), 47, "2 mm per tap at 10");
        // Monotonic and odd, every sensitivity, with and without a size.
        for size in [Some(74.0f32), None] {
            for s in 1..=10u8 {
                let mut last = 0;
                for i in 0..=100 {
                    let t = i as f32 / 50.0;
                    let n = scrub_steps(t, s, size);
                    assert!(n >= last, "s={s} i={i}");
                    assert_eq!(scrub_steps(-t, s, size), -n, "odd at s={s} t={t}");
                    last = n;
                }
            }
        }
    }

    /// The reader's step engine: bring the sent count to the target. A
    /// still finger (same target) sends nothing however many ticks pass; a
    /// return past the landing point sends the backward key.
    #[test]
    fn step_towards_sends_one_key_per_step_nothing_while_still_and_backward_on_the_way_back() {
        let mut sent = 0i32;
        let mut log: Vec<bool> = Vec::new();
        // A slide out to 3 steps, reported over several ticks.
        assert_eq!(step_towards(&mut sent, 0, |f| log.push(f)), 0, "inside the dead zone: nothing");
        assert_eq!(step_towards(&mut sent, 1, |f| log.push(f)), 1);
        assert_eq!(step_towards(&mut sent, 3, |f| log.push(f)), 2, "two steps in one tick: two taps");
        assert_eq!(log, [true, true, true]);
        // Still finger: 50 ticks at the same target send nothing.
        for _ in 0..50 {
            assert_eq!(step_towards(&mut sent, 3, |f| log.push(f)), 0);
        }
        assert_eq!(log.len(), 3, "nothing over time while still");
        // Back to the landing point: three backward taps, then past it: two more.
        assert_eq!(step_towards(&mut sent, 0, |f| log.push(f)), 3);
        assert_eq!(step_towards(&mut sent, -2, |f| log.push(f)), 2);
        assert_eq!(log, [true, true, true, false, false, false, false, false]);
        assert_eq!(sent, -2);
        // The same engine drives scrub end to end: mm travel → taps.
        let mm = Some(74.0f32);
        let mut sent = 0;
        let mut taps = 0u32;
        for t in [0.0f32, 0.01, 0.02, 0.05, 0.05, 0.05, 0.10, 0.20, 0.20, 0.10, 0.0, -0.10] {
            taps += step_towards(&mut sent, scrub_steps(t, 5, mm), |_| {});
        }
        assert_eq!(sent, scrub_steps(-0.10, 5, mm));
        assert_eq!(sent, -1);
        assert_eq!(taps, 3 + 3 + 1, "3 out (0.20 = 2 steps + the first), 3 back, 1 past");
        // The generic quantiser refuses a bad step.
        assert_eq!(quantise_steps(0.5, 0.03, 0.0), 0);
        assert_eq!(quantise_steps(0.5, 0.03, f32::NAN), 0);
        assert_eq!(quantise_steps(0.5, 0.03, -1.0), 0);
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
