//! TOUCHPAD 1.0.130 — the PRESETS of the page's "Does what" list: named
//! forward/backward shortcut pairs the user picks without recording. PURE —
//! a table and a latch, nothing else; `touchpad::mod` drives them exactly as
//! it drives `BandAction::Chords` (one chord per notch through `send_chord`),
//! except that a `once_per_slide` preset fires on the FIRST notch only and
//! then nothing until the finger lifts (`OnceLatch`).
//!
//! Owner decisions 2026-09-20 00:50:
//!   Tabs        Ctrl+Tab           / Ctrl+Shift+Tab      stepped
//!   Zoom        Ctrl+=             / Ctrl+−              stepped
//!   Undo / Redo Ctrl+Y (forward)   / Ctrl+Z (backward)   stepped — forward = redo
//!   Copy / Paste Ctrl+V (forward)  / Ctrl+C (backward)   once — forward = paste
//!   Track       next track         / previous track      once — through the
//!               media session's TrySkipNext/PreviousAsync when one exists
//!               (`touchpad::seek::skip_track`), else the VK_MEDIA_* keys below.
//!
//! The same table lives in `src/components/touchpad-page.ts` (`PRESETS`) so
//! the page can pre-fill "Any shortcut" with a preset's pair for editing.

use crate::config::schema::PresetId;

pub const VK_SHIFT: u16 = 0x10;
pub const VK_CONTROL: u16 = 0x11;
pub const VK_TAB: u16 = 0x09;
pub const VK_C: u16 = 0x43;
pub const VK_V: u16 = 0x56;
pub const VK_Y: u16 = 0x59;
pub const VK_Z: u16 = 0x5A;
/// `VK_OEM_PLUS` — the `=`/`+` key on a US layout (Ctrl+= is "zoom in").
pub const VK_OEM_PLUS: u16 = 0xBB;
/// `VK_OEM_MINUS` — the `-`/`_` key.
pub const VK_OEM_MINUS: u16 = 0xBD;
pub const VK_MEDIA_NEXT_TRACK: u16 = 0xB0;
pub const VK_MEDIA_PREV_TRACK: u16 = 0xB1;

/// One preset, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetSpec {
    pub id: PresetId,
    /// The row's name on the page and in the live pill.
    pub name: &'static str,
    /// The one-line description under the row.
    pub description: &'static str,
    /// Slide up/right.
    pub forward: &'static [u16],
    /// Slide down/left.
    pub backward: &'static [u16],
    /// Fire on the first notch only, then nothing until lift.
    pub once_per_slide: bool,
}

/// The table. Every `PresetId` has exactly one row.
pub fn spec(id: PresetId) -> PresetSpec {
    match id {
        PresetId::Tabs => PresetSpec {
            id,
            name: "Tabs",
            description: "Next tab, previous tab — a tab per notch",
            forward: &[VK_CONTROL, VK_TAB],
            backward: &[VK_CONTROL, VK_SHIFT, VK_TAB],
            once_per_slide: false,
        },
        PresetId::Zoom => PresetSpec {
            id,
            name: "Zoom",
            description: "Zoom in, zoom out — a step per notch",
            forward: &[VK_CONTROL, VK_OEM_PLUS],
            backward: &[VK_CONTROL, VK_OEM_MINUS],
            once_per_slide: false,
        },
        PresetId::UndoRedo => PresetSpec {
            id,
            name: "Undo / Redo",
            description: "Slide back to undo, forward to redo — one per notch",
            forward: &[VK_CONTROL, VK_Y],
            backward: &[VK_CONTROL, VK_Z],
            once_per_slide: false,
        },
        PresetId::CopyPaste => PresetSpec {
            id,
            name: "Copy / Paste",
            description: "Slide back to copy, forward to paste — once per slide",
            forward: &[VK_CONTROL, VK_V],
            backward: &[VK_CONTROL, VK_C],
            once_per_slide: true,
        },
        PresetId::Track => PresetSpec {
            id,
            name: "Track",
            description: "Next track, previous track — once per slide",
            forward: &[VK_MEDIA_NEXT_TRACK],
            backward: &[VK_MEDIA_PREV_TRACK],
            once_per_slide: true,
        },
    }
}

