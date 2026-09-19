# Touchpad brief T1 — the hardware proof (read-only logger, no UI, no feature)

Owner (2026-09-19): two-finger slides inside a band along the touchpad's
edge do things — left edge brightness, right edge volume, top edge video
scrubbing (continuous, rate by travel, like a Touch Bar scrubber, NOT
5-second hops). Design is done (`D:\Claude-Projects\design\spaceadom-touchpad_1.html`,
six screens) and is T2. **T1 proves the two mechanisms on the owner's
laptop before any feature code exists.** Rules: no installer, no install,
no commit, never write `%APPDATA%\Spaceadom\config.json`, never delete or
rename files you did not create, no elevation, no keyboard/mouse injection.
Read CLAUDE.md first (hook laws; the hook thread already owns a
message-only raw-input sink window for the KEYBOARD — PROBLEM 268,
`register_raw_keyboard_sink`; `WH_MOUSE_LL` is ours; the agent shell is a
container — machine-touching runs go through `explorer.exe`).

## Mechanism 1 — read the fingers (Raw Input, Precision Touchpad)

A Windows Precision Touchpad is a HID digitizer: usage page 0x0D, usage
0x05 (Touch Pad). Register it with `RegisterRawInputDevices`
(`RIDEV_INPUTSINK`, a message-only window) and parse `WM_INPUT` HID
reports with the HID parser (`HidP_GetCaps`, `HidP_GetLinkCollectionNodes`,
`HidP_GetUsageValue` per contact: Contact ID (0x0D/0x51), Tip Switch
(0x0D/0x42), X (0x01/0x30), Y (0x01/0x31), Contact Count (0x0D/0x54)), plus
the physical/logical ranges from `HidP_GetValueCaps` (physical min/max in
the descriptor's units give the pad's real size — mm if the unit exponent
says so). Reference implementations to READ, not invent: Microsoft's
"Windows Precision Touchpad" HID spec (`Windows Precision Touchpad
Implementation Guide`), and open-source readers of the same reports
(GestureSign, TouchpadGestures, `raw-touchpad` samples). Choose: `windows`
crate 0.58 features `Win32_Devices_HumanInterfaceDevice`,
`Win32_UI_Input`.

Deliver `src-tauri/src/touchpad/raw.rs` (new) behind a **debug-only,
opt-in** switch: an env var `SPACEADOM_TOUCHPAD_PROBE=1` read at boot, OR
a standalone `cargo run --example touchpad-probe` binary — the example is
preferred (nothing in the shipped app changes). It prints, at ≤ 20 lines/s
to stdout and to `%APPDATA%\Spaceadom\touchpad-probe.log`: device found
(vendor/product, pad size in logical units and mm), then per report
`contacts=N  id:x,y(tip)  …`, and a summary every 5 s (reports/s, max
contacts seen). Detection line if NO 0x0D/0x05 device exists:
`no Precision Touchpad — the feature would show its unavailable state`.

## Mechanism 2 — eat the scroll while a band is live

Windows turns the same two-finger slide into `WM_MOUSEWHEEL` /
`WM_MOUSEHWHEEL` (and, on some pads, `WM_MOUSEMOVE` micro-jitter). Our
`WH_MOUSE_LL` hook sees those. The claim to prove: when the probe's own
state says "two contacts, both inside the right 12% of the pad, moving",
the LL hook can return `LRESULT(1)` for wheel messages and the window under
the cursor does NOT scroll — with no perceptible lag and no leaked ticks at
the start or end of the gesture. Implement in the example only: a second
thread with a `WH_MOUSE_LL` hook reading an `AtomicBool BAND_LIVE` the
raw-input side sets (edge band = configurable fraction, default 0.12, all
four edges for the probe); count `eaten` vs `passed` wheel events; print
both in the 5-second summary along with `first_leak_ms` (time from band
entry to the first eaten event) if any tick passed through in the first
100 ms. Laws still apply: nothing but atomics in the callback; the raw
input thread does the parsing.

## What the probe must NOT do

No brightness, no volume, no key taps, no cursor moves. It reads and
counts. The owner will run it (he is on the laptop): give him ONE command
to paste, exactly what to do with his fingers (one finger anywhere; two
fingers in the middle; two fingers sliding up the right edge over a
scrollable page such as a long web page; two fingers along the top edge),
and where the log lands. He reports whether the page scrolled during the
edge slide.

## Also in this pass (small, unrelated, owner-visible)

`src/components/settings-panel.ts`: toggling Advanced mode re-renders the
whole panel, so every segmented control replays its slide-in ("All layout
and Ring layout jump"). Re-render only the row(s) that changed, or render
the segmented sliders with transitions disabled on a full rebuild and
enabled after the first frame. No visual change otherwise.

## Gates + report

`cargo build --release --example touchpad-probe` clean, `cargo clippy
--release --lib` 0 warnings, `cargo test --release --lib` unchanged
count, `npx tsc --noEmit -p .`. Report: the exact command for the owner,
the HID usages actually found on this machine if you can enumerate them
from the agent shell (device enumeration needs no window; do it and paste
the caps), the pad's physical size, and any QUESTION one line each.
PROJECT_STATUS.md dated entry "Touchpad T1 — probe built, awaiting the
owner's run". No version bump.
