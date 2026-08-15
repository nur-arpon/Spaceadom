/// config/defaults.rs — Hardcoded seed data for the three built-in profiles.
/// Mirrors the V11 AHK RouteShortcut() maps exactly (install-v11.ps1 lines 472–557).

use super::schema::{KeyBinding, Profile};
use std::collections::HashMap;

fn binding(app: Option<&str>, web: Option<&str>, label: &str) -> KeyBinding {
    KeyBinding {
        app: app.map(str::to_string),
        web_url: web.map(str::to_string),
        label: Some(label.to_string()),
        icon_override: None,
    }
}

/// Founders profile — productivity / communication / web-focused.
pub fn founders_profile() -> Profile {
    let mut b: HashMap<String, KeyBinding> = HashMap::new();
    b.insert("a".into(), binding(None, Some("https://gemini.google.com"), "Gemini"));
    b.insert("b".into(), binding(Some("brave.exe"), None, "Brave"));
    b.insert("c".into(), binding(Some("chrome.exe"), None, "Chrome"));
    b.insert("d".into(), binding(Some("Discord.exe"), None, "Discord"));
    b.insert("e".into(), binding(None, Some("https://docs.google.com/spreadsheets"), "Sheets"));
    b.insert("f".into(), binding(Some("explorer.exe"), None, "Explorer"));
    b.insert("g".into(), binding(None, Some("https://mail.google.com"), "Gmail"));
    b.insert("h".into(), binding(None, Some("https://github.com"), "GitHub"));
    b.insert("i".into(), binding(None, Some("https://instagram.com"), "Instagram"));
    b.insert("j".into(), binding(None, Some("https://docs.google.com"), "Docs"));
    b.insert("k".into(), binding(None, Some("https://calendar.google.com"), "Calendar"));
    b.insert("l".into(), binding(None, Some("https://linkedin.com"), "LinkedIn"));
    b.insert("m".into(), binding(None, Some("https://cinemaos.live/"), "CinemaOS"));
    b.insert("n".into(), binding(None, Some("https://keep.google.com"), "Keep"));
    b.insert("o".into(), binding(None, Some("https://drive.google.com"), "Drive"));
    b.insert("p".into(), binding(None, Some("https://photos.google.com"), "Photos"));
    b.insert("q".into(), binding(None, Some("https://notebooklm.google.com"), "NotebookLM"));
    b.insert("r".into(), binding(None, Some("https://reddit.com"), "Reddit"));
    b.insert("s".into(), binding(Some("Spotify.exe"), None, "Spotify"));
    b.insert("t".into(), binding(Some("wt.exe"), None, "Terminal"));
    b.insert("u".into(), binding(Some("uTorrent.exe"), None, "uTorrent"));
    b.insert("v".into(), binding(Some("vlc.exe"), None, "VLC"));
    b.insert("w".into(), binding(Some("WhatsApp.exe"), None, "WhatsApp"));
    b.insert("x".into(), binding(None, Some("https://x.com"), "X"));
    b.insert("y".into(), binding(None, Some("https://youtube.com"), "YouTube"));
    b.insert("z".into(), binding(Some("Zoom.exe"), None, "Zoom"));
    Profile { name: "Founders".into(), bindings: b }
}

