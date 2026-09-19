pub mod boss_key;
pub mod focus_engine;
pub mod opacity;
pub mod pip;
pub mod smart_cascade;
pub mod voice_typing;
pub mod screenshot;
pub mod osk;
// PHASE A (2026-09-18) — the four action kinds beyond "open this app or link".
pub mod brightness;
pub mod chord;
pub mod command;
pub mod uri;
// PHASE A step 2 (2026-09-19) — flip a Windows setting and say the new state.
pub mod toggle;
// 1.0.125 (2026-09-19) — "Next speaker": cycle the default output device.
pub mod audio_output;
pub mod app_volume;
