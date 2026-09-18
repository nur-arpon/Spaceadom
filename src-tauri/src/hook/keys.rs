//! PHASE A (2026-09-18) — the NON-LETTER key table, the "Space owns this VK"
//! bitmap the callback reads, and the chord recorder's hook-side switch.
//!
//! Before Phase A the hook had twelve FIXED `KeyCombo` variants (`Escape`,
//! `Backtick`, `Comma`, …) and the engine matched each to one hard-coded
//! special. Now every non-letter key is a `KeyCombo::Vk(u16)` and the engine
//! looks the key id up in the active profile's `bindings`, exactly as it does
//! for a letter. What the hook still has to decide — in microseconds, with no
//! config in reach — is whether Space OWNS this key at all (suppress and
//! dispatch) or not (pass it through to Windows). That is `BOUND_VKS`: a
//! 256-bit bitmap published on every config save, at boot and on every
//! profile switch, and read with ONE relaxed load and a shift.
//!
//! The table below is the ONLY mapping between the dashboard's key ids
//! (`src/components/keyboard-matrix.ts`) and virtual-key codes, in both
//! directions, and its ORDER is load-bearing: `middle_ring` derives a tile's
//! Private Use Area code from a key's index here (`'\u{E000}' + index`), so
//! rows are APPEND-ONLY — never reorder, never delete.

use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};

/// `(key id, virtual key)` for every non-letter key the dashboard knows that
/// has a VK at all. Letters are NOT here — they stay `KeyCombo::Alpha`. Fn
/// keys never reach the OS and Space itself is the modifier, so neither is
/// listed (the test names those exclusions explicitly).
///
/// APPEND-ONLY — see the module header.
pub const KEY_TABLE: &[(&str, u16)] = &[
    ("esc", 0x1B),
    ("backtick", 0xC0),  // VK_OEM_3, ` / ~ on a US layout
    ("tab", 0x09),
    ("backspace", 0x08),
    ("ralt", 0xA5),      // VK_RMENU
    ("comma", 0xBC),     // VK_OEM_COMMA
    ("period", 0xBE),    // VK_OEM_PERIOD
    ("semicolon", 0xBA), // VK_OEM_1
    ("slash", 0xBF),     // VK_OEM_2
    ("quote", 0xDE),     // VK_OEM_7
    ("up", 0x26),
    ("down", 0x28),
    ("left", 0x25),
    ("right", 0x27),
    ("enter", 0x0D),
    ("delete", 0x2E),
    ("pgup", 0x21),
    ("pgdn", 0x22),
    ("minus", 0xBD),     // VK_OEM_MINUS
    ("equal", 0xBB),     // VK_OEM_PLUS
    ("lbracket", 0xDB),  // VK_OEM_4
    ("rbracket", 0xDD),  // VK_OEM_6
    ("backslash", 0xDC), // VK_OEM_5
    ("caps", 0x14),
    ("lshift", 0xA0),
    ("rshift", 0xA1),
    ("lctrl", 0xA2),
    ("rctrl", 0xA3),
    ("lalt", 0xA4),
    ("win", 0x5B),       // VK_LWIN
    ("home", 0x24),
    ("end", 0x23),
    ("insert", 0x2D),
    ("0", 0x30),
    ("1", 0x31),
    ("2", 0x32),
    ("3", 0x33),
    ("4", 0x34),
    ("5", 0x35),
    ("6", 0x36),
    ("7", 0x37),
    ("8", 0x38),
    ("9", 0x39),
    ("f1", 0x70),
    ("f2", 0x71),
    ("f3", 0x72),
    ("f4", 0x73),
    ("f5", 0x74),
    ("f6", 0x75),
    ("f7", 0x76),
    ("f8", 0x77),
    ("f9", 0x78),
    ("f10", 0x79),
    ("f11", 0x7A),
    ("f12", 0x7B),
];

