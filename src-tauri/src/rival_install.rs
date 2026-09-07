//! rival_install.rs — detect and remove a SECOND copy of Spaceadom.
//!
//! PROBLEM 141. The app already offers a one-click elevated repair for a stale
//! logon task (PROBLEM 75), and the owner remembered it correctly: *"there
//! would be something on the top of the dashboard saying there is a conflict,
//! there is an old version installed, just press this and a prompt will come up
//! and it will delete the old version."* That banner is real — but it detects a
//! stale TASK, and says nothing about a second INSTALL.
//!
//! A second install is a different fault with the same shape. The `.msi`
//! installed per-machine into `C:\Program Files\Spaceadom`; the `setup.exe`
//! installs per-user into `%LOCALAPPDATA%\Spaceadom`. Windows treats them as
//! two unrelated programs: two uninstall entries, two autostart registrations,
//! and at logon two processes each installing a `WH_KEYBOARD_LL` hook and
//! fighting over the spacebar. Measured on the owner's own machine on
//! 2026-08-17 (PROBLEM 129):
//!
//! ```text
//! HKLM: v1.0.37 -> C:\Program Files\Spaceadom\        (from the .msi)
//! HKCU: v1.0.40 -> %LOCALAPPDATA%\Spaceadom\          (from the setup.exe)
//! ```
//!
//! What the user actually sees is not "two apps are running". It is Space+D
//! opening Discord twice, or settings that keep reverting because two processes
//! write one config.json. Nothing about that points at an installer, which is
//! why this has to be detected and named rather than left to be diagnosed.
//!
//! Dropping the `.msi` in 1.0.41 stopped this happening to NEW installs. It did
//! nothing for machines that already have both — including anyone the owner
//! shared an old build with. That is the gap this closes.
//!
//! PROBLEM 238 (2026-09-04). The check above only ever looks for a live
//! `spaceadom.exe` under `%ProgramFiles%\Spaceadom` — it never reads the
//! registry. Audited on the owner's own machine after the 1.0.95→1.0.96
//! upgrade: ONE exe, ONE HKCU uninstall entry, ONE Run key — but ALSO an
//! orphaned `HKLM\...\Uninstall\{C68DC702-9414-421F-A3E4-12EDBBADD76C5}`
//! (`DisplayName` "Spaceadom", `DisplayVersion` 1.0.94, `InstallLocation` the
//! **per-user** folder, `UninstallString` `MsiExec.exe /X{…}`), with no files
//! anywhere that belong to that old MSI — it is a leftover uninstall
//! registration, not a second running copy. `Program Files\Spaceadom` never
//! existed on this machine, so the check above logged "no second copy found"
//! while Programs and Features plainly showed two "Spaceadom" entries.
//!
//! `repair()` already handles removing an entry like this (it enumerates HKLM
//! Uninstall + WOW6432Node for `DisplayName -like '*Spaceadom*'` and runs
//! `msiexec /X{GUID}`) — this was a missing TRIGGER, not missing removal
//! logic. `detect_msi_uninstall_entry` below is the second detection path:
//! it reads the same two HKLM roots read-only (HKLM is not virtualised for
//! this app's agent shell; HKCU is — see CLAUDE.md PROBLEM 143 — so this
//! module must never depend on HKCU) and classifies what it finds with
//! `classify_msi_entry`, a pure function kept separate so it can be unit
//! tested without a registry. It distinguishes an orphaned entry (nothing
//! running twice) from a genuine second copy at a non-default location
//! (PROBLEM 129's shape again, just not caught by the Program-Files check) —
//! both still set `RIVAL_FOUND` so the existing banner + one-click elevated
//! repair fire, but the banner text should read differently for the two; see
//! `status_kind()`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set at startup when a second install is found. Read by `get_rival_install`.
pub static RIVAL_FOUND: AtomicBool = AtomicBool::new(false);

/// Where the other copy lives, for the banner text. Written once at startup.
static RIVAL_PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static RIVAL_VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
/// Which shape was found — `"second_copy"` or `"orphaned_entry"` — for a
/// caller that wants the PROBLEM 238 banner variant. See `status_kind()`.
static RIVAL_KIND: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// The per-machine install directory the `.msi` used, and the only place a
/// rival can be: the per-user path is where WE live.
#[cfg(windows)]
fn per_machine_exe() -> PathBuf {
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    PathBuf::from(pf).join("Spaceadom").join("spaceadom.exe")
}

/// What an HKLM Spaceadom-named MSI uninstall entry actually means, judged
/// without touching the registry or filesystem — see `classify_msi_entry`'s
/// tests for the four shapes this (or any) machine can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsiEntryVerdict {
    /// Not a Spaceadom MSI entry at all: wrong `DisplayName`, or an
    /// `UninstallString` that is not an MsiExec `/X` (e.g. our own healthy
    /// per-user entry, which also carries `DisplayName` "Spaceadom" but
    /// uninstalls through NSIS's `uninstall.exe`, never `MsiExec`).
    NotOurs,
    /// A Spaceadom MSI entry with no genuinely separate live copy behind it:
    /// either its `InstallLocation` has no `spaceadom.exe` any more, or the
    /// exe there IS the one currently running (this machine's 2026-09-04
    /// case — an old MSI's uninstall registration outlived the MSI itself).
    /// Nothing is running twice; Programs and Features just lists two.
    OrphanedEntry,
    /// A Spaceadom MSI entry whose `InstallLocation` holds a live
    /// `spaceadom.exe` that is a DIFFERENT file from the one running now —
    /// PROBLEM 129's original shape, just reached through the registry
    /// instead of the fixed Program-Files path.
    RealSecondCopy,
}

/// `"MsiExec.exe /X{GUID}"` (or `/I`) is how every MSI uninstall entry looks;
/// an NSIS uninstall string — our healthy per-user entry — points at
/// `uninstall.exe` instead and never matches.
/// Shared with `updater.rs` (PROBLEM 245), which needs the same yardstick
/// to tell an MSI product registration from our NSIS entry.
pub(crate) fn is_msiexec_uninstall(uninstall_string: &str) -> bool {
    let lower = uninstall_string.to_ascii_lowercase();
    lower.contains("msiexec") && lower.contains("/x")
}

/// Pure classifier: no registry or filesystem access, so it is unit-testable
/// on any machine. `exe_exists` / `is_same_file` describe whatever the caller
/// found at `install_location\spaceadom.exe` compared to `current_exe()`.
pub fn classify_msi_entry(
    display_name: &str,
    uninstall_string: &str,
    install_location: &str,
    exe_exists: bool,
    is_same_file: bool,
) -> MsiEntryVerdict {
    if !display_name.trim().eq_ignore_ascii_case("Spaceadom") {
        return MsiEntryVerdict::NotOurs;
    }
    if !is_msiexec_uninstall(uninstall_string) {
        return MsiEntryVerdict::NotOurs;
    }
    // A "real" second copy needs somewhere to actually live; an empty
    // InstallLocation with an exe magically "existing" there cannot happen
    // in practice (the caller refuses to build a path from it), but keep the
    // classifier honest about what it was told rather than trusting a flag
    // that contradicts its own location.
    if exe_exists && !is_same_file && !install_location.trim().is_empty() {
        MsiEntryVerdict::RealSecondCopy
    } else {
        MsiEntryVerdict::OrphanedEntry
    }
}

/// The exe path an HKLM `InstallLocation` implies — the value `detect()` hands
/// back for a `RealSecondCopy`.
///
/// **REVIEW FIXES 2026-09-04, and this one is why the guard below exists.**
/// `repair()` derives the directory it feeds to an elevated
/// `Remove-Item -Recurse -Force` with `Path::parent()`. That call was written
/// against Path 1, which returns an EXE — `…\Spaceadom\spaceadom.exe`, whose
/// parent is `…\Spaceadom`, the install directory. Path 2 (PROBLEM 238) returned
/// the registry's `InstallLocation`, which is the DIRECTORY itself, so
/// `parent()` climbed one level too high and the elevated script was aimed at
/// the install directory's PARENT: `D:\Apps`, `C:\Program Files (x86)`, or the
/// user's entire `AppData\Local`. Joining the exe name back on puts `parent()`
/// where it has always belonged, and makes the two detection paths return the
/// same SHAPE of value instead of two shapes one caller then guesses between.
///
/// A trailing separator is ordinary in the registry (`…\Spaceadom\`) and must
/// not produce a doubled one. `Path::join` already handles that; the test pins
/// it so a later "tidy" to string concatenation cannot quietly regress it.
///
/// Generalise: **when two producers feed one consumer, they must agree on the
/// KIND of thing they produce, not merely on the type.** Both returned `String`
/// and the compiler was satisfied; one meant a file and the other a folder.
#[cfg(windows)]
pub(crate) fn install_location_to_exe(install_location: &str) -> PathBuf {
    std::path::Path::new(install_location.trim()).join("spaceadom.exe")
}

/// Case-insensitive "is `dir` the same as, or an ancestor of, `other`" —
/// compared COMPONENT-wise, so `C:\Apps\Space` is not an ancestor of
/// `C:\Apps\Spaceadom` the way a naive `starts_with` on the strings would say.
#[cfg(windows)]
fn is_same_or_ancestor(dir: &std::path::Path, other: &std::path::Path) -> bool {
    let parts = |p: &std::path::Path| -> Vec<String> {
        p.components()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect()
    };
    let a = parts(dir);
    let b = parts(other);
    !a.is_empty() && a.len() <= b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y)
}

