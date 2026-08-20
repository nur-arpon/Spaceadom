/// hook/conflicts.rs — detect other software that also remaps the keyboard.
///
/// WHY THIS EXISTS (2026-08-11): a tester's Space+key did nothing, and his own
/// first guess was "I used to have AutoHotkey scripts on the spacebar". We had
/// no way to confirm or rule that out — the app was blind to its neighbours,
/// and the log said nothing. Two programs owning a WH_KEYBOARD_LL hook on the
/// same key is a genuine, common failure mode, and the user should be TOLD,
/// not left guessing.
///
/// SCOPE, deliberately narrow: this only ever OBSERVES and REPORTS. It never
/// kills, suspends or modifies another process — someone else's running
/// software is not ours to terminate, and a remapper that force-closes its
/// competitors is malware behaviour. The dashboard shows a banner and lets the
/// user decide.

use serde::Serialize;

/// One detected piece of remapping software.
#[derive(Debug, Clone, Serialize)]
pub struct Conflict {
    /// Process name as shown to the user, e.g. "AutoHotkey64.exe".
    pub process: String,
    /// Friendly product name, e.g. "AutoHotkey".
    pub product: String,
    /// Plain-English explanation of the risk.
    pub detail: String,
}

/// Known keyboard-remapping / macro software, matched on process name.
/// Keep this list conservative: a false positive tells the user their machine
/// is misconfigured when it is fine, which is worse than staying quiet.
const KNOWN: &[(&str, &str, &str)] = &[
    ("autohotkey.exe",    "AutoHotkey",         "AutoHotkey scripts can capture Space before Spaceadom sees it."),
    ("autohotkey64.exe",  "AutoHotkey v2",      "AutoHotkey scripts can capture Space before Spaceadom sees it."),
    ("autohotkeyu64.exe", "AutoHotkey",         "AutoHotkey scripts can capture Space before Spaceadom sees it."),
    ("autohotkeyu32.exe", "AutoHotkey",         "AutoHotkey scripts can capture Space before Spaceadom sees it."),
    ("powertoys.exe",     "PowerToys",          "PowerToys Keyboard Manager can remap keys system-wide."),
    ("powertoys.keyboardmanager.engine.exe", "PowerToys Keyboard Manager", "This remaps keys system-wide and can swallow Space."),
    ("sharpkeys.exe",     "SharpKeys",          "SharpKeys rewrites keys at the registry level."),
    ("keytweak.exe",      "KeyTweak",           "KeyTweak remaps keys at the driver level."),
    ("kbdedit.exe",       "KbdEdit",            "KbdEdit installs custom keyboard layouts."),
    ("hidmacros.exe",     "HIDmacros",          "HIDmacros intercepts raw keyboard input."),
    ("luamacros.exe",     "LuaMacros",          "LuaMacros intercepts raw keyboard input."),
    ("spacedesk",         "spacedesk",          "spacedesk forwards input to a second display and can intercept keys."),
    ("spacetoggleruntime.exe", "SpaceToggle v11", "An older SpaceToggle build is running — two spacebar hooks will fight."),
    ("space-toggle-os.exe",    "SpaceToggle V13", "An older SpaceToggle build is running — two spacebar hooks will fight."),
    ("space-toggle-v14.exe",   "SpaceToggle V14", "An older SpaceToggle build is running — two spacebar hooks will fight."),
];

/// Scan running processes for known remappers.
///
/// Skips our OWN process (matching on name alone would otherwise make
/// Spaceadom report itself as a conflict).
#[cfg(windows)]
pub fn detect() -> Vec<Conflict> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut found: Vec<Conflict> = Vec::new();
    let me = std::process::id();
    let my_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_lowercase()))
        .unwrap_or_default();

    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return found;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();

                if entry.th32ProcessID != me && name != my_name {
                    for (proc_name, product, detail) in KNOWN {
                        // spacedesk ships several executables; match by prefix
                        // for that one entry, exact otherwise.
                        let hit = if *proc_name == "spacedesk" {
                            name.starts_with("spacedesk")
                        } else {
                            name == *proc_name
                        };
                        if hit && !found.iter().any(|c| c.product == *product) {
                            found.push(Conflict {
                                process: name.clone(),
                                product: (*product).to_string(),
                                detail: (*detail).to_string(),
                            });
                        }
                    }
                }

                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }

    if found.is_empty() {
        log::info!("conflicts: no known keyboard-remapping software running");
    } else {
        for c in &found {
            log::warn!(
                "conflicts: {} is running ({}) — {}",
                c.product, c.process, c.detail
            );
        }
    }
    found
}

#[cfg(not(windows))]
pub fn detect() -> Vec<Conflict> { Vec::new() }

/// Every process name this app recognises as a keyboard conflict.
///
/// PROBLEM 155 — `conflict_close` validates against this before it will end
/// anything. That command is reachable from the webview, so without a closed
/// list it would be a "terminate any process by name" primitive.
pub fn known_process_names() -> Vec<&'static str> {
    KNOWN.iter().map(|(p, _, _)| *p).collect()
}

/// Is `name` a process this app recognises as a keyboard conflict?
///
/// **Uses the SAME matching rule as `detect()`, and that is the whole point.**
/// `Conflict.process` carries the REAL running exe name, which for spacedesk
/// is `spacedeskservice.exe` while its list key is `spacedesk` — so a plain
/// equality check against the keys refused every spacedesk close silently
/// (PROBLEM 157: the owner pressed the button, nothing happened, and the
/// refusal was one line of small grey text he never saw).
///
/// Any future prefix entry inherits this for free. If `detect()`'s rule ever
/// changes, change it HERE too — two matchers that must agree and live apart
/// are exactly how this broke.
pub fn is_known_process(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    KNOWN.iter().any(|(proc_name, _, _)| {
        if *proc_name == "spacedesk" { n.starts_with("spacedesk") } else { n == *proc_name }
    })
}
