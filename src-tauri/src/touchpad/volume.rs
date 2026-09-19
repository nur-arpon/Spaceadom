//! TOUCHPAD T2 — Volume through Core Audio, in-process (a slide ticks at
//! 30–60 Hz; a shell-out per tick is out of the question). Same COM recipe
//! as `engine/actions/boss_key.rs`'s mute: default render endpoint,
//! `IAudioEndpointVolume`, master scalar.
//!
//! The session keeps a FLOAT target so a slow slide's tiny deltas are not
//! rounded away, and reads the level back after every set so the page shows
//! what Windows actually accepted.
#![cfg(windows)]

use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

pub struct VolumeSession {
    endpoint: IAudioEndpointVolume,
    /// 0..=100, fractional.
    target: f64,
}

impl VolumeSession {
    /// Open the default render endpoint and read where it is. `None` when
    /// there is no audio device (a tester's dock unplugged) — the band