/// **THE HARD GUARD.** May this directory be handed to an elevated
/// `Remove-Item -Recurse -Force`?
///
/// Pure on purpose — no registry, no filesystem — so every refusal below is
/// unit-tested rather than reasoned about. The caller supplies the live
/// `current_exe_dir` and the machine's root folders.
///
///   * `Ok(None)`  — there is nothing on disk to remove. `detect()` returns a
///     descriptive SENTENCE for an `OrphanedEntry` (deliberately: its
///     `InstallLocation` can be our own live folder), and a sentence has no
///     path separator, so `parent()` yields `""`. The script simply omits the
///     delete; the `msiexec /X{GUID}` loop, which matches on `DisplayName` and
///     never on this path, still runs and IS the whole repair for that case.
///   * `Ok(Some(dir))` — a folder named exactly `Spaceadom`, that is not ours
///     and is not a root.
///   * `Err(why)` — anything else. `repair()` logs it and refuses to run at all.
///
/// The name check is the load-bearing one: a drive root has no `file_name()`,
/// and `C:\Program Files` / `…\AppData\Local` are named "Program Files" and
/// "Local". The explicit root list after it is belt-and-braces, and is written
/// out so the refusal SAYS which root it recognised.
#[cfg(windows)]
pub(crate) fn removal_target(
    dir: &str,
    current_exe_dir: &str,
    forbidden_roots: &[&str],
) -> Result<Option<String>, String> {
    let dir = dir.trim();
    if dir.is_empty() {
        return Ok(None);
    }
    let p = std::path::Path::new(dir);

    // 1. It must be named exactly "Spaceadom".
    let named_ours = p
        .file_name()
        .map(|n| n.to_string_lossy().trim().eq_ignore_ascii_case("Spaceadom"))
        .unwrap_or(false);
    if !named_ours {
        return Err(format!(
            "{dir:?} is not a folder named \"Spaceadom\" — an install directory's PARENT, a drive \
             root, Program Files or AppData\\Local all land here, and none of them may be deleted \
             recursively"
        ));
    }

    // 2. It must have a real parent, so it cannot itself be a root.
    if p.parent().map(|q| q.as_os_str().is_empty()).unwrap_or(true) {
        return Err(format!("{dir:?} has no parent directory — refusing to delete a root"));
    }

    // 3. Never ourselves, never anything containing us, and — REVIEW FIXES
    //    2026-09-05 (MEDIUM) — never anything CONTAINED BY us either. An app
    //    must not offer to delete the folder it is running from
    //    (`detect_verbose`'s Path 1 has the same rule for the same reason).
    //
    //    The guard used to test one direction only: is the target an ancestor
    //    of the live directory. That catches the catastrophic case
    //    (`C:\Program Files` as the target) and misses the merely destructive
    //    one: a target INSIDE the live install, such as
    //    `%LOCALAPPDATA%\Spaceadom\Spaceadom` — which a stale registry entry
    //    can name, because an `InstallLocation` is written by whatever wrote
    //    it and is never validated against reality. `Remove-Item -Recurse
    //    -Force` on a subfolder of the running app removes part of the running
    //    app, and this whole function exists because that is not a theoretical
    //    outcome here (PROBLEM 244 deleted the live exe on 2026-09-04).
    //
    //    Generalise: **a containment guard has two directions, and writing one
    //    of them reads as having written both.**
    let me = current_exe_dir.trim();
    if !me.is_empty() {
        let mine = std::path::Path::new(me);
        if is_same_or_ancestor(p, mine) {
            return Err(format!(
                "{dir:?} IS, or contains, the directory this copy of Spaceadom is running from \
                 ({me:?}) — refusing to delete ourselves"
            ));
        }
        if is_same_or_ancestor(mine, p) {
            return Err(format!(
                "{dir:?} is INSIDE the directory this copy of Spaceadom is running from \
                 ({me:?}) — a recursive delete there removes part of the running app; refusing"
            ));
        }
    }

    // 4. Belt-and-braces: never a machine root, whatever it happens to be named.
    for root in forbidden_roots {
        let root = root.trim();
        if root.is_empty() {
            continue;
        }
        if is_same_or_ancestor(p, std::path::Path::new(root)) {
            return Err(format!(
                "{dir:?} is, or contains, the system folder {root:?} — refusing to delete it"
            ));
        }
    }

    Ok(Some(dir.to_string()))
}

/// The machine folders `removal_target` must never accept. Read from the
/// environment so a non-standard `ProgramFiles` or a relocated profile is
/// covered too.
#[cfg(windows)]
fn machine_roots() -> Vec<String> {
    [
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "LOCALAPPDATA",
        "APPDATA",
        "USERPROFILE",
        "SystemDrive",
        "SystemRoot",
    ]
    .iter()
    .filter_map(|v| std::env::var(v).ok())
    .filter(|v| !v.trim().is_empty())
    .collect()
}

// ───────────────────────── PROBLEM 244 (2026-09-04 incident) ─────────────────
//
// WHAT HAPPENED. The PROBLEM 238 banner classified the owner's HKLM entry
// `{C68DC702-9414-421F-A3E4-12EDBBAD76C5}` (DisplayVersion 1.0.94,
// InstallLocation = the LIVE `%LOCALAPPDATA%\Spaceadom`) as `OrphanedEntry`.
// He clicked "Remove the old copy". `repair()` ran `msiexec /X{GUID}`. The
// Restart Manager tried and failed to close the running 1.0.97
// (RestartManager 10010, "SID does not match"), and Windows Installer then
// deleted THAT PRODUCT'S REGISTERED FILES — which were the live
// `%LOCALAPPDATA%\Spaceadom\spaceadom.exe` — and logged MsiInstaller 1034
// "removed the product … status 0". The app vanished off the machine.
//
// WHY THE FILES WERE THERE. `src-tauri/wix/main.wxs` resolves `INSTALLDIR`
// with a `RegistrySearch` on HKCU `Software\{manufacturer}\{product_name}`.
// NSIS writes the per-user install directory into exactly that key. So the
// 1.0.94 `.msi`, double-clicked out of a WhatsApp transfer folder on
// 2026-08-30 15:09 (MsiInstaller 1040/1042 name the file), installed itself
// INTO the live per-user folder and registered those files as ITS components.
// From then on the product's file list pointed at the running app.
//
// THE LAW THIS FILE NOW ENFORCES. **An MSI product's file list may point
// anywhere, including the folder we are running from — so a "leftover entry"
// is removed from the REGISTRY ONLY, never through the Installer.** The
// decision is `plan_removal`, pure and unit-tested, and it is the only thing
// allowed to authorise `msiexec`.

/// What `repair()` is permitted to do about a found entry. There is no third
/// arm on purpose: everything that is not a vetted, genuinely separate
/// installation is registry-only.
#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RemovalPlan {
    /// Delete the uninstall registration and nothing else. No `msiexec`, no
    /// file operations, no `Stop-Process` — the running app is untouched.
    RegistryOnly { reason: String },
    /// A genuinely separate installation at `dir`, which is NOT ours and does
    /// not contain us. Only this arm may reach Windows Installer.
    MsiexecUninstall { dir: String },
}

/// Trim whitespace, surrounding quotes and any trailing separator off a
/// registry path value. `InstallLocation` routinely carries a trailing `\`,
/// and comparing it raw against a `current_exe()` parent would miss.
#[cfg(windows)]
pub(crate) fn normalize_dir(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim()
        .trim_end_matches(['\\', '/'])
        .to_string()
}

/// The DIRECTORY implied by a registry value that names a file — `DisplayIcon`
/// (`C:\…\spaceadom.exe,0`) or a non-MSI `UninstallString`
/// (`"C:\…\uninstall.exe" /S`). Returns `""` for anything that is not a
/// drive-rooted path, which is what `MsiExec.exe /X{GUID}` gives.
#[cfg(windows)]
pub(crate) fn dir_of_path_value(value: &str) -> String {
    let mut t = value.trim().to_string();
    if let Some(rest) = t.strip_prefix('"') {
        t = rest.split('"').next().unwrap_or("").to_string();
    } else if let Some(cut) = t.find('"') {
        t.truncate(cut);
    }
    let t = t.trim();
    // `…\spaceadom.exe,0` — an icon index, not part of the path.
    let t = match t.rsplit_once(',') {
        Some((head, idx)) if !idx.is_empty() && idx.trim().chars().all(|c| c.is_ascii_digit()) => head,
        _ => t,
    };
    let t = t.trim();
    let b = t.as_bytes();
    let drive_rooted =
        b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
    if !drive_rooted {
        return String::new();
    }
    std::path::Path::new(t)
        .parent()
        .map(|p| normalize_dir(&p.to_string_lossy()))
        .unwrap_or_default()
}

/// **THE PROBLEM 244 DECISION.** May Windows Installer be run against this
/// product at all?
///
/// Pure — no registry, no filesystem — so every refusal is a unit test rather
/// than an argument. The caller supplies the three registry fields it read and
/// the directory `current_exe()` actually lives in.
///
/// The rule, in one sentence: **`msiexec /X` is allowed only against a product
/// whose registered location is a DIFFERENT directory from ours and is not an
/// ancestor of ours; everything else is a registry-only cleanup.** Comparison
/// is component-wise and case-insensitive (`is_same_or_ancestor`), so
/// `…\Local\Spaceadom\`, `…\local\spaceadom` and `…\AppData\Local` are all
/// caught, and a sibling `SpaceadomBeta` is correctly NOT us.
#[cfg(windows)]
pub(crate) fn plan_removal(
    verdict: MsiEntryVerdict,
    install_location: &str,
    display_icon: &str,
    uninstall_string: &str,
    current_exe_dir: &str,
) -> RemovalPlan {
    if verdict != MsiEntryVerdict::RealSecondCopy {
        return RemovalPlan::RegistryOnly {
            reason: "a leftover uninstall registration with no separate copy behind it"
                .to_string(),
        };
    }

    let me = normalize_dir(current_exe_dir);
    let loc = normalize_dir(install_location);

    // Every directory this product claims. Any one of them landing on us is
    // enough to refuse: an MSI removes its COMPONENTS, and a component can sit
    // in any of them.
    let candidates: [(&str, String); 3] = [
        ("InstallLocation", loc.clone()),
        ("DisplayIcon", dir_of_path_value(display_icon)),
        ("UninstallString", dir_of_path_value(uninstall_string)),
    ];

    if loc.is_empty() {
        return RemovalPlan::RegistryOnly {
            reason: "the entry records no InstallLocation, so there is no vetted directory \
                     msiexec could safely be aimed at"
                .to_string(),
        };
    }

    if !me.is_empty() {
        for (field, dir) in candidates.iter() {
            if dir.is_empty() {
                continue;
            }
            if is_same_or_ancestor(std::path::Path::new(dir), std::path::Path::new(&me)) {
                return RemovalPlan::RegistryOnly {
                    reason: format!(
                        "rival install: REFUSING msiexec /X for this product (PROBLEM 244) - its \
                         registered install location is, or contains, the folder this copy is \
                         running from. Field {field} says {dir:?}; we are running from {me:?}. \
                         An MSI uninstall deletes the product REGISTERED FILES, and on \
                         2026-09-04 that list was the live app. Removing the registry \
                         registration only."
                    ),
                };
            }
        }
    }

    RemovalPlan::MsiexecUninstall { dir: loc }
}

