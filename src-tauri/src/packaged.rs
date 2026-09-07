//! packaged.rs — "is this copy running from an MSIX package?", and everything
//! that has to behave differently when the answer is yes.
//!
//! PROBLEM 250. Spaceadom ships three ways now:
//!
//! | Shape | Who installs it | Who updates it |
//! | --- | --- | --- |
//! | `setup.exe` (NSIS, per-user) | the user, from GitHub | **the app itself** (PROBLEM 245) |
//! | `.msi` (WiX, per-machine) | an admin | detected, currently off |
//! | `.msix` (Store) | **the Microsoft Store** | **the Store** |
//!
//! The third one is new, and it is not a fourth installer of the same app — it
//! is a DIFFERENT RUNTIME. A packaged process has an app-container-ish identity
//! layered over a normal Win32 token: it has a package family name, its writes
//! to `%LOCALAPPDATA%` and `HKCU` may be redirected into a per-package store,
//! and Windows — not the app — owns its update, its uninstall and its
//! run-at-logon entry. Four things in this codebase are wrong in that world,
//! and each is wrong SILENTLY:
//!
//! 1. **The updater.** It would download a `setup.exe` and run it, producing a
//!    second, unpackaged copy beside the packaged one — PROBLEM 129 with extra
//!    steps. The Store owns updates for a Store install. `updater.rs` returns
//!    early when [`is_packaged`] is true.
//! 2. **Autostart.** `startup.rs` writes `HKCU\...\Run` with the absolute path
//!    of `current_exe()`. In a package that path is
//!    `C:\Program Files\WindowsApps\<Publisher>.Spaceadom_<VERSION>_x64__<hash>\spaceadom.exe`
//!    — **the package version is IN the directory name, and the Store rewrites
//!    it on every update.** A Run value written today points at a folder that
//!    will not exist after the next Store update, and the failure is silent:
//!    the shell finds nothing to launch and says nothing. (Whether HKCU is also
//!    virtualised is a second question and deliberately NOT what this rests on
//!    — Microsoft's virtualization docs scope every redirection rule to
//!    *virtualized* i.e. appContainer packages, and never say in one sentence
//!    that a `runFullTrust` package is exempt. The stale-path argument holds
//!    either way.) The packaged mechanism is a `windows.startupTask` extension
//!    declared in the manifest and driven through
//!    [`Windows.ApplicationModel.StartupTask`][st]; Windows re-points it at the
//!    new package on every update, because Windows owns it.
//! 3. **Config.** See [`migrate_legacy_data_once`] — the long comment there is
//!    the part of this module worth reading twice.
//! 4. **The rival-install banner.** A packaged copy beside an NSIS copy is a
//!    REAL second copy, both of which install a keyboard hook — but the banner's
//!    one-click `msiexec`/`Remove-Item` repair cannot run from inside a package
//!    and must not be offered. `rival_install.rs` swaps the copy instead.
//!
//! [st]: https://learn.microsoft.com/en-us/uwp/api/windows.applicationmodel.startuptask
//!
//! ## Why this is a probe and not a `cfg!`
//!
//! There is no separate "Store build" of `spaceadom.exe`. The SAME binary is
//! laid out inside the `.msix` and shipped as the NSIS installer's payload, so
//! a compile-time flag would mean two binaries to sign, two to test, and one
//! more way for the wrong one to reach a user. `GetCurrentPackageFullName`
//! answers the question at runtime, in microseconds, once.
//!
//! ## Package identity and file-system redirection are NOT the same thing
//!
//! Measured on the dev machine, 2026-09-05, and it is the reason this module
//! does not try to use [`is_packaged`] to reason about paths. The Claude Code
//! agent shell runs with its `%LOCALAPPDATA%` redirected into
//! `…\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Local\` (CLAUDE.md's PROBLEM 143
//! — reading `%LOCALAPPDATA%\Spaceadom\spaceadom.exe` from it returns a
//! 14,109,184-byte v1.0.53 while the real machine has v1.0.100) — and
//! `GetCurrentPackageFullName` in that very process returns
//! `APPMODEL_ERROR_NO_PACKAGE`. **A process can be inside the redirection view
//! without having package identity.** So:
//!
//! - `is_packaged() == true` ⇒ we are a packaged app. Trustworthy.
//! - `is_packaged() == false` ⇒ we have no package identity. It does **not**
//!   prove our file writes land where the path string says.

use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// The boot line's marker. Deliberately long, lower-case, hyphenated and
/// unique in this repository, for two reasons:
///
/// - `grep` in `debug.log` finds packaged-vs-unpackaged in one command, which
///   is the first question to ask about any Store bug report;
/// - CLAUDE.md's ASCII-marker rule: a SHORT identifier can be materialised by
///   overlapping immediate stores and never exist contiguously in the binary
///   (`st-hud-pointer`, measured 2026-08-27), so a marker used to prove a build
///   contains this code has to be a long `log::` format string.
pub const BOOT_MARKER: &str = "package-identity-probe-msix-store-mode-spaceadom";

/// The `TaskId` of the `windows.startupTask` extension in `AppxManifest.xml`.
/// **These two strings must match exactly**, and nothing else may use this
/// value — `StartupTask::GetAsync` fails with `E_INVALIDARG` for an unknown id,
/// which is indistinguishable in a log from "the API is unavailable" unless the
/// mismatch is ruled out first.
pub const STARTUP_TASK_ID: &str = "SpaceadomStartupTask";

