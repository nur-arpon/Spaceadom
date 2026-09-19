//! 1.0.119 (brief 4 §2) — the Space ring's centre pill names the FOCUSED APP.
//!
//! The mouse ring's centre already names what the hand is over; the Space
//! ring's said "SPACE". Now, at hold start, the engine's existing foreground
//! query (the `hold start` line) also hands the HUD the app in front —
//! `HudFocus`: a short name ("Brave", "Word", "Explorer") and its icon from
//! the same cache the ring's pills use. The pill keeps its 230×60 box; only
//! the text and the glyph change.
//!
//! `None` keeps the word SPACE, and it is the answer for everything that is
//! not an app the user is working in: the desktop or the shell (explorer.exe
//! with no file window — `Progman` / `WorkerW` / the taskbar classes), the
//! lock screen and the other shell hosts, Spaceadom's own windows, a process
//! whose exe could not be read, and any error. Every rule here is pure and
//! pinned by `tests`; the one impure caller is `engine::dispatch`'s
//! hold-start path, through `hook::exclusions::foreground_info`.

use crate::config::AppConfig;

/// What the pill shows in place of "SPACE".
#[derive(serde::Serialize, Clone, Debug, PartialEq, Eq)]
pub struct HudFocus {
    /// Already truncated (`truncate_name`, `NAME_MAX`).
    pub name: String,
    /// A complete `data:` URL, or `None` for name only.
    pub icon: Option<String>,
}

/// The foreground window as the OS reports it (see `foreground_info`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ForegroundInfo {
    /// The owning process's full exe path.
    pub path: String,
    /// `normalize_stem(path)` — lowercase, no `.exe`.
    pub stem: String,
    /// The window class name (`GetClassNameW`), possibly empty.
    pub class: String,
}

/// The pill's character budget before the ellipsis (the brief: "~14").
pub const NAME_MAX: usize = 14;

/// Exe stems that are the SHELL, not an app: the pill says SPACE for them.
const SHELL_STEMS: &[&str] = &[
    "lockapp",
    "logonui",
    "searchhost",
    "searchapp",
    "searchui",
    "startmenuexperiencehost",
    "shellexperiencehost",
    "textinputhost",
    "applicationframehost",
    "dwm",
    "winlogon",
    "csrss",
];

/// explorer.exe window classes that are the desktop or the taskbar, not a
/// file window (`CabinetWClass` is the file window and is NOT listed).
const DESKTOP_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "NotifyIconOverflowWindow",
    "Windows.UI.Core.CoreWindow",
];

/// Short names for the exes whose stem is not their name. Everything else
/// is the stem with its first letter raised ("notepad" → "Notepad").
const KNOWN_NAMES: &[(&str, &str)] = &[
    ("brave", "Brave"),
    ("chrome", "Chrome"),
    ("msedge", "Edge"),
    ("firefox", "Firefox"),
    ("opera", "Opera"),
    ("vivaldi", "Vivaldi"),
    ("winword", "Word"),
    ("excel", "Excel"),
    ("powerpnt", "PowerPoint"),
    ("outlook", "Outlook"),
    ("olk", "Outlook"),
    ("onenote", "OneNote"),
    ("teams", "Teams"),
    ("ms-teams", "Teams"),
    ("explorer", "Explorer"),
    ("code", "VS Code"),
    ("devenv", "Visual Studio"),
    ("windowsterminal", "Terminal"),
    ("wt", "Terminal"),
    ("cmd", "Command Prompt"),
    ("powershell", "PowerShell"),
    ("pwsh", "PowerShell"),
    ("discord", "Discord"),
    ("spotify", "Spotify"),
    ("slack", "Slack"),
    ("claude", "Claude"),
    ("acrobat", "Acrobat"),
    ("acrord32", "Acrobat"),
    ("photoshop", "Photoshop"),
    ("illustrator", "Illustrator"),
    ("sldworks", "SolidWorks"),
    ("obs64", "OBS"),
    ("vlc", "VLC"),
    ("steam", "Steam"),
    ("whatsapp", "WhatsApp"),
    ("telegram", "Telegram"),
    ("zoom", "Zoom"),
    ("mspaint", "Paint"),
    ("calculatorapp", "Calculator"),
    ("snippingtool", "Snipping Tool"),
    ("msteams", "Teams"),
    ("wordpad", "WordPad"),
];

