//! Precision Touchpad reader — Raw Input + the HID parser (Touchpad T1).
//!
//! A Windows Precision Touchpad is a HID digitizer top-level collection with
//! usage page 0x0D (Digitizer) / usage 0x05 (Touch Pad). Each finger is a
//! nested link collection 0x0D/0x22 (Finger) carrying Tip Switch (0x0D/0x42,
//! a button), Contact ID (0x0D/0x51), X (0x01/0x30) and Y (0x01/0x31); the
//! top level carries Contact Count (0x0D/0x54). This module:
//!
//! * `enumerate()` — walks `GetRawInputDeviceList`, and for every HID device
//!   reads vendor/product/usage and, for a touch pad, the value caps of X/Y
//!   (logical + physical ranges, unit, exponent → pad size in mm) and the
//!   contact-count cap. Needs no window; usable from any shell.
//! * `run_sink()` — a message-only window registered with
//!   `RegisterRawInputDevices(0x0D/0x05, RIDEV_INPUTSINK)` on the CALLING
//!   thread, pumping `WM_INPUT`, parsing every report with `HidP_*` and
//!   handing the caller a `Report` per HID report. Also read-only.
//!
//! Same shape as `hook::register_raw_keyboard_sink` (PROBLEM 268) — the
//! keyboard sink stamps a clock; this one parses the payload.
//!
//! Nothing here injects input, moves the cursor, or writes anything to
//! disk — the example owns printing and logging.
#![cfg(windows)]

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use windows::Win32::Devices::HumanInterfaceDevice::{
    HidP_GetCaps, HidP_GetLinkCollectionNodes, HidP_GetUsageValue, HidP_GetUsages,
    HidP_GetValueCaps, HidP_Input, HIDP_CAPS, HIDP_LINK_COLLECTION_NODE, HIDP_STATUS_SUCCESS,
    HIDP_VALUE_CAPS, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList, HRAWINPUT, RAWINPUT,
    RAWINPUTDEVICE, RAWINPUTDEVICELIST, RAWINPUTHEADER, RIDEV_INPUTSINK, RIDI_DEVICEINFO,
    RIDI_DEVICENAME, RIDI_PREPARSEDDATA, RID_DEVICE_INFO, RID_INPUT, RIM_TYPEHID,
};

/// HID usage pages / usages used here (Microsoft "Windows Precision Touchpad
/// Collection" — the required report descriptor).
pub const PAGE_GENERIC_DESKTOP: u16 = 0x01;
pub const PAGE_DIGITIZER: u16 = 0x0D;
pub const USAGE_TOUCH_PAD: u16 = 0x05;
pub const USAGE_FINGER: u16 = 0x22;
pub const USAGE_X: u16 = 0x30;
pub const USAGE_Y: u16 = 0x31;
pub const USAGE_TIP_SWITCH: u16 = 0x42;
pub const USAGE_CONTACT_ID: u16 = 0x51;
pub const USAGE_CONTACT_COUNT: u16 = 0x54;

/// Set by the raw-input side (`BandTracker`) while two tips sit inside one
/// edge band and are moving. Read by the example's `WH_MOUSE_LL` callback
/// and by nothing else — an atomic is the only thing a hook callback may
/// touch (keyboard-hook laws).
pub static BAND_LIVE: AtomicBool = AtomicBool::new(false);
/// `GetTickCount64` at the last transition of `BAND_LIVE` false → true.
pub static BAND_ENTERED_AT: AtomicU64 = AtomicU64::new(0);
/// `GetTickCount64` at the last transition of `BAND_LIVE` true → false.
pub static BAND_LEFT_AT: AtomicU64 = AtomicU64::new(0);

/// One axis of the digitizer as the descriptor declares it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AxisCaps {
    pub logical_min: i32,
    pub logical_max: i32,
    pub physical_min: i32,
    pub physical_max: i32,
    /// Raw HID `Units` field (0x11 = SI linear cm, 0x13 = English linear inch).
    pub unit: u32,
    /// Raw HID `UnitsExp` nibble (0..7 = +0..+7, 8..15 = -8..-1).
    pub unit_exp: u32,
}

