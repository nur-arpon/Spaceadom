//! portable.rs — PROBLEM 254. "Is this copy a portable, self-contained
//! build, and if so where does ALL of its data live?"
//!
//! THE FEATURE. A friend who does not want an installer touching their
//! machine should be able to unzip Spaceadom somewhere, run the exe, and have
//! every file it ever writes — config, log, backups, the app-picker cache,
//! cached release notes, the update rollback archive, last-run-version —
//! land inside that same folder. Nothing in `%APPDATA%`, nothing in
//! `%LOCALAPPDATA%`, nothing in the registry beyond what Windows itself
//! always touches for any running process. Deleting the folder deletes the
//! app AND its data, completely, which is the whole point of "portable".
//!
//! THE SIGNAL. A marker file, `portable.txt`, beside the exe. Explicit and
//! testable on purpose — "is my directory an installed location?" has no
//! reliable answer (a user can extract a zip anywhere, including somewhere
//! that happens to look like an install path), while "does this exact file
//! exist next to me?" is a single `Path::exists()` call with no ambiguity.
//! `scripts/build-portable.mjs` is the only thing that ever creates the
//! marker; nothing in the running app ever writes or deletes it.
//!
//! THE PRECEDENCE, and why packaged wins unconditionally: a Microsoft Store
//! (MSIX) install cannot carry a marker file of its own choosing — its
//! package layout is whatever `AppxManifest.xml` + the package root declare,
//! entirely outside this app's control, and Windows owns its data location
//! exactly as `packaged.rs` documents. So [`data_root`] asks
//! `packaged::is_packaged()` FIRST; only an unpackaged process is ever
//! eligible to be portable. This is a fact worth pinning down with a test
//! (`packaged_always_wins_even_with_a_marker_present`) rather than trusting
//! every future caller to check the two in the right order.
//!
//! THE ONE RESOLVER. Every site in this codebase that used to build a data
//! path from `%APPDATA%`/`%LOCALAPPDATA%`/`dirs::` now goes through
//! [`data_root`], directly or (for the overwhelming majority of them)
//! through `startup::data_dir()`, which as of this feature is a one-line
//! wrapper around it:
//!
//! | What | Where it already funnelled through `data_dir()` |
//! | --- | --- |
//! | `config.json` | `config::config_path()` |
//! | the rolling config backups | `config::backup_dir()` — see below, this ONE bypassed `data_dir()` on purpose and needed its own portable check |
//! | `debug.log` | `lib.rs::run()` passes `startup::data_dir()` straight to `logger::init` |
//! | the app-picker disk cache | `picker_worker.rs::cache_path()` |
//! | cached release notes | `release_notes.rs` |
//! | `last-run-version.txt` + the update rollback archive | `updater.rs::{last_run_path, rollback_dir}` |
//! | the one-shot overlay re-test marker | `commands.rs` |
//! | the first-packaged-launch migration marker + snapshot | `packaged.rs` — unaffected here: packaged always resolves to roaming regardless of a marker file, see above |
//!
//! `config::backup_dir()` was the ONE exception: it reads `%LOCALAPPDATA%`
//! directly rather than calling `data_dir()`, deliberately, so an uninstall
//! that removes the app's data folder does not also take the backups with
//! it. That reasoning does not apply to a portable copy — there is no
//! separate "uninstall" step, the whole point is that the copy is one
//! self-contained folder — so `backup_dir()` now checks [`is_portable`]
//! itself and, when true, nests the backups under [`data_root`] like
//! everything else. See the comment there.
//!
//! `startup::legacy_data_dir()` (the one-time migration from the pre-1.0.0
//! "SpaceToggle V14" product identity) is DELIBERATELY left untouched by this
//! feature: it is asking "did an older INSTALL of a different product exist
//! on this machine", which has no portable analogue — a portable copy is
//! always a fresh, self-contained folder with nothing to migrate from, and
//! the check is harmless (it simply finds nothing) if it ever runs anyway.
//!
//! WHAT ELSE CHANGES WHEN PORTABLE, and where (not all of it lives here):
//!
//! - **Autostart is never registered** — no Scheduled Task, no HKCU Run
//!   value. `startup.rs`'s three write sites (`set_run_key`,
//!   `ensure_startup_task`, `apply_task_enabled`) each check [`is_portable`]
//!   right after their existing `packaged::is_packaged()` guard and return
//!   without touching the registry or Task Scheduler.
//! - **The in-app updater is inert.** A portable copy has no installer of
//!   its own to silently re-run — there is nothing to hand `setup.exe /S
//!   /UPDATE`. See `updater.rs` (not edited here — another agent owns it;
//!   the exact lines are in this task's report under "UPDATER.RS LINES TO
//!   ADD").
//! - **The Settings "Run at startup" row goes inert**, with a note explaining
//!   why and what to do instead (put a shortcut in the Startup folder). See
//!   `settings-panel.ts` (not edited here — "SETTINGS-PANEL LINES TO ADD").
//! - **Rival-install detection still runs and still matters**: a portable
//!   copy sitting beside an installed one is a REAL second `WH_KEYBOARD_LL`
//!   hook, exactly like two installed copies would be. Nothing in
//!   `rival_install.rs` needs to change for THIS to keep working — a
//!   portable copy is a normal, unpackaged, non-elevated process, so its
//!   `repair()` can still elevate via `runas` exactly as an NSIS copy's can.
//!   What changes is only the banner's WORDING, so it names the shape
//!   correctly instead of implying "installer vs installer". See
//!   `main.ts` ("MAIN.TS LINES TO ADD").
//!
//! WHAT NEVER CHANGES: `promote_tray_icon_once` and
//! `remove_legacy_run_entries` in `startup.rs` are left alone on purpose.
//! Neither writes an autostart entry — one is a tray-visibility preference,
//! the other cleans up entries a DIFFERENT, older build may have left — and
//! both are harmless (and arguably still useful hygiene) for a portable copy.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The marker file. Its presence beside the exe, and nothing else, decides
/// portable mode. `scripts/build-portable.mjs` writes it into the zip with a
/// one-line explanation inside; the running app never reads its contents.
pub const MARKER_FILE: &str = "portable.txt";