/// True when the foreground is the shell rather than an app the pill should
/// name: a shell-host process, or explorer.exe's desktop/taskbar windows.
pub fn is_shell_surface(stem: &str, class: &str) -> bool {
    if SHELL_STEMS.contains(&stem) {
        return true;
    }
    stem == "explorer" && (class.is_empty() || DESKTOP_CLASSES.iter().any(|c| c.eq_ignore_ascii_case(class)))
}

/// The short display name for an exe stem (`KNOWN_NAMES`, else capitalised).
pub fn short_name_for(stem: &str) -> String {
    let stem = stem.trim().to_lowercase();
    if let Some((_, name)) = KNOWN_NAMES.iter().find(|(s, _)| *s == stem) {
        return (*name).to_string();
    }
    let spaced = stem.replace(['-', '_'], " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `name` cut to `max` characters with a single "…" — by CHARACTERS, so a
/// multibyte name is never split inside a code point. Unchanged when it fits.
pub fn truncate_name(name: &str, max: usize) -> String {
    let name = name.trim();
    if name.chars().count() <= max {
        return name.to_string();
    }
    let keep = max.saturating_sub(1).max(1);
    let mut out: String = name.chars().take(keep).collect();
    out = out.trim_end().to_string();
    out.push('…');
    out
}

/// THE DECISION. `fg` is the foreground window at hold start (`None` = it
/// could not be read); `own_stem` is our own exe's stem; `lookup` is the
/// picker's icon cache (`extract_icon_cmd` keys it by the string the picker
/// bound — a full path or a bare `name.exe`), never a shell call.
///
/// Name precedence: the active profile's own binding for that exe (its
/// label, vendor prefix stripped as the ring does), else `short_name_for`.
/// Icon precedence, as `hud_icons_for`: the binding's `icon_override`, then
/// the cache by the binding's target, then the cache by the exe's path and
/// bare name. `None` for the shell, ourselves, and anything unreadable.
pub fn hud_focus_for(
    cfg: &AppConfig,
    profile_name: &str,
    fg: Option<&ForegroundInfo>,
    own_stem: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Option<HudFocus> {
    let fg = fg?;
    if fg.stem.is_empty() || fg.stem == own_stem || is_shell_surface(&fg.stem, &fg.class) {
        return None;
    }
    let as_url = |b64: String| crate::middle_ring::as_png_data_url(&b64);
    let bound = cfg.profiles.iter().find(|p| p.name == profile_name).and_then(|p| {
        p.bindings.values().find(|b| {
            b.is_mapped()
                && b.action.is_none()
                && b.app.as_deref().is_some_and(|a| crate::hook::exclusions::normalize_stem(a) == fg.stem)
        })
    });
    let (name, icon) = match bound {
        Some(b) => {
            let label = crate::engine::specials::binding_name(b);
            let (short, _) = crate::middle_ring::split_display_name(&label);
            let icon = b
                .icon_override
                .clone()
                .filter(|s| !s.is_empty())
                .map(as_url)
                .or_else(|| b.app.as_deref().and_then(lookup).map(as_url));
            (short, icon)
        }
        None => (short_name_for(&fg.stem), None),
    };
    let icon = icon
        .or_else(|| lookup(&fg.path).map(as_url))
        .or_else(|| lookup(&format!("{}.exe", fg.stem)).map(as_url));
    let name = truncate_name(&name, NAME_MAX);
    if name.is_empty() {
        return None;
    }
    Some(HudFocus { name, icon })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{KeyBinding, Profile};

    fn fg(path: &str, class: &str) -> ForegroundInfo {
        ForegroundInfo {
            stem: crate::hook::exclusions::normalize_stem(path),
            path: path.to_string(),
            class: class.to_string(),
        }
    }

    fn cfg_with(app: &str, label: Option<&str>) -> AppConfig {
        let mut bindings = std::collections::BTreeMap::new();
        bindings.insert(
            "b".to_string(),
            KeyBinding { app: Some(app.to_string()), label: label.map(str::to_string), ..Default::default() },
        );
        AppConfig {
            active_profile: "P".into(),
            profiles: vec![Profile { name: "P".into(), bindings, emoji: None, specials_seeded: true }],
            ..Default::default()
        }
    }

    const NO_ICON: &dyn Fn(&str) -> Option<String> = &|_| None;

    #[test]
    fn a_bound_app_uses_its_label_and_its_cached_icon() {
        let cfg = cfg_with(r"C:\Apps\brave.exe", Some("Google Chrome"));
        let lookup = |t: &str| (t == r"C:\Apps\brave.exe").then(|| "iVBOR".to_string());
        let f = hud_focus_for(&cfg, "P", Some(&fg(r"C:\Program Files\Brave\brave.exe", "Chrome_WidgetWin_1")), "spaceadom", &lookup)
            .expect("an app in front");
        assert_eq!(f.name, "Chrome", "the binding's label, vendor prefix stripped like the ring");
        assert_eq!(f.icon.as_deref(), Some("data:image/png;base64,iVBOR"));
    }

    #[test]
    fn an_unbound_app_gets_its_short_name_and_the_cache_by_path() {
        let cfg = cfg_with("notepad.exe", None);
        let lookup = |t: &str| (t == r"C:\Office\WINWORD.EXE").then(|| "AAAA".to_string());
        let f = hud_focus_for(&cfg, "P", Some(&fg(r"C:\Office\WINWORD.EXE", "OpusApp")), "spaceadom", &lookup).unwrap();
        assert_eq!(f.name, "Word");
        assert_eq!(f.icon.as_deref(), Some("data:image/png;base64,AAAA"));
        let f = hud_focus_for(&cfg, "P", Some(&fg(r"D:\tools\my-tool.exe", "X")), "spaceadom", NO_ICON).unwrap();
        assert_eq!(f.name, "My tool");
        assert!(f.icon.is_none());
    }

    /// Every fallback the brief lists keeps the word SPACE.
    #[test]
    fn the_shell_ourselves_and_the_unreadable_keep_space() {
        let cfg = cfg_with("brave.exe", None);
        let cases = [
            ("no foreground", None),
            ("desktop", Some(fg(r"C:\Windows\explorer.exe", "Progman"))),
            ("worker window", Some(fg(r"C:\Windows\explorer.exe", "WorkerW"))),
            ("taskbar", Some(fg(r"C:\Windows\explorer.exe", "Shell_TrayWnd"))),
            ("lock screen", Some(fg(r"C:\Windows\SystemApps\LockApp.exe", "Windows.UI.Core.CoreWindow"))),
            ("logon", Some(fg(r"C:\Windows\System32\LogonUI.exe", ""))),
            ("ourselves", Some(fg(r"C:\Users\x\AppData\Local\Spaceadom\spaceadom.exe", "Chrome_WidgetWin_1"))),
            ("unreadable", Some(ForegroundInfo::default())),
        ];
        for (what, f) in cases {
            assert!(hud_focus_for(&cfg, "P", f.as_ref(), "spaceadom", NO_ICON).is_none(), "{what}");
        }
        // But an Explorer FILE window is an app.
        let f = hud_focus_for(&cfg, "P", Some(&fg(r"C:\Windows\explorer.exe", "CabinetWClass")), "spaceadom", NO_ICON).unwrap();
        assert_eq!(f.name, "Explorer");
    }

    #[test]
    fn names_are_cut_at_fourteen_with_an_ellipsis() {
        assert_eq!(truncate_name("Brave", NAME_MAX), "Brave");
        assert_eq!(truncate_name("Fourteen chars", NAME_MAX), "Fourteen chars");
        assert_eq!(truncate_name("Fifteen charss!", NAME_MAX), "Fifteen chars…");
        assert_eq!(truncate_name("Visual Studio Code Insiders", NAME_MAX), "Visual Studio…");
        assert_eq!(truncate_name("ééééééééééééééé", NAME_MAX).chars().count(), NAME_MAX, "by characters, never bytes");
        let cfg = cfg_with("x.exe", None);
        let f = hud_focus_for(&cfg, "P", Some(&fg(r"C:\a\averyveryverylongprogramname.exe", "W")), "spaceadom", NO_ICON).unwrap();
        assert_eq!(f.name.chars().count(), NAME_MAX);
        assert!(f.name.ends_with('…'));
    }

    #[test]
    fn short_names_come_from_the_table_then_capitalisation() {
        assert_eq!(short_name_for("MSEDGE"), "Edge");
        assert_eq!(short_name_for("powerpnt"), "PowerPoint");
        assert_eq!(short_name_for("obsidian"), "Obsidian");
        assert_eq!(short_name_for("some_app-x"), "Some app x");
        assert_eq!(short_name_for(""), "");
    }
}