/// The Windows Installer "packed" (a.k.a. squished/compressed) form of a
/// ProductCode — how `HKLM\SOFTWARE\Classes\Installer\Products` names its
/// subkeys. Groups 1-3 are reversed whole; groups 4 and 5 have each BYTE PAIR
/// swapped.
///
/// Pinned by the canonical example:
/// `{01234567-89AB-CDEF-0123-456789ABCDEF}` → `76543210BA98FEDC1032547698BADCFE`.
/// The incident's own GUID `{C68DC702-9414-421F-A3E4-12EDBBAD76C5}` packs to
/// `207CD86C4149F1243A4E21DEBBDA675C`.
#[cfg(windows)]
pub(crate) fn packed_guid(guid: &str) -> Option<String> {
    let g = guid.trim().trim_start_matches('{').trim_end_matches('}');
    let parts: Vec<&str> = g.split('-').collect();
    if parts.len() != 5 {
        return None;
    }
    for (p, want) in parts.iter().zip([8usize, 4, 4, 4, 12]) {
        if p.len() != want || !p.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
    }
    let up: Vec<String> = parts.iter().map(|p| p.to_ascii_uppercase()).collect();
    Some(format!(
        "{}{}{}{}{}",
        rev_hex(&up[0]),
        rev_hex(&up[1]),
        rev_hex(&up[2]),
        swap_hex_pairs(&up[3]),
        swap_hex_pairs(&up[4])
    ))
}

/// The inverse of `packed_guid`, kept so the round trip can be tested — a
/// conversion that is only ever run forwards is a conversion nobody can check.
#[cfg(windows)]
pub(crate) fn unpack_guid(packed: &str) -> Option<String> {
    let p = packed.trim();
    if p.len() != 32 || !p.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let u = p.to_ascii_uppercase();
    Some(format!(
        "{{{}-{}-{}-{}-{}}}",
        rev_hex(&u[0..8]),
        rev_hex(&u[8..12]),
        rev_hex(&u[12..16]),
        swap_hex_pairs(&u[16..20]),
        swap_hex_pairs(&u[20..32])
    ))
}

#[cfg(windows)]
fn rev_hex(s: &str) -> String {
    s.chars().rev().collect()
}

/// Swap the two characters of every pair. Its own inverse, which is why one
/// helper serves both directions.
#[cfg(windows)]
fn swap_hex_pairs(s: &str) -> String {
    s.as_bytes()
        .chunks(2)
        .map(|c| {
            if c.len() == 2 {
                format!("{}{}", c[1] as char, c[0] as char)
            } else {
                (c[0] as char).to_string()
            }
        })
        .collect()
}

/// True if `candidate` and `current` are the same file. Prefers a real
/// filesystem identity check (`canonicalize`); falls back to a
/// case-insensitive path comparison if either side cannot be canonicalized
/// (e.g. `candidate` does not exist — callers only reach this when it does,
/// but a TOCTOU race is not worth a panic over).
#[cfg(windows)]
fn is_same_exe(candidate: &std::path::Path, current: &std::path::Path) -> bool {
    match (candidate.canonicalize(), current.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => candidate
            .to_string_lossy()
            .eq_ignore_ascii_case(&current.to_string_lossy()),
    }
}

/// Read `DisplayVersion` off an HKLM Uninstall entry. Best-effort, same
/// reasoning as `file_version`: a missing version must not suppress the find.
#[cfg(windows)]
fn display_version(entry: &winreg::RegKey) -> Option<String> {
    entry.get_value::<String, _>("DisplayVersion").ok()
}

/// Everything a detection produced, in one value. PROBLEM 244 added the last
/// four fields: `repair()` used to re-derive its target from `path` alone and
/// then enumerate HKLM by `DisplayName`, which is how a removal aimed at ONE
/// entry reached every Spaceadom-named product on the machine — including the
/// one whose files were the live app.
#[derive(Debug, Clone)]
pub(crate) struct Finding {
    /// The exe path (`RealSecondCopy`) or the descriptive sentence
    /// (`OrphanedEntry`). Unchanged shape — `status()`/the banner read this.
    pub path: String,
    pub version: String,
    /// `"second_copy"` or `"orphaned_entry"`.
    pub kind: &'static str,
    /// The ProductCode, `{…}` form, or `""` for the Program-Files path which
    /// found a file rather than a registration.
    pub guid: String,
    pub install_location: String,
    pub display_icon: String,
    pub uninstall_string: String,
}

/// Path 2 (PROBLEM 238): enumerate HKLM's Uninstall keys (native +
/// WOW6432Node) for a Spaceadom MSI entry the Program-Files check cannot
/// see — an orphaned leftover, or a genuine second copy at a non-default
/// location. Read-only, no elevation, HKLM only (never HKCU — HKCU is
/// virtualised for this app's agent shell; HKLM is not, but that guarantee
/// is about THIS shell, not a license to add an HKCU dependence here).
#[cfg(windows)]
fn detect_msi_uninstall_entry(me: &std::path::Path) -> Option<Finding> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    const ROOTS: [&str; 2] = [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ];

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for root in ROOTS {
        let Ok(uninstall) = hklm.open_subkey_with_flags(root, KEY_READ) else {
            continue;
        };
        for guid in uninstall.enum_keys().filter_map(Result::ok) {
            let Ok(entry) = uninstall.open_subkey_with_flags(&guid, KEY_READ) else {
                continue;
            };
            let display_name: String = entry.get_value("DisplayName").unwrap_or_default();
            let uninstall_string: String = entry.get_value("UninstallString").unwrap_or_default();
            let install_location: String = entry.get_value("InstallLocation").unwrap_or_default();
            let display_icon: String = entry.get_value("DisplayIcon").unwrap_or_default();
            let location = install_location.trim();

            let (exe_exists, is_same_file) = if location.is_empty() {
                (false, false)
            } else {
                let exe_path = std::path::Path::new(location).join("spaceadom.exe");
                let exists = exe_path.is_file();
                (exists, exists && is_same_exe(&exe_path, me))
            };

            let verdict = classify_msi_entry(
                &display_name,
                &uninstall_string,
                location,
                exe_exists,
                is_same_file,
            );
            if verdict == MsiEntryVerdict::NotOurs {
                continue;
            }

            // Identify the finding without printing InstallLocation's
            // username path a second time — `scan()`'s existing warn! line
            // already prints the path this returns, once.
            log::info!(
                "rival install: HKLM Uninstall\\{guid} names \"{display_name}\" via an MsiExec \
                 /X uninstall string, classified {verdict:?} (PROBLEM 238)"
            );

            let version = display_version(&entry).unwrap_or_else(|| "unknown version".to_string());
            let kind = if verdict == MsiEntryVerdict::RealSecondCopy {
                "second_copy"
            } else {
                "orphaned_entry"
            };
            let path = if verdict == MsiEntryVerdict::RealSecondCopy {
                // REVIEW FIXES 2026-09-04 — the EXE, never the directory. See
                // `install_location_to_exe`: `repair()` takes `parent()` of
                // whatever this returns, so returning the directory here aimed
                // an elevated recursive delete at the directory ABOVE the
                // install.
                install_location_to_exe(&install_location)
                    .to_string_lossy()
                    .to_string()
            } else {
                "a leftover installer entry — no separate copy of Spaceadom is actually running"
                    .to_string()
            };
            return Some(Finding {
                path,
                version,
                kind,
                guid,
                install_location,
                display_icon,
                uninstall_string,
            });
        }
    }
    None
}

#[cfg(not(windows))]
fn detect_msi_uninstall_entry(_me: &std::path::Path) -> Option<Finding> {
    None
}

// ──────────── PROBLEM 250 follow-up — LIVE TEST 2026-09-05, FINDING C ────────
//
// The two detection paths above share one blind spot, and the first real MSIX
// install on this machine walked straight into it. On 2026-09-05 a packaged
// copy ran with the owner's per-user NSIS copy still fully present
// (`%LOCALAPPDATA%\Spaceadom\spaceadom.exe`, v1.0.100, its HKCU Run value
// intact) and logged *"rival install: no second copy found — this machine has
// one Spaceadom"*. Both would have installed a `WH_KEYBOARD_LL` hook at the
// next logon. The `packaged_host` banner variant PROBLEM 250 built — the one
// that says "open Installed apps and remove the older one" — could therefore
// never fire in the single most likely Store scenario: **a Store install
// arriving beside the setup.exe the user already had.**
//
// Why neither path saw it. Path 1 looks only at `%ProgramFiles%\Spaceadom`,
// because when this module was written the per-user directory was where WE
// live and so could not be a rival. That stopped being true the moment a
// second KIND of install existed. Path 2 reads HKLM only, and a per-user NSIS
// install registers under HKCU.
//
// **Why the file check, and not the HKCU Uninstall entry.** An NSIS per-user
// install does write `HKCU\…\Uninstall\Spaceadom`, and reading it would give a
// `DisplayVersion` for free. It is not the trigger here, for two reasons that
// both have to hold:
//
//   1. This module's own law (see the module doc, PROBLEM 238) is that it
//      never depends on HKCU, because the agent shell this project is
//      developed in virtualises HKCU — a negative read there is not evidence.
//   2. Under an MSIX package HKCU is virtualised too, and it is the ONE half
//      of the split that Microsoft documents without an escape hatch: "All
//      writes under HKCU are copied on write to a private per-user, per-app
//      location" (Understanding how packaged desktop apps run on Windows,
//      learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-behind-the-scenes).
//      The 2026-09-05 test measured the write half of exactly that: the
//      packaged copy's `NotifyIconSettings` writes landed in
//      `…\Packages\<PFN>\SystemAppData\Helium\User.dat` and never reached the
//      real hive (log: *"still BLANK on the WindowsApps entry"*). Copy-on-write
//      means a READ that misses the private hive does fall through to the real
//      one — but that is an inference about a mechanism, and `spaceadom.exe`
//      either is on disk or is not.
//
// `file_version()` already reads the version straight off the rival exe, so
// the HKCU entry buys nothing that would justify depending on it.
//
// **The reverse direction** — an UNPACKAGED copy noticing that a Store copy is
// also installed — is the same fault seen from the other side, and it is
// strictly read-only: `PackageManager` for the current user needs no elevation
// and no capability. There is no repair for it, ever. A Store package is
// removed from Settings ▸ Apps (or by uninstalling this copy instead); an
// unpackaged process running `msiexec` or `Remove-Item` at a package is
// meaningless, and `Program Files\WindowsApps` is ACL'd against exactly that.

/// Which cross-kind pairing exists on this machine — the decision, with no
/// registry, filesystem or WinRT access, so both directions are unit tests
/// rather than a branch that runs once on somebody else's PC (PROBLEM 118).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossKindVerdict {
    /// Nothing found in either direction.
    None,
    /// WE are the packaged (Store) copy, and an unpackaged per-user install
    /// is sitting beside us. Banner: `packaged_host` — directions, no button.
    PerUserBesidePackaged,
    /// WE are the unpackaged copy, and a Microsoft Store copy is registered
    /// for this user. Banner: `store_copy` — directions, and NEVER a repair.
    StoreBesideUnpackaged,
}