impl AxisCaps {
    /// Logical span (`max - min`), or 0 when the descriptor is degenerate.
    pub fn logical_span(&self) -> i32 {
        self.logical_max.saturating_sub(self.logical_min)
    }

    /// The axis length in millimetres, when the descriptor declares a linear
    /// unit and a non-degenerate physical range. `None` otherwise.
    pub fn length_mm(&self) -> Option<f64> {
        let span = (self.physical_max as f64) - (self.physical_min as f64);
        if span <= 0.0 {
            return None;
        }
        let exp = unit_exponent(self.unit_exp);
        let scaled = span * 10f64.powi(exp);
        // Exactly the two linear-length units; 0x1001 (seconds) and the
        // like also have a low nibble of 1 and must not read as cm.
        match self.unit {
            0x11 => Some(scaled * 10.0), // SI linear: centimetres → mm
            0x13 => Some(scaled * 25.4), // English linear: inches → mm
            _ => None,
        }
    }

    /// Where `v` sits on the axis as a fraction 0.0..=1.0 of the logical
    /// range (clamped). Used for the edge-band test.
    pub fn fraction(&self, v: i32) -> f64 {
        let span = self.logical_span();
        if span <= 0 {
            return 0.0;
        }
        (((v - self.logical_min) as f64) / span as f64).clamp(0.0, 1.0)
    }
}

/// Decode the HID 4-bit unit exponent nibble to a signed exponent.
pub fn unit_exponent(nibble: u32) -> i32 {
    let n = (nibble & 0xF) as i32;
    if n >= 8 {
        n - 16
    } else {
        n
    }
}

/// One raw-input HID device as `enumerate()` saw it.
#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub handle: isize,
    pub name: String,
    pub vendor_id: u32,
    pub product_id: u32,
    pub version: u32,
    pub usage_page: u16,
    pub usage: u16,
    /// Present only for 0x0D/0x05 devices whose preparsed data parsed.
    pub pad: Option<PadCaps>,
}

impl DeviceInfo {
    pub fn is_touch_pad(&self) -> bool {
        self.usage_page == PAGE_DIGITIZER && self.usage == USAGE_TOUCH_PAD
    }
}

/// What the HID parser says about a Precision Touchpad's input report.
#[derive(Clone, Debug, Default)]
pub struct PadCaps {
    pub input_report_bytes: u16,
    pub link_collections: u16,
    pub input_value_caps: u16,
    pub input_button_caps: u16,
    /// Link-collection indices of every 0x0D/0x22 Finger collection, in
    /// descriptor order.
    pub finger_collections: Vec<u16>,
    /// X/Y as declared inside the FIRST finger collection (all fingers share
    /// one descriptor on a compliant pad).
    pub x: Option<AxisCaps>,
    pub y: Option<AxisCaps>,
    /// Logical max of Contact Count (0x0D/0x54) — the pad's finger limit.
    pub contact_count_max: Option<i32>,
    /// Every input value cap, one line each, for the report.
    pub value_cap_lines: Vec<String>,
}

impl PadCaps {
    /// `(width_mm, height_mm)` when the descriptor gives both.
    pub fn size_mm(&self) -> Option<(f64, f64)> {
        Some((self.x.as_ref()?.length_mm()?, self.y.as_ref()?.length_mm()?))
    }
}

/// One finger in one report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Contact {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub tip: bool,
}

/// One parsed HID input report.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Contact Count (0x0D/0x54) as the report declares it, when present.
    pub contact_count: Option<u32>,
    pub contacts: Vec<Contact>,
    /// The report's raw byte length (for the log).
    pub raw_len: usize,
}

impl Report {
    /// Contacts whose tip switch is down.
    pub fn tips(&self) -> impl Iterator<Item = &Contact> {
        self.contacts.iter().filter(|c| c.tip)
    }
}

// ---------------------------------------------------------------------------
// Enumeration (no window needed)
// ---------------------------------------------------------------------------