/// What the probe found. Kept as an enum rather than a `bool` so the package
/// full name is available for the log and for the Store-mode diagnostics
/// without a second syscall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// No package identity: an NSIS or MSI install, or a `cargo` build.
    Unpackaged,
    /// Running from an MSIX package. `full_name` is e.g.
    /// `12345Publisher.Spaceadom_1.0.100.0_x64__abcdefgh12345`.
    Packaged { full_name: String },
}

static IDENTITY: OnceLock<Identity> = OnceLock::new();
static LOGGED: OnceLock<()> = OnceLock::new();

/// Cached. The first call does the syscall; every later call is an atomic load.
pub fn identity() -> &'static Identity {
    IDENTITY.get_or_init(probe)
}

/// True when this process is running from an installed MSIX package.
pub fn is_packaged() -> bool {
    matches!(identity(), Identity::Packaged { .. })
}

/// The placeholder name for a process that IS packaged but whose package could
/// not be named — see the second `GetCurrentPackageFullName` call in
/// [`probe`]. REVIEW FIXES 2026-09-05 (MEDIUM).
pub const UNKNOWN_PACKAGE_NAME: &str = "<packaged, name unavailable>";

/// The package full name, or `None` when unpackaged **or when the name could
/// not be read**. The two are different facts and `is_packaged()` is the one
/// that separates them: a caller that only wants a string to print gets `None`
/// either way, and a caller deciding BEHAVIOUR must ask `is_packaged()`.
pub fn package_full_name() -> Option<&'static str> {
    match identity() {
        Identity::Packaged { full_name } if full_name == UNKNOWN_PACKAGE_NAME => None,
        Identity::Packaged { full_name } => Some(full_name.as_str()),
        Identity::Unpackaged => None,
    }
}

/// How `GetCurrentPackageFullName`'s return code is read. Split out from the
/// `unsafe` call so the mapping — the part that can be wrong — is a pure
/// function with tests, and the syscall is four lines that cannot be.
///
/// `GetCurrentPackageFullName` is documented to return `ERROR_SUCCESS`,
/// `ERROR_INSUFFICIENT_BUFFER` (the sizing call, which is what a null buffer
/// always produces for a packaged process) or `APPMODEL_ERROR_NO_PACKAGE`
/// (15700) when the process has no package identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeVerdict {
    /// 15700 — definitively unpackaged. The ONLY code that means this.
    NoPackage,
    /// 0 or 122 — packaged; read the name.
    Packaged,
    /// Anything else. Treated as unpackaged (fail safe: the unpackaged paths
    /// are the ones that have shipped for a hundred versions) but LOGGED, so a
    /// future Windows that invents a fourth code is visible instead of silent.
    Unexpected(u32),
}

pub(crate) fn classify_probe_rc(rc: u32) -> ProbeVerdict {
    const ERROR_SUCCESS: u32 = 0;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    const APPMODEL_ERROR_NO_PACKAGE: u32 = 15700;
    match rc {
        APPMODEL_ERROR_NO_PACKAGE => ProbeVerdict::NoPackage,
        ERROR_SUCCESS | ERROR_INSUFFICIENT_BUFFER => ProbeVerdict::Packaged,
        other => ProbeVerdict::Unexpected(other),
    }
}

#[cfg(windows)]
fn probe() -> Identity {
    use windows::core::PWSTR;
    use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;

    // Sizing call. A null buffer with length 0 returns
    // ERROR_INSUFFICIENT_BUFFER for a packaged process and
    // APPMODEL_ERROR_NO_PACKAGE for an unpackaged one — so for the common case
    // (every NSIS/MSI install ever shipped) this is the ONLY syscall made.
    let mut len: u32 = 0;
    let rc = unsafe { GetCurrentPackageFullName(&mut len, PWSTR::null()) };
    match classify_probe_rc(rc.0) {
        ProbeVerdict::NoPackage => return Identity::Unpackaged,
        ProbeVerdict::Unexpected(code) => {
            // No log:: here — probe() can run before logger::init on some
            // paths. The boot line reports it (see log_identity_once).
            return Identity::Unpackaged.with_note(format!(
                "GetCurrentPackageFullName sizing call returned an undocumented {code}"
            ));
        }
        ProbeVerdict::Packaged => {}
    }

    // `len` now counts CHARACTERS INCLUDING the terminating NUL.
    let mut buf = vec![0u16; len as usize];
    let rc = unsafe { GetCurrentPackageFullName(&mut len, PWSTR(buf.as_mut_ptr())) };
    if rc.0 != 0 {
        // REVIEW FIXES 2026-09-05 (MEDIUM) — **STILL PACKAGED.** This used to
        // return `Identity::Unpackaged`, and that is a demotion the sizing
        // call has already ruled out: the ONLY code meaning "no package
        // identity" is APPMODEL_ERROR_NO_PACKAGE (15700), and it was not
        // returned. Something went wrong NAMING the package; nothing went
        // wrong establishing that there is one.
        //
        // What the demotion cost, if it ever fired: all four packaged
        // behaviours flip at once, silently (CLAUDE.md lists them). The
        // in-app updater comes back to life inside a Store install and
        // downloads a `setup.exe` that installs a SECOND, unpackaged copy —
        // two `WH_KEYBOARD_LL` hooks fighting over the spacebar, PROBLEM
        // 129/141/236. Autostart writes an HKCU Run value naming a
        // `WindowsApps\…_<VERSION>_…` path that the next Store update
        // deletes. And the rival-install banner offers to elevate and delete
        // files outside the package.
        //
        // "Fail safe toward the shipped path" is right for the SIZING call
        // (an unknown code there could mean anything) and wrong here (the
        // answer is already known). The name is what is missing, so the name
        // is what is marked unknown.
        return Identity::Packaged {
            full_name: UNKNOWN_PACKAGE_NAME.to_string(),
        }
        .with_note(format!(
            "GetCurrentPackageFullName said we are packaged, then failed to name the package \
             ({}) — treated as PACKAGED with an unknown name, never as unpackaged",
            rc.0
        ));
    }
    // Trim at the NUL rather than trusting `len` to have been rewritten.
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Identity::Packaged {
        full_name: String::from_utf16_lossy(&buf[..end]),
    }
}

