//! PHASE A (2026-09-18) — the built-in specials' NAMES, and the two lists the
//! rings draw from the active profile's non-letter bindings.
//!
//! Before Phase A the Space ring read a static `HUD_SPECIALS` slice and the
//! icon ring a static `RING_SPECIALS` slice, each naming the twelve fixed
//! keys. Both are DERIVED now — from whatever the active profile binds on its
//! non-letter keys — so that a user who moves the Boss Key to F1, or removes
//! the pause special, or puts `ms-settings:display` on `7`, sees exactly that
//! on both rings. With the seeded defaults the output is what the two static
//! lists used to say, row for row; `band_gate_tests` and the middle-ring
//! tests pin that against a seeded fixture.
//!
//! Pure module: no I/O, no locks, no Tauri. Everything here runs on the
//! Space-hold latency path inside a config borrow that already exists.

use crate::config::{Action, AppConfig, KeyBinding};
use crate::hook::keys::{key_id_at, table_index, KEY_TABLE};

/// One built-in special: `(id, HUD name, ring name, glyph)`.
///
/// * The HUD name is the Space ring's inner-band label — today's strings,
///   byte for byte ("Boss Key (Hide All + Mute)" and the rest).
/// * The ring name is the icon ring's centre-pill label — SHORT, because the
///   pill grows to fit and a long one crowds the inner circle.
/// * The glyph stands in for an icon when the special sits on a LETTER (the
///   ring and the HUD draw the letter disc; the glyph is the disc's text).
const SPECIALS: &[(&str, &str, &str, &str)] = &[
    ("boss_key", "Boss Key (Hide All + Mute)", "Boss Key", "⏏"),
    ("pip", "Multi-Corner PiP Mode", "PiP", "▣"),
    ("pip_fullscreen", "Fullscreen PiP", "Fullscreen PiP", "⛶"),
    ("force_close", "Force Close App", "Force Close", "⌧"),
    ("cycle_profile", "Cycle OS Profiles", "Cycle Profiles", "⟳"),
    ("search", "Contextual Search/Input", "Search / Input", "⌕"),
    ("pause", "Pause Spaceadom", "Pause Spaceadom", "⏸"),
    ("voice_typing", "Voice Typing", "Voice Typing", "♪"),
    ("screenshot", "Screenshot", "Screenshot", "✂"),
    ("osk", "On-screen Keyboard", "Keyboard", "⌨"),
    ("scroll_top", "Scroll Top", "Scroll Top", "⤒"),
    ("scroll_bottom", "Scroll Bottom", "Scroll Bottom", "⤓"),
];

fn row(id: &str) -> Option<&'static (&'static str, &'static str, &'static str, &'static str)> {
    SPECIALS.iter().find(|(i, _, _, _)| *i == id)
}

/// The Space ring's label for a special id (`"Boss Key (Hide All + Mute)"`),
/// or the id itself for one this build does not know.
pub fn display_name(id: &str) -> String {
    row(id).map(|r| r.1.to_string()).unwrap_or_else(|| id.to_string())
}

/// The icon ring's short label (`"Boss Key"`).
pub fn ring_name(id: &str) -> String {
    row(id).map(|r| r.2.to_string()).unwrap_or_else(|| id.to_string())
}

/// The one-character stand-in for a special on a letter disc.
pub fn glyph(id: &str) -> &'static str {
    row(id).map(|r| r.3).unwrap_or("★")
}

/// The generic glyph for an action that is not a special.
pub fn action_glyph(action: &Action) -> &'static str {
    match action {
        Action::Uri { .. } => "⚙",
        Action::Chord { .. } => "⌨",
        Action::Command { .. } => ">_",
        Action::Brightness { .. } => "☼",
        Action::Toggle { .. } => "⇄",
        Action::Special { id } => glyph(id),
    }
}