/// Pure. `per_user_exe_is_us` exists so the classifier cannot be talked into
/// reporting the caller as its own rival: a packaged process runs from
/// `…\WindowsApps\…` and can never BE `%LOCALAPPDATA%\Spaceadom\spaceadom.exe`,
/// but the same rule written down is what makes a future portable or relocated
/// copy safe here too — and `detect_full`'s Path 1 has carried that same guard
/// since PROBLEM 129 for the same reason.
///
/// The `we_are_packaged` arm is exclusive on purpose: a packaged process must
/// never consult the appx registration list looking for "a Store copy",
/// because it would find ITSELF and warn the user about the app they are
/// looking at.
pub fn classify_cross_kind(
    we_are_packaged: bool,
    per_user_exe_exists: bool,
    per_user_exe_is_us: bool,
    store_package_registered: bool,
) -> CrossKindVerdict {
    if we_are_packaged {
        if per_user_exe_exists && !per_user_exe_is_us {
            CrossKindVerdict::PerUserBesidePackaged
        } else {
            CrossKindVerdict::None
        }
    } else if store_package_registered {
        CrossKindVerdict::StoreBesideUnpackaged
    } else {
        CrossKindVerdict::None
    }
}

/// `%LOCALAPPDATA%\Spaceadom\spaceadom.exe` — where `setup.exe` (NSIS,
/// `installMode = "currentUser"`) has put every install since 1.0.41.
///
/// `None` when `LOCALAPPDATA` is unset, rather than a relative fallback: a bare
/// `Spaceadom\spaceadom.exe` would be resolved against the process's current
/// directory, and "is there a file at a relative path" is not the question
/// being asked.
#[cfg(windows)]
fn per_user_exe() -> Option<PathBuf> {
    let root = std::env::var("LOCALAPPDATA").ok()?;
    if root.trim().is_empty() {
        return None;
    }
    Some(PathBuf::from(root).join("Spaceadom").join("spaceadom.exe"))
}

/// The first MSIX package registered for the CURRENT USER whose package
/// identity Name contains "Spaceadom" — the `Get-AppxPackage *Spaceadom*` the
/// 2026-09-05 test log ran by hand, done from inside the app. Returns
/// `(package full name, version)`, e.g.
/// `("LOCALTEST.Spaceadom_1.0.100.0_x64__nj4cr7rfsqc4c", "1.0.100")`.
///
/// **Why enumerate rather than `FindPackagesByPackageFamily`.** That API wants
/// a package FAMILY name, which is `<Identity/Name>_<publisher hash>` — and
/// `Identity/Name` comes from `src-tauri/msix/identity.json`, which is
/// gitignored and is compiled into this binary in no form at all. There is
/// nothing to hand it. `FindPackagesByUserSecurityId("")` means "the calling
/// user", needs no elevation and no `packageQuery` capability (that is
/// `FindPackages()`, the all-users form, which does).
///
/// Called only from the unpackaged branch of `detect_cross_kind`, which itself
/// runs on `scan()`'s background thread — enumerating a few hundred packages is
/// not on any critical path. Every failure is a `debug!` and a `None`: not
/// being able to ask is not evidence of an answer, and this check exists to add
/// a banner, never to suppress one.
#[cfg(windows)]
fn find_store_package() -> Option<(String, String)> {
    use windows::core::HSTRING;
    use windows::Management::Deployment::PackageManager;

    let pm = match PackageManager::new() {
        Ok(pm) => pm,
        Err(e) => {
            log::debug!(
                "rival install: PackageManager unavailable ({e}) — skipping the Store-copy check"
            );
            return None;
        }
    };
    let packages = match pm.FindPackagesByUserSecurityId(&HSTRING::new()) {
        Ok(p) => p,
        Err(e) => {
            log::debug!(
                "rival install: could not enumerate this user's packages ({e}) — skipping the \
                 Store-copy check"
            );
            return None;
        }
    };
    for package in packages {
        let Ok(id) = package.Id() else { continue };
        let Ok(name) = id.Name() else { continue };
        if !name.to_string().to_ascii_lowercase().contains("spaceadom") {
            continue;
        }
        let full = id
            .FullName()
            .map(|h| h.to_string())
            .unwrap_or_else(|_| name.to_string());
        let version = id
            .Version()
            .map(|v| format!("{}.{}.{}", v.Major, v.Minor, v.Build))
            .unwrap_or_else(|_| "unknown version".to_string());
        return Some((full, version));
    }
    None
}

#[cfg(not(windows))]
fn find_store_package() -> Option<(String, String)> {
    None
}

/// The impure half of the two new paths: ask the machine the questions
/// `classify_cross_kind` decides on, then build the `Finding`.
#[cfg(windows)]
fn detect_cross_kind(me: &std::path::Path) -> Option<Finding> {
    let packaged = crate::packaged::is_packaged();

    // Only ask the filesystem question in the direction that can use it…
    let (exists, is_us, exe) = if packaged {
        match per_user_exe() {
            Some(exe) => {
                let exists = exe.is_file();
                (exists, exists && is_same_exe(&exe, me), Some(exe))
            }
            None => (false, false, None),
        }
    } else {
        (false, false, None)
    };
    // …and the WinRT question only in the other one.
    let store = if packaged { None } else { find_store_package() };

    match classify_cross_kind(packaged, exists, is_us, store.is_some()) {
        CrossKindVerdict::None => None,
        CrossKindVerdict::PerUserBesidePackaged => {
            let exe = exe?;
            let version = file_version(&exe).unwrap_or_else(|| "unknown version".to_string());
            log::info!(
                "rival install: this PACKAGED copy found an unpackaged per-user install beside \
                 it (PROBLEM 250 follow-up — LIVE TEST 2026-09-05, FINDING C). The banner shows \
                 directions, never a button: a packaged app must not elevate to remove a \
                 product outside its own package."
            );
            Some(Finding {
                path: exe.to_string_lossy().to_string(),
                version,
                // The `packaged_host` banner variant is chosen by
                // `status_kind()` from `is_packaged()`, not from this string;
                // "second_copy" is the honest shape — two live installs.
                kind: "second_copy",
                // A file, not a registration: no ProductCode, and `repair()`
                // refuses outright while packaged in any case.
                guid: String::new(),
                install_location: exe
                    .parent()
                    .map(|d| d.to_string_lossy().to_string())
                    .unwrap_or_default(),
                display_icon: String::new(),
                uninstall_string: String::new(),
            })
        }
        CrossKindVerdict::StoreBesideUnpackaged => {
            let (full, version) = store?;
            log::warn!(
                "rival install: a Microsoft Store (MSIX) copy of Spaceadom is registered for \
                 this user ({full}) while THIS copy is unpackaged. Both start with Windows and \
                 both install a WH_KEYBOARD_LL hook, so one has to go — but there is no repair \
                 for this shape and the banner offers none: a Store package is removed from \
                 Settings > Apps, and Program Files\\WindowsApps is ACL'd against anything else."
            );
            Some(Finding {
                // A SENTENCE, deliberately — the same device PROBLEM 238 uses
                // for an orphaned entry. `repair()` derives its delete target
                // with `Path::parent()` of this value, and a string with no
                // path separator yields `""`, which `removal_target` answers
                // `Ok(None)` for. The explicit refusal in `repair()` is the
                // real guard; this is the second one.
                path: format!("a Microsoft Store copy is also installed ({full}) — keep one"),
                version,
                kind: "store_copy",
                guid: String::new(),
                install_location: String::new(),
                display_icon: String::new(),
                uninstall_string: String::new(),
            })
        }
    }
}

#[cfg(not(windows))]
fn detect_cross_kind(_me: &std::path::Path) -> Option<Finding> {
    None
}

/// Everything `detect()` returns, plus which shape it is. `detect()` keeps
/// the narrower `(path, version)` shape `repair()` and `status()` were
/// already written against; `scan()` calls this directly so it can also
/// record the kind for `status_kind()`.
#[cfg(windows)]
pub(crate) fn detect_full() -> Option<Finding> {
    let me = std::env::current_exe().ok()?;

    // Path 1 (PROBLEM 129): a genuinely separate per-machine copy, found by
    // its known install directory. If we ARE that copy there is nothing to
    // warn about at all — this module only ever offers to remove the
    // per-machine one, and an app must never offer to delete itself.
    let pm = per_machine_exe();
    if let Some(pm_dir) = pm.parent() {
        if me.starts_with(pm_dir) {
            return None;
        }
    }
    if pm.exists() {
        let version = file_version(&pm).unwrap_or_else(|| "unknown version".to_string());
        return Some(Finding {
            path: pm.to_string_lossy().to_string(),
            version,
            kind: "second_copy",
            // Path 1 found a FILE, not a registration; there is no ProductCode
            // to delete and `plan_removal` is fed the directory it did find.
            guid: String::new(),
            install_location: pm
                .parent()
                .map(|d| d.to_string_lossy().to_string())
                .unwrap_or_default(),
            display_icon: String::new(),
            uninstall_string: String::new(),
        });
    }

    // Path 3/4 (PROBLEM 250 follow-up, LIVE TEST 2026-09-05): the two
    // cross-KIND pairings — a packaged copy beside a per-user NSIS one, or an
    // unpackaged copy beside a Store package.
    //
    // **Ordered before Path 2 on purpose.** Paths 1, 3 and 4 all find a
    // SECOND LIVE INSTALL; Path 2 usually finds a leftover REGISTRATION
    // (PROBLEM 238's shape — "nothing is running twice"). Only one finding is
    // ever reported, so when both are true the banner must name the one that
    // will actually fight over the spacebar at the next logon. On a machine
    // with no cross-kind pairing this returns `None` in a few hundred
    // microseconds (one `LOCALAPPDATA` join + one `is_file`) or, unpackaged,
    // one read-only package enumeration on `scan()`'s background thread — and
    // Path 2 then runs exactly as it did before.
    if let Some(found) = detect_cross_kind(&me) {
        return Some(found);
    }

    // Path 2 (PROBLEM 238): nothing on disk at the fixed Program-Files path,
    // but HKLM may still carry a Spaceadom MsiExec uninstall entry — orphaned
    // or a real second copy at a non-default location.
    detect_msi_uninstall_entry(&me)
}

#[cfg(not(windows))]
pub(crate) fn detect_full() -> Option<Finding> {
    None
}

fn detect_verbose() -> Option<(String, String, &'static str)> {
    detect_full().map(|f| (f.path, f.version, f.kind))
}

/// A description of the OTHER install, or None.
pub fn detect() -> Option<(String, String)> {
    detect_verbose().map(|(path, version, _kind)| (path, version))
}