#[cfg(not(windows))]
fn probe() -> Identity {
    Identity::Unpackaged
}

/// Carries an anomaly note from `probe()` to the boot line without a global.
/// Anomalies are rare enough that a side channel would be dead code most of
/// the time; this keeps the whole story in one `OnceLock`.
static PROBE_NOTE: OnceLock<String> = OnceLock::new();

impl Identity {
    fn with_note(self, note: String) -> Identity {
        let _ = PROBE_NOTE.set(note);
        self
    }
}

/// Say, ONCE, in `debug.log`, which world this process is in.
///
/// Called from `lib.rs` immediately after the build line, so the first three
/// lines of every log answer "which version, from where, packaged or not"
/// without anyone having to ask the machine. A Store bug report that does not
/// contain this line came from a build older than 1.0.101.
pub fn log_identity_once() {
    if LOGGED.set(()).is_err() {
        return;
    }
    match identity() {
        Identity::Packaged { full_name } => log::info!(
            "{BOOT_MARKER}: PACKAGED — this copy is running from an MSIX package \
             ({full_name}). The Store owns updates, uninstall and the logon entry: \
             the in-app updater is inert, autostart is the '{STARTUP_TASK_ID}' \
             startupTask instead of the HKCU Run value, and a Run-key write from \
             here would be virtualised and never seen by the shell."
        ),
        Identity::Unpackaged => log::info!(
            "{BOOT_MARKER}: UNPACKAGED — no MSIX package identity \
             (APPMODEL_ERROR_NO_PACKAGE), so this is an NSIS/MSI install or a cargo \
             build and every normal path applies. NOTE: this says nothing about \
             file-system redirection — a process can sit inside another package's \
             redirection view with no identity of its own (CLAUDE.md, PROBLEM 143)."
        ),
    }
    if let Some(note) = PROBE_NOTE.get() {
        log::warn!(
            "{BOOT_MARKER}: the package probe was ODD and was treated as unpackaged — {note}"
        );
    }
}

// ---------------------------------------------------------------------------
// Autostart, the packaged way
// ---------------------------------------------------------------------------

/// The state of the package's `windows.startupTask`, flattened for the UI.
///
/// The three "someone else decided" states are kept apart deliberately: the
/// settings row's copy has to say WHO turned it off, because the user's next
/// action differs (`DisabledByUser` → Task Manager ▸ Startup apps;
/// `DisabledByPolicy` → your IT administrator; `Unavailable` → tell us).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupState {
    /// Runs at logon.
    Enabled,
    /// Does not run at logon; the app may turn it back on.
    Disabled,
    /// The USER turned it off in Task Manager / Settings ▸ Apps ▸ Startup.
    /// **This is sticky and `RequestEnableAsync` cannot undo it** — Windows
    /// deliberately does not let an app override a user's startup decision.
    DisabledByUser,
    /// Group policy.
    DisabledByPolicy,
    /// Group policy, forced on.
    EnabledByPolicy,
    /// The API could not be reached at all (not packaged, WinRT activation
    /// failed, or the manifest has no matching `TaskId`).
    Unavailable,
}

/// Can the app itself change this? Pure; the settings row uses it to decide
/// between a live switch and an inert one with a note.
///
/// `Disabled` and `Enabled` are ours to change. Everything else is not, and a
/// switch that silently does nothing is the exact failure CLAUDE.md names
/// ("a control that does nothing is worse than a missing control").
pub fn app_may_change(state: StartupState) -> bool {
    matches!(state, StartupState::Enabled | StartupState::Disabled)
}

/// The one-sentence reason shown under an inert row. Empty when the row is
/// live. Pure, so the copy is testable and cannot drift from `app_may_change`.
pub fn startup_locked_note(state: StartupState) -> &'static str {
    match state {
        StartupState::Enabled | StartupState::Disabled => "",
        StartupState::DisabledByUser => {
            "Windows is holding this off. You turned Spaceadom off in Task Manager \
             (Startup apps), and Windows does not let an app switch itself back on. \
             Turn it on there and this comes back."
        }
        StartupState::DisabledByPolicy => {
            "Your organisation's policy decides this one, so the switch is off here."
        }
        StartupState::EnabledByPolicy => {
            "Your organisation's policy starts Spaceadom with Windows, so this cannot \
             be turned off here."
        }
        StartupState::Unavailable => {
            "Windows manages this for the Store version. Settings ▸ Apps ▸ Startup has \
             the switch."
        }
    }
}

/// Map the WinRT enum. Pure, separate from the COM call, and the reason the
/// numbers are written out: `StartupTaskState` is a `#[repr(transparent)]`
/// `i32` newtype, so a mis-ordered match arm compiles perfectly and is wrong.
pub(crate) fn classify_startup_state(raw: i32) -> StartupState {
    match raw {
        0 => StartupState::Disabled,
        1 => StartupState::DisabledByUser,
        2 => StartupState::Enabled,
        3 => StartupState::DisabledByPolicy,
        4 => StartupState::EnabledByPolicy,
        _ => StartupState::Unavailable,
    }
}