/// The human name of a virtual key inside a chord: `Win`, `Shift`, `Ctrl`,
/// `Alt`, a letter, a digit, an F-key, the key table's label, else `VK 0xNN`.
pub fn vk_name(vk: u16) -> String {
    match vk {
        0x5B | 0x5C => "Win".into(),
        0x10 | 0xA0 | 0xA1 => "Shift".into(),
        0x11 | 0xA2 | 0xA3 => "Ctrl".into(),
        0x12 | 0xA4 => "Alt".into(),
        0xA5 => "RAlt".into(),
        0x20 => "Space".into(),
        0x2C => "PrtSc".into(),
        0x5D => "Menu".into(),
        0xAD => "Mute".into(),
        0xAE => "Vol−".into(),
        0xAF => "Vol+".into(),
        0xB0 => "Next".into(),
        0xB1 => "Prev".into(),
        0xB2 => "Stop".into(),
        0xB3 => "Play".into(),
        0x41..=0x5A => char::from_u32(vk as u32).unwrap_or('?').to_string(),
        0x30..=0x39 => char::from_u32(vk as u32).unwrap_or('?').to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        _ => match crate::hook::key_id_for_vk(vk) {
            Some(id) => key_label(id),
            None => format!("VK 0x{vk:02X}"),
        },
    }
}

/// `Win+Shift+S` from `[0x5B, 0x10, 0x53]`. An empty chord reads as "(no
/// keys)" rather than an empty string, so a toast can never be blank.
pub fn chord_name(keys: &[u16]) -> String {
    if keys.is_empty() {
        return "(no keys)".into();
    }
    keys.iter().map(|k| vk_name(*k)).collect::<Vec<_>>().join("+")
}

/// A SHORT name for an action with no label: what the rings and the board
/// show, and the toast's subject.
pub fn action_name(action: &Action) -> String {
    match action {
        Action::Uri { target } => target.clone(),
        Action::Chord { keys } => chord_name(keys),
        Action::Command { line } => line
            .split_whitespace()
            .next()
            .map(|t| t.rsplit(['\\', '/']).next().unwrap_or(t).to_string())
            .unwrap_or_else(|| "Command".into()),
        Action::Brightness { delta } if *delta >= 0 => format!("Brightness +{delta}"),
        Action::Brightness { delta } => format!("Brightness −{}", delta.abs()),
        // The catalogue row's name, so the ring says what the editor said.
        Action::Toggle { what } => crate::engine::actions::toggle::display_name(what)
            .map(str::to_string)
            .unwrap_or_else(|| format!("Toggle {what}")),
        Action::Special { id } => display_name(id),
    }
}

/// The name a binding shows: its `label` if it has one, else the action's
/// derived name, else the legacy app / URL, else nothing. ONE rule for the
/// Space ring, the icon ring, the toast and the board.
pub fn binding_name(bind: &KeyBinding) -> String {
    if let Some(l) = bind.label.as_deref().filter(|l| !l.trim().is_empty()) {
        return l.to_string();
    }
    if let Some(a) = &bind.action {
        return action_name(a);
    }
    bind.app
        .clone()
        .or_else(|| bind.web_url.clone())
        .unwrap_or_default()
}

/// The board's label for a non-letter key id — what the HUD prints in the
/// key column. Unknown ids come back as typed.
pub fn key_label(id: &str) -> String {
    match id {
        "esc" => "Esc",
        "backtick" => "`",
        "tab" => "Tab",
        "backspace" => "⌫",
        "ralt" => "RAlt",
        "comma" => ",",
        "period" => ".",
        "semicolon" => ";",
        "slash" => "/",
        "quote" => "'",
        "up" => "↑",
        "down" => "↓",
        "left" => "←",
        "right" => "→",
        "enter" => "↵",
        "delete" => "Del",
        "pgup" => "PgUp",
        "pgdn" => "PgDn",
        "minus" => "-",
        "equal" => "=",
        "lbracket" => "[",
        "rbracket" => "]",
        "backslash" => "\\",
        "caps" => "Caps",
        "lshift" | "rshift" => "Shift",
        "lctrl" | "rctrl" => "Ctrl",
        "lalt" => "Alt",
        "win" => "Win",
        "home" => "Home",
        "end" => "End",
        "insert" => "Ins",
        other => {
            return match other.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                Some(n) if (1..=12).contains(&n) => format!("F{n}"),
                _ => other.to_uppercase(),
            }
        }
    }
    .to_string()
}