/// Read a PE file's version resource. Best-effort: the banner is still useful
/// without a version number, so a failure here must not suppress the warning.
#[cfg(windows)]
fn file_version(path: &std::path::Path) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
    };
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    use std::os::windows::ffi::OsStrExt;
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        // `dwHandle` is a documented-ignored u32 here, not an Option.
        GetFileVersionInfoW(PCWSTR(wide.as_ptr()), 0, size, buf.as_mut_ptr() as *mut _).ok()?;
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        let sub: Vec<u16> = "\\".encode_utf16().chain(std::iter::once(0)).collect();
        if !VerQueryValueW(
            buf.as_ptr() as *const _,
            PCWSTR(sub.as_ptr()),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            || ptr.is_null()
        {
            return None;
        }
        let info = &*(ptr as *const VS_FIXEDFILEINFO);
        Some(format!(
            "{}.{}.{}",
            (info.dwFileVersionMS >> 16) & 0xffff,
            info.dwFileVersionMS & 0xffff,
            (info.dwFileVersionLS >> 16) & 0xffff,
        ))
    }
}

/// Run at startup, off the critical path. Records what was found so the
/// dashboard can offer the repair.
pub fn scan() {
    if let Some((path, version, kind)) = detect_verbose() {
        RIVAL_FOUND.store(true, Ordering::Relaxed);
        let _ = RIVAL_PATH.set(path.clone());
        let _ = RIVAL_VERSION.set(version.clone());
        let _ = RIVAL_KIND.set(kind);
        if kind == "store_copy" {
            // `detect_cross_kind` has already logged the detail (the package
            // full name and why there is no button). Say the consequence here,
            // in the same place and the same shape as the other two, so a log
            // read from the top still finds one "rival install:" verdict line.
            log::warn!(
                "rival install: a MICROSOFT STORE copy of Spaceadom (v{version}) is installed \
                 for this user as well as this unpackaged one — {path}. Both start with Windows \
                 and both install a keyboard hook, so one has to go (PROBLEM 129/141/236). The \
                 dashboard shows directions; there is no one-click removal for a package."
            );
        } else if kind == "orphaned_entry" {
            log::warn!(
                "rival install: an OLD INSTALLER ENTRY is left over for Spaceadom (v{version}) — \
                 {path}. Nothing is running twice, but Programs and Features lists two \
                 (PROBLEM 238). The dashboard is offering a one-click elevated removal."
            );
        } else {
            log::warn!(
                "rival install: a SECOND copy of Spaceadom is installed at {path} (v{version}). \
                 Both register autostart, so at the next logon two processes will each install a \
                 keyboard hook and fight over the spacebar (PROBLEM 129/141/236). The dashboard is \
                 offering a one-click elevated removal."
            );
        }
    } else {
        log::info!("rival install: no second copy found — this machine has one Spaceadom");
    }
}

/// `(found, path, version)` for the banner.
pub fn status() -> (bool, String, String) {
    (
        RIVAL_FOUND.load(Ordering::Relaxed),
        RIVAL_PATH.get().cloned().unwrap_or_default(),
        RIVAL_VERSION.get().cloned().unwrap_or_default(),
    )
}

/// `"second_copy"`, `"orphaned_entry"`, `"packaged_rival"`, or `""` if nothing
/// was found — for a caller that wants the PROBLEM 238 banner variant ("an old
/// installer entry is left over — nothing is running twice, but Programs and
/// Features lists two") instead of the PROBLEM 129/141 "second copy" text.
///
/// **REVIEW FIXES 2026-09-05 (MEDIUM) — this comment used to end "Not yet
/// wired to a Tauri command or the dashboard: see this module's doc comment /
/// the handoff notes for the exact frontend change needed." That has been
/// false since PROBLEM 238 shipped.** It is wired in three places, verified by
/// grep: `commands.rs:1108` returns it as the fourth element of the rival
/// status tuple, `src/main.ts:1289` switches the banner text on it, and
/// `diagnostics.rs` puts it in `system.txt`.
///
/// A stale "not wired yet" is worse than no note at all — it is the sentence
/// that sends the next reader off to build a thing that already exists, and
/// this project has already paid for that once (CLAUDE.md: "document the
/// CONDITION, not just the failure"; a solved problem was re-solved because a
/// note said it did not work).
pub fn status_kind() -> &'static str {
    let kind = RIVAL_KIND.get().copied().unwrap_or("");
    // PROBLEM 250 — a THIRD banner variant, and it exists because the fix this
    // banner offers is unavailable to a Store install.
    //
    // A packaged copy sitting beside an NSIS or MSI copy is a REAL second copy
    // in the sense that matters here: both start at logon and both install a
    // WH_KEYBOARD_LL hook, so they fight over the spacebar exactly as two
    // unpackaged copies would. What is different is the REMEDY. `repair()`
    // works by `ShellExecuteExW("runas", …)` — an elevated `msiexec` or an
    // elevated `Remove-Item` — and a process inside an MSIX package must not be
    // in the business of elevating itself to delete files outside its package.
    // It is also pointless: an app cannot uninstall an unrelated product on the
    // user's behalf without exactly the kind of aimed-by-the-thing-being-removed
    // step that PROBLEM 244 was.
    //
    // So when WE are the packaged copy the banner stops offering a button and
    // gives directions instead. `repair()` refuses on the same condition, so
    // the two cannot drift: even a stale frontend that still renders the old
    // button gets a logged refusal rather than an elevated action.
    if !kind.is_empty() && crate::packaged::is_packaged() {
        return "packaged_host";
    }
    // PROBLEM 250 follow-up (LIVE TEST 2026-09-05) — a FOURTH variant, the
    // mirror image of `packaged_host`: WE are unpackaged and a Microsoft Store
    // copy is registered for this user. `is_packaged()` is false here, so the
    // branch above cannot cover it, and the remedy is different again — a
    // Store package is not in Programs and Features and has no uninstaller we
    // could name. It never reaches `kind` from anywhere but
    // `detect_cross_kind`, and `repair()` refuses it independently.
    kind
}