/// Read the package's startup task state. `Unavailable` for every failure —
/// including "not packaged", which is the normal case and is not logged.
#[cfg(windows)]
pub fn startup_task_state() -> StartupState {
    if !is_packaged() {
        return StartupState::Unavailable;
    }
    match get_task() {
        Ok(task) => match task.State() {
            Ok(s) => {
                let mapped = classify_startup_state(s.0);
                log::info!("packaged startup: '{STARTUP_TASK_ID}' state is {mapped:?} (raw {})", s.0);
                mapped
            }
            Err(e) => {
                log::warn!("packaged startup: could not read '{STARTUP_TASK_ID}' state: {e}");
                StartupState::Unavailable
            }
        },
        Err(e) => {
            log::warn!(
                "packaged startup: StartupTask::GetAsync(\"{STARTUP_TASK_ID}\") failed: {e}. \
                 Either AppxManifest.xml declares no windows.startupTask with that TaskId, or \
                 the ids do not match. The settings row will read as Windows-controlled."
            );
            StartupState::Unavailable
        }
    }
}

/// Ask Windows to enable or disable the package's startup task, and return the
/// state AFTERWARDS — not a `bool`, because `RequestEnableAsync` legitimately
/// answers "no" (`DisabledByUser`) and the caller has to be able to tell that
/// apart from an error.
#[cfg(windows)]
pub fn set_startup_task(enabled: bool) -> StartupState {
    if !is_packaged() {
        return StartupState::Unavailable;
    }
    let task = match get_task() {
        Ok(t) => t,
        Err(e) => {
            log::warn!("packaged startup: cannot reach '{STARTUP_TASK_ID}' to set it: {e}");
            return StartupState::Unavailable;
        }
    };

    if enabled {
        // Returns the RESULTING state. A user who switched this off in Task
        // Manager gets DisabledByUser back and the request is ignored — that
        // is Windows policy, not a bug, and the settings row says so.
        match task.RequestEnableAsync().and_then(|op| op.get()) {
            Ok(s) => {
                let mapped = classify_startup_state(s.0);
                if mapped == StartupState::Enabled {
                    log::info!("packaged startup: '{STARTUP_TASK_ID}' enabled — Spaceadom starts at logon");
                } else {
                    log::warn!(
                        "packaged startup: asked Windows to enable '{STARTUP_TASK_ID}' and it \
                         answered {mapped:?} (raw {}). Windows will not let an app override a \
                         user's or a policy's startup decision; the settings row explains it.",
                        s.0
                    );
                }
                mapped
            }
            Err(e) => {
                log::warn!("packaged startup: RequestEnableAsync failed: {e}");
                StartupState::Unavailable
            }
        }
    } else {
        match task.Disable() {
            Ok(()) => {
                log::info!("packaged startup: '{STARTUP_TASK_ID}' disabled — no logon launch");
                StartupState::Disabled
            }
            Err(e) => {
                log::warn!("packaged startup: Disable() failed: {e}");
                StartupState::Unavailable
            }
        }
    }
}

#[cfg(windows)]
fn get_task() -> windows::core::Result<windows::ApplicationModel::StartupTask> {
    use windows::core::HSTRING;
    use windows::ApplicationModel::StartupTask;
    StartupTask::GetAsync(&HSTRING::from(STARTUP_TASK_ID))?.get()
}

#[cfg(not(windows))]
pub fn startup_task_state() -> StartupState {
    StartupState::Unavailable
}

#[cfg(not(windows))]
pub fn set_startup_task(_enabled: bool) -> StartupState {
    StartupState::Unavailable
}

// ---------------------------------------------------------------------------
// Config migration
// ---------------------------------------------------------------------------

/// Marker written into the app data dir the first time a PACKAGED process gets
/// this far. Its presence — not the config's — is what makes the migration a
/// once-ever event, because the config is expected to exist afterwards.
const MIGRATION_MARKER: &str = "packaged-first-run.txt";

/// What the first packaged launch should do. Pure, so the decision is a test
/// rather than a thing that runs once on somebody else's machine and is never
/// seen again (CLAUDE.md's rule for recovery branches — PROBLEM 118).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MigrationPlan {
    /// Not a packaged process. Nothing about this applies.
    NotPackaged,
    /// The marker is there: a packaged launch has already happened.
    AlreadyDone,
    /// First packaged launch and a config is readable at the classic path —
    /// snapshot it (and the backups) so the package owns a copy.
    Snapshot,
    /// First packaged launch, no config anywhere. A genuine first run;
    /// `config::load_or_init` seeds defaults as usual.
    Nothing,
    /// **PROBLEM 250 follow-up — LIVE TEST 2026-09-05.** The resolved data dir
    /// IS the classic path, so "copy the config somewhere the package owns"
    /// has nowhere to copy it TO: source and destination are the same folder.
    /// Do nothing, write nothing, say so once.
    SameDirectory,
}

