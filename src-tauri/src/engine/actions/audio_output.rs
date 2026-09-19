//! "Next speaker" (special id `next_speaker`, 1.0.125) — move Windows'
//! DEFAULT OUTPUT device to the next one in a stable list of the ACTIVE
//! render endpoints, wrapping, and say where it went: "🔊 → Headphones
//! (Realtek Audio)". Owner ask 2026-09-19. NOT seeded on any key — the user
//! binds it from the key editor's "Spaceadom special" list (Advanced mode).
//!
//! HOW THE LIST IS BUILT. `IMMDeviceEnumerator::EnumAudioEndpoints(eRender,
//! DEVICE_STATE_ACTIVE)` — connected AND enabled endpoints only, in the order
//! Windows hands them back (stable between calls while nothing is plugged or
//! unplugged). Each device's id (`IMMDevice::GetId`, the `{0.0.0.00000000}.
//! {guid}` string) and friendly name (`PKEY_Device_FriendlyName` from its
//! property store). The current default is `GetDefaultAudioEndpoint(eRender,
//! eConsole)`; `next_index` (pure, tested) picks the one after it, wrapping,
//! and `Some(0)` when the default is not in the active list at all (a device
//! that was just unplugged is still "default" for a moment).
//!
//! HOW THE DEFAULT IS SET — READ THIS BEFORE TOUCHING IT. Windows has NO
//! documented API to set the default audio endpoint; the Sound control panel
//! does it through a private COM object, `PolicyConfigClient`, CLSID
//! `{870af99c-171d-4f9e-af0d-e63df40c2bc9}`, interface `IPolicyConfig` IID
//! `{f8679f50-850a-41cf-9c72-430f290290c8}`, method `SetDefaultEndpoint(
//! deviceId, role)`. It is UNDOCUMENTED BUT STANDARD: the same interface,
//! same vtable, that SoundSwitch, nircmd (`setdefaultsounddevice`), EarTrumpet
//! and AudioSwitcher have called since Windows 7, unchanged through Windows 11
//! 26200. The vtable below is declared in full (twelve methods, in order) with
//! `windows-core`'s `#[interface]` macro; only `SetDefaultEndpoint` is ever
//! called, the others exist so the slot index is right. It is called for all
//! THREE roles — eConsole, eMultimedia, eCommunications — so every app follows
//! (a media player reads eMultimedia, a call app eCommunications; Windows'
//! own panel sets all three when "Set as default" is pressed).
//!
//! FAILURE IS A TOAST, NEVER A PANIC. If `CoCreateInstance` of the policy
//! object fails (a future Windows that removes it), or any role's call answers
//! an error, the failure is logged with the HRESULT and the toast says
//! "🔊 Windows refused to switch". No elevation is needed or requested — the
//! panel does this as the user, and so do we. Reversible by the owner's rule
//! in CLAUDE.md: the same key cycles back round.
//!
//! No sound is ever muted or played, nothing is written to disk. The COM
//! calls run on the engine thread; `CoInitializeEx(MTA)` is called per press,
//! exactly as `boss_key::set_system_mute` and the touchpad actions do.

/// The toast for a successful switch: "🔊 → <friendly name>".
pub fn toast_text(name: &str) -> String {
    format!("🔊 → {name}")
}

/// One active render endpoint, one other — nothing to cycle to.
pub const ONLY_ONE_TOAST: &str = "🔊 Only one speaker connected";
/// No active render endpoint at all (a dock unplugged, a driver mid-reset).
pub const NO_SPEAKER_TOAST: &str = "🔊 No speaker connected";
/// The undocumented policy object refused, or was not there.
pub const REFUSED_TOAST: &str = "🔊 Windows refused to switch";

/// PURE — the index in `ids` of the device to switch to, given the id of the
/// current default. `None` when there is nothing to switch to (zero or one
/// device). The one after `current`, wrapping; `Some(0)` when `current` is
/// not in the list at all.
pub fn next_index<S: AsRef<str>>(current: &str, ids: &[S]) -> Option<usize> {
    if ids.len() < 2 {
        return None;
    }
    match ids.iter().position(|id| id.as_ref() == current) {
        Some(i) => Some((i + 1) % ids.len()),
        None => Some(0),
    }
}