/// Gamers profile — gaming launchers and services.
pub fn gamers_profile() -> Profile {
    let mut b: HashMap<String, KeyBinding> = HashMap::new();
    b.insert("a".into(), binding(Some("RadeonSoftware.exe"), None, "Radeon"));
    b.insert("b".into(), binding(Some("Battle.net.exe"), None, "Battle.net"));
    b.insert("c".into(), binding(Some("cs2.exe"), None, "CS2"));
    b.insert("d".into(), binding(Some("Discord.exe"), None, "Discord"));
    b.insert("e".into(), binding(Some("EpicGamesLauncher.exe"), None, "Epic"));
    b.insert("f".into(), binding(Some("FortniteClient-Win64-Shipping.exe"), None, "Fortnite"));
    b.insert("g".into(), binding(Some("NVIDIA GeForce Experience.exe"), None, "GeForce"));
    b.insert("h".into(), binding(Some("HaloInfinite.exe"), None, "Halo"));
    b.insert("i".into(), binding(Some("itch.exe"), None, "itch.io"));
    b.insert("j".into(), KeyBinding::default());
    b.insert("k".into(), KeyBinding::default());
    b.insert("l".into(), binding(Some("LeagueClient.exe"), None, "League"));
    b.insert("m".into(), binding(Some("MSIAfterburner.exe"), None, "Afterburner"));
    b.insert("n".into(), binding(Some("NVIDIA app.exe"), None, "NVIDIA"));
    b.insert("o".into(), binding(Some("obs64.exe"), None, "OBS"));
    b.insert("p".into(), binding(Some("TslGame.exe"), None, "PUBG"));
    b.insert("q".into(), KeyBinding::default());
    b.insert("r".into(), binding(None, Some("https://reddit.com"), "Reddit"));
    b.insert("s".into(), binding(Some("steam.exe"), None, "Steam"));
    b.insert("t".into(), binding(None, Some("https://twitch.tv"), "Twitch"));
    b.insert("u".into(), binding(Some("uTorrent.exe"), None, "uTorrent"));
    b.insert("v".into(), KeyBinding::default());
    b.insert("w".into(), KeyBinding::default());
    b.insert("x".into(), binding(Some("Xbox.exe"), None, "Xbox"));
    b.insert("y".into(), binding(None, Some("https://gaming.youtube.com"), "YT Gaming"));
    b.insert("z".into(), KeyBinding::default());
    Profile { name: "Gamers".into(), bindings: b }
}

/// Professionals profile — creative and productivity tools.
pub fn professionals_profile() -> Profile {
    let mut b: HashMap<String, KeyBinding> = HashMap::new();
    b.insert("a".into(), binding(Some("Photoshop.exe"), None, "Photoshop"));
    b.insert("b".into(), binding(Some("blender.exe"), None, "Blender"));
    b.insert("c".into(), binding(Some("Canva.exe"), None, "Canva"));
    b.insert("d".into(), binding(Some("Resolve.exe"), None, "DaVinci"));
    b.insert("e".into(), binding(Some("excel.exe"), None, "Excel"));
    b.insert("f".into(), binding(Some("explorer.exe"), None, "Explorer"));
    b.insert("g".into(), binding(None, Some("https://github.com"), "GitHub"));
    b.insert("h".into(), binding(None, Some("https://github.com"), "GitHub"));
    b.insert("i".into(), binding(Some("Illustrator.exe"), None, "Illustrator"));
    b.insert("j".into(), binding(Some("idea64.exe"), None, "IntelliJ"));
    b.insert("k".into(), KeyBinding::default());
    b.insert("l".into(), binding(None, Some("https://linkedin.com"), "LinkedIn"));
    b.insert("m".into(), KeyBinding::default());
    b.insert("n".into(), binding(Some("Notion.exe"), None, "Notion"));
    b.insert("o".into(), binding(Some("outlook.exe"), None, "Outlook"));
    b.insert("p".into(), binding(Some("powerpnt.exe"), None, "PowerPoint"));
    b.insert("q".into(), KeyBinding::default());
    b.insert("r".into(), binding(None, Some("https://reddit.com"), "Reddit"));
    b.insert("s".into(), binding(Some("slack.exe"), None, "Slack"));
    b.insert("t".into(), binding(Some("Telegram.exe"), None, "Telegram"));
    b.insert("u".into(), KeyBinding::default());
    b.insert("v".into(), KeyBinding::default());
    b.insert("w".into(), KeyBinding::default());
    b.insert("x".into(), KeyBinding::default());
    b.insert("y".into(), KeyBinding::default());
    b.insert("z".into(), KeyBinding::default());
    Profile { name: "Professionals".into(), bindings: b }
}

/// Returns all three built-in seed profiles.
pub fn generate() -> Vec<super::schema::Profile> {
    vec![founders_profile(), gamers_profile(), professionals_profile()]
}