/// Is this key id a single lowercase letter (a LETTER binding)?
pub fn is_letter_id(id: &str) -> bool {
    let mut c = id.chars();
    matches!((c.next(), c.next()), (Some(ch), None) if ch.is_ascii_lowercase())
}

fn active_profile(cfg: &AppConfig) -> Option<&crate::config::Profile> {
    cfg.profiles.iter().find(|p| p.name == cfg.active_profile)
}

/// The active profile's mapped NON-letter bindings in key-table order:
/// `(key id, binding)`. The order is `KEY_TABLE`'s, which starts with the
/// twelve seeded keys in seed order — so a default profile lists Esc, `,
/// Tab, ⌫, RAlt, `,`, `.`, `;`, `/`, `'`, ↑, ↓ exactly as the old static
/// lists did.
pub fn non_letter_bindings(cfg: &AppConfig) -> Vec<(&'static str, &KeyBinding)> {
    let Some(p) = active_profile(cfg) else {
        return Vec::new();
    };
    KEY_TABLE
        .iter()
        .filter_map(|(id, _)| p.bindings.get(*id).filter(|b| b.is_mapped()).map(|b| (*id, b)))
        .collect()
}

fn special_id(bind: &KeyBinding) -> Option<&str> {
    match &bind.action {
        Some(Action::Special { id }) => Some(id.as_str()),
        _ => None,
    }
}

/// The Space ring's inner-band rows for `cfg`'s active profile:
/// `(key label, name)`, in key-table order, then the two GESTURE rows the
/// ring has always carried — "Scroll → Layer Opacity" (Space + wheel is a
/// gesture, not a key) and the double-tap row for the scroll specials, ONE
/// row for both when both are bound ("Up/Dn ×2" on their default keys), one
/// each otherwise, none when neither is.
pub fn hud_specials_for(cfg: &AppConfig) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut top: Option<&str> = None;
    let mut bottom: Option<&str> = None;
    for (id, bind) in non_letter_bindings(cfg) {
        match special_id(bind) {
            Some("scroll_top") => top = Some(id),
            Some("scroll_bottom") => bottom = Some(id),
            _ => rows.push((key_label(id), binding_name(bind))),
        }
    }
    rows.push(("Scroll".into(), "Layer Opacity".into()));
    let short = |id: &str| match id {
        "up" => "Up".to_string(),
        "down" => "Dn".to_string(),
        other => key_label(other),
    };
    match (top, bottom) {
        (Some(t), Some(b)) => {
            rows.push((format!("{}/{} ×2", short(t), short(b)), "Scroll Top/Bottom".into()))
        }
        (Some(t), None) => rows.push((format!("{} ×2", short(t)), "Scroll Top".into())),
        (None, Some(b)) => rows.push((format!("{} ×2", short(b)), "Scroll Bottom".into())),
        (None, None) => {}
    }
    rows
}

/// The icon ring's special tiles for `cfg`'s active profile: `(key label,
/// name, code)`. The scroll specials are left out — a tile release is one
/// press, and those need two. `code` is `'\u{E000}' + the key's table
/// index`: Private Use Area, so it can never collide with a bound letter, and
/// stable because the table is append-only.
pub fn ring_specials_for(cfg: &AppConfig) -> Vec<(String, String, char)> {
    non_letter_bindings(cfg)
        .into_iter()
        .filter(|(_, b)| !matches!(special_id(b), Some("scroll_top" | "scroll_bottom")))
        .filter_map(|(id, b)| {
            let code = code_for_key_id(id)?;
            let name = match &b.action {
                Some(Action::Special { id }) if b.label.as_deref().map_or(true, str::is_empty) => {
                    ring_name(id)
                }
                _ => binding_name(b),
            };
            Some((key_label(id), name, code))
        })
        .collect()
}

/// The tile code for a non-letter key id.
pub fn code_for_key_id(id: &str) -> Option<char> {
    table_index(id).and_then(|i| char::from_u32(0xE000 + i as u32))
}

/// The key id a tile code stands for (the inverse of `code_for_key_id`).
pub fn key_id_for_code(c: char) -> Option<&'static str> {
    let n = c as u32;
    if !(0xE000..0xE000 + KEY_TABLE.len() as u32).contains(&n) {
        return None;
    }
    key_id_at((n - 0xE000) as usize)
}