/// The sub-folder of the exe's own directory that holds every file this app
/// writes when running portable. Named plainly (not after the product or the
/// bundle id) because it lives INSIDE a folder that is already named
/// Spaceadom-something — `Spaceadom_1.2.3_x64-portable\data\` — so nesting
/// the name again would be noise.
const PORTABLE_DATA_SUBDIR: &str = "data";

/// The boot line's marker. Long, lower-case, hyphenated and unique in this
/// repository for the same reason `packaged::BOOT_MARKER` is: a SHORT
/// identifier can be assembled at runtime by overlapping immediate stores and
/// never exist contiguously in the binary (CLAUDE.md, `st-hud-pointer`,
/// measured 2026-08-27), so a marker used to prove a build contains this code
/// has to be a long `log::` format string.
pub const BOOT_MARKER: &str = "portable-mode-data-root-resolution-spaceadom-marker";

/// `(is_portable, resolved data root)`, computed once and cached. Both
/// [`is_portable`] and [`data_root`] read from here so the two can never
/// disagree with each other, and so the (cheap, but not free) `current_exe()`
/// + `Path::exists()` probe runs exactly once per process.
static RESOLVED: OnceLock<(bool, PathBuf)> = OnceLock::new();
static LOGGED: OnceLock<()> = OnceLock::new();

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

fn marker_present(exe_dir: Option<&Path>) -> bool {
    exe_dir.map(|d| d.join(MARKER_FILE).is_file()).unwrap_or(false)
}