/// Cycle the default output device and return the toast line. Every branch
/// returns a toast; nothing here panics on a COM error. NEVER called from a
/// test that is not `#[ignore]` — it really changes the machine's speaker.
pub fn next_speaker() -> String {
    #[cfg(windows)]
    {
        win::next_speaker()
    }
    #[cfg(not(windows))]
    {
        REFUSED_TOAST.to_string()
    }
}

#[cfg(windows)]
pub(crate) mod win {
    // The COM vtable keeps Windows' PascalCase method names, like every
    // interface the `windows` crate itself generates.
    #![allow(non_snake_case)]

    use super::{next_index, toast_text, NO_SPEAKER_TOAST, ONLY_ONE_TOAST, REFUSED_TOAST};
    use core::ffi::c_void;
    // `IUnknown_Vtbl` is not used by name here — the `#[interface]` macro
    // expects it in scope beside `IUnknown`.
    use windows::core::{IUnknown, IUnknown_Vtbl, Result, GUID, HRESULT, PCWSTR, HSTRING};
    use windows::Win32::Media::Audio::{
        eCommunications, eConsole, eMultimedia, eRender, ERole, IMMDevice, IMMDeviceEnumerator,
        MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;

    /// `PolicyConfigClient` — the Sound panel's private object (see the module
    /// header). Not in the `windows` crate because it is not in the SDK.
    const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    /// `PKEY_Device_FriendlyName` = `{a45c254e-df1c-4efd-8020-67d146a850e0}, 14`
    /// (functiondiscoverykeys_devpkey.h). Declared here rather than pulling in
    /// the `Win32_Devices_FunctionDiscovery` feature for one constant.
    const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY =
        PROPERTYKEY { fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 14 };

    /// The Windows 7+ `IPolicyConfig` vtable, in order. Only `SetDefaultEndpoint`
    /// is called; the preceding ten are declared with opaque pointers so the
    /// slot arithmetic is right (IUnknown's three, then these twelve).
    #[windows::core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    unsafe trait IPolicyConfig: IUnknown {
        fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut c_void) -> HRESULT;
        fn GetDeviceFormat(&self, device: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT;
        fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
        fn SetDeviceFormat(&self, device: PCWSTR, endpoint_format: *mut c_void, mix_format: *mut c_void) -> HRESULT;
        fn GetProcessingPeriod(&self, device: PCWSTR, default: i32, default_period: *mut i64, min_period: *mut i64) -> HRESULT;
        fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> HRESULT;
        fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn SetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn GetPropertyValue(&self, device: PCWSTR, fx_store: i32, key: *const c_void, value: *mut c_void) -> HRESULT;
        fn SetPropertyValue(&self, device: PCWSTR, fx_store: i32, key: *const c_void, value: *mut c_void) -> HRESULT;
        fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> HRESULT;
        fn SetEndpointVisibility(&self, device: PCWSTR, visible: i32) -> HRESULT;
    }