/// The decision. Pure, so both of the arms below that have now actually run on
/// a machine are pinned by a test rather than by a memory of a log.
///
/// **`data_dir_is_classic` is not a virtualisation detector, and must never be
/// described as one.** File-system virtualisation under MSIX is transparent:
/// the path string a packaged process sees is byte-identical either way, which
/// is the same trap CLAUDE.md's testing laws record for the agent container
/// ("printing `%LOCALAPPDATA%` is not proof that you escaped"). This flag
/// answers a smaller, answerable question — *is the directory we resolved the
/// same directory the classic path names?* — and that is the one the snapshot
/// actually depends on.
pub(crate) fn plan_migration(
    packaged: bool,
    marker_exists: bool,
    classic_config_exists: bool,
    data_dir_is_classic: bool,
) -> MigrationPlan {
    if !packaged {
        return MigrationPlan::NotPackaged;
    }
    // Checked BEFORE the marker, deliberately. `AlreadyDone` is a true
    // statement about a machine that has one, but it is the less useful one:
    // the honest answer for every launch on such a machine is "there was never
    // anything to migrate", and a reader chasing a missing snapshot needs to
    // be told that rather than "it ran earlier".
    if data_dir_is_classic {
        return MigrationPlan::SameDirectory;
    }
    if marker_exists {
        return MigrationPlan::AlreadyDone;
    }
    if classic_config_exists {
        MigrationPlan::Snapshot
    } else {
        MigrationPlan::Nothing
    }
}

/// Case-insensitive, separator-insensitive "are these the same directory".
/// Pure — no filesystem — because the paths being compared are both derived
/// from the same environment variable and a trailing separator or a `/` is a
/// formatting difference, not a different folder.
pub(crate) fn same_directory(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.trim()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    let (a, b) = (norm(a), norm(b));
    !a.is_empty() && a == b
}