/// Every raw-input HID device on the machine, with pad caps for touch pads.
pub fn enumerate() -> Vec<DeviceInfo> {
    // SAFETY: plain Win32 calls with buffers sized by the API's own count.
    unsafe {
        let mut n: u32 = 0;
        let cb = std::mem::size_of::<RAWINPUTDEVICELIST>() as u32;
        if GetRawInputDeviceList(None, &mut n, cb) == u32::MAX || n == 0 {
            return Vec::new();
        }
        let mut list = vec![RAWINPUTDEVICELIST::default(); n as usize];
        let got = GetRawInputDeviceList(Some(list.as_mut_ptr()), &mut n, cb);
        if got == u32::MAX {
            return Vec::new();
        }
        list.truncate(got as usize);
        list.into_iter()
            .filter(|d| d.dwType == RIM_TYPEHID)
            .filter_map(|d| describe(d.hDevice))
            .collect()
    }
}

unsafe fn describe(h: HANDLE) -> Option<DeviceInfo> {
    let mut info = RID_DEVICE_INFO {
        cbSize: std::mem::size_of::<RID_DEVICE_INFO>() as u32,
        ..Default::default()
    };
    let mut cb = info.cbSize;
    let r = GetRawInputDeviceInfoW(
        h,
        RIDI_DEVICEINFO,
        Some(&mut info as *mut _ as *mut core::ffi::c_void),
        &mut cb,
    );
    if r == u32::MAX || info.dwType != RIM_TYPEHID {
        return None;
    }
    let hid = info.Anonymous.hid;
    let mut dev = DeviceInfo {
        handle: h.0 as isize,
        name: device_name(h),
        vendor_id: hid.dwVendorId,
        product_id: hid.dwProductId,
        version: hid.dwVersionNumber,
        usage_page: hid.usUsagePage,
        usage: hid.usUsage,
        pad: None,
    };
    if dev.is_touch_pad() {
        if let Some(pre) = preparsed(h) {
            dev.pad = Some(pad_caps(PHIDP_PREPARSED_DATA(pre.as_ptr() as isize)));
        }
    }
    Some(dev)
}