/// The dashboard key id for a virtual key, or `None` for a letter or a key
/// the board does not have. Hook-path safe: a linear scan over a 55-row
/// const, no allocation.
pub fn key_id_for_vk(vk: u16) -> Option<&'static str> {
    KEY_TABLE.iter().find(|(_, v)| *v == vk).map(|(id, _)| *id)
}

/// The virtual key for a dashboard key id, or `None` for a letter or an id
/// with no VK (`space`, `lfn`, `rfn`).
pub fn vk_for_key_id(id: &str) -> Option<u16> {
    KEY_TABLE.iter().find(|(k, _)| *k == id).map(|(_, v)| *v)
}

/// The table index of a key id — the icon ring's tile code is
/// `'\u{E000}' + this`. Stable because the table is append-only.
pub fn table_index(id: &str) -> Option<usize> {
    KEY_TABLE.iter().position(|(k, _)| *k == id)
}

/// The key id at a table index (the inverse of `table_index`).
pub fn key_id_at(index: usize) -> Option<&'static str> {
    KEY_TABLE.get(index).map(|(id, _)| *id)
}

/// The virtual key a key id OR a letter maps to. Letters are `a`–`z`.
pub fn vk_for_any_key_id(id: &str) -> Option<u16> {
    let mut chars = id.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_lowercase() => Some(c.to_ascii_uppercase() as u16),
        _ => vk_for_key_id(id),
    }
}

// ---------------------------------------------------------------------------
// The bitmap
// ---------------------------------------------------------------------------