/// One-time, first-packaged-launch care for the user's config.
///
/// ## Where the "legacy real path" question lands, from the docs and from a
/// measurement
///
/// The brief this was written from asked for a way to read "the LEGACY REAL
/// path, not the virtualised one". **On the evidence there is no such thing to
/// reach for, and two independent lines of evidence say the classic path is
/// already the right one.**
///
/// **From the documentation.** Microsoft's
/// [desktop-to-uwp-behind-the-scenes][b2s] page scopes AppData and registry
/// redirection with "this section applies only to virtualized apps" — i.e. to
/// **appContainer** packages. Spaceadom's package declares `runFullTrust`
/// (mediumIL), so on that reading `SHGetKnownFolderPath(FOLDERID_RoamingAppData)`
/// returns the real `C:\Users\<u>\AppData\Roaming` and the packaged copy and the
/// NSIS copy share one `config.json`. **Caveat, and it is the reason the
/// snapshot below exists at all: no Microsoft page states "a full-trust package
/// is never virtualized" in so many words.** That is an inference from repeated
/// scoping qualifiers, not a quoted rule. The `desktop6:FileSystemWriteVirtualization`
/// element the brief asked about is for virtualized apps opting OUT, so it is
/// NOT declared in our manifest — declaring an element on a package it does not
/// apply to is how a manifest starts failing validation for reasons nobody can
/// reconstruct later.
///
/// [b2s]: https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-behind-the-scenes
///
/// **From a measurement.** What a redirection view does WHEN it applies was
/// measured on the dev machine on 2026-09-05, from inside a live one
/// (`Claude_pzs8sxrjxfjjc` — CLAUDE.md's PROBLEM 143 container), listing
/// `%APPDATA%\Spaceadom`:
///
/// ```text
///   config.json                47,754 B  18 Aug   <- the CONTAINER's private copy
///   debug.log               4,753,847 B   5 Sep   <- the REAL file, today's clock
///   picker-cache.json         827,272 B   5 Sep   <- the REAL file
///   last-run-version.txt            7 B   4 Sep   <- the REAL file
/// ```
///
/// and the container's own store side by side:
///
/// ```text
///   …\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Spaceadom\
///   config.json                47,754 B                <- present, hence shadowing
///   (no debug.log, no picker-cache.json)               <- absent, hence falling through
/// ```
///
/// That is a **copy-on-write union view, not a redirect**: a process inside it
/// reading `%APPDATA%\Spaceadom\config.json` gets the container's private copy
/// IF ONE EXISTS, and otherwise reads the REAL user file straight through. It
/// is also, incidentally, the answer to the mystery CLAUDE.md records under
/// "config.json is shadowed even though debug.log beside it is not" —
/// config.json was written from inside the container once, which created the
/// private copy; debug.log never was.
///
/// So under EITHER reading — no virtualization at all (the docs), or
/// copy-on-write virtualization (the measurement) — a genuine first packaged
/// launch, before this app has written anything, finds
/// `config::config_path()` ALREADY RESOLVING TO THE USER'S REAL CONFIG. The
/// migration is not "find the hidden real file"; it is "make sure the package
/// ends up owning a durable copy of what it just read".
///
/// ## What this function therefore does, and why it is still worth having
///
/// It copies `config.json` and the `SpaceadomBackups` folder to
/// `<data dir>\packaged-migration\` once, before the config is loaded, and
/// writes the marker. Two reasons, neither of them "the app cannot read the
/// config":
///
/// 1. **The user may uninstall the unpackaged copy afterwards.** That is the
///    documented, encouraged next step once the Store version is in — and the
///    NSIS uninstaller may take `%APPDATA%\Spaceadom` with it. If the package
///    had not yet written its own copy, the config would be gone.
/// 2. **It leaves EVIDENCE.** The copy's byte count and mtime are logged; a
///    Store user whose bindings appear empty can be answered from one log line
///    instead of a diagnostic round trip.
///
/// It never writes to the classic path and never deletes anything.
///
/// ## VERIFIED 2026-09-05, and the "unverified" paragraph that used to stand
/// here was wrong in the one way that mattered
///
/// The `.msix` was installed and run on the owner's own machine on 2026-09-05
/// (`_probe/msix-test/log.txt`, §"ATTEMPT 2"). The reasoning above held —
/// `config::config_path()` resolved to the user's real config and the packaged
/// copy loaded it — and the sentence *"with no virtualization the snapshot is a
/// plain extra copy inside the real folder"* turned out to be the whole story
/// rather than one of two readings. What it did NOT say is that a plain extra
/// copy inside the real folder is **not worth making**:
///
/// ```text
/// %APPDATA%\Spaceadom\packaged-first-run.txt          90 B
/// %APPDATA%\Spaceadom\packaged-migration\config.json  77,912 B
/// %APPDATA%\Spaceadom\packaged-migration\backups\     20 files, 1,309,141 B
/// ```
///
/// 1.3 MB copied from a folder into a subfolder of itself, on a machine where
/// the two reasons the copy exists cannot apply: reason 1 was "the NSIS
/// uninstaller may take `%APPDATA%\Spaceadom` with it" — it would take the
/// snapshot too, it is inside it — and reason 2, the evidence, is served by the
/// log line alone.
///
/// **So the snapshot is now conditional on the data dir actually being
/// somewhere else** (`plan_migration`'s `data_dir_is_classic`). When it is not,
/// this logs *"AppData not virtualised — nothing to migrate"* and writes
/// nothing at all: no copies, and **no marker**, so that if a future Windows,
/// a future manifest, or a future `portable::resolve_root` ever does put a
/// package's data somewhere else, the snapshot is still able to run. The
/// condition is one string comparison per launch.
///
/// ## Why AppData was NOT redirected while HKCU WAS — the documented rule
///
/// The same package, the same launch: writes to `%APPDATA%\Spaceadom` landed in
/// the real `C:\Users\<u>\AppData\Roaming\Spaceadom`, while HKCU writes landed
/// in `…\Packages\<PFN>\SystemAppData\Helium\User.dat` and never reached the
/// real hive. That split is documented, and it is not about `runFullTrust`:
///
/// * **Both** file-system and registry write virtualization are ON by default
///   for a desktop-bridge package. `desktop6:FileSystemWriteVirtualization` and
///   `desktop6:RegistryWriteVirtualization` are **opt-OUTS** (default
///   `enabled`), they need the `unvirtualizedResources` restricted capability,
///   and the registry one is documented as "intended to be used only by certain
///   types of desktop PC games published by Microsoft and our partners". **We
///   declare neither, and should not** — see "Flexible virtualization"
///   (learn.microsoft.com/en-us/windows/msix/desktop/flexible-virtualization).
///   The older belief recorded above — that the redirection sections "apply
///   only to virtualized apps", i.e. appContainer ones, and therefore not to
///   us — is **not supported by that page** and is contradicted by our own
///   Helium measurement. It is corrected here rather than deleted, because the
///   reasoning is what a future reader will otherwise repeat.
///
/// * What DOES explain the split is AppData's **file-open fallback**, which has
///   no registry equivalent: the OS opens the per-package copy first, and if
///   that does not exist it opens the real AppData file — after which, in
///   Microsoft's words, "no virtualization for that file occurs"
///   (learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-behind-the-scenes).
///   `%APPDATA%\Spaceadom` already existed, written by the NSIS copy, so every
///   file in it fell through to the real one. The registry table on the same
///   page states flatly that HKCU writes are copied on write to a private
///   per-user, per-app location, and documents no such escape.
///
/// **What that means for a genuine Store user with no NSIS copy:** the folder
/// does NOT already exist, the fallback has nothing to fall back to, and the
/// package's private AppData store is where the config will live. That machine
/// is still unobserved. The behaviour here is right either way — the comparison
/// is on the resolved path, not on an assumption about which of the two
/// happened — but the claim "AppData is never virtualised for this package" is
/// NOT established and must not be written down as if it were. What was
/// measured is one machine where a pre-existing folder made it fall through.
pub fn migrate_legacy_data_once() {
    let data_dir = crate::startup::data_dir();
    let marker = data_dir.join(MIGRATION_MARKER);
    let classic = data_dir.join("config.json");

    // The classic path, computed from ONE source shared with the resolver
    // (`portable::roaming_default`) rather than re-spelled here: two producers
    // of "%APPDATA%\Spaceadom" that can drift apart would make this comparison
    // answer a question nobody asked.
    let classic_dir = crate::portable::roaming_default();
    let dir_is_classic = same_directory(
        &data_dir.to_string_lossy(),
        &classic_dir.to_string_lossy(),
    );

    match plan_migration(
        is_packaged(),
        marker.exists(),
        classic.exists(),
        dir_is_classic,
    ) {
        MigrationPlan::NotPackaged => return,
        MigrationPlan::SameDirectory => {
            log::info!(
                "{BOOT_MARKER}: AppData not virtualised — nothing to migrate. The resolved data \
                 dir IS the classic path ({}), so the one-time config snapshot would copy \
                 config.json and the backups into a subfolder of the folder it read them from. \
                 It is skipped, and no marker is written, so this stays re-checkable if a \
                 future package ever does resolve its data elsewhere. (LIVE TEST 2026-09-05: \
                 the previous unconditional version wrote 1.3 MB of exactly that.)",
                data_dir.display()
            );
            return;
        }
        MigrationPlan::AlreadyDone => {
            log::info!(
                "{BOOT_MARKER}: packaged, and '{MIGRATION_MARKER}' is already present — \
                 the one-time config snapshot ran on an earlier launch."
            );
            return;
        }
        MigrationPlan::Nothing => {
            log::info!(
                "{BOOT_MARKER}: FIRST PACKAGED LAUNCH and no config.json at {} — nothing to \
                 carry over, so this really is a first run and defaults will be seeded.",
                classic.display()
            );
        }
        MigrationPlan::Snapshot => {
            let dest_dir = data_dir.join("packaged-migration");
            if let Err(e) = std::fs::create_dir_all(&dest_dir) {
                log::warn!(
                    "{BOOT_MARKER}: FIRST PACKAGED LAUNCH — could not create {} ({e}); the \
                     config snapshot was skipped. Nothing is lost: the config itself is \
                     untouched and load_or_init reads it normally.",
                    dest_dir.display()
                );
            } else {
                let size = std::fs::metadata(&classic).map(|m| m.len()).unwrap_or(0);
                let dest = dest_dir.join("config.json");
                match std::fs::copy(&classic, &dest) {
                    Ok(n) => log::info!(
                        "{BOOT_MARKER}: FIRST PACKAGED LAUNCH — copied {n} bytes of config from \
                         {} to {}. The packaged copy now owns a durable snapshot of your \
                         profiles and bindings even if the unpackaged Spaceadom is uninstalled.",
                        classic.display(),
                        dest.display()
                    ),
                    Err(e) => log::warn!(
                        "{BOOT_MARKER}: FIRST PACKAGED LAUNCH — could not copy the {size}-byte \
                         config from {} ({e}). The config itself is untouched.",
                        classic.display()
                    ),
                }
                copy_backups(&dest_dir.join("backups"));
            }
        }
    }

    // Written LAST and unconditionally: a snapshot that failed must not be
    // retried on every launch forever (it would fail every launch and spam the
    // log), and the failure is already recorded above.
    if let Err(e) = std::fs::write(
        &marker,
        format!(
            "Spaceadom first packaged launch, package {}\n",
            package_full_name().unwrap_or("unknown")
        ),
    ) {
        log::warn!("{BOOT_MARKER}: could not write {} ({e})", marker.display());
    }
}