/// Remove the per-machine copy with ONE elevated step, the same shape as
/// PROBLEM 75's `repair_stale_task`: a single `runas` ShellExecute the user
/// consents to once, then verify by looking at the disk rather than trusting an
/// exit code (PROBLEM 127's lesson — an installer's exit code is a claim about
/// the installer, not about the machine).
///
/// **PROBLEM 244, 2026-09-04, and this is the reason `msiexec` is no longer
/// reachable from an orphaned entry at all.** On the owner's own machine this
/// function ran `msiexec /X{C68DC702-…}` against an entry PROBLEM 238 had
/// correctly classified `OrphanedEntry` — and Windows Installer deleted THAT
/// PRODUCT'S REGISTERED FILES, which were `%LOCALAPPDATA%\Spaceadom\*`, the
/// live app. It logged 1034 "removed the product … status 0" and the app was
/// gone. An MSI's file list is whatever the package recorded at install time;
/// `wix/main.wxs` resolves `INSTALLDIR` from an HKCU RegistrySearch that NSIS
/// populates with the per-user folder, so a double-clicked `.msi` records the
/// live install as its own. **A leftover entry is therefore removed from the
/// REGISTRY ONLY — never through the Installer.** `plan_removal` is the single
/// place that may authorise `msiexec`, it is pure, and every refusal is a test.
///
/// PROBLEM 238: for an `OrphanedEntry`, `detect()`'s `path` is a descriptive
/// sentence, not `InstallLocation` — deliberately, because `InstallLocation`
/// for that entry can be OUR OWN live per-user folder (that is exactly what
/// makes it orphaned rather than a real second copy). Using it here for `dir`
/// would feed `Remove-Item '{dir}' -Recurse -Force` the currently-running
/// app's own directory. The sentence has no path separator, so `Path::parent`
/// yields an empty `dir`, `removal_target` reports `Ok(None)`, the
/// `Stop-Process`/`Remove-Item` steps below are OMITTED, and the HKLM
/// `msiexec /X{GUID}` loop — which matches by `DisplayName`, not by `dir` —
/// still removes the orphaned entry correctly. Do not change `detect()` to
/// return the raw `InstallLocation` for that case without re-deriving `dir`
/// some other way first.
///
/// REVIEW FIXES 2026-09-04, and this is the reason `repair()` is no longer
/// allowed to trust its own arithmetic: for a `RealSecondCopy`, `detect()` used
/// to return `InstallLocation` — a DIRECTORY — while this function has always
/// taken `parent()` of it, which is correct only for an EXE. `detect()` now
/// returns `…\Spaceadom\spaceadom.exe` (`install_location_to_exe`), and
/// `removal_target` refuses outright anything that is not a folder named
/// exactly "Spaceadom", is a root, or is/contains our own directory.
#[cfg(windows)]
pub fn repair() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::System::Threading::{WaitForSingleObject, INFINITE};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    // PROBLEM 250 — a Store install may not do this, and the refusal lives HERE
    // rather than only in the frontend on purpose. `status_kind()` already
    // returns "packaged_host" so the banner renders directions instead of a
    // button, but a banner is a rendering and this is the thing that elevates:
    // the two must not be able to drift, and the one that must never be wrong
    // is this one. An MSIX package elevating itself to delete a directory
    // outside the package is both a Store-policy problem and the exact shape
    // PROBLEM 244 turned into a deleted app.
    if crate::packaged::is_packaged() {
        log::warn!(
            "rival install: REFUSING the one-click removal — this copy of Spaceadom is \
             running from an MSIX package ({}), and a packaged app must not elevate \
             itself to uninstall a product outside its own package. The other copy has \
             to be removed by the user from Settings > Apps > Installed apps. The banner \
             says so; if a button reached this function anyway, that banner is stale.",
            crate::packaged::package_full_name().unwrap_or("unknown package")
        );
        return false;
    }

    let Some(found) = detect_full() else {
        RIVAL_FOUND.store(false, Ordering::Relaxed);
        return true; // already gone
    };

    // PROBLEM 250 follow-up (LIVE TEST 2026-09-05) — the OTHER direction, and
    // it refuses for a different reason than the packaged check above. Here we
    // are the ordinary unpackaged copy and the rival is an MSIX package. There
    // is no ProductCode, no `uninstall.exe`, and no directory we may touch:
    // `Program Files\WindowsApps` is ACL'd against this user by design. The
    // only correct removal is `Remove-AppxPackage` / Settings > Apps, run by
    // the person, and a `runas` that ends in "access denied" is worse than no
    // button at all — it teaches the user to accept a UAC prompt from this app
    // for an action that cannot work.
    if found.kind == "store_copy" {
        log::warn!(
            "rival install: REFUSING the one-click removal — the other copy is a Microsoft \
             Store (MSIX) package ({}). A package is removed from Settings > Apps > Installed \
             apps, or by uninstalling THIS copy instead; nothing this function can elevate to \
             is capable of removing it. The banner gives directions and no button; if a button \
             reached this function anyway, that banner is stale.",
            found.path
        );
        return false;
    }

    let path = found.path.clone();

    let me_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_string_lossy().to_string()))
        .unwrap_or_default();

    // PROBLEM 244 — the decision, before anything elevated is even composed.
    let verdict = if found.kind == "second_copy" {
        MsiEntryVerdict::RealSecondCopy
    } else {
        MsiEntryVerdict::OrphanedEntry
    };
    let plan = plan_removal(
        verdict,
        &found.install_location,
        &found.display_icon,
        &found.uninstall_string,
        &me_dir,
    );

    // PowerShell single-quoted strings escape a quote by doubling it. A profile
    // path like `C:\Users\O'Brien\…` is legal on Windows and would otherwise
    // close the string early.
    let ps_quote = |s: &str| s.replace('\'', "''");

    let script = match &plan {
        // ── REGISTRY ONLY ────────────────────────────────────────────────────
        // The whole PROBLEM 244 fix. Delete the uninstall registration and
        // NOTHING else: no msiexec (so no Restart Manager, so the running app
        // is never asked to close), no Stop-Process, no Remove-Item on any
        // directory. `Installer\Products\<packed>` is what keeps the entry
        // visible to some tools after the ARP key is gone; it is removed too
        // when the ProductCode packs cleanly, and skipped when it does not.
        RemovalPlan::RegistryOnly { reason } => {
            if found.guid.trim().is_empty() {
                log::error!(
                    "rival install: nothing to do — the finding is registry-only ({reason}) but \
                     carries no ProductCode, so there is no key to delete. Refusing to guess."
                );
                return false;
            }
            let guid = ps_quote(found.guid.trim());
            let products_step = match packed_guid(&found.guid) {
                Some(packed) => format!(
                    "Remove-Item 'HKLM:\\SOFTWARE\\Classes\\Installer\\Products\\{}' -Recurse \
                     -Force -EA SilentlyContinue; ",
                    ps_quote(&packed)
                ),
                None => String::new(),
            };
            log::info!(
                "rival install: REGISTRY-ONLY removal (PROBLEM 244) - deleting the leftover \
                 uninstall registration and NOTHING else: no msiexec, no files touched, the \
                 running app is left alone. ProductCode {}, packed {:?}. Reason: {reason}",
                found.guid,
                packed_guid(&found.guid).unwrap_or_default()
            );
            format!(
                "Remove-Item 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{guid}' \
                   -Recurse -Force -EA SilentlyContinue; \
                 Remove-Item 'HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{guid}' \
                   -Recurse -Force -EA SilentlyContinue; \
                 {products_step}"
            )
        }

        // ── A GENUINELY SEPARATE INSTALL ─────────────────────────────────────
        RemovalPlan::MsiexecUninstall { dir } => {
            // REVIEW FIXES 2026-09-04 — THE HARD GUARD, still in front of the
            // path-deleting steps. `removal_target` is pure and unit-tested; a
            // refusal here refuses the whole pass.
            let roots = machine_roots();
            let root_refs: Vec<&str> = roots.iter().map(String::as_str).collect();
            let vetted = match removal_target(dir, &me_dir, &root_refs) {
                Ok(Some(d)) => d,
                Ok(None) => {
                    log::error!(
                        "rival install: REFUSING the elevated removal for {path} — a second copy \
                         was classified but no directory survived the guard. Nothing was run."
                    );
                    return false;
                }
                Err(why) => {
                    log::error!(
                        "rival install: REFUSING the elevated removal for {path} — {why}. Nothing \
                         was run and nothing was deleted. This is the guard added on 2026-09-04 \
                         review: the registry path (PROBLEM 238) returns an InstallLocation, and \
                         taking its parent once aimed `Remove-Item -Recurse -Force` at the folder \
                         ABOVE the install."
                    );
                    return false;
                }
            };
            let q = ps_quote(&vetted);
            log::info!(
                "rival install: elevated removal for {path} — a genuinely separate install at \
                 {vetted:?} passed the PROBLEM 244 check, so msiexec may run against products \
                 registered THERE and nowhere else"
            );
            // PROBLEM 244, three tightenings on this arm too:
            //   * only entries whose InstallLocation IS the vetted directory
            //     are uninstalled — `DisplayName -eq 'Spaceadom'` alone once
            //     swept up the orphaned entry whose files were the live app;
            //   * `MSIRESTARTMANAGERCONTROL=Disable` so the Restart Manager
            //     never closes US (it tried, and failed, on 2026-09-04);
            //   * `REBOOT=ReallySuppress` so a refused close cannot schedule a
            //     reboot-time file replacement (PROBLEM 127's family).
            format!(
                "$t = '{q}'.TrimEnd('\\'); \
                 Get-Process spaceadom -EA SilentlyContinue | \
                   Where-Object {{ $_.Path -like '{q}\\*' }} | Stop-Process -Force; \
                 Start-Sleep -Milliseconds 800; \
                 foreach ($r in @('HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall',\
                                  'HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall')) {{ \
                   Get-ChildItem $r -EA SilentlyContinue | ForEach-Object {{ \
                     $p = Get-ItemProperty $_.PSPath -EA SilentlyContinue; \
                     if ($p.DisplayName -eq 'Spaceadom' -and $p.InstallLocation -and \
                         $p.InstallLocation.TrimEnd('\\') -ieq $t) {{ \
                       Start-Process msiexec.exe -ArgumentList \"/X$($_.PSChildName)\",'/qn',\
                         '/norestart','REBOOT=ReallySuppress','MSIRESTARTMANAGERCONTROL=Disable' -Wait }} }} }}; \
                 Start-Sleep -Milliseconds 800; \
                 Remove-Item '{q}' -Recurse -Force -EA SilentlyContinue; \
                 Remove-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run' \
                   -Name 'Spaceadom' -EA SilentlyContinue"
            )
        }
    };

    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let verb = wide("runas");
    let file = wide("powershell.exe");
    let params = wide(&format!(
        "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command \"{}\"",
        script.replace('"', "\\\"")
    ));

    unsafe {
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(params.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        // Declining the prompt returns an error. That is a clean "no".
        if ShellExecuteExW(&mut sei).is_err() {
            log::info!("rival install: removal cancelled at the UAC prompt");
            return false;
        }
        if !sei.hProcess.is_invalid() {
            WaitForSingleObject(sei.hProcess, INFINITE);
            let _ = windows::Win32::Foundation::CloseHandle(sei.hProcess);
        }
    }

    // Verify against the DISK, not the exit code.
    let gone = detect().is_none();
    RIVAL_FOUND.store(!gone, Ordering::Relaxed);
    if gone {
        log::info!("rival install: the second copy at {path} is gone — one Spaceadom remains");
    } else {
        log::warn!("rival install: {path} is STILL present after the elevated removal");
    }
    gone
}