/// The normal, non-portable answer: `%APPDATA%\Spaceadom`. Identical to what
/// `startup::data_dir()` computed before this feature existed — moved here
/// rather than duplicated, since `startup::data_dir()` is now a wrapper
/// around [`data_root`].
///
/// `pub(crate)` since the PROBLEM 250 follow-up (2026-09-05):
/// `packaged::migrate_legacy_data_once` has to ask whether the RESOLVED data
/// dir is this one, and re-spelling `%APPDATA%\Spaceadom` there would put two
/// producers of one string on either side of a comparison — the exact shape
/// `rival_install::install_location_to_exe`'s note warns about.
pub(crate) fn roaming_default() -> PathBuf {
    std::env::var("APPDATA")
        .map(|p| PathBuf::from(p).join("Spaceadom"))
        .unwrap_or_else(|_| PathBuf::from("Spaceadom"))
}

/// The pure decision, unit-tested without touching the filesystem, the
/// environment, or `packaged::is_packaged()`'s real syscall.
///
/// Spelled out as "packaged wins, THEN marker, THEN roaming" rather than
/// leaving the precedence to whichever order a caller happens to check things
/// in — see the module doc's "THE PRECEDENCE" section for why packaged must
/// win even though in practice a packaged process can never actually have a
/// marker of its own.
pub(crate) fn resolve_root(
    packaged: bool,
    exe_dir: Option<&Path>,
    marker_present: bool,
    roaming: &Path,
) -> PathBuf {
    if !packaged {
        if let Some(dir) = exe_dir {
            if marker_present {
                return dir.join(PORTABLE_DATA_SUBDIR);
            }
        }
    }
    roaming.to_path_buf()
}

fn resolved() -> &'static (bool, PathBuf) {
    RESOLVED.get_or_init(|| {
        let packaged = crate::packaged::is_packaged();
        let dir = exe_dir();
        let marker = marker_present(dir.as_deref());
        let roaming = roaming_default();
        let root = resolve_root(packaged, dir.as_deref(), marker, &roaming);
        let portable = !packaged && marker && dir.is_some();
        (portable, root)
    })
}

/// True when this copy is running portable: unpackaged, with `portable.txt`
/// found beside the exe. Cached after the first call — like
/// `packaged::is_packaged()`, this can never change during the life of the
/// process, so every caller after the first is an atomic load.
pub fn is_portable() -> bool {
    resolved().0
}

/// THE resolver. Returns where ALL of this app's own data lives:
/// `<exe dir>\data` when portable, `%APPDATA%\Spaceadom` otherwise (packaged
/// or not — a packaged copy's real location is a question for `packaged.rs`,
/// not this module; `data_root` only ever returns the roaming path for it).
///
/// Callers should almost always go through `startup::data_dir()` instead —
/// it is the name every existing path site already used before this feature,
/// so routing it through here (rather than replacing it everywhere) is what
/// keeps this a one-function change for most of the codebase. Call this
/// directly only from code that has a reason to ask the question on its own
/// (`config::backup_dir()`, the autostart guards in `startup.rs`).
pub fn data_root() -> PathBuf {
    resolved().1.clone()
}