/// The once-per-slide latch — PURE. `reset` on Enter; `fire(target)` on
/// every Move with the quantised step target: the first call with a non-zero
/// target answers `Some(forward)` (the sign of that first notch) and every
/// later call answers `None` until the next `reset`, however far the finger
/// goes on or comes back. A slide that never leaves the dead zone fires
/// nothing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OnceLatch {
    fired: bool,
}

impl OnceLatch {
    pub fn reset(&mut self) {
        self.fired = false;
    }

    pub fn fire(&mut self, target: i32) -> Option<bool> {
        if self.fired || target == 0 {
            return None;
        }
        self.fired = true;
        Some(target > 0)
    }

    pub fn has_fired(self) -> bool {
        self.fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_resolves_to_the_owners_pair() {
        let t = spec(PresetId::Tabs);
        assert_eq!(t.forward, &[0x11, 0x09], "Ctrl+Tab");
        assert_eq!(t.backward, &[0x11, 0x10, 0x09], "Ctrl+Shift+Tab");
        assert!(!t.once_per_slide);
        let z = spec(PresetId::Zoom);
        assert_eq!(z.forward, &[0x11, 0xBB], "Ctrl+=");
        assert_eq!(z.backward, &[0x11, 0xBD], "Ctrl+−");
        assert!(!z.once_per_slide);
        let u = spec(PresetId::UndoRedo);
        assert_eq!(u.forward, &[0x11, 0x59], "forward = redo = Ctrl+Y");
        assert_eq!(u.backward, &[0x11, 0x5A], "backward = undo = Ctrl+Z");
        assert!(!u.once_per_slide);
        let c = spec(PresetId::CopyPaste);
        assert_eq!(c.forward, &[0x11, 0x56], "forward = paste = Ctrl+V");
        assert_eq!(c.backward, &[0x11, 0x43], "backward = copy = Ctrl+C");
        assert!(c.once_per_slide);
        let k = spec(PresetId::Track);
        assert_eq!(k.forward, &[0xB0], "VK_MEDIA_NEXT_TRACK");
        assert_eq!(k.backward, &[0xB1], "VK_MEDIA_PREV_TRACK");
        assert!(k.once_per_slide);
        // Every id has a row whose id is itself, a name and a description,
        // and every chord is non-empty and within the batch limit.
        for id in PresetId::ALL {
            let s = spec(id);
            assert_eq!(s.id, id);
            assert!(!s.name.is_empty() && !s.description.is_empty(), "{id:?}");
            for chord in [s.forward, s.backward] {
                assert!(!chord.is_empty() && chord.len() <= crate::engine::actions::chord::MAX_KEYS, "{id:?}");
            }
        }
        // The names the page shows, in the owner's list order.
        let names: Vec<&str> = PresetId::ALL.iter().map(|&id| spec(id).name).collect();
        assert_eq!(names, ["Tabs", "Zoom", "Undo / Redo", "Copy / Paste", "Track"]);
    }

    #[test]
    fn the_once_per_slide_latch_fires_on_the_first_notch_only() {
        let mut l = OnceLatch::default();
        assert_eq!(l.fire(0), None, "inside the dead zone: nothing");
        assert!(!l.has_fired());
        assert_eq!(l.fire(0), None);
        assert_eq!(l.fire(1), Some(true), "the first notch, forward");
        assert!(l.has_fired());
        assert_eq!(l.fire(2), None, "a second notch: nothing");
        assert_eq!(l.fire(5), None);
        assert_eq!(l.fire(-3), None, "coming back past the start: still nothing");
        assert_eq!(l.fire(0), None);
        // Lift → reset → the next slide fires again, this time backward.
        l.reset();
        assert!(!l.has_fired());
        assert_eq!(l.fire(-1), Some(false), "the first notch, backward");
        assert_eq!(l.fire(-2), None);
        assert_eq!(l.fire(1), None, "reversing after the fire does not fire again");
        // A fresh latch fed a big first target still fires exactly once.
        let mut l2 = OnceLatch::default();
        assert_eq!(l2.fire(7), Some(true));
        assert_eq!(l2.fire(8), None);
    }
}