/// 256 bits — "Space owns this VK". Bit `vk` set = suppress and dispatch as
/// `KeyCombo::Vk(vk)`. Starts all-clear, so it MUST be published at boot
/// (lib.rs) as well as on every save (`config::save`) and every profile
/// switch (`set_active_profile` goes through `save`, so that is the same
/// funnel). Read with `bound_vk`: one relaxed load and a shift, which is the
/// whole budget the callback has (PROBLEM 58).
pub static BOUND_VKS: [AtomicU64; 4] =
    [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

/// The VK the PAUSE special currently sits on (0 = none). The one key the
/// hook must still recognise while `BYPASS_MODE` is on — it is how the user
/// un-pauses — and the pause special is remappable, so the VK is published
/// beside the bitmap rather than hard-coded to `.`.
pub static PAUSE_VK: AtomicU16 = AtomicU16::new(0);

/// Hook-path safe. Letters are NOT in the bitmap (they are `Alpha`).
#[inline]
pub fn bound_vk(vk: u16) -> bool {
    bound_in(&load_bitmap(), vk)
}

/// The same test over a bitmap VALUE — the pure half, for tests and for
/// `own_window_combo_for_vk`'s test fixtures.
#[inline]
pub fn bound_in(bits: &[u64; 4], vk: u16) -> bool {
    let (word, bit) = ((vk >> 6) as usize, vk & 63);
    word < 4 && bits[word] & (1u64 << bit) != 0
}

fn load_bitmap() -> [u64; 4] {
    [
        BOUND_VKS[0].load(Ordering::Relaxed),
        BOUND_VKS[1].load(Ordering::Relaxed),
        BOUND_VKS[2].load(Ordering::Relaxed),
        BOUND_VKS[3].load(Ordering::Relaxed),
    ]
}

fn set_bit(bits: &mut [u64; 4], vk: u16) {
    let (word, bit) = ((vk >> 6) as usize, vk & 63);
    if word < 4 {
        bits[word] |= 1u64 << bit;
    }
}

/// PURE: the bitmap a config publishes. Two sources, both mapped through the
/// key table so a letter can never land here:
///
///  (a) the ACTIVE profile's mapped bindings whose key id is a non-letter —
///      the seeded specials, and anything the user put on a non-letter key;
///  (b) `special_keys` (the legacy Enter / F1–F12 / Left / Right map), minus
///      `tab`, which `publish_bound_specials` has treated as shadowed since
///      PROBLEM 219 and which stays that way.
///
/// Also answers the pause VK: the first key in the active profile whose
/// action is `Special { id: "pause" }`, letter or not.
pub fn bound_vks_for(cfg: &crate::config::AppConfig) -> ([u64; 4], u16) {
    let mut bits = [0u64; 4];
    let mut pause_vk = 0u16;
    if let Some(p) = cfg.profiles.iter().find(|p| p.name == cfg.active_profile) {
        for (key, bind) in &p.bindings {
            if !bind.is_mapped() {
                continue;
            }
            if let Some(vk) = vk_for_key_id(key) {
                set_bit(&mut bits, vk);
            }
            if pause_vk == 0
                && matches!(&bind.action, Some(crate::config::Action::Special { id }) if id == "pause")
            {
                pause_vk = vk_for_any_key_id(key).unwrap_or(0);
            }
        }
    }
    for (name, bind) in &cfg.special_keys {
        if !bind.is_mapped() || name == "tab" {
            continue;
        }
        if let Some(vk) = vk_for_key_id(name) {
            set_bit(&mut bits, vk);
        }
    }
    (bits, pause_vk)
}

/// Publish the bitmap and the pause VK. Called from `config::save` (the one
/// funnel every mutation goes through, profile switches included), and from
/// the startup load in lib.rs. Logs ONCE per change, off the hook path.
pub fn publish_bound_vks(cfg: &crate::config::AppConfig) {
    let (bits, pause_vk) = bound_vks_for(cfg);
    let mut changed = false;
    for (slot, word) in BOUND_VKS.iter().zip(bits.iter()) {
        if slot.swap(*word, Ordering::Relaxed) != *word {
            changed = true;
        }
    }
    if PAUSE_VK.swap(pause_vk, Ordering::Relaxed) != pause_vk {
        changed = true;
    }
    if changed {
        let ids: Vec<&str> = KEY_TABLE
            .iter()
            .filter(|(_, vk)| bound_in(&bits, *vk))
            .map(|(id, _)| *id)
            .collect();
        log::info!(
            "hook: Space now owns these non-letter keys for profile '{}' (Phase A trigger \
             map, published from config): [{}]; pause special on VK {pause_vk:#04X}. Every \
             other non-letter key passes through to Windows while Space is held.",
            cfg.active_profile,
            ids.join(", ")
        );
    }
}

// ---------------------------------------------------------------------------
// The chord recorder's hook-side switch
// ---------------------------------------------------------------------------

/// While true the callback forwards EVERY key (down and up, Space included)
/// to the engine as `HookEvent::RawKey` and passes it through untouched —
/// nothing is suppressed, no hold starts. Flipped by
/// `engine::chord_recorder::start/stop`; the callback itself clears it once
/// `RECORD_DEADLINE` has passed, so a page that forgot to stop cannot leave
/// the app in record mode.
pub static RECORDING: AtomicBool = AtomicBool::new(false);
/// `tick_count()` after which `RECORDING` reads as off. 15 s from start.
pub static RECORD_DEADLINE: AtomicU64 = AtomicU64::new(0);
/// How long a recording may run before the hook stops it by itself.
pub const RECORD_MAX_MS: u64 = 15_000;

/// Hook-path: is a recording live at `now`? One load each; clears the flag
/// when the deadline has passed so the NEXT keystroke is ordinary again.
#[inline]
pub fn recording_at(now: u64) -> bool {
    if !RECORDING.load(Ordering::Relaxed) {
        return false;
    }
    if now > RECORD_DEADLINE.load(Ordering::Relaxed) {
        RECORDING.store(false, Ordering::Relaxed);
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Action, AppConfig, BindingMap, KeyBinding, Profile};

    /// Every id round-trips both ways, no VK is listed twice, no id is
    /// listed twice, and no letter is in the table.
    #[test]
    fn every_table_row_round_trips_and_is_unique() {
        for (i, (id, vk)) in KEY_TABLE.iter().enumerate() {
            assert_eq!(vk_for_key_id(id), Some(*vk), "{id}");
            assert_eq!(key_id_for_vk(*vk), Some(*id), "{vk:#04X}");
            assert_eq!(table_index(id), Some(i));
            assert_eq!(key_id_at(i), Some(*id));
            assert!(!(0x41..=0x5A).contains(vk), "{id}: letters stay Alpha");
        }
        let mut vks: Vec<u16> = KEY_TABLE.iter().map(|(_, v)| *v).collect();
        vks.sort_unstable();
        vks.dedup();
        assert_eq!(vks.len(), KEY_TABLE.len(), "one VK, one id");
        let mut ids: Vec<&str> = KEY_TABLE.iter().map(|(k, _)| *k).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), KEY_TABLE.len(), "one id, one VK");
        assert_eq!(key_id_for_vk(0x41), None, "A is a letter");
        assert_eq!(vk_for_key_id("a"), None);
        assert_eq!(vk_for_any_key_id("a"), Some(0x41));
        assert_eq!(vk_for_any_key_id("esc"), Some(0x1B));
        assert_eq!(vk_for_any_key_id("space"), None);
    }

    /// THE FIRST TWELVE ROWS ARE THE SEEDED SPECIALS' KEYS, IN SEED ORDER.
    /// `middle_ring` codes a tile as `'\u{E000}' + index`, so the seeded
    /// specials keep the U+E000–U+E00B codes the ring has always used.
    /// Append-only: this test is the fence.
    #[test]
    fn the_seed_keys_are_the_first_rows_in_seed_order() {
        for (i, (key, _)) in crate::config::DEFAULT_SPECIALS.iter().enumerate() {
            assert_eq!(KEY_TABLE[i].0, *key, "row {i}");
        }
    }

    /// Every non-letter key the DASHBOARD draws is in the table, except the
    /// three that have no VK. Read from the TypeScript source so the two
    /// cannot drift: a key added to the board without a row here would be
    /// `.bindable` on the page and dead in the hook.
    #[test]
    fn every_board_key_id_without_a_vk_exception_is_in_the_table() {
        // Source: src/components/keyboard-matrix.ts, the ROWS table. Each
        // entry is `["label", "id", units]`.
        let ts = include_str!("../../../src/components/keyboard-matrix.ts");
        let start = ts.find("const ROWS").expect("ROWS table");
        let end = ts[start..].find("];\n").map(|e| start + e).expect("ROWS end");
        let rows = &ts[start..end];
        let mut ids = Vec::new();
        // The label may itself be `,` or `\\`, so this reads QUOTED STRINGS,
        // not comma-separated fields: the first quoted string after `["` is
        // the label, the second is the id.
        let mut rest = rows;
        while let Some(open) = rest.find("[\"") {
            let after = &rest[open + 2..];
            let Some(q1) = after.find('"') else { break };
            let after2 = &after[q1 + 1..];
            let Some(q2) = after2.find('"') else { break };
            let after3 = &after2[q2 + 1..];
            let Some(q3) = after3.find('"') else { break };
            ids.push(after3[..q3].to_string());
            rest = &after3[q3..];
        }
        assert!(ids.len() > 60, "parsed {} ids from ROWS — parser broke?", ids.len());
        const NO_VK: &[&str] = &["space", "lfn", "rfn"];
        for id in &ids {
            let letter = id.len() == 1 && id.chars().all(|c| c.is_ascii_lowercase());
            if letter || NO_VK.contains(&id.as_str()) {
                assert_eq!(vk_for_key_id(id), None, "{id} must not be in the table");
                continue;
            }
            assert!(vk_for_key_id(id).is_some(), "board key {id:?} has no VK row");
        }
        // And the board still draws every seeded key that it can (Esc is
        // not on the board — it lives in the tray's cards).
        for (key, _) in crate::config::DEFAULT_SPECIALS {
            if *key == "esc" {
                continue;
            }
            assert!(ids.iter().any(|i| i == key), "seeded key {key} is not on the board");
        }
    }

    fn cfg_with(bindings: BindingMap, specials: BindingMap) -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.profiles = vec![Profile {
            name: "P".into(),
            bindings,
            emoji: None,
            specials_seeded: true,
        }];
        cfg.active_profile = "P".into();
        cfg.special_keys = specials;
        cfg
    }

    /// The bitmap: seeded specials set their bits, letters set nothing,
    /// an unmapped non-letter sets nothing, `special_keys` adds its own,
    /// and `special_keys["tab"]` stays shadowed.
    #[test]
    fn the_bitmap_is_built_from_the_active_profile_and_special_keys() {
        let mut cfg = cfg_with(BindingMap::new(), BindingMap::new());
        assert!(!crate::config::seed_specials(&mut cfg), "already flagged");
        cfg.profiles[0].specials_seeded = false;
        assert!(crate::config::seed_specials(&mut cfg));
        cfg.profiles[0].bindings.insert(
            "a".into(),
            KeyBinding { app: Some("a.exe".into()), ..Default::default() },
        );
        cfg.profiles[0].bindings.insert("pgup".into(), KeyBinding::default());
        cfg.special_keys.insert(
            "enter".into(),
            KeyBinding { web_url: Some("https://x".into()), ..Default::default() },
        );
        cfg.special_keys.insert(
            "tab".into(),
            KeyBinding { web_url: Some("https://y".into()), ..Default::default() },
        );
        let (bits, pause) = bound_vks_for(&cfg);
        for (key, _) in crate::config::DEFAULT_SPECIALS {
            assert!(bound_in(&bits, vk_for_key_id(key).unwrap()), "{key}");
        }
        assert!(!bound_in(&bits, 0x41), "letters are Alpha, never in the bitmap");
        assert!(!bound_in(&bits, 0x21), "an unmapped pgup is not owned");
        assert!(bound_in(&bits, 0x0D), "special_keys enter");
        // Tab IS set — but from the seeded pip_fullscreen, not from special_keys.
        cfg.profiles[0].bindings.remove("tab");
        let (bits2, _) = bound_vks_for(&cfg);
        assert!(!bound_in(&bits2, 0x09), "special_keys tab stays shadowed (PROBLEM 219)");
        assert_eq!(pause, 0xBE, "pause is on `.` by default");

        // Move pause to a letter: the pause VK follows it.
        cfg.profiles[0].bindings.remove("period");
        cfg.profiles[0].bindings.insert(
            "q".into(),
            KeyBinding { action: Some(Action::Special { id: "pause".into() }), ..Default::default() },
        );
        let (bits3, pause3) = bound_vks_for(&cfg);
        assert!(!bound_in(&bits3, 0xBE), "`.` is free again");
        assert_eq!(pause3, 0x51, "pause now on Q");

        // No pause anywhere → 0.
        cfg.profiles[0].bindings.remove("q");
        assert_eq!(bound_vks_for(&cfg).1, 0);
        // An inactive profile contributes nothing (special_keys cleared, so
        // the only remaining source is the profile that is no longer active).
        cfg.special_keys.clear();
        cfg.active_profile = "Other".into();
        assert_eq!(bound_vks_for(&cfg).0, [0, 0, 0, 0]);
    }

    /// The recorder's hook-side switch honours its deadline by itself.
    #[test]
    fn recording_stops_itself_at_the_deadline() {
        RECORDING.store(true, Ordering::Relaxed);
        RECORD_DEADLINE.store(1_000, Ordering::Relaxed);
        assert!(recording_at(999));
        assert!(recording_at(1_000));
        assert!(!recording_at(1_001), "past the deadline");
        assert!(!RECORDING.load(Ordering::Relaxed), "and the flag is cleared");
        assert!(!recording_at(0), "off stays off");
    }
}