/// A freshly seeded profile — the fixture every ring test shares (the engine's
/// `band_gate_tests` and the middle ring's tests read it too).
#[cfg(test)]
pub(crate) fn seeded_cfg() -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.profiles = vec![crate::config::Profile {
        name: "Seeded".into(),
        bindings: crate::config::BindingMap::new(),
        emoji: None,
        specials_seeded: false,
    }];
    cfg.active_profile = "Seeded".into();
    assert!(crate::config::seed_specials(&mut cfg));
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_special_id_has_a_row_and_todays_names_are_kept() {
        for id in crate::config::SPECIAL_IDS {
            assert!(row(id).is_some(), "{id}");
        }
        assert_eq!(SPECIALS.len(), crate::config::SPECIAL_IDS.len());
        assert_eq!(display_name("boss_key"), "Boss Key (Hide All + Mute)");
        assert_eq!(display_name("pip"), "Multi-Corner PiP Mode");
        assert_eq!(display_name("pip_fullscreen"), "Fullscreen PiP");
        assert_eq!(display_name("force_close"), "Force Close App");
        assert_eq!(display_name("cycle_profile"), "Cycle OS Profiles");
        assert_eq!(display_name("search"), "Contextual Search/Input");
        assert_eq!(display_name("pause"), "Pause Spaceadom");
        assert_eq!(ring_name("osk"), "Keyboard");
        assert_eq!(ring_name("search"), "Search / Input");
        assert_eq!(display_name("nope"), "nope", "unknown ids come back as typed");
    }

    #[test]
    fn chord_and_action_names_read_like_a_person_would_write_them() {
        assert_eq!(chord_name(&[0x5B, 0x10, 0x53]), "Win+Shift+S");
        assert_eq!(chord_name(&[0x11, 0x24]), "Ctrl+Home");
        assert_eq!(chord_name(&[0xA2, 0x70]), "Ctrl+F1");
        assert_eq!(chord_name(&[]), "(no keys)");
        assert_eq!(vk_name(0xFF), "VK 0xFF");
        assert_eq!(action_name(&Action::Uri { target: "ms-settings:display".into() }), "ms-settings:display");
        assert_eq!(action_name(&Action::Command { line: r"C:\Tools\thing.exe --flag".into() }), "thing.exe");
        assert_eq!(action_name(&Action::Command { line: "".into() }), "Command");
        assert_eq!(action_name(&Action::Brightness { delta: 10 }), "Brightness +10");
        assert_eq!(action_name(&Action::Brightness { delta: -10 }), "Brightness −10");
        assert_eq!(action_name(&Action::Toggle { what: "night_light".into() }), "Night light on/off");
        assert_eq!(action_name(&Action::Toggle { what: "hologram".into() }), "Toggle hologram");
        assert_eq!(action_glyph(&Action::Toggle { what: "wifi".into() }), "⇄");
        assert_eq!(action_name(&Action::Special { id: "osk".into() }), "On-screen Keyboard");
        assert_eq!(action_glyph(&Action::Command { line: "x".into() }), ">_");
        assert_eq!(action_glyph(&Action::Special { id: "pause".into() }), "⏸");
    }

    #[test]
    fn binding_name_prefers_the_label_then_the_action_then_the_legacy_fields() {
        let mut b = KeyBinding { action: Some(Action::Special { id: "pip".into() }), ..Default::default() };
        assert_eq!(binding_name(&b), "Multi-Corner PiP Mode");
        b.label = Some("My PiP".into());
        assert_eq!(binding_name(&b), "My PiP");
        b.label = Some("   ".into());
        assert_eq!(binding_name(&b), "Multi-Corner PiP Mode", "a blank label is no label");
        let legacy = KeyBinding { app: Some("brave.exe".into()), ..Default::default() };
        assert_eq!(binding_name(&legacy), "brave.exe");
        assert_eq!(binding_name(&KeyBinding::default()), "");
    }

    #[test]
    fn key_labels_and_letter_ids() {
        assert_eq!(key_label("backspace"), "⌫");
        assert_eq!(key_label("f7"), "F7");
        assert_eq!(key_label("7"), "7");
        assert_eq!(key_label("rctrl"), "Ctrl");
        assert!(is_letter_id("q"));
        assert!(!is_letter_id("esc"));
        assert!(!is_letter_id("Q"));
        assert!(!is_letter_id(""));
    }

    /// THE DEFAULT RING READS AS IT ALWAYS DID: Esc, `, Tab, ⌫, RAlt, `,`,
    /// `.`, then the three 2026-09-17/18 specials, then the two gesture rows.
    #[test]
    fn a_seeded_profile_yields_todays_hud_rows() {
        let rows = hud_specials_for(&seeded_cfg());
        let keys: Vec<&str> = rows.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            vec!["Esc", "`", "Tab", "⌫", "RAlt", ",", ".", ";", "/", "'", "Scroll", "Up/Dn ×2"]
        );
        assert_eq!(rows[0].1, "Boss Key (Hide All + Mute)");
        assert_eq!(rows[2].1, "Fullscreen PiP");
        assert_eq!(rows[10].1, "Layer Opacity");
        assert_eq!(rows[11].1, "Scroll Top/Bottom");
    }

    /// Remove one special and its row goes; remove ONE scroll special and
    /// the double-tap row names only the other; move the Boss Key to F1 and
    /// the row follows it; a label wins over the special's name.
    #[test]
    fn the_hud_rows_follow_the_bindings() {
        let mut cfg = seeded_cfg();
        let p = &mut cfg.profiles[0];
        p.bindings.remove("period");
        p.bindings.remove("down");
        let esc = p.bindings.remove("esc").unwrap();
        p.bindings.insert("f1".into(), KeyBinding { label: Some("Panic".into()), ..esc });
        p.bindings.insert(
            "7".into(),
            KeyBinding { action: Some(Action::Uri { target: "ms-settings:display".into() }), ..Default::default() },
        );
        let rows = hud_specials_for(&cfg);
        let keys: Vec<&str> = rows.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["`", "Tab", "⌫", "RAlt", ",", ";", "/", "'", "7", "F1", "Scroll", "Up ×2"]);
        assert_eq!(rows[8].1, "ms-settings:display");
        assert_eq!(rows[9].1, "Panic");
        assert_eq!(rows[11].1, "Scroll Top");
        // Neither scroll special → no double-tap row at all.
        cfg.profiles[0].bindings.remove("up");
        let rows = hud_specials_for(&cfg);
        assert_eq!(rows.last().unwrap().0, "Scroll");
        // An unknown active profile → just the opacity gesture.
        cfg.active_profile = "Nope".into();
        assert_eq!(hud_specials_for(&cfg), vec![("Scroll".to_string(), "Layer Opacity".to_string())]);
    }

    /// The icon ring's tiles: the ten non-scroll specials with the codes the
    /// ring has always used (U+E000…), and the codes invert.
    #[test]
    fn a_seeded_profile_yields_todays_ring_tiles_with_stable_codes() {
        let tiles = ring_specials_for(&seeded_cfg());
        let keys: Vec<&str> = tiles.iter().map(|(k, _, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["Esc", "`", "Tab", "⌫", "RAlt", ",", ".", ";", "/", "'"]);
        let names: Vec<&str> = tiles.iter().map(|(_, n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec!["Boss Key", "PiP", "Fullscreen PiP", "Force Close", "Cycle Profiles",
                 "Search / Input", "Pause Spaceadom", "Voice Typing", "Screenshot", "Keyboard"]
        );
        let codes: Vec<char> = tiles.iter().map(|(_, _, c)| *c).collect();
        assert_eq!(codes[0], '\u{E000}');
        assert_eq!(codes[9], '\u{E009}');
        for (i, c) in codes.iter().enumerate() {
            assert!(!c.is_ascii_lowercase());
            let id = key_id_for_code(*c).unwrap();
            assert_eq!(code_for_key_id(id), Some(*c));
            assert_eq!(id, crate::config::DEFAULT_SPECIALS[i].0);
        }
        assert_eq!(key_id_for_code('m'), None);
        assert_eq!(key_id_for_code('\u{E0FF}'), None, "past the table");
    }
}