    /// One active render endpoint: its Core Audio id and its friendly name.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Endpoint {
        pub id: String,
        pub name: String,
    }

    /// `IMMDevice::GetId` → owned `String`; the PWSTR is CoTaskMem and freed here.
    unsafe fn device_id(device: &IMMDevice) -> Result<String> {
        let p = device.GetId()?;
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const c_void));
        Ok(s)
    }

    /// The friendly name, or "Unnamed device" when the property is absent.
    unsafe fn device_name(device: &IMMDevice) -> String {
        let name = device
            .OpenPropertyStore(STGM_READ)
            .and_then(|store| store.GetValue(&PKEY_DEVICE_FRIENDLY_NAME))
            .map(|v| v.to_string())
            .unwrap_or_default();
        if name.trim().is_empty() {
            "Unnamed device".to_string()
        } else {
            name
        }
    }

    /// READ-ONLY: every ACTIVE render endpoint (connected and enabled), in
    /// Windows' order. The caller has `CoInitializeEx`'d this thread.
    pub unsafe fn active_render_endpoints(enumerator: &IMMDeviceEnumerator) -> Result<Vec<Endpoint>> {
        let coll = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        let n = coll.GetCount()?;
        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n {
            let dev = coll.Item(i)?;
            let id = device_id(&dev)?;
            let name = device_name(&dev);
            out.push(Endpoint { id, name });
        }
        Ok(out)
    }

    /// READ-ONLY: the id of the current default render endpoint (eConsole),
    /// `None` when there is none.
    pub unsafe fn default_render_id(enumerator: &IMMDeviceEnumerator) -> Option<String> {
        let dev = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        device_id(&dev).ok()
    }

    /// WRITES THE MACHINE'S DEFAULT: `SetDefaultEndpoint` for all three roles
    /// through the undocumented policy object. Returns the first failing
    /// role's HRESULT.
    pub(crate) unsafe fn set_default_endpoint(id: &str) -> Result<()> {
        let policy: IPolicyConfig = CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)?;
        let wide = HSTRING::from(id);
        for role in [eConsole, eMultimedia, eCommunications] {
            policy.SetDefaultEndpoint(PCWSTR(wide.as_ptr()), role).ok()?;
        }
        Ok(())
    }

    pub fn next_speaker() -> String {
        // SAFETY: plain COM object creation and calls on the engine thread;
        // every result is checked and every failure becomes a toast.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let enumerator: IMMDeviceEnumerator = match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                Ok(e) => e,
                Err(e) => {
                    log::warn!("next_speaker: MMDeviceEnumerator failed: {e:?}");
                    return REFUSED_TOAST.to_string();
                }
            };
            let devices = match active_render_endpoints(&enumerator) {
                Ok(d) => d,
                Err(e) => {
                    log::warn!("next_speaker: EnumAudioEndpoints(eRender, ACTIVE) failed: {e:?}");
                    return REFUSED_TOAST.to_string();
                }
            };
            if devices.is_empty() {
                log::info!("next_speaker: no active render endpoint — nothing to switch");
                return NO_SPEAKER_TOAST.to_string();
            }
            let current = default_render_id(&enumerator).unwrap_or_default();
            let ids: Vec<&str> = devices.iter().map(|d| d.id.as_str()).collect();
            let Some(next) = next_index(&current, &ids) else {
                log::info!("next_speaker: one active render endpoint ({}) — nothing to cycle to", devices[0].name);
                return ONLY_ONE_TOAST.to_string();
            };
            let target = &devices[next];
            match set_default_endpoint(&target.id) {
                Ok(()) => {
                    log::info!(
                        "next_speaker: default output moved to {} of {} — {:?} ({}) (next-speaker-default-endpoint-switched-spaceadom-125)",
                        next + 1,
                        devices.len(),
                        target.name,
                        target.id
                    );
                    toast_text(&target.name)
                }
                Err(e) => {
                    log::warn!(
                        "next_speaker: IPolicyConfig::SetDefaultEndpoint refused for {:?} ({}): {e:?} — Windows refused to switch",
                        target.name,
                        target.id
                    );
                    REFUSED_TOAST.to_string()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_index_wraps_and_skips_the_current_default() {
        let ids = ["a", "b", "c"];
        assert_eq!(next_index("a", &ids), Some(1));
        assert_eq!(next_index("b", &ids), Some(2));
        assert_eq!(next_index("c", &ids), Some(0), "wraps");
        let two = ["x", "y"];
        assert_eq!(next_index("x", &two), Some(1));
        assert_eq!(next_index("y", &two), Some(0));
    }

    #[test]
    fn next_index_with_one_or_no_device_is_none() {
        assert_eq!(next_index("a", &["a"]), None, "one device: only one speaker");
        assert_eq!(next_index("zzz", &["a"]), None, "one device, and it is not the default: still nothing to cycle to");
        assert_eq!(next_index("a", &[] as &[&str]), None);
    }

    #[test]
    fn next_index_when_the_default_is_not_in_the_active_list_starts_at_the_top() {
        let ids = ["a", "b", "c"];
        assert_eq!(next_index("unplugged", &ids), Some(0));
        assert_eq!(next_index("", &ids), Some(0), "no default at all");
        let owned: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        assert_eq!(next_index("b", &owned), Some(2), "works over owned Strings too");
    }

    #[test]
    fn the_toasts_name_the_speaker() {
        assert_eq!(toast_text("Headphones (Realtek Audio)"), "🔊 → Headphones (Realtek Audio)");
        assert!(ONLY_ONE_TOAST.contains("Only one speaker"));
        assert!(REFUSED_TOAST.contains("refused"));
        assert!(NO_SPEAKER_TOAST.contains("No speaker"));
    }

    /// READ-ONLY on the machine that runs it: prints the active render
    /// endpoints and which is the default. Never calls `set_default_endpoint`.
    /// `cargo test --release --lib list_active_render -- --ignored --nocapture`
    #[test]
    #[ignore = "reads this machine's audio endpoints; run by hand with --nocapture"]
    #[cfg(windows)]
    fn list_active_render_endpoints_on_this_machine() {
        use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator};
        use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).expect("MMDeviceEnumerator");
            let devices = win::active_render_endpoints(&enumerator).expect("EnumAudioEndpoints");
            let default = win::default_render_id(&enumerator).unwrap_or_default();
            eprintln!("active render endpoints: {}", devices.len());
            for (i, d) in devices.iter().enumerate() {
                let mark = if d.id == default { " (DEFAULT)" } else { "" };
                eprintln!("  [{i}] {:?} — {}{mark}", d.name, d.id);
            }
            let ids: Vec<&str> = devices.iter().map(|d| d.id.as_str()).collect();
            eprintln!("next_index → {:?}", next_index(&default, &ids));
        }
    }

    /// WRITES THE MACHINE'S DEFAULT, then puts it back — the owner's
    /// authorised round trip (2026-09-19): record the default, switch to the
    /// next active device, prove `GetDefaultAudioEndpoint` moved, switch BACK
    /// to the recorded one, prove it is the default again. Never leaves the
    /// default anywhere but where it found it; a failed switch-back is a loud
    /// panic, not a quiet log line.
    /// `cargo test --release --lib round_trip -- --ignored --nocapture`
    #[test]
    #[ignore = "CHANGES this machine's default speaker and changes it back; run by hand, once"]
    #[cfg(windows)]
    fn round_trip_switch_to_the_next_speaker_and_back_on_this_machine() {
        use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator};
        use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).expect("MMDeviceEnumerator");
            let devices = win::active_render_endpoints(&enumerator).expect("EnumAudioEndpoints");
            let original = win::default_render_id(&enumerator).expect("a default render endpoint");
            let name_of = |id: &str| devices.iter().find(|d| d.id == id).map(|d| d.name.clone()).unwrap_or_else(|| "?".into());
            let ids: Vec<&str> = devices.iter().map(|d| d.id.as_str()).collect();
            let Some(next) = next_index(&original, &ids) else {
                eprintln!("round trip: only {} active device(s) — nothing to switch to, nothing changed", devices.len());
                return;
            };
            let target = devices[next].id.clone();
            eprintln!("round trip: ORIGINAL default = {:?} ({original})", name_of(&original));
            eprintln!("round trip: switching to    = {:?} ({target})", name_of(&target));

            let forward = win::set_default_endpoint(&target);
            let after_forward = win::default_render_id(&enumerator).unwrap_or_default();
            eprintln!("round trip: SetDefaultEndpoint(next) → {forward:?}; default now = {:?}", name_of(&after_forward));

            // ALWAYS switch back, whatever the forward leg said.
            let back = win::set_default_endpoint(&original);
            let after_back = win::default_render_id(&enumerator).unwrap_or_default();
            eprintln!("round trip: SetDefaultEndpoint(original) → {back:?}; default now = {:?}", name_of(&after_back));

            assert!(back.is_ok(), "SWITCH-BACK FAILED: {back:?} — the default may be left on {:?}", name_of(&after_back));
            assert_eq!(after_back, original, "SWITCH-BACK DID NOT TAKE: default is {:?}, not the original", name_of(&after_back));
            assert!(forward.is_ok(), "forward switch refused: {forward:?}");
            assert_eq!(after_forward, target, "forward switch did not take");
            eprintln!("round trip: OK — default is back on {:?}", name_of(&after_back));
        }
    }
}