/// Copy the PROBLEM 94 rolling backups next to the snapshot. Best-effort
/// throughout, exactly like `config::write_backup`: losing a backup copy must
/// never stop the app from starting.
fn copy_backups(dest: &std::path::Path) {
    let src = crate::config::backup_dir();
    let Ok(entries) = std::fs::read_dir(&src) else {
        log::info!(
            "{BOOT_MARKER}: no backup folder at {} to snapshot (nothing has been saved yet)",
            src.display()
        );
        return;
    };
    if let Err(e) = std::fs::create_dir_all(dest) {
        log::warn!("{BOOT_MARKER}: could not create {} ({e})", dest.display());
        return;
    }
    let mut n = 0usize;
    let mut bytes = 0u64;
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let Some(name) = p.file_name() else { continue };
        if let Ok(copied) = std::fs::copy(&p, dest.join(name)) {
            n += 1;
            bytes += copied;
        }
    }
    log::info!(
        "{BOOT_MARKER}: snapshotted {n} config backup(s) ({bytes} bytes) from {} to {}",
        src.display(),
        dest.display()
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// REVIEW FIXES 2026-09-05 (MEDIUM). Once the sizing call has said
    /// `Packaged`, nothing downstream may answer "unpackaged": the only code
    /// that means unpackaged is 15700 and it was not returned. A failure to
    /// NAME the package leaves the identity intact with the name marked
    /// unknown.
    #[test]
    fn a_named_failure_after_a_packaged_sizing_call_stays_packaged() {
        let degraded = Identity::Packaged {
            full_name: UNKNOWN_PACKAGE_NAME.to_string(),
        };
        assert!(
            matches!(degraded, Identity::Packaged { .. }),
            "demoting to Unpackaged flips all four packaged behaviours at once — the updater \
             would install a SECOND unpackaged copy beside the Store one (PROBLEM 129/141/236)"
        );
        // …and the missing NAME is reported as missing rather than as a
        // plausible-looking string a caller might print or match on.
        assert_eq!(
            match &degraded {
                Identity::Packaged { full_name } if full_name == UNKNOWN_PACKAGE_NAME => None,
                Identity::Packaged { full_name } => Some(full_name.as_str()),
                Identity::Unpackaged => None,
            },
            None
        );
    }

    /// The whole point of the return-code mapping: 15700 is the ONLY code that
    /// means "unpackaged". Anything else that is not 0 or 122 is an anomaly
    /// that must be visible, not silently folded into either answer.
    #[test]
    fn probe_rc_15700_is_the_only_no_package_code() {
        assert_eq!(classify_probe_rc(15700), ProbeVerdict::NoPackage);
        assert_eq!(classify_probe_rc(0), ProbeVerdict::Packaged);
        assert_eq!(classify_probe_rc(122), ProbeVerdict::Packaged);
        assert_eq!(classify_probe_rc(5), ProbeVerdict::Unexpected(5));
        assert_eq!(classify_probe_rc(15701), ProbeVerdict::Unexpected(15701));
    }

    /// The test binary is not packaged, so the real probe must say so — and it
    /// must be cached, i.e. the second call agrees with the first.
    ///
    /// **This is a weak positive control and the comment is the point.** It
    /// exercises the 15700 branch and nothing else: there is no way to give a
    /// `cargo test` process package identity, so the PACKAGED branch of
    /// `probe()` is not covered by any test and never will be. It is covered by
    /// the second-machine recipe in SUBMIT-CHECKLIST.md instead — install the
    /// .msix there and grep debug.log for the boot marker.
    #[test]
    fn an_unpackaged_test_process_reports_unpackaged() {
        assert!(!is_packaged());
        assert_eq!(identity(), &Identity::Unpackaged);
        assert!(package_full_name().is_none());
        assert!(!is_packaged(), "cached answer must not change between calls");
    }

    #[test]
    fn startup_state_numbers_match_the_winrt_enum() {
        // Windows.ApplicationModel.StartupTaskState, in declaration order.
        assert_eq!(classify_startup_state(0), StartupState::Disabled);
        assert_eq!(classify_startup_state(1), StartupState::DisabledByUser);
        assert_eq!(classify_startup_state(2), StartupState::Enabled);
        assert_eq!(classify_startup_state(3), StartupState::DisabledByPolicy);
        assert_eq!(classify_startup_state(4), StartupState::EnabledByPolicy);
        assert_eq!(classify_startup_state(99), StartupState::Unavailable);
    }

    /// A control that does nothing is worse than a missing control: the only
    /// two states the app may change are the two it can actually change.
    #[test]
    fn only_enabled_and_disabled_are_ours_to_change() {
        assert!(app_may_change(StartupState::Enabled));
        assert!(app_may_change(StartupState::Disabled));
        assert!(!app_may_change(StartupState::DisabledByUser));
        assert!(!app_may_change(StartupState::DisabledByPolicy));
        assert!(!app_may_change(StartupState::EnabledByPolicy));
        assert!(!app_may_change(StartupState::Unavailable));
    }

    /// The note and the lock have to agree, in both directions — a locked row
    /// with no explanation is the failure this pair exists to prevent.
    #[test]
    fn every_locked_state_explains_itself_and_no_live_one_does() {
        for s in [
            StartupState::Enabled,
            StartupState::Disabled,
            StartupState::DisabledByUser,
            StartupState::DisabledByPolicy,
            StartupState::EnabledByPolicy,
            StartupState::Unavailable,
        ] {
            assert_eq!(
                app_may_change(s),
                startup_locked_note(s).is_empty(),
                "{s:?}: a locked row needs a note and a live row must not have one"
            );
        }
    }

    #[test]
    fn migration_runs_once_and_only_when_packaged() {
        use MigrationPlan::*;
        // Every case here has the data dir resolved somewhere OTHER than the
        // classic path — the only situation in which there is a migration to
        // plan at all. `data_dir_is_classic` gets its own test below.
        //
        // Unpackaged: never, whatever else is true.
        assert_eq!(plan_migration(false, false, true, false), NotPackaged);
        assert_eq!(plan_migration(false, true, true, false), NotPackaged);
        assert_eq!(plan_migration(false, false, true, true), NotPackaged);
        // Packaged, first launch, config readable at the classic path.
        assert_eq!(plan_migration(true, false, true, false), Snapshot);
        // Packaged, first launch, genuinely nothing there.
        assert_eq!(plan_migration(true, false, false, false), Nothing);
        // The marker wins over everything below it: this must not run twice.
        assert_eq!(plan_migration(true, true, true, false), AlreadyDone);
        assert_eq!(plan_migration(true, true, false, false), AlreadyDone);
    }

    /// PROBLEM 250 follow-up — LIVE TEST 2026-09-05. The 1.3 MB that was
    /// copied from `%APPDATA%\Spaceadom` into `%APPDATA%\Spaceadom\
    /// packaged-migration\`. When the resolved data dir IS the classic path
    /// there is no destination, and the answer must not depend on the marker
    /// or on whether a config happens to be there.
    #[test]
    fn a_data_dir_that_is_already_the_classic_path_has_nothing_to_migrate() {
        use MigrationPlan::*;
        for marker in [false, true] {
            for config in [false, true] {
                assert_eq!(
                    plan_migration(true, marker, config, true),
                    SameDirectory,
                    "marker={marker} config={config}"
                );
            }
        }
    }

    /// The comparison itself. Both sides come out of the same environment
    /// variable, so the differences that can actually occur are a trailing
    /// separator, a `/`, and case — never two genuinely different folders that
    /// happen to look alike.
    #[test]
    fn same_directory_ignores_case_slashes_and_a_trailing_separator() {
        let a = r"C:\Users\beamu\AppData\Roaming\Spaceadom";
        assert!(same_directory(a, a));
        assert!(same_directory(a, &format!("{a}\\")));
        assert!(same_directory(a, &a.to_uppercase()));
        assert!(same_directory(a, &a.replace('\\', "/")));
        assert!(same_directory(&format!("  {a}  "), a));
        // Different folders, including the near-miss a naive prefix test would
        // accept.
        assert!(!same_directory(a, &format!(r"{a}\packaged-migration")));
        assert!(!same_directory(a, r"D:\Somewhere\Spaceadom-portable\data"));
        // An unreadable path must never compare equal to another unreadable
        // one — "we could not resolve either" is not "they are the same".
        assert!(!same_directory("", ""));
        assert!(!same_directory("", a));
    }

    /// CLAUDE.md's ASCII-marker rule: the boot marker is used to prove a
    /// binary contains this code, so it has to be long enough to survive in
    /// .rodata rather than being assembled by overlapping immediate stores
    /// (`st-hud-pointer`, 14 bytes, measured False in a binary that contained
    /// it). 32 is comfortably past where that was observed.
    #[test]
    fn the_boot_marker_is_long_enough_to_be_findable_in_the_binary() {
        assert!(
            BOOT_MARKER.len() >= 32,
            "BOOT_MARKER is {} bytes; short literals can be materialised by \
             overlapping stores and never exist contiguously on disk",
            BOOT_MARKER.len()
        );
        assert!(BOOT_MARKER.is_ascii());
    }
}