unsafe fn device_name(h: HANDLE) -> String {
    let mut cch: u32 = 0;
    let _ = GetRawInputDeviceInfoW(h, RIDI_DEVICENAME, None, &mut cch);
    if cch == 0 {
        return String::new();
    }
    let mut buf = vec![0u16; cch as usize + 1];
    let r = GetRawInputDeviceInfoW(
        h,
        RIDI_DEVICENAME,
        Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
        &mut cch,
    );
    if r == u32::MAX {
        return String::new();
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// The device's preparsed data, owned by us (a byte buffer the HidP_* calls
/// take by pointer).
unsafe fn preparsed(h: HANDLE) -> Option<Vec<u8>> {
    let mut cb: u32 = 0;
    let _ = GetRawInputDeviceInfoW(h, RIDI_PREPARSEDDATA, None, &mut cb);
    if cb == 0 {
        return None;
    }
    let mut buf = vec![0u8; cb as usize];
    let r = GetRawInputDeviceInfoW(
        h,
        RIDI_PREPARSEDDATA,
        Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
        &mut cb,
    );
    if r == u32::MAX {
        return None;
    }
    Some(buf)
}

unsafe fn pad_caps(pre: PHIDP_PREPARSED_DATA) -> PadCaps {
    let mut out = PadCaps::default();
    let mut caps = HIDP_CAPS::default();
    if HidP_GetCaps(pre, &mut caps) != HIDP_STATUS_SUCCESS {
        return out;
    }
    out.input_report_bytes = caps.InputReportByteLength;
    out.link_collections = caps.NumberLinkCollectionNodes;
    out.input_value_caps = caps.NumberInputValueCaps;
    out.input_button_caps = caps.NumberInputButtonCaps;

    // Finger collections.
    let mut n_nodes = caps.NumberLinkCollectionNodes as u32;
    if n_nodes > 0 {
        let mut nodes = vec![HIDP_LINK_COLLECTION_NODE::default(); n_nodes as usize];
        if HidP_GetLinkCollectionNodes(nodes.as_mut_ptr(), &mut n_nodes, pre)
            == HIDP_STATUS_SUCCESS
        {
            for (i, node) in nodes.iter().enumerate().take(n_nodes as usize) {
                if node.LinkUsagePage == PAGE_DIGITIZER && node.LinkUsage == USAGE_FINGER {
                    out.finger_collections.push(i as u16);
                }
            }
        }
    }

    // Value caps: X/Y in the first finger collection, contact count at top.
    let mut n_vals = caps.NumberInputValueCaps;
    if n_vals > 0 {
        let mut vals = vec![HIDP_VALUE_CAPS::default(); n_vals as usize];
        if HidP_GetValueCaps(HidP_Input, vals.as_mut_ptr(), &mut n_vals, pre)
            == HIDP_STATUS_SUCCESS
        {
            let first_finger = out.finger_collections.first().copied();
            for v in vals.iter().take(n_vals as usize) {
                let usage = if v.IsRange.as_bool() {
                    v.Anonymous.Range.UsageMin
                } else {
                    v.Anonymous.NotRange.Usage
                };
                let axis = AxisCaps {
                    logical_min: v.LogicalMin,
                    logical_max: v.LogicalMax,
                    physical_min: v.PhysicalMin,
                    physical_max: v.PhysicalMax,
                    unit: v.Units,
                    unit_exp: v.UnitsExp,
                };
                out.value_cap_lines.push(format!(
                    "  page 0x{:02X} usage 0x{:02X} coll {} (link 0x{:02X}/0x{:02X}) report {} \
                     bits {} count {} logical {}..{} physical {}..{} unit 0x{:X} exp {}{}",
                    v.UsagePage,
                    usage,
                    v.LinkCollection,
                    v.LinkUsagePage,
                    v.LinkUsage,
                    v.ReportID,
                    v.BitSize,
                    v.ReportCount,
                    v.LogicalMin,
                    v.LogicalMax,
                    v.PhysicalMin,
                    v.PhysicalMax,
                    v.Units,
                    unit_exponent(v.UnitsExp),
                    axis.length_mm()
                        .map(|mm| format!(" = {mm:.1} mm"))
                        .unwrap_or_default(),
                ));
                let in_finger = first_finger.map_or(
                    v.LinkUsagePage == PAGE_DIGITIZER && v.LinkUsage == USAGE_FINGER,
                    |f| v.LinkCollection == f,
                );
                if v.UsagePage == PAGE_GENERIC_DESKTOP && in_finger {
                    if usage == USAGE_X && out.x.is_none() {
                        out.x = Some(axis);
                    } else if usage == USAGE_Y && out.y.is_none() {
                        out.y = Some(axis);
                    }
                } else if v.UsagePage == PAGE_DIGITIZER
                    && usage == USAGE_CONTACT_COUNT
                    && out.contact_count_max.is_none()
                {
                    out.contact_count_max = Some(v.LogicalMax);
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Report parsing
// ---------------------------------------------------------------------------

/// Parse one HID input report against a pad's preparsed data. Every HidP
/// call that fails (wrong report ID, usage absent) simply leaves its value
/// out — a Precision Touchpad may interleave reports of other IDs.
pub fn parse_report(pre: PHIDP_PREPARSED_DATA, caps: &PadCaps, bytes: &[u8]) -> Report {
    let mut rep = Report {
        raw_len: bytes.len(),
        ..Default::default()
    };
    // SAFETY: HidP_* read `bytes` and `pre` only.
    unsafe {
        let mut v: u32 = 0;
        if HidP_GetUsageValue(
            HidP_Input,
            PAGE_DIGITIZER,
            0,
            USAGE_CONTACT_COUNT,
            &mut v,
            pre,
            bytes,
        ) == HIDP_STATUS_SUCCESS
        {
            rep.contact_count = Some(v);
        }
        let mut report_copy = bytes.to_vec();
        for &coll in &caps.finger_collections {
            let mut x: u32 = 0;
            let mut y: u32 = 0;
            let mut id: u32 = 0;
            let sx = HidP_GetUsageValue(HidP_Input, PAGE_GENERIC_DESKTOP, coll, USAGE_X, &mut x, pre, bytes);
            let sy = HidP_GetUsageValue(HidP_Input, PAGE_GENERIC_DESKTOP, coll, USAGE_Y, &mut y, pre, bytes);
            if sx != HIDP_STATUS_SUCCESS || sy != HIDP_STATUS_SUCCESS {
                continue;
            }
            let _ = HidP_GetUsageValue(HidP_Input, PAGE_DIGITIZER, coll, USAGE_CONTACT_ID, &mut id, pre, bytes);
            let mut usages = [0u16; 8];
            let mut n = usages.len() as u32;
            let tip = HidP_GetUsages(
                HidP_Input,
                PAGE_DIGITIZER,
                coll,
                usages.as_mut_ptr(),
                &mut n,
                pre,
                &mut report_copy,
            ) == HIDP_STATUS_SUCCESS
                && usages[..n as usize].contains(&USAGE_TIP_SWITCH);
            rep.contacts.push(Contact {
                id,
                x: x as i32,
                y: y as i32,
                tip,
            });
        }
    }
    rep
}

// ---------------------------------------------------------------------------
// Edge-band tracker (pure; feeds BAND_LIVE)
// ---------------------------------------------------------------------------

/// Which edge band a point sits in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Classify a point by its axis fractions; `band` is the edge fraction
/// (0.12 = the outer 12 %). Corners resolve to the nearer vertical edge.
pub fn edge_of(fx: f64, fy: f64, band: f64) -> Option<Edge> {
    if fx <= band {
        Some(Edge::Left)
    } else if fx >= 1.0 - band {
        Some(Edge::Right)
    } else if fy <= band {
        Some(Edge::Top)
    } else if fy >= 1.0 - band {
        Some(Edge::Bottom)
    } else {
        None
    }
}

/// Decides "two tips, both inside the same edge band, moving" from
/// successive reports and publishes it to `BAND_LIVE`. Movement is held for
/// `hold_ms` after the last report that moved, so a pad idling between
/// frames does not flicker the flag.
pub struct BandTracker {
    pub band: f64,
    pub hold_ms: u64,
    x: AxisCaps,
    y: AxisCaps,
    prev: Vec<Contact>,
    last_moved_at: u64,
    live: bool,
    /// The edge that is (or was last) live.
    pub edge: Option<Edge>,
}

impl BandTracker {
    pub fn new(x: AxisCaps, y: AxisCaps, band: f64) -> Self {
        Self {
            band,
            hold_ms: 200,
            x,
            y,
            prev: Vec::new(),
            last_moved_at: 0,
            live: false,
            edge: None,
        }
    }

    pub fn live(&self) -> bool {
        self.live
    }

    /// Feed one report at tick `now` (ms). Returns the new live state.
    pub fn feed(&mut self, rep: &Report, now: u64) -> bool {
        let tips: Vec<Contact> = rep.tips().copied().collect();
        let mut moved = false;
        for c in &tips {
            if let Some(p) = self.prev.iter().find(|p| p.id == c.id) {
                if p.x != c.x || p.y != c.y {
                    moved = true;
                }
            }
        }
        if moved {
            self.last_moved_at = now;
        }
        let same_edge = if tips.len() == 2 {
            let e0 = edge_of(self.x.fraction(tips[0].x), self.y.fraction(tips[0].y), self.band);
            let e1 = edge_of(self.x.fraction(tips[1].x), self.y.fraction(tips[1].y), self.band);
            match (e0, e1) {
                (Some(a), Some(b)) if a == b => Some(a),
                _ => None,
            }
        } else {
            None
        };
        let recently_moved = now.saturating_sub(self.last_moved_at) <= self.hold_ms;
        let live = same_edge.is_some() && recently_moved;
        if same_edge.is_some() {
            self.edge = same_edge;
        }
        self.prev = tips;
        if live != self.live {
            self.live = live;
            if live {
                BAND_ENTERED_AT.store(now, Ordering::Relaxed);
            } else {
                BAND_LEFT_AT.store(now, Ordering::Relaxed);
            }
            BAND_LIVE.store(live, Ordering::Relaxed);
        }
        live
    }
}

// ---------------------------------------------------------------------------
// The sink window (runs on the calling thread)
// ---------------------------------------------------------------------------

struct SinkState {
    pre: Vec<u8>,
    caps: PadCaps,
    on_report: Box<dyn FnMut(&Report)>,
}

thread_local! {
    static SINK: RefCell<Option<SinkState>> = const { RefCell::new(None) };
}

/// Errors from `run_sink`.
#[derive(Debug)]
pub enum SinkError {
    NoTouchPad,
    NoPreparsedData,
    WindowFailed(String),
    RegisterFailed(String),
}

impl std::fmt::Display for SinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SinkError::NoTouchPad => write!(f, "no Precision Touchpad (0x0D/0x05) device"),
            SinkError::NoPreparsedData => write!(f, "the touch pad returned no preparsed data"),
            SinkError::WindowFailed(e) => write!(f, "sink window could not be created: {e}"),
            SinkError::RegisterFailed(e) => write!(f, "RegisterRawInputDevices failed: {e}"),
        }
    }
}

/// Create the message-only sink on THIS thread, register for 0x0D/0x05 with
/// `RIDEV_INPUTSINK`, and pump messages forever, calling `on_report` for
/// every parsed touch-pad report. Returns only on failure to set up (or when
/// the thread's message loop ends with `WM_QUIT`).
pub fn run_sink(pad: &DeviceInfo, on_report: Box<dyn FnMut(&Report)>) -> Result<(), SinkError> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Input::RegisterRawInputDevices;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DispatchMessageW, GetMessageW, RegisterClassW, TranslateMessage,
        HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
    };
    if !pad.is_touch_pad() {
        return Err(SinkError::NoTouchPad);
    }
    let caps = pad.pad.clone().ok_or(SinkError::NoPreparsedData)?;
    // SAFETY: standard message-only window creation + raw-input registration,
    // the same recipe as hook::register_raw_keyboard_sink.
    unsafe {
        let pre = preparsed(HANDLE(pad.handle as *mut core::ffi::c_void))
            .ok_or(SinkError::NoPreparsedData)?;
        SINK.with(|s| {
            *s.borrow_mut() = Some(SinkState {
                pre,
                caps,
                on_report,
            })
        });
        const CLASS: PCWSTR = w!("SpaceadomTouchpadProbeSink");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(sink_wndproc),
            lpszClassName: CLASS,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS,
            w!("spaceadom-touchpad-probe-sink"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            None,
            None,
            None,
        ) {
            Ok(h) if !h.is_invalid() => h,
            other => return Err(SinkError::WindowFailed(format!("{other:?}"))),
        };
        let rid = [RAWINPUTDEVICE {
            usUsagePage: PAGE_DIGITIZER,
            usUsage: USAGE_TOUCH_PAD,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        }];
        RegisterRawInputDevices(&rid, std::mem::size_of::<RAWINPUTDEVICE>() as u32)
            .map_err(|e| SinkError::RegisterFailed(e.to_string()))?;

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

unsafe extern "system" fn sink_wndproc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, WM_INPUT};
    if msg == WM_INPUT {
        let hri = HRAWINPUT(lparam.0 as *mut core::ffi::c_void);
        let header = std::mem::size_of::<RAWINPUTHEADER>() as u32;
        let mut cb: u32 = 0;
        let _ = GetRawInputData(hri, RID_INPUT, None, &mut cb, header);
        if cb > 0 {
            // RAWINPUT is variable-length; keep the buffer 8-aligned.
            let words = (cb as usize).div_ceil(8);
            let mut buf = vec![0u64; words];
            let got = GetRawInputData(
                hri,
                RID_INPUT,
                Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
                &mut cb,
                header,
            );
            if got != u32::MAX && got >= header {
                let ri = &*(buf.as_ptr() as *const RAWINPUT);
                if ri.header.dwType == RIM_TYPEHID.0 {
                    let hid = &ri.data.hid;
                    let size = hid.dwSizeHid as usize;
                    let count = hid.dwCount as usize;
                    let base = hid.bRawData.as_ptr();
                    let avail = got as usize - (base as usize - buf.as_ptr() as usize);
                    SINK.with(|s| {
                        if let Some(state) = s.borrow_mut().as_mut() {
                            let pre = PHIDP_PREPARSED_DATA(state.pre.as_ptr() as isize);
                            for i in 0..count {
                                let off = i * size;
                                if off + size > avail {
                                    break;
                                }
                                let bytes = std::slice::from_raw_parts(base.add(off), size);
                                let rep = parse_report(pre, &state.caps, bytes);
                                (state.on_report)(&rep);
                            }
                        }
                    });
                }
            }
        }
    }
    // WM_INPUT must reach DefWindowProc so the system can free the buffer.
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_exponent_nibble_decodes_signed() {
        assert_eq!(unit_exponent(0), 0);
        assert_eq!(unit_exponent(3), 3);
        assert_eq!(unit_exponent(0xF), -1);
        assert_eq!(unit_exponent(0xE), -2);
        assert_eq!(unit_exponent(0x8), -8);
    }

    #[test]
    fn axis_length_mm_from_cm_and_inch() {
        // 0x11 = SI linear (cm), exponent -2 → physical in 1/100 cm: 1200 → 12.00 cm → 120 mm
        let cm = AxisCaps { physical_min: 0, physical_max: 1200, unit: 0x11, unit_exp: 0xE, ..Default::default() };
        assert!((cm.length_mm().unwrap() - 120.0).abs() < 1e-9);
        // 0x13 = English linear (inch), exponent -2: 400 → 4.00 in → 101.6 mm
        let inch = AxisCaps { physical_min: 0, physical_max: 400, unit: 0x13, unit_exp: 0xE, ..Default::default() };
        assert!((inch.length_mm().unwrap() - 101.6).abs() < 1e-9);
        // Degenerate physical range → unknown.
        let none = AxisCaps { unit: 0x11, ..Default::default() };
        assert_eq!(none.length_mm(), None);
    }

    #[test]
    fn edge_bands_resolve_by_fraction() {
        assert_eq!(edge_of(0.05, 0.5, 0.12), Some(Edge::Left));
        assert_eq!(edge_of(0.95, 0.5, 0.12), Some(Edge::Right));
        assert_eq!(edge_of(0.5, 0.05, 0.12), Some(Edge::Top));
        assert_eq!(edge_of(0.5, 0.95, 0.12), Some(Edge::Bottom));
        assert_eq!(edge_of(0.5, 0.5, 0.12), None);
    }

    fn axes() -> (AxisCaps, AxisCaps) {
        let x = AxisCaps { logical_min: 0, logical_max: 1000, ..Default::default() };
        let y = AxisCaps { logical_min: 0, logical_max: 500, ..Default::default() };
        (x, y)
    }

    fn rep(contacts: &[(u32, i32, i32)]) -> Report {
        Report {
            contact_count: Some(contacts.len() as u32),
            contacts: contacts.iter().map(|&(id, x, y)| Contact { id, x, y, tip: true }).collect(),
            raw_len: 0,
        }
    }

    #[test]
    fn band_tracker_goes_live_for_two_moving_tips_on_one_edge() {
        let (x, y) = axes();
        let mut t = BandTracker::new(x, y, 0.12);
        // First frame: two tips on the right edge, nothing to compare against.
        assert!(!t.feed(&rep(&[(1, 950, 100), (2, 960, 150)]), 1000));
        // Second frame: they moved → live.
        assert!(t.feed(&rep(&[(1, 950, 120), (2, 960, 170)]), 1010));
        assert_eq!(t.edge, Some(Edge::Right));
        assert!(BAND_LIVE.load(Ordering::Relaxed));
        // Stationary for longer than hold_ms → not live.
        assert!(!t.feed(&rep(&[(1, 950, 120), (2, 960, 170)]), 1400));
        // One finger in the middle → not live even if moving.
        assert!(!t.feed(&rep(&[(1, 950, 130), (2, 500, 250)]), 1410));
        // Reset the shared flag so other tests see a clean state.
        BAND_LIVE.store(false, Ordering::Relaxed);
    }

    #[test]
    fn band_tracker_ignores_one_finger_and_middle_pairs() {
        let (x, y) = axes();
        let mut t = BandTracker::new(x, y, 0.12);
        t.feed(&rep(&[(1, 500, 250)]), 1);
        assert!(!t.feed(&rep(&[(1, 510, 250)]), 2));
        t.feed(&rep(&[(1, 500, 250), (2, 520, 260)]), 3);
        assert!(!t.feed(&rep(&[(1, 510, 250), (2, 530, 260)]), 4));
    }
}