#[cfg(not(windows))]
pub fn repair() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // The audited GUID itself, PROBLEM 238 — used verbatim so the test
    // reads as the real machine, not a stand-in.
    //
    // CORRECTED 2026-09-04 (PROBLEM 244): this constant, and PROBLEM 238's
    // writeup, both carried `12EDBBADD76C5` — one D too many. The Application
    // event log's 1040/1042 transaction pair names the real ProductCode as
    // `{C68DC702-9414-421F-A3E4-12EDBBAD76C5}`, and that is what `msiexec /X`
    // was actually handed. A GUID quoted from memory into a document is not
    // evidence; the one in the log is.
    const ORPHANED_GUID_UNINSTALL_STRING: &str =
        "MsiExec.exe /X{C68DC702-9414-421F-A3E4-12EDBBAD76C5}";
    /// The same string, named for what it is in the PROBLEM 244 tests below.
    const INCIDENT_UNINSTALL_STRING: &str = ORPHANED_GUID_UNINSTALL_STRING;
    const NSIS_UNINSTALL_STRING: &str = r"C:\Users\owner\AppData\Local\Spaceadom\uninstall.exe";
    const PER_USER_DIR: &str = r"C:\Users\owner\AppData\Local\Spaceadom";
    const PROGRAM_FILES_DIR: &str = r"C:\Program Files\Spaceadom";

    #[test]
    fn healthy_per_user_entry_is_not_flagged() {
        // The HKCU entry NSIS itself creates: DisplayName "Spaceadom", but it
        // uninstalls through uninstall.exe, never MsiExec — this is what a
        // machine with exactly one, correctly-installed Spaceadom looks like.
        let verdict = classify_msi_entry(
            "Spaceadom",
            NSIS_UNINSTALL_STRING,
            PER_USER_DIR,
            true, // spaceadom.exe genuinely exists at that folder
            true, // and it IS the one currently running
        );
        assert_eq!(verdict, MsiEntryVerdict::NotOurs);
    }

    #[test]
    fn orphaned_msi_entry_pointing_at_our_own_folder_is_flagged_as_orphaned() {
        // PROBLEM 238, the audited case: an old MSI's uninstall entry
        // outlived the MSI. InstallLocation happens to equal the live
        // per-user folder, and the exe found there IS the current process —
        // nothing is running twice, but the entry must still be flagged so
        // it can be offered for removal (Programs and Features lists two).
        let verdict = classify_msi_entry(
            "Spaceadom",
            ORPHANED_GUID_UNINSTALL_STRING,
            PER_USER_DIR,
            true, // the folder has a spaceadom.exe...
            true, // ...but it's literally the one running now
        );
        assert_eq!(verdict, MsiEntryVerdict::OrphanedEntry);
    }

    #[test]
    fn orphaned_msi_entry_with_no_files_left_at_all_is_also_orphaned() {
        // The more common shape of a dead MSI entry: InstallLocation points
        // at a folder that no longer has anything in it.
        let verdict = classify_msi_entry(
            "Spaceadom",
            ORPHANED_GUID_UNINSTALL_STRING,
            PROGRAM_FILES_DIR,
            false, // no spaceadom.exe there any more
            false,
        );
        assert_eq!(verdict, MsiEntryVerdict::OrphanedEntry);
    }

    #[test]
    fn real_second_copy_in_program_files_is_flagged_as_a_real_second_copy() {
        // PROBLEM 129's original shape: a genuinely different exe, still
        // installed and able to register its own autostart + keyboard hook.
        let verdict = classify_msi_entry(
            "Spaceadom",
            ORPHANED_GUID_UNINSTALL_STRING,
            PROGRAM_FILES_DIR,
            true,  // a spaceadom.exe exists in Program Files
            false, // and it is NOT the one currently running
        );
        assert_eq!(verdict, MsiEntryVerdict::RealSecondCopy);
    }

    #[test]
    fn unrelated_app_with_spaceadom_in_the_name_is_never_flagged() {
        // The exact-match requirement: a substring match on "Spaceadom"
        // would have false-positived on any unrelated product that mentions
        // it (a plugin, an uninstaller helper, a differently-cased vendor
        // name). Only an EXACT (case-insensitive) DisplayName counts.
        for name in [
            "Spaceadom Cloud Sync Helper",
            "My Spaceadom Extension",
            "spaceadom",   // exact but different case — this one SHOULD match
            "Spaceadom ",  // trailing space — still exact after trim
        ] {
            let verdict = classify_msi_entry(
                name,
                ORPHANED_GUID_UNINSTALL_STRING,
                PROGRAM_FILES_DIR,
                true,
                false,
            );
            let expect_match = name.trim().eq_ignore_ascii_case("Spaceadom");
            assert_eq!(
                verdict != MsiEntryVerdict::NotOurs,
                expect_match,
                "name {name:?} should {}match",
                if expect_match { "" } else { "NOT " }
            );
        }
    }

    #[test]
    fn a_matching_name_with_a_non_msi_uninstall_string_is_never_flagged() {
        // DisplayName alone is not enough — an unrelated app that happens to
        // be named exactly "Spaceadom" but uninstalls via its own .exe (not
        // MsiExec) must not be offered up for `msiexec /X` removal.
        let verdict = classify_msi_entry(
            "Spaceadom",
            r"C:\Program Files\Spaceadom\some-other-uninstaller.exe",
            PROGRAM_FILES_DIR,
            true,
            false,
        );
        assert_eq!(verdict, MsiEntryVerdict::NotOurs);
    }

    // ───────────────────────── REVIEW FIXES 2026-09-04 ─────────────────────
    //
    // The bug these pin, in one line: `repair()` takes `Path::parent()` of
    // whatever `detect()` returns, PROBLEM 238's new path returned a DIRECTORY,
    // and the parent of a directory is the folder above the install — which an
    // elevated `Remove-Item -Recurse -Force` was then pointed at.

    /// Half one: what `detect()` returns for a `RealSecondCopy` must be an EXE,
    /// so `parent()` is the install directory and nothing above it. The
    /// trailing-backslash case is the one the registry actually produces.
    #[cfg(windows)]
    #[test]
    fn a_real_second_copys_path_is_the_exe_so_parent_is_the_install_dir() {
        for location in [
            r"D:\Apps\Spaceadom",
            r"D:\Apps\Spaceadom\",
            "  D:\\Apps\\Spaceadom\\  ", // whitespace + trailing separator
        ] {
            let exe = install_location_to_exe(location);
            assert_eq!(
                exe,
                std::path::PathBuf::from(r"D:\Apps\Spaceadom\spaceadom.exe"),
                "InstallLocation {location:?} must normalise to one exe path"
            );
            assert_eq!(
                exe.parent().unwrap(),
                std::path::Path::new(r"D:\Apps\Spaceadom"),
                "parent() must be the install dir — NOT D:\\Apps"
            );
        }
    }

    /// Half two, the guard. Every one of these is a directory the OLD code
    /// could compute and hand to `Remove-Item -Recurse -Force`.
    #[cfg(windows)]
    #[test]
    fn the_removal_guard_refuses_every_directory_that_is_not_an_install() {
        let me = r"C:\Users\owner\AppData\Local\Spaceadom";
        let roots = [r"C:\Program Files", r"C:\Users\owner\AppData\Local", r"C:\"];
        for bad in [
            r"D:\Apps",                             // the parent of an install
            r"C:\Program Files (x86)",              // ditto, per-machine
            r"C:\Users\owner\AppData\Local",        // ditto, per-user
            r"C:\Program Files",
            r"C:\",
            r"D:\",
            r"C:\Users\owner",
            "Spaceadom",                            // relative: no parent at all
        ] {
            assert!(
                removal_target(bad, me, &roots).is_err(),
                "{bad:?} must be refused, not deleted recursively"
            );
        }
    }

    /// An app must never offer to delete the folder it is running from — the
    /// same rule `detect_verbose`'s Path 1 already applies, now enforced on the
    /// registry path too. `contains` counts: a parent of our folder would take
    /// us with it.
    #[cfg(windows)]
    #[test]
    fn the_removal_guard_refuses_our_own_directory_and_anything_containing_it() {
        let me = r"C:\Users\owner\AppData\Local\Spaceadom";
        let roots: [&str; 0] = [];
        // Exactly us, in a different case — the orphaned-entry shape whose
        // InstallLocation IS our live folder (PROBLEM 238's audited machine).
        assert!(removal_target(r"c:\users\owner\appdata\local\spaceadom", me, &roots).is_err());
        assert!(removal_target(me, me, &roots).is_err());
        // A different install of the same name is fine — that is the whole job.
        assert_eq!(
            removal_target(r"C:\Program Files\Spaceadom", me, &roots),
            Ok(Some(r"C:\Program Files\Spaceadom".to_string()))
        );
        // And a sibling whose name merely STARTS with ours is not us: a string
        // `starts_with` would have called this our own directory.
        assert_eq!(
            removal_target(r"C:\Users\owner\AppData\Local\Spaceadom", r"C:\Users\owner\AppData\Local\SpaceadomBeta", &roots),
            Ok(Some(r"C:\Users\owner\AppData\Local\Spaceadom".to_string()))
        );
    }

    /// REVIEW FIXES 2026-09-05 (MEDIUM) — the OTHER direction of containment.
    ///
    /// The guard tested "is the target an ancestor of us", which catches
    /// `C:\Program Files` and misses a target INSIDE the live install. An
    /// `InstallLocation` is written by whatever wrote it and is never
    /// validated, so a stale entry naming a subfolder of our own directory is
    /// reachable — and `Remove-Item -Recurse -Force` there deletes part of the
    /// running app, which is PROBLEM 244 with a smaller blast radius.
    #[cfg(windows)]
    #[test]
    fn the_removal_guard_refuses_a_descendant_of_our_own_directory() {
        let me = r"C:\Users\owner\AppData\Local\Spaceadom";
        let roots: [&str; 0] = [];
        for inside in [
            r"C:\Users\owner\AppData\Local\Spaceadom\Spaceadom",
            r"C:\Users\owner\AppData\Local\Spaceadom\old\Spaceadom",
            r"c:\users\owner\appdata\local\spaceadom\backup\spaceadom",
        ] {
            let verdict = removal_target(inside, me, &roots);
            assert!(
                verdict.is_err(),
                "{inside} is inside the live install directory and must be refused, got \
                 {verdict:?}"
            );
            let why = verdict.unwrap_err();
            assert!(
                why.contains("INSIDE"),
                "the refusal must say WHICH containment it caught: {why}"
            );
        }
        // The legitimate case is unaffected: a different install of the same
        // name, in a different tree, is still removable — that is the job.
        assert_eq!(
            removal_target(r"C:\Program Files\Spaceadom", me, &roots),
            Ok(Some(r"C:\Program Files\Spaceadom".to_string()))
        );
    }

    /// The `OrphanedEntry` sentence has no path separator, so `parent()` is
    /// empty — and that must stay a clean "nothing to delete", NOT a refusal:
    /// the `msiexec /X{GUID}` pass is the entire repair for that case and it
    /// does not use this path at all.
    #[cfg(windows)]
    #[test]
    fn the_orphaned_entry_sentence_asks_for_no_deletion_and_is_not_an_error() {
        let sentence =
            "a leftover installer entry — no separate copy of Spaceadom is actually running";
        let dir = std::path::Path::new(sentence)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        assert_eq!(dir, "", "the sentence must carry no path separator");
        assert_eq!(removal_target(&dir, r"C:\x\Spaceadom", &[]), Ok(None));
    }

    // ─────────────────────── PROBLEM 244 — 2026-09-04 INCIDENT ──────────────
    //
    // What these pin, in one line: `msiexec /X` against the owner's orphaned
    // HKLM entry deleted that product's registered FILES, and those files were
    // the running `%LOCALAPPDATA%\Spaceadom\spaceadom.exe`. An orphaned entry
    // is now a REGISTRY-ONLY cleanup, and even a real second copy may only
    // reach the Installer when nothing it claims resolves to our own folder.

    /// The incident, verbatim. `DisplayVersion` 1.0.94, `InstallLocation` the
    /// live per-user directory, `UninstallString` `MsiExec.exe /X{GUID}` — the
    /// exact three values that were on the machine at 19:45. It must not be
    /// possible for this to authorise Windows Installer.
    #[cfg(windows)]
    #[test]
    fn the_2026_09_04_incident_entry_is_registry_only_and_can_never_reach_msiexec() {
        let me = r"C:\Users\beamu\AppData\Local\Spaceadom";
        let verdict = classify_msi_entry(
            "Spaceadom",
            INCIDENT_UNINSTALL_STRING,
            r"C:\Users\beamu\AppData\Local\Spaceadom",
            true, // a spaceadom.exe is there…
            true, // …and it is the one running: that is what makes it orphaned
        );
        assert_eq!(verdict, MsiEntryVerdict::OrphanedEntry);

        let plan = plan_removal(
            verdict,
            r"C:\Users\beamu\AppData\Local\Spaceadom",
            r"C:\Users\beamu\AppData\Local\Spaceadom\spaceadom.exe,0",
            INCIDENT_UNINSTALL_STRING,
            me,
        );
        assert!(
            matches!(plan, RemovalPlan::RegistryOnly { .. }),
            "the incident's own entry must be registry-only, got {plan:?}"
        );
    }

    /// PROBLEM 129's shape — a real per-machine second copy — must still be
    /// removable, or the fix has traded one fault for another.
    #[cfg(windows)]
    #[test]
    fn a_genuine_program_files_second_copy_may_still_be_uninstalled_with_msiexec() {
        let me = r"C:\Users\beamu\AppData\Local\Spaceadom";
        let plan = plan_removal(
            MsiEntryVerdict::RealSecondCopy,
            r"C:\Program Files\Spaceadom",
            r"C:\Program Files\Spaceadom\spaceadom.exe,0",
            INCIDENT_UNINSTALL_STRING,
            me,
        );
        assert_eq!(
            plan,
            RemovalPlan::MsiexecUninstall { dir: r"C:\Program Files\Spaceadom".to_string() }
        );
    }

    /// The nastier half of the incident: an `InstallLocation` that is an
    /// ANCESTOR of the live folder. Uninstalling that product would take us
    /// with it, so it must be refused even though it is a different directory.
    #[cfg(windows)]
    #[test]
    fn an_install_location_that_is_a_parent_of_the_live_directory_is_refused() {
        let me = r"C:\Users\beamu\AppData\Local\Spaceadom";
        for parent in [
            r"C:\Users\beamu\AppData\Local",
            r"C:\Users\beamu\AppData",
            r"C:\Users\beamu",
            r"C:\",
        ] {
            let plan = plan_removal(MsiEntryVerdict::RealSecondCopy, parent, "", "", me);
            assert!(
                matches!(plan, RemovalPlan::RegistryOnly { .. }),
                "{parent:?} contains the live install and must be refused, got {plan:?}"
            );
        }
    }

    /// The registry does not spell paths the way a comparison would like:
    /// trailing separators and arbitrary case are both routine. Neither may
    /// let the live folder through as "a different directory".
    #[cfg(windows)]
    #[test]
    fn a_trailing_backslash_or_a_different_case_still_resolves_to_our_own_directory() {
        let me = r"C:\Users\beamu\AppData\Local\Spaceadom";
        for spelling in [
            r"C:\Users\beamu\AppData\Local\Spaceadom\",
            r"c:\users\beamu\appdata\local\spaceadom",
            r"C:\USERS\BEAMU\APPDATA\LOCAL\SPACEADOM\",
            "  C:\\Users\\beamu\\AppData\\Local\\Spaceadom\\  ",
            r"C:\Users\beamu\AppData\Local\Spaceadom\\",
        ] {
            let plan = plan_removal(MsiEntryVerdict::RealSecondCopy, spelling, "", "", me);
            assert!(
                matches!(plan, RemovalPlan::RegistryOnly { .. }),
                "{spelling:?} IS our own directory and must be refused, got {plan:?}"
            );
        }
        // …and a sibling that merely starts with our name is genuinely not us.
        let plan = plan_removal(
            MsiEntryVerdict::RealSecondCopy,
            r"C:\Users\beamu\AppData\Local\SpaceadomBeta",
            "",
            "",
            me,
        );
        assert_eq!(
            plan,
            RemovalPlan::MsiexecUninstall {
                dir: r"C:\Users\beamu\AppData\Local\SpaceadomBeta".to_string()
            }
        );
    }

    /// `InstallLocation` is not the only field that names a directory. If the
    /// icon or the uninstall command lives in our folder, the product's
    /// components can too.
    #[cfg(windows)]
    #[test]
    fn a_display_icon_or_uninstall_path_inside_our_folder_also_refuses_msiexec() {
        let me = r"C:\Users\beamu\AppData\Local\Spaceadom";
        let by_icon = plan_removal(
            MsiEntryVerdict::RealSecondCopy,
            r"D:\Apps\Spaceadom",
            r"C:\Users\beamu\AppData\Local\Spaceadom\spaceadom.exe,0",
            "",
            me,
        );
        assert!(matches!(by_icon, RemovalPlan::RegistryOnly { .. }), "{by_icon:?}");

        let by_uninstaller = plan_removal(
            MsiEntryVerdict::RealSecondCopy,
            r"D:\Apps\Spaceadom",
            "",
            "\"C:\\Users\\beamu\\AppData\\Local\\Spaceadom\\uninstall.exe\" /S",
            me,
        );
        assert!(matches!(by_uninstaller, RemovalPlan::RegistryOnly { .. }), "{by_uninstaller:?}");

        // With both fields pointing somewhere else entirely, the same entry is
        // allowed — proving the refusals above are caused by the paths, not by
        // the extra arguments being present at all.
        let clean = plan_removal(
            MsiEntryVerdict::RealSecondCopy,
            r"D:\Apps\Spaceadom",
            r"D:\Apps\Spaceadom\spaceadom.exe,0",
            r"MsiExec.exe /X{C68DC702-9414-421F-A3E4-12EDBBAD76C5}",
            me,
        );
        assert_eq!(clean, RemovalPlan::MsiexecUninstall { dir: r"D:\Apps\Spaceadom".to_string() });
    }

    /// An entry with no `InstallLocation` gives nothing to vet, so it can only
    /// ever be a registry cleanup.
    #[cfg(windows)]
    #[test]
    fn an_entry_with_no_install_location_is_registry_only() {
        let plan = plan_removal(
            MsiEntryVerdict::RealSecondCopy,
            "   ",
            "",
            "",
            r"C:\Users\beamu\AppData\Local\Spaceadom",
        );
        assert!(matches!(plan, RemovalPlan::RegistryOnly { .. }), "{plan:?}");
    }

    /// `dir_of_path_value` has to survive the two shapes the registry writes:
    /// an icon index appended after a comma, and a quoted command with
    /// arguments. Anything that is not a drive-rooted path — `MsiExec.exe
    /// /X{GUID}` above all — must yield nothing rather than a guess.
    #[cfg(windows)]
    #[test]
    fn a_directory_is_only_taken_from_a_value_that_really_is_a_path() {
        assert_eq!(
            dir_of_path_value(r"C:\Program Files\Spaceadom\spaceadom.exe,0"),
            r"C:\Program Files\Spaceadom"
        );
        assert_eq!(
            dir_of_path_value("\"C:\\Program Files\\Spaceadom\\uninstall.exe\" /S"),
            r"C:\Program Files\Spaceadom"
        );
        assert_eq!(dir_of_path_value(r"MsiExec.exe /X{C68DC702-9414-421F-A3E4-12EDBBAD76C5}"), "");
        assert_eq!(dir_of_path_value(""), "");
        assert_eq!(dir_of_path_value("Spaceadom"), "");
    }

    /// The packed/squished ProductCode, both directions. The first case is the
    /// canonical Windows Installer example, which is what proves the algorithm
    /// itself; the second is the incident's own GUID, which is what the
    /// registry-only script will actually delete.
    #[cfg(windows)]
    #[test]
    fn the_packed_product_code_matches_the_canonical_example_and_round_trips() {
        assert_eq!(
            packed_guid("{01234567-89AB-CDEF-0123-456789ABCDEF}").unwrap(),
            "76543210BA98FEDC1032547698BADCFE"
        );
        assert_eq!(
            packed_guid("{C68DC702-9414-421F-A3E4-12EDBBAD76C5}").unwrap(),
            "207CD86C4149F1243A4E21DEBBDA675C"
        );
        // Braces optional, case irrelevant, output always upper-case.
        assert_eq!(
            packed_guid("c68dc702-9414-421f-a3e4-12edbbad76c5").unwrap(),
            "207CD86C4149F1243A4E21DEBBDA675C"
        );
        for g in [
            "{01234567-89AB-CDEF-0123-456789ABCDEF}",
            "{C68DC702-9414-421F-A3E4-12EDBBAD76C5}",
            "{00000000-0000-0000-0000-000000000000}",
            "{FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF}",
        ] {
            let packed = packed_guid(g).expect("must pack");
            assert_eq!(unpack_guid(&packed).as_deref(), Some(g), "round trip failed for {g}");
        }
        // Rubbish in, None out — never a half-converted key path.
        assert_eq!(packed_guid("not-a-guid"), None);
        assert_eq!(packed_guid("{C68DC702-9414-421F-A3E4-12EDBBAD76C}"), None);
        assert_eq!(packed_guid("{G68DC702-9414-421F-A3E4-12EDBBAD76C5}"), None);
        assert_eq!(unpack_guid("207CD86C4149F1243A4E21DEBBDA675"), None);
        assert_eq!(unpack_guid(""), None);
    }

    // ── PROBLEM 250 follow-up (LIVE TEST 2026-09-05) — both cross-kind
    //    directions. Seven cases, and the two that matter most are the
    //    NEGATIVE ones: a copy that reports ITSELF as its own rival would put
    //    a permanent, unactionable banner on the dashboard of a perfectly
    //    healthy machine.

    /// FINDING C itself: the packaged copy running on 2026-09-05 with the
    /// owner's per-user NSIS 1.0.100 still on disk. Before this, `detect_full`
    /// saw nothing and logged "this machine has one Spaceadom".
    #[test]
    fn a_packaged_copy_sees_the_per_user_install_beside_it() {
        assert_eq!(
            classify_cross_kind(true, true, false, false),
            CrossKindVerdict::PerUserBesidePackaged
        );
    }

    /// A Store install on a machine that never had the setup.exe — the clean
    /// case, and it must stay silent.
    #[test]
    fn a_packaged_copy_alone_on_the_machine_reports_nothing() {
        assert_eq!(classify_cross_kind(true, false, false, false), CrossKindVerdict::None);
    }

    /// The self-report guard on the filesystem side. A packaged process runs
    /// from `…\WindowsApps\…` so it can never be the per-user exe today; the
    /// rule is written down anyway, because Path 1 has carried the same one
    /// since PROBLEM 129 and a relocated or portable copy would need it.
    #[test]
    fn a_copy_that_is_itself_the_per_user_exe_is_not_its_own_rival() {
        assert_eq!(classify_cross_kind(true, true, true, false), CrossKindVerdict::None);
    }

    /// The self-report guard on the PACKAGE side, and the reason the two arms
    /// are mutually exclusive rather than two independent `if`s. A packaged
    /// copy asking "is a Spaceadom package installed?" always gets "yes" —
    /// itself. It must never reach the Store branch, whatever that flag says.
    #[test]
    fn a_packaged_copy_never_reports_a_store_copy_because_it_would_be_itself() {
        assert_eq!(classify_cross_kind(true, false, false, true), CrossKindVerdict::None);
    }

    /// The reverse direction: the ordinary NSIS copy, with a Store copy
    /// registered for this user as well.
    #[test]
    fn an_unpackaged_copy_sees_a_store_copy_registered_beside_it() {
        assert_eq!(
            classify_cross_kind(false, false, false, true),
            CrossKindVerdict::StoreBesideUnpackaged
        );
    }

    /// The owner's own machine, every day: one unpackaged per-user install,
    /// no package. `per_user_exe_exists` is TRUE here and irrelevant — that
    /// file is us — which is why the unpackaged arm never looks at it.
    #[test]
    fn one_ordinary_per_user_install_and_no_package_reports_nothing() {
        assert_eq!(classify_cross_kind(false, true, true, false), CrossKindVerdict::None);
        assert_eq!(classify_cross_kind(false, true, false, false), CrossKindVerdict::None);
    }

    /// A `store_copy` finding's `path` is a SENTENCE, and that is load-bearing
    /// twice over: `repair()` refuses on the kind, and `dir_of_path_value`
    /// (which is how `repair()` derives a delete target) must independently
    /// answer "nothing" for it. A package full name contains no separator, so
    /// the sentence never grows one.
    #[cfg(windows)]
    #[test]
    fn a_store_copy_finding_can_never_yield_a_deletable_directory() {
        let sentence = format!(
            "a Microsoft Store copy is also installed ({}) — keep one",
            "LOCALTEST.Spaceadom_1.0.100.0_x64__nj4cr7rfsqc4c"
        );
        assert_eq!(dir_of_path_value(&sentence), "");
        assert_eq!(removal_target("", r"C:\Users\owner\AppData\Local\Spaceadom", &[]), Ok(None));
    }

}