/// Log the decision once, at boot, immediately after
/// `packaged::log_identity_once()` — same rationale, same place: which world
/// a bug report came from is unanswerable from a version number or a path
/// string alone, so it has to be IN the log, every time, near the top.
pub fn log_root_once() {
    if LOGGED.set(()).is_err() {
        return;
    }
    let (portable, root) = resolved();
    if *portable {
        log::info!(
            "{BOOT_MARKER}: PORTABLE — '{MARKER_FILE}' was found beside the exe, so ALL app \
             data (config.json, debug.log, the rolling backups, the picker cache, cached \
             release notes, last-run-version.txt, the update rollback archive) lives under \
             {}. Autostart is never registered (no Run key, no Scheduled Task) and the \
             in-app updater is inert — a portable copy has no installer of its own to re-run.",
            root.display()
        );
    } else {
        log::info!(
            "{BOOT_MARKER}: not portable — using the normal per-user data dir {}",
            root.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe() -> PathBuf {
        PathBuf::from(r"D:\Somewhere\Spaceadom_1.2.3_x64-portable")
    }
    fn roaming() -> PathBuf {
        PathBuf::from(r"C:\Users\arpon\AppData\Roaming\Spaceadom")
    }

    /// The feature's whole reason to exist: the marker present, unpackaged,
    /// routes every path under the exe's own folder.
    #[test]
    fn marker_present_and_unpackaged_routes_to_exe_dir_data() {
        let root = resolve_root(false, Some(&exe()), true, &roaming());
        assert_eq!(root, exe().join("data"));
    }

    /// No marker beside the exe is the overwhelmingly common case — every
    /// existing NSIS/MSI/dev-build user — and must reproduce EXACTLY what
    /// `startup::data_dir()` returned before this feature existed.
    #[test]
    fn marker_absent_falls_back_to_the_roaming_default() {
        let root = resolve_root(false, Some(&exe()), false, &roaming());
        assert_eq!(root, roaming());
    }

    /// PACKAGED WINS, EVEN WITH A MARKER PRESENT. A Store package cannot
    /// actually carry a file it did not declare in its manifest, but the
    /// precedence must hold as a FACT the resolver enforces, not an
    /// assumption about what a package layout can contain — see the module
    /// doc's "THE PRECEDENCE" section.
    #[test]
    fn packaged_always_wins_even_with_a_marker_present() {
        let root = resolve_root(true, Some(&exe()), true, &roaming());
        assert_eq!(
            root,
            roaming(),
            "a packaged copy must resolve to the roaming default no matter what a marker check \
             says"
        );
    }

    /// `current_exe()` can fail (rare, but documented as fallible). No exe
    /// directory to test for a marker in must never panic and must never
    /// silently invent a relative path — the safe answer is the roaming one.
    #[test]
    fn no_exe_dir_falls_back_to_the_roaming_default() {
        let root = resolve_root(false, None, true, &roaming());
        assert_eq!(root, roaming());
    }

    /// The real (non-pure) entry points, exercised against whatever this test
    /// binary's own exe directory happens to be. `cargo test`'s own output
    /// directory certainly has no `portable.txt` beside it, so this is a
    /// weak positive control in the same sense `packaged.rs`'s equivalent
    /// test is: it proves the unpackaged, non-portable branch, and is cached
    /// correctly across repeated calls. The PORTABLE branch is proved by the
    /// pure `resolve_root` tests above plus the build-script proof in
    /// `scripts/build-portable.mjs`'s own checks.
    #[test]
    fn a_cargo_test_binary_is_never_portable() {
        assert!(!is_portable());
        assert!(!is_portable(), "cached answer must not change between calls");
    }

    /// CLAUDE.md's ASCII-marker rule: long enough to survive in `.rodata`
    /// rather than being assembled by overlapping immediate stores
    /// (`st-hud-pointer`, 14 bytes, measured False in a binary that
    /// contained it).
    #[test]
    fn the_boot_marker_is_long_enough_to_be_findable_in_the_binary() {
        assert!(BOOT_MARKER.len() >= 32, "BOOT_MARKER is {} bytes", BOOT_MARKER.len());
        assert!(BOOT_MARKER.is_ascii());
    }

    /// `resolve_root` always returns the SAME data root whichever function
    /// asks — `data_root()` cloning out of the cached tuple must not drift
    /// from what `is_portable()` read off the same tuple.
    #[test]
    fn is_portable_and_data_root_agree_on_the_same_process() {
        let portable = is_portable();
        let root = data_root();
        if portable {
            assert!(root.ends_with("data"));
        } else {
            assert!(root.ends_with("Spaceadom"));
        }
    }
}